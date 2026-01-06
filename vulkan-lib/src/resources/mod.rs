use std::collections::HashMap;
use std::sync::Arc;
use ash::vk;
use ash::vk::{AccessFlags, BufferCreateFlags, BufferUsageFlags, DescriptorSetLayout, DescriptorSetLayoutBinding, DescriptorSetLayoutCreateInfo, DeviceSize, Format, ImageCreateFlags, ImageUsageFlags, PipelineStageFlags, SampleCountFlags, SamplerCreateInfo};
use log::{error, info};
use slotmap::DefaultKey;
use smallvec::SmallVec;
use descriptor_pool::DescriptorSetAllocator;
use crate::resources::buffer::{destroy_buffer_resource, BufferResource};
use crate::resources::descriptor_set::DescriptorSetResource;
use crate::resources::image::{destroy_image_resource, ImageResource};
use crate::resources::pipeline::{destroy_pipeline, GraphicsPipelineDesc, GraphicsPipelineResource};
use crate::resources::render_pass::{destroy_render_pass, AttachmentsDescription, FrameBufferAttachment, RenderPassResource};
use crate::resources::sampler::SamplerResource;
use crate::queue::memory_manager::MemoryManager;
use crate::queue::shared::{HostWaitedNum, SharedState};
use crate::resources::staging_buffer::{destroy_staging_buffer_resource, StagingBuffer, StagingBufferResource};
use crate::shaders::DescriptorSetLayoutBindingDesc;
use crate::VulkanInstance;
use crate::wrappers::device::VkDeviceRef;

pub mod buffer;
pub mod image;
pub mod render_pass;
pub mod pipeline;
pub mod descriptor_set;
pub mod sampler;
pub mod descriptor_pool;
pub mod staging_buffer;

pub struct VulkanAllocator {
    memory_manager: MemoryManager,
    descriptor_set_layouts: HashMap<Vec<DescriptorSetLayoutBindingDesc>, DescriptorSetLayout>,
    descriptor_set_allocator: DescriptorSetAllocator,

    staging_buffers: Vec<Arc<StagingBuffer>>,
    buffers: Vec<Arc<BufferResource>>,
    images: Vec<Arc<ImageResource>>,
    render_passes: Vec<Arc<RenderPassResource>>,
    pipelines: Vec<Arc<GraphicsPipelineResource>>,
    samplers: Vec<Arc<SamplerResource>>,
    instance: Arc<VulkanInstance>,
}

impl VulkanAllocator {
    pub(crate) fn new(
        instance: Arc<VulkanInstance>,
        memory_manager: MemoryManager,
    ) -> Self {
        let device = instance.device.clone();
        let shared_state = instance.shared_state.clone();
        let descriptor_set_allocator = DescriptorSetAllocator::new(device.clone(), shared_state.clone());

        Self {
            instance,
            memory_manager,
            descriptor_set_layouts: HashMap::new(),
            descriptor_set_allocator,
            staging_buffers: Vec::new(),
            buffers: Vec::new(),
            images: Vec::new(),
            render_passes: Vec::new(),
            pipelines: Vec::new(),
            samplers: Vec::new(),
        }
    }

    pub fn shared(&self) -> SharedState {
        self.instance.shared_state.clone()
    }

    pub fn allocate_descriptor_set(&mut self, bindings: &'static [DescriptorSetLayoutBindingDesc]) -> Arc<DescriptorSetResource> {
        let layout = self.get_or_create_descriptor_set_layout(bindings);
        let resource = self.descriptor_set_allocator.allocate_descriptor_set(layout, bindings);
        resource
    }

    pub fn new_buffer(&mut self, usage: BufferUsageFlags, flags: BufferCreateFlags, size: DeviceSize) -> Arc<BufferResource> {
        let res = Arc::new(BufferResource::new(&self.instance.device, &mut self.memory_manager, usage, flags, size));
        self.buffers.push(res.clone());
        res
    }
    
    pub fn new_staging_buffer(&mut self, size: DeviceSize) -> StagingBufferResource {
        let usage = BufferUsageFlags::empty();
        let flags = BufferCreateFlags::empty();
        let res = Arc::new(StagingBuffer::new(&self.instance.device, &mut self.memory_manager, usage, flags, size));
        self.staging_buffers.push(res.clone());
        StagingBufferResource(res)
    }

