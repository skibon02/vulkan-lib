use std::sync::atomic::AtomicBool;
use ash::vk::{AccessFlags, Buffer, BufferCreateFlags, BufferCreateInfo, BufferUsageFlags, DeviceMemory, DeviceSize, Extent3D, Format, Image, ImageLayout, MemoryAllocateInfo, MemoryMapFlags, PipelineStageFlags};
use slotmap::{DefaultKey, SlotMap};
use crate::runtime::{OptionSeqNumShared, SharedState};
use crate::runtime::buffers::BufferResource;
use crate::runtime::images::ImageResourceHandle;
use crate::runtime::pipeline::GraphicsPipelineInner;
use crate::wrappers::device::VkDeviceRef;

#[derive(Copy, Clone, Debug)]
pub struct ResourceUsage {
    pub submission_num: Option<usize>,
    pub stage_flags: PipelineStageFlags,
    pub access_flags: AccessFlags,
    pub is_readonly: bool,
}

impl ResourceUsage {
    pub fn new(submission_num: Option<usize>, stage_flags: PipelineStageFlags, access_flags: AccessFlags, is_readonly: bool) -> Self {
        Self {
            submission_num,
            stage_flags,
            access_flags,
            is_readonly
        }
    }
    
    pub fn empty(submission_num: Option<usize>) -> Self {
        Self {
            submission_num,
            stage_flags: PipelineStageFlags::empty(),
            access_flags: AccessFlags::empty(),
            is_readonly: true
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
        if let Self::DeviceUsage(resource_usage) = self && let Some(submission_num) = resource_usage.submission_num && last_waited_num >= submission_num {
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

#[derive(Default)]
pub struct BufferHostState {
    // Seq number of last submission which uses this buffer
    // None - no such pending submissions
    pub last_used_in: OptionSeqNumShared,
    pub has_host_writes: AtomicBool,
}

pub(crate) struct BufferInner {
    pub buffer: Buffer,
    pub memory: DeviceMemory,
    pub usages: ResourceUsages,
}

pub(crate) struct ImageInner {
    pub image: Image,
    pub memory: Option<DeviceMemory>,
    pub usages: ResourceUsages,
    pub layout: ImageLayout,
    pub format: Format,
}

pub(crate) struct ResourceStorage {
    device: VkDeviceRef,
    buffers: SlotMap<DefaultKey, BufferInner>,
    images: SlotMap<DefaultKey, ImageInner>,
    pipelines: SlotMap<DefaultKey, GraphicsPipelineInner>,
}

impl ResourceStorage {
    pub fn new(device: VkDeviceRef) -> Self{
        Self {
            device,
            buffers: SlotMap::new(),
            images: SlotMap::new(),
            pipelines: SlotMap::new(),
        }
    }

    pub fn create_buffer(&mut self, usage: BufferUsageFlags, flags: BufferCreateFlags, size: DeviceSize, memory_type: u32, shared: SharedState) -> (BufferResource, DeviceMemory) {
        // create buffer
        let buffer = unsafe {
            self.device.create_buffer(&BufferCreateInfo::default()
                .usage(usage)
                .flags(flags)
                .size(size), None).unwrap()
        };
        let memory_requirements = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let allocation_size = memory_requirements.size;

        //allocate memory
        let memory = unsafe {
            self.device.allocate_memory(&MemoryAllocateInfo::default()
                .allocation_size(allocation_size)
                .memory_type_index(memory_type),
                                        None).unwrap() };

        unsafe {
            self.device.bind_buffer_memory(buffer, memory, 0).unwrap();
        }

        let buffer_inner = BufferInner {
            buffer,
            usages: ResourceUsages::new(),
            memory,
        };
        let state_key = self.buffers.insert(buffer_inner);

        let buffer = BufferResource::new(shared, state_key, memory, size);
        (buffer, memory)
    }
    pub fn buffer(&mut self, key: DefaultKey) -> &mut BufferInner {
        self.buffers.get_mut(key).unwrap()
    }
    pub fn destroy_buffer(&mut self, key: DefaultKey) {
        if let Some(buffer_inner) = self.buffers.remove(key) {
            unsafe {
                self.device.destroy_buffer(buffer_inner.buffer, None);
                self.device.free_memory(buffer_inner.memory, None);
            }
        }
    }

    pub fn add_image(&mut self, image: ImageInner) -> DefaultKey {
        self.images.insert(image)
    }
    pub fn image(&mut self, key: DefaultKey) -> &mut ImageInner {
        self.images.get_mut(key).unwrap()
    }
    pub fn destroy_image(&mut self, key: DefaultKey) {
        if let Some(image_inner) = self.images.remove(key) {
            if let Some(memory) = image_inner.memory {
                unsafe {
                    self.device.destroy_image(image_inner.image, None);
                    self.device.free_memory(memory, None);
                }
            }
        }
    }
    
    pub fn add_pipeline(&mut self, pipeline: GraphicsPipelineInner) -> DefaultKey {
        self.pipelines.insert(pipeline)
    }
    pub fn pipeline(&mut self, key: DefaultKey) -> &mut GraphicsPipelineInner {
        self.pipelines.get_mut(key).unwrap()
    }
    pub fn destroy_pipeline(&mut self, key: DefaultKey) {
        self.pipelines.remove(key);
    }
}

impl Drop for ResourceStorage {
    fn drop(&mut self) {
        unsafe {
            for (_, buffer_inner) in self.buffers.drain() {
                self.device.destroy_buffer(buffer_inner.buffer, None);
                self.device.free_memory(buffer_inner.memory, None);
            }

            for (_, image_inner) in self.images.drain() {
                if let Some(memory) = image_inner.memory {
                    unsafe {
                        self.device.destroy_image(image_inner.image, None);
                        self.device.free_memory(memory, None);
                    }
                }
            }
        }
    }
}