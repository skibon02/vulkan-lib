use std::ops::Range;
use std::slice::from_raw_parts_mut;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use ash::vk;
use ash::vk::{BufferCreateFlags, BufferCreateInfo, BufferUsageFlags, DeviceSize, MemoryAllocateInfo, MemoryMapFlags};
use log::{error, warn};
use crate::try_get_instance;
use crate::queue::queue_local::QueueLocal;
use crate::resources::LastResourceUsage;
use crate::queue::memory_manager::{MemoryManager, MemoryTypeAlgorithm};
use crate::queue::OptionSeqNumShared;
use crate::queue::shared::HostWaitedNum;
use crate::resources::buffer::BufferResourceInner;
use crate::wrappers::device::VkDeviceRef;

pub struct StagingBufferRange {
    pub(crate) buffer: Arc<StagingBuffer>,
    pub(crate) range: Range<u64>,
}

impl StagingBufferRange {
    pub fn update(&mut self, f: impl FnOnce(&mut [u8])) {
        // Safety: owning StagingBufferRange guarantees unique access to this buffer range
        let data = unsafe {
            from_raw_parts_mut(self.buffer.mapped.add(self.range.start as usize), self.range.end as usize - self.range.start as usize)
        };
        f(data);
    }
}

pub struct StagingBufferResource(pub(super) Arc<StagingBuffer>);

impl StagingBufferResource {
    pub fn try_freeze(&self, size: usize) -> Option<StagingBufferRange> {
        self.0.try_freeze(size)
    }
    #[must_use]
    pub fn try_unfreeze(&self, host_waited_num: HostWaitedNum) -> Option<()> {
        self.0.try_unfreeze(host_waited_num)
    }
}

pub(crate) struct StagingBuffer {
    pub(crate) buffer: vk::Buffer,
    pub(crate) memory: vk::DeviceMemory,
    size: usize,
    pub(crate) submission_usage: OptionSeqNumShared,
    pub(crate) inner: QueueLocal<BufferResourceInner>,

    frozen_len: Mutex<u64>,
    mapped: *mut u8,

    dropped: AtomicBool,
}

impl StagingBuffer {
    pub(crate) fn new(device: &VkDeviceRef, memory_manager: &mut MemoryManager, usage: BufferUsageFlags, flags: BufferCreateFlags, size: DeviceSize) -> StagingBuffer {
        let usage = usage | BufferUsageFlags::TRANSFER_SRC;
        let (_, memory_type_bits) = memory_manager.get_buffer_memory_requirements(usage, flags);
        let memory_type = memory_manager.select_memory_type(memory_type_bits, MemoryTypeAlgorithm::Host);

        // create buffer
        let buffer = unsafe {
            device.create_buffer(&BufferCreateInfo::default()
                .usage(usage)
                .flags(flags)
                .size(size), None).unwrap()
        };
        let memory_requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
        let allocation_size = memory_requirements.size;

        //allocate memory
        let memory = unsafe {
            device.allocate_memory(&MemoryAllocateInfo::default()
                .allocation_size(allocation_size)
                .memory_type_index(memory_type),
                                   None).unwrap() };

        unsafe {
            device.bind_buffer_memory(buffer, memory, 0).unwrap();
        }

        let data = unsafe {
            device.map_memory(memory, 0, size, MemoryMapFlags::empty()).unwrap() as *mut u8
        };

        StagingBuffer {
            buffer,
            memory,
            size: size as usize,
            submission_usage: OptionSeqNumShared::default(),
            inner: QueueLocal::new(BufferResourceInner {
                usages: LastResourceUsage::FenceWaited,
            }),
            frozen_len: Mutex::new(0),

            mapped: data,
            dropped: AtomicBool::new(false),
        }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn try_freeze(self: &Arc<Self>, size: usize) -> Option<StagingBufferRange> {
        let mut current_frozen = self.frozen_len.lock().unwrap();
        if *current_frozen as usize + size <= self.size {
            let start = *current_frozen;
            *current_frozen += size as u64;

            Some(StagingBufferRange {
                buffer: self.clone(),
                range: start..start + size as u64,
            })
        }
        else {
            None
        }
    }

    #[must_use]
    pub fn try_unfreeze(self: &Arc<Self>, host_waited_num: HostWaitedNum) -> Option<()> {
        if Arc::strong_count(self) == 2 && self.submission_usage.load().is_none_or(|num| host_waited_num.num() >= num) {
            // safe to unfreeze
            *self.frozen_len.lock().unwrap() = 0;
            Some(())
        }
        else {
            None
        }
    }
}

impl Drop for StagingBuffer {
    fn drop(&mut self) {
        if !self.dropped.load(Ordering::Relaxed) {
            destroy_staging_buffer_resource(self, false);
        }
    }
}
pub(crate) fn destroy_staging_buffer_resource(buffer_resource: &StagingBuffer, no_usages: bool) {
    if !buffer_resource.dropped.swap(true, Ordering::Relaxed) {
        if let Some(instance) = try_get_instance() {
            if !no_usages {
                let last_host_waited = instance.shared_state.last_host_waited_cached().num();
                if buffer_resource.submission_usage.load().is_some_and(|u| u > last_host_waited) {
                    warn!("Trying to destroy staging buffer resource, but VulkanAllocator was destroyed earlier! Calling device_wait_idle...");
                    unsafe {
                        instance.device.device_wait_idle().unwrap();
                    }
                }
            }
            let device = instance.device.clone();
            unsafe {
                device.unmap_memory(buffer_resource.memory);
                device.destroy_buffer(buffer_resource.buffer, None);
                device.free_memory(buffer_resource.memory, None);
            }
        }
        else {
            error!("VulkanInstance was destroyed! Cannot destroy staging buffer resource");
        }
    }
}