    pub fn new_image(&mut self, usage: ImageUsageFlags, flags: ImageCreateFlags,
                     width: u32, height: u32, format: Format, samples: SampleCountFlags) -> Arc<ImageResource> {
        let res = Arc::new(ImageResource::new(&self.instance.device, &mut self.memory_manager, usage, flags, width, height, format, samples));
        self.images.push(res.clone());
        res
    }

    pub fn new_sampler(&mut self, f: impl FnOnce(SamplerCreateInfo) -> SamplerCreateInfo) -> Arc<SamplerResource> {
        let default_info =
            SamplerCreateInfo::default()
                .mag_filter(vk::Filter::LINEAR)
                .min_filter(vk::Filter::LINEAR)
                .address_mode_u(vk::SamplerAddressMode::REPEAT)
                .address_mode_v(vk::SamplerAddressMode::REPEAT)
                .address_mode_w(vk::SamplerAddressMode::REPEAT)
                .anisotropy_enable(false)
                .max_anisotropy(16.0)
                .border_color(vk::BorderColor::INT_OPAQUE_BLACK)
                .unnormalized_coordinates(false)
                .compare_enable(false)
                .compare_op(vk::CompareOp::ALWAYS)
                .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                .min_lod(0.0)
                .max_lod(0.0)
                .mip_lod_bias(0.0);
        let sampler_info = f(default_info);
        let sampler = SamplerResource::new(&self.instance.device, &sampler_info);
        let res = Arc::new(sampler);
        self.samplers.push(res.clone());

        res
    }

    pub fn new_render_pass(
        &mut self,
        attachments_description: AttachmentsDescription,
        swapchain_format: vk::Format,
    ) -> Arc<RenderPassResource> {
        let res = Arc::new(RenderPassResource::new(
            &self.instance.device,
            attachments_description,
            swapchain_format,
        ));
        self.render_passes.push(res.clone());
        res
    }
    pub fn new_pipeline(&mut self, render_pass: Arc<RenderPassResource>, pipeline_desc: GraphicsPipelineDesc, with_depth_test: bool) -> Arc<GraphicsPipelineResource> {
        let descriptor_set_layouts = pipeline_desc.bindings.iter()
            .map(|bindings_desc| self.get_or_create_descriptor_set_layout(bindings_desc))
            .collect();

        let res = Arc::new(GraphicsPipelineResource::new(&self.instance.device, render_pass, pipeline_desc, descriptor_set_layouts, with_depth_test));
        self.pipelines.push(res.clone());

        res
    }
    fn get_or_create_descriptor_set_layout(&mut self, bindings_desc: &[DescriptorSetLayoutBindingDesc]) -> DescriptorSetLayout {
        let key: Vec<DescriptorSetLayoutBindingDesc> = bindings_desc.to_vec();

        if let Some(&layout) = self.descriptor_set_layouts.get(&key) {
            return layout;
        }

        let bindings: Vec<DescriptorSetLayoutBinding> = bindings_desc.iter().map(|desc| {
            DescriptorSetLayoutBinding::default()
                .binding(desc.binding)
                .descriptor_type(desc.descriptor_type)
                .descriptor_count(desc.descriptor_count)
                .stage_flags(desc.stage_flags)
        }).collect();

        let layout_create_info = DescriptorSetLayoutCreateInfo::default()
            .bindings(&bindings);

        let layout = unsafe {
            self.instance.device.create_descriptor_set_layout(&layout_create_info, None).unwrap()
        };

        self.descriptor_set_layouts.insert(key, layout);
        layout
    }


    pub fn dump_resource_usage(&self) {
        let buffer_count = self.buffers.len();
        let staging_buffer_count = self.staging_buffers.len();
        let image_count = self.images.len();
        let render_pass_count = self.render_passes.len();
        let pipeline_count = self.pipelines.len();
        let sampler_count = self.samplers.len();
        println!("Resource usage dump:");
        println!("Buffers: {}", buffer_count);
        println!("Staging buffers: {}", staging_buffer_count);
        println!("Images: {}", image_count);
        println!("Render passes: {}", render_pass_count);
        println!("Pipelines: {}", pipeline_count);
        println!("Samplers: {}", sampler_count);
    }

