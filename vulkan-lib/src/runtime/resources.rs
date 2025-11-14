use std::ops::{Deref, DerefMut, Range};
use std::slice::from_raw_parts_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use ash::vk::{AccessFlags, Buffer, DeviceMemory, MemoryMapFlags, PipelineStageFlags};
use slotmap::{DefaultKey, SlotMap};
use crate::runtime::{OptionSeqNumShared, SharedState};

#[derive(Copy, Clone, Debug)]
pub struct ResourceUsage {
    pub submission_num: usize,
    pub submission_group_num: usize,
    pub stage_flags: PipelineStageFlags,
    pub access_flags: AccessFlags,
    pub is_readonly: bool,
}

impl ResourceUsage {
    pub fn new(submission_num: usize, submission_group_num: usize, stage_flags: PipelineStageFlags, access_flags: AccessFlags, is_readonly: bool) -> Self {
        Self {
            submission_num,
            submission_group_num,
            stage_flags,
            access_flags,
            is_readonly
        }
    }
}

#[derive(Clone, Debug)]
pub enum ResourceUsages {
    DeviceUsage (ResourceUsage),
    None
}

impl ResourceUsages {
    pub fn new() -> Self {
        Self::None
    }

    pub fn on_host_waited(&mut self, last_waited_num: usize) {
        if let Self::DeviceUsage(resource_usage) = self && last_waited_num >= resource_usage.submission_num {
            *self = Self::None;
        }
    }

    /// Add new usage, returning previous usage if a sync barrier is needed.
    /// Returns Some(previous_usage) if we need synchronization, None if no sync needed.
    pub fn add_usage(&mut self, new_usage: ResourceUsage) -> Option<ResourceUsage> {
        if let ResourceUsages::DeviceUsage (prev_usage)= self {
            if prev_usage.is_readonly && new_usage.is_readonly {
                prev_usage.submission_num = new_usage.submission_num;
                prev_usage.stage_flags |= new_usage.stage_flags;
                prev_usage.access_flags |= new_usage.access_flags;
                return None;
            }
        }

        let prev_usage = self.last_usage();

        *self = ResourceUsages::DeviceUsage(new_usage);
        
        prev_usage
    }

    pub fn last_usage(&self) -> Option<ResourceUsage> {
        if let ResourceUsages::DeviceUsage (last_usage) = self {
            Some(*last_usage)
        } else { 
            None
        }
    }
    
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

pub struct BufferResource {
    shared: SharedState,

    state_key: DefaultKey,
    size: u64,
    dropped: bool,
}

impl BufferResource {
    pub fn new(shared: SharedState, state_key: DefaultKey, memory: DeviceMemory, size: u64) -> Self {
        Self {
            shared,
            state_key,
            size,
            dropped: false,
        }
    }
    pub fn handle_static(&self) -> BufferResourceHandle<'static> {
        BufferResourceHandle {
            state_key: self.state_key,
            size: self.size,
            host_state: None,
        }
    }

    fn set_dropped(&mut self) {
        self.dropped = true
    }
}

impl Drop for BufferResource {
    fn drop(&mut self) {
        if !self.dropped {
            self.shared.schedule_destroy_buffer(self.handle_static().into())
        }
        self.dropped = true
    }
}

pub struct MappableBufferResource{
    inner:  BufferResource,
    memory: DeviceMemory,
    host_state: BufferHostState,
}

impl MappableBufferResource {
    pub(crate) fn new(resource: BufferResource, memory: DeviceMemory) -> Self {
        Self {
            inner: resource,
            memory,
            host_state: BufferHostState::default(),
        }
    }

