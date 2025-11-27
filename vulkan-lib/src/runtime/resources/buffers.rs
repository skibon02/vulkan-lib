use std::ops::Range;
use std::slice::from_raw_parts_mut;
use std::sync::atomic::Ordering;
use ash::vk::{DeviceMemory, MemoryMapFlags};
use slotmap::DefaultKey;
use sparkles::range_event_start;
use crate::runtime::{resources::BufferHostState, shared::SharedState};


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
        let g = range_event_start!("[Vulkan] Map buffer memory");
        if let Some(seq_num) = self.host_state.last_used_in.load() {
            self.inner.shared.wait_submission(seq_num);
        }

        if range.start > self.inner.size {
            panic!("Assertion failed: range start index is outside of buffer bounds! offset: {}, size: {}", range.start, self.inner.size);
        }

        if range.end > self.inner.size {
            panic!("Assertion failed: range end index is outside of buffer bounds! offset: {}, size: {}", range.end, self.inner.size);
        }

        let device = self.inner.shared.device().clone();
        let size = range.end - range.start;
        let ptr = unsafe { device.map_memory(self.memory, range.start, size, MemoryMapFlags::empty()).unwrap() } as *mut u8;
        let slice = unsafe { from_raw_parts_mut(ptr, size as usize) };

        let g = range_event_start!("Application writes");
        f(slice);
        drop(g);

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

    pub fn handle<'a>(&'a self) -> BufferResourceHandle<'a> {
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

impl PartialEq for BufferResourceHandle<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.state_key == other.state_key && self.size == other.size
    }
}

#[derive(Clone)]
pub struct BufferResourceDestroyHandle {
    pub(crate) state_key: DefaultKey,
    size: u64,
    pub(crate) host_used_in: Option<usize>
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