    pub fn destroy_old_resources(&mut self) {
        let last_waited = self.instance.shared_state.last_host_waited_submission().num();

        let mut i = 0;
        while i < self.buffers.len() {
            if self.buffers[i].submission_usage.load().is_none_or(|n| n <= last_waited) && Arc::strong_count(&self.buffers[i]) == 1 {
                destroy_buffer_resource(&self.buffers[i], true);
                self.buffers.swap_remove(i);
            }
            else {
                i += 1;
            }
        }

        let mut i = 0;
        while i < self.staging_buffers.len() {
            if self.staging_buffers[i].submission_usage.load().is_none_or(|n| n <= last_waited) && Arc::strong_count(&self.staging_buffers[i]) == 1 {
                destroy_staging_buffer_resource(&self.staging_buffers[i], true);
                self.staging_buffers.swap_remove(i);
            }
            else {
                i += 1;
            }
        }


        let mut i = 0;
        while i < self.images.len() {
            if self.images[i].submission_usage.load().is_none_or(|n| n <= last_waited) && Arc::strong_count(&self.images[i]) == 1 {
                destroy_image_resource(&self.images[i], true);
                self.images.swap_remove(i);
            }
            else {
                i += 1;
            }
        }

        self.descriptor_set_allocator.on_submission_waited(last_waited);

        let mut i = 0;
        while i < self.pipelines.len() {
            if self.pipelines[i].submission_usage.load().is_none_or(|n| n <= last_waited) && Arc::strong_count(&self.pipelines[i]) == 1 {
                destroy_pipeline(&self.pipelines[i], true);
                self.pipelines.swap_remove(i);
            }
            else {
                i += 1;
            }
        }

        let mut i = 0;
        while i < self.render_passes.len() {
            if self.render_passes[i].submission_usage.load().is_none_or(|n| n <= last_waited) && Arc::strong_count(&self.render_passes[i]) == 1 {
                destroy_render_pass(&self.render_passes[i], true);
                self.render_passes.swap_remove(i);
            }
            else {
                i += 1;
            }
        }
    }
}

impl Drop for VulkanAllocator {
    fn drop(&mut self) {
        unsafe {
            self.instance.device.device_wait_idle().unwrap();
        }

        info!("Dropping vulkan allocator...");
        self.destroy_old_resources();

        for (_, descriptor_set_layout) in self.descriptor_set_layouts.drain() {
            unsafe {
                self.instance.device.destroy_descriptor_set_layout(descriptor_set_layout, None);
            }
        }

        // check if something left not allocated
    }
}

/// Event of specific resource usage
#[derive(Copy, Clone, Debug)]
pub struct ResourceUsage {
    pub submission_num: usize,
    pub stage_flags: PipelineStageFlags,
    pub access_flags: AccessFlags,
}

impl ResourceUsage {
    pub fn default(submission_num: usize) -> Self {
        Self {
            submission_num,
            stage_flags: PipelineStageFlags::empty(),
            access_flags: AccessFlags::empty(),
        }
    }
    pub fn new(submission_num: usize, stage_flags: PipelineStageFlags, access_flags: AccessFlags) -> Self {
        // todo: validate access flags over stage flags
        Self {
            submission_num,
            stage_flags,
            access_flags,
        }
    }

