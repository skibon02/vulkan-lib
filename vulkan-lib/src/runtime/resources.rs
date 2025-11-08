use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut, Range};
use std::slice::from_raw_parts_mut;
use ash::vk::{Buffer, DeviceMemory, MemoryMapFlags};
use slotmap::{DefaultKey, SlotMap};
use crate::runtime::{OptionSeqNumShared, SharedState};

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
            host_used_in: None,
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
    used_in: OptionSeqNumShared,
}

impl Deref for MappableBufferResource {
    type Target = BufferResource;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for MappableBufferResource {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl MappableBufferResource {
    pub(crate) fn new(resource: BufferResource, memory: DeviceMemory) -> Self {
        Self {
            inner: resource,
            memory,
            used_in: OptionSeqNumShared::new(),
        }
    }

    pub fn map_update<F: FnOnce(&mut [u8])>(&mut self, range: Range<u64>, f: F) {
        if let Some(seq_num) = self.used_in.load() {
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
        self.used_in.store(None);
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
            state_key: self.state_key,
            size: self.size,
            host_used_in: Some(&self.used_in),
        }
    }
}

impl Drop for MappableBufferResource {
    fn drop(&mut self) {
        self.shared.schedule_destroy_buffer(self.handle().into());
        self.inner.set_dropped();
    }
}


#[derive(Copy, Clone)]
pub struct BufferResourceHandle<'a> {
    pub(crate) state_key: DefaultKey,
    pub(crate) size: u64,
    pub(crate) host_used_in: Option<&'a OptionSeqNumShared>
}

impl From<BufferResourceHandle<'_>> for BufferResourceDestroyHandle {
    fn from(handle: BufferResourceHandle) -> Self {
        BufferResourceDestroyHandle {
            state_key: handle.state_key,
            size: handle.size,
            host_used_in: handle.host_used_in.and_then(|v| v.load())
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
    pub used_in: Vec<usize>,
    pub memory: DeviceMemory,
}

pub(crate) struct ResourceStorage {
    buffers: SlotMap<DefaultKey, BufferInner>,
}
impl ResourceStorage {
    pub fn new() -> Self{
        Self {
            buffers: SlotMap::new(),
        }
    }

    pub fn add_buffer(&mut self, buffer: BufferInner) -> DefaultKey {
        self.buffers.insert(buffer)
    }
    pub fn buffer(&mut self, key: DefaultKey) -> &mut BufferInner {
        self.buffers.get_mut(key).unwrap()
    }

    pub fn remove_buffer(&mut self, key: DefaultKey) {
        self.buffers.remove(key);
    }
}