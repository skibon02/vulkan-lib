use std::ops::Range;
use std::sync::Arc;
use ash::vk;
use ash::vk::{BufferCreateFlags, BufferCreateInfo, BufferUsageFlags, DeviceSize, MemoryAllocateInfo};
use log::{error, warn};
use crate::queue::queue_local::QueueLocal;
use crate::resources::LastResourceUsage;
use crate::queue::memory_manager::{MemoryManager, MemoryTypeAlgorithm};
use crate::queue::OptionSeqNumShared;
use crate::queue::recording::BufferRange;
use crate::wrappers::device::VkDeviceRef;

pub struct BufferResource {
    pub(crate) buffer: vk::Buffer,
    pub(crate) memory: vk::DeviceMemory,
    size: usize,
    pub(crate) submission_usage: OptionSeqNumShared,
    pub(crate) inner: QueueLocal<BufferResourceInner>,

    dropped: bool,
}

pub(crate) struct BufferResourceInner {
    pub usages: LastResourceUsage,
}

impl BufferResource {
    pub(crate) fn new(device: &VkDeviceRef, memory_manager: &mut MemoryManager, usage: BufferUsageFlags, flags: BufferCreateFlags, size: DeviceSize) -> BufferResource {
        let (_, memory_type_bits) = memory_manager.get_buffer_memory_requirements(usage, flags);
        let memory_type = memory_manager.select_memory_type(memory_type_bits, MemoryTypeAlgorithm::Device);

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

        BufferResource {
            buffer,
            memory,
            size: size as usize,
            submission_usage: OptionSeqNumShared::default(),
            inner: QueueLocal::new(BufferResourceInner {
                usages: LastResourceUsage::None,
            }),

            dropped: false,
        }
    }
    
    pub fn size(&self) -> usize {
        self.size
    }
    
    pub fn full(self: &Arc<Self>) -> BufferRange {
        BufferRange {
            buffer: self.clone(),
            custom_range: None,
        }
    }

    pub fn range(self: &Arc<Self>, range: Range<usize>) -> BufferRange {
        let custom_range = if range.end > self.size || range.start > range.end {
            warn!(
                "Buffer range [{}, {}) is out of bounds (buffer size: {}). Using full buffer instead.",
                range.start, range.end, self.size
            );
            None
        } else {
            Some(range)
        };

        BufferRange {
            buffer: self.clone(),
            custom_range,
        }
    }
}

impl Drop for BufferResource {
    fn drop(&mut self) {
        if !self.dropped {
            error!("BufferResource was not destroyed before being dropped. This may lead to memory leaks.");
        }
    }
}
pub(crate) fn destroy_buffer_resource(device: &VkDeviceRef, mut buffer_resource: BufferResource) {
    if !buffer_resource.dropped {
        unsafe {
            device.destroy_buffer(buffer_resource.buffer, None);
            device.free_memory(buffer_resource.memory, None);
        }
        buffer_resource.dropped = true;
    }
}