    pub fn is_readonly(&self) -> bool {
        // todo: add flags from extensions
        // A usage is considered readonly if it does not have any write access flags
        let write_access_flags = AccessFlags::SHADER_WRITE
            | AccessFlags::COLOR_ATTACHMENT_WRITE
            | AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE
            | AccessFlags::TRANSFER_WRITE
            | AccessFlags::HOST_WRITE
            | AccessFlags::MEMORY_WRITE;

        self.access_flags & write_access_flags == AccessFlags::empty()
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct RequiredSync {
    pub src_stages: PipelineStageFlags,
    pub dst_stages: PipelineStageFlags,
    pub src_access: AccessFlags,
    pub dst_access: AccessFlags,
}

impl RequiredSync {
    pub fn is_empty(&self) -> bool {
        if self.src_stages.is_empty() && self.dst_stages.is_empty() && self.src_access.is_empty() && self.dst_access.is_empty() {
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Debug)]
pub enum LastResourceUsage {
    /// Has a write that is not yet available
    HasWrite {
        last_write: ResourceUsage,
    },
    /// Has available write, that can be visible for specific access flags
    HasAvailableWrite {
        submission_num: usize,
        visible_for: AccessFlags,
    },
    /// All previous writes are available and visible to all accesses
    FenceWaited,
    /// Special state for swapchain image resource. No operations with this image is possible until acquire.
    Presented,
}

impl LastResourceUsage {
    pub fn new() -> Self {
        Self::FenceWaited
    }

    pub fn last_write_submission_num(&mut self) -> Option<&mut usize> {
        match self {
            Self::HasWrite { last_write } => Some(&mut last_write.submission_num),
            Self::HasAvailableWrite { submission_num, .. } => Some(submission_num),
            Self::FenceWaited => None,
            Self::Presented => None,
        }
    }
    pub fn on_host_waited(&mut self, last_waited_num: HostWaitedNum) {
        if self.last_write_submission_num().is_some_and(|u| last_waited_num.num() >= *u) {
            *self = Self::FenceWaited;
        }
    }

    /// Add new usage, returning previous usage if a sync barrier is needed.
    /// Returns Some(previous_usage) if we need synchronization, None if no sync needed.
    pub fn add_usage(&mut self, new_usage: ResourceUsage) -> Option<RequiredSync> {
        let mut res = RequiredSync::default();
        
        if matches!(self, Self::Presented) {
            panic!("Trying to use a presented swapchain image before acquiring it!");
        }
        
        // Visibility operation
        if let Self::HasWrite {
            last_write,
        } = self {
            res.src_stages = last_write.stage_flags;
            res.src_access = last_write.access_flags;

            *self = Self::HasAvailableWrite {
                submission_num: last_write.submission_num,
                visible_for: AccessFlags::empty(),
            }
        }


        // Availability operation
        if let Self::HasAvailableWrite {
            submission_num,
            visible_for,
        } = self {
            let need_visible = new_usage.access_flags & !*visible_for;
            if !need_visible.is_empty() {
                res.dst_stages = new_usage.stage_flags;
                res.dst_access = need_visible;
                *visible_for |= need_visible;
            }
        }

        // Update last usage
        if !new_usage.is_readonly() {
            *self = Self::HasWrite {
                last_write: new_usage,
            }
        }

        (!res.is_empty())
            .then_some(res)
    }

    /// try adding usage without requiring new sync
    pub fn validate_usage(&mut self, new_usage: ResourceUsage) -> bool {
        match self {
            Self::HasWrite { last_write } => {
                // cannot add new usage without sync if there is a pending write
                false
            }
            Self::HasAvailableWrite { visible_for, .. } => {
                // check visibility
                let need_visible = new_usage.access_flags & !*visible_for;
                if need_visible.is_empty() {
                    // all good, update last usage
                    if !new_usage.is_readonly() {
                        *self = Self::HasWrite {
                            last_write: new_usage,
                        }
                    }
                    true
                } else {
                    false
                }
            },
            Self::FenceWaited => {
                if !new_usage.is_readonly() {
                    *self = Self::HasWrite {
                        last_write: new_usage,
                    }
                }
                true
            },
            Self::Presented => false,
        }
    }

    pub fn validate_layout_transition(&mut self, sync: RequiredSync, submission_num: usize) -> bool {
        match self {
            Self::HasWrite { last_write } => {
                // perform availability operation
                // todo: handle composition in stage flags
                if (sync.src_stages == PipelineStageFlags::ALL_COMMANDS || sync.src_stages.contains(last_write.stage_flags))
                    &&
                    (sync.src_access == AccessFlags::MEMORY_WRITE || sync.src_access.contains(last_write.access_flags)){
                    *self = Self::HasAvailableWrite {
                        submission_num: last_write.submission_num,
                        visible_for: sync.dst_access,
                    };
                    true
                }
                else {
                    false
                }
            }
            Self::HasAvailableWrite { visible_for, .. } => {
                // perform visibility operation
                *visible_for |= sync.dst_access;
                true
            },
            Self::FenceWaited => {
                // we got write from layout transition
                *self = Self::HasAvailableWrite {
                    submission_num,
                    visible_for: sync.dst_access,
                };
                true
            },
            Self::Presented => false,
        }
    }

    pub fn is_full_visible(&self) -> bool {
        matches!(self, Self::FenceWaited)
    }
}