    pub fn map_update<F: FnOnce(&mut [u8])>(&mut self, range: Range<u64>, f: F) {
        if let Some(seq_num) = self.host_state.last_used_in.load() {
            self.inner.shared.wait_submission(seq_num);
        }

        if range.start > self.inner.size {
            panic!("Assertion failed: range start index is outside of buffer bounds! offset: {}, size: {}", range.start, self.inner.size);
        }

        if range.end > self.inner.size {
            panic!("Assertion failed: range end index is outside of buffer bounds! offset: {}, size: {}", range.end, self.inner.size);
        }

        let device = self.inner.shared.device.clone();
        let size = range.end - range.start;
        let ptr = unsafe { device.map_memory(self.memory, range.start, size, MemoryMapFlags::empty()).unwrap() } as *mut u8;
        let slice = unsafe { from_raw_parts_mut(ptr, size as usize) };

        f(slice);

        unsafe {
            device.unmap_memory(self.memory);
        }
        self.host_state.last_used_in.store(None);
        self.host_state.has_host_writes.store(true, Ordering::Relaxed);
    }

    pub fn map_write(&mut self, offset: usize, data: &[u8]) {
        let size = data.len() as u64;
        let range = offset as u64..offset as u64 + size;
        self.map_update(range, |slice| {
            slice.copy_from_slice(data);
        });
    }

    pub fn handle(&self) -> BufferResourceHandle {
        BufferResourceHandle {
            state_key: self.inner.state_key,
            size: self.inner.size,
            host_state: Some(&self.host_state),
        }
    }
}

impl Drop for MappableBufferResource {
    fn drop(&mut self) {
        self.inner.shared.schedule_destroy_buffer(self.handle().into());
        self.inner.set_dropped();
    }
}


#[derive(Copy, Clone)]
pub struct BufferResourceHandle<'a> {
    pub(crate) state_key: DefaultKey,
    pub(crate) size: u64,
    pub(crate) host_state: Option<&'a BufferHostState>,
}

#[derive(Default)]
pub struct BufferHostState {
    // Seq number of last submission which uses this buffer
    // None - no such pending submissions
    pub last_used_in: OptionSeqNumShared,
    pub has_host_writes: AtomicBool,
}

impl From<BufferResourceHandle<'_>> for BufferResourceDestroyHandle {
    fn from(handle: BufferResourceHandle) -> Self {
        BufferResourceDestroyHandle {
            state_key: handle.state_key,
            size: handle.size,
            host_used_in: handle.host_state.and_then(|v| v.last_used_in.load())
        }
    }
}

pub struct BufferResourceDestroyHandle {
    state_key: DefaultKey,
    size: u64,
    host_used_in: Option<usize>
}


pub(crate) struct BufferInner {
    pub buffer: Buffer,
    pub memory: DeviceMemory,
    pub usages: ResourceUsages,
}

pub(crate) struct ResourceStorage {
    device: crate::wrappers::device::VkDeviceRef,
    buffers: SlotMap<DefaultKey, BufferInner>,
}

impl ResourceStorage {
    pub fn new(device: crate::wrappers::device::VkDeviceRef) -> Self{
        Self {
            device,
            buffers: SlotMap::new(),
        }
    }

    pub fn add_buffer(&mut self, buffer: BufferInner) -> DefaultKey {
        self.buffers.insert(buffer)
    }

    pub fn buffer(&mut self, key: DefaultKey) -> &mut BufferInner {
        self.buffers.get_mut(key).unwrap()
    }

    pub fn remove_buffer(&mut self, key: DefaultKey) -> Option<BufferInner> {
        self.buffers.remove(key)
    }

    pub fn destroy_buffer(&mut self, key: DefaultKey) {
        if let Some(buffer_inner) = self.buffers.remove(key) {
            unsafe {
                self.device.destroy_buffer(buffer_inner.buffer, None);
                self.device.free_memory(buffer_inner.memory, None);
            }
        }
    }
}

impl Drop for ResourceStorage {
    fn drop(&mut self) {
        unsafe {
            for (_, buffer_inner) in self.buffers.drain() {
                self.device.destroy_buffer(buffer_inner.buffer, None);
                self.device.free_memory(buffer_inner.memory, None);
            }
        }
    }
}