use std::ops::{Deref, DerefMut, Range};
use std::slice::from_raw_parts_mut;
use ash::vk::{Buffer, DeviceMemory, MemoryMapFlags};
use slotmap::{DefaultKey, SlotMap};
use crate::runtime::{OptionSeqNumShared, SharedState};
use crate::wrappers::device::VkDeviceRef;

pub struct BufferResource {
    shared: SharedState,

    state_key: DefaultKey,
    memory: DeviceMemory,
    size: u64,
}

#[derive(Copy, Clone)]
pub struct BufferResourceHandle<'a> {
    state_key: DefaultKey,
    memory: DeviceMemory,
    size: u64,
    host_used_in: Option<&'a OptionSeqNumShared>
}

impl BufferResource {
    pub fn new(shared: SharedState, state_key: DefaultKey, memory: DeviceMemory, size: u64) -> Self {
        Self {
            shared,
            state_key,
            memory,
            size
        }
    }
    pub fn handle(&self) -> BufferResourceHandle {
        BufferResourceHandle {
            state_key: self.state_key,
            memory: self.memory,
            size: self.size,
            host_used_in: None,
        }
    }
}

impl Drop for BufferResource {
    fn drop(&mut self) {
        self.shared.schedule_destroy_buffer(self.handle())
    }
}

pub struct MappableBufferResource{
    inner: BufferResource,
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
    pub(crate) fn new(resource: BufferResource) -> Self {
        Self {
            inner: resource,
            used_in: OptionSeqNumShared::new()
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
        let ptr = unsafe { device.map_memory(self.inner.memory, range.start, size, MemoryMapFlags::empty()).unwrap() } as *mut u8;
        let slice = unsafe { from_raw_parts_mut(ptr, size as usize) };

        f(slice);

        unsafe {
            device.unmap_memory(self.inner.memory);
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
}

pub struct BufferInner {
    buffer: Buffer,
    used_in: Vec<usize>,
}

pub struct ResourceStorage {
    buffers: SlotMap<DefaultKey, BufferInner>,
}
impl ResourceStorage {
    pub fn new() -> Self{
        Self {
            buffers: SlotMap::new(),
        }
    }

    pub fn buffer(&mut self, key: DefaultKey) -> &mut BufferInner {
        self.buffers.get_mut(key).unwrap()
    }

    pub fn remove_buffer(&mut self, key: DefaultKey) {
        self.buffers.remove(key);
    }
}