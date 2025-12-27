use std::collections::HashMap;
use std::sync::Arc;
use ash::vk;
use ash::vk::{AccessFlags, BufferCreateFlags, BufferUsageFlags, DescriptorSetLayout, DescriptorSetLayoutBinding, DescriptorSetLayoutCreateInfo, DeviceSize, Format, ImageCreateFlags, ImageUsageFlags, PipelineStageFlags, SampleCountFlags, SamplerCreateInfo};
use slotmap::DefaultKey;
use smallvec::SmallVec;
use descriptor_pool::DescriptorSetAllocator;
use crate::resources::buffer::{destroy_buffer_resource, BufferResource};
use crate::resources::descriptor_set::DescriptorSetResource;
use crate::resources::image::{destroy_image_resource, ImageResource};
use crate::resources::pipeline::{destroy_pipeline, GraphicsPipelineDesc, GraphicsPipelineResource};
use crate::resources::render_pass::{destroy_render_pass, RenderPassResource};
use crate::resources::sampler::SamplerResource;
use crate::queue::memory_manager::MemoryManager;
use crate::queue::shared::SharedState;
use crate::shaders::DescriptorSetLayoutBindingDesc;
use crate::wrappers::device::VkDeviceRef;

pub mod buffer;
pub mod image;
pub mod render_pass;
pub mod pipeline;
pub mod descriptor_set;
pub mod sampler;
pub mod descriptor_pool;

pub struct VulkanAllocator {
    device: VkDeviceRef,
    shared_state: SharedState,
    memory_manager: MemoryManager,
    descriptor_set_layouts: HashMap<Vec<DescriptorSetLayoutBindingDesc>, DescriptorSetLayout>,
    descriptor_set_allocator: DescriptorSetAllocator,

    buffers: Vec<Arc<BufferResource>>,
    images: Vec<Arc<ImageResource>>,
    render_passes: Vec<Arc<RenderPassResource>>,
    pipelines: Vec<Arc<GraphicsPipelineResource>>,
    samplers: Vec<Arc<SamplerResource>>,
}

impl VulkanAllocator {
    pub fn allocate_descriptor_set(&mut self, bindings: &'static [DescriptorSetLayoutBindingDesc]) -> Arc<DescriptorSetResource> {
        let layout = self.get_or_create_descriptor_set_layout(bindings);
        let resource = self.descriptor_set_allocator.allocate_descriptor_set(layout, bindings);
        resource
    }

    pub fn new_buffer(&mut self, usage: BufferUsageFlags, flags: BufferCreateFlags, size: DeviceSize) -> Arc<BufferResource> {
        Arc::new(BufferResource::new(&self.device, &mut self.memory_manager, usage, flags, size))
    }

    pub fn new_image(&mut self, usage: ImageUsageFlags, flags: ImageCreateFlags,
                     width: u32, height: u32, format: Format, samples: SampleCountFlags) -> Arc<ImageResource> {
        Arc::new(ImageResource::new(&self.device, &mut self.memory_manager, usage, flags, width, height, format, samples))
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
        let sampler = SamplerResource::new(&self.device, &sampler_info);
        Arc::new(sampler)
    }

    pub fn new_render_pass(&mut self) -> Arc<RenderPassResource> {

    }
    pub fn new_pipeline(&mut self, render_pass: Arc<RenderPassResource>, pipeline_desc: GraphicsPipelineDesc) -> Arc<GraphicsPipelineResource> {
        let descriptor_set_layouts = pipeline_desc.bindings.iter()
            .map(|bindings_desc| self.get_or_create_descriptor_set_layout(bindings_desc))
            .collect();

        Arc::new(GraphicsPipelineResource::new(&self.device, render_pass, pipeline_desc, descriptor_set_layouts))
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
            self.device.create_descriptor_set_layout(&layout_create_info, None).unwrap()
        };

        self.descriptor_set_layouts.insert(key, layout);
        layout
    }


    pub fn dump_resource_usage(&self) {
        let buffer_count = self.buffers.len();
        let image_count = self.images.len();
        let render_pass_count = self.render_passes.len();
        let pipeline_count = self.pipelines.len();
        println!("Resource usage dump:");
        println!("Buffers: {}", buffer_count);
        println!("Images: {}", image_count);
        println!("Render passes: {}", render_pass_count);
        println!("Pipelines: {}", pipeline_count);
    }

    pub fn destroy_old_resources(&mut self) {
        let last_waited = self.shared_state.last_host_waited_submission();

        let mut i = 0;
        while i < self.buffers.len() {
            if self.buffers[i].submission_usage.load().is_none_or(|n| n <= last_waited) && Arc::strong_count(&self.buffers[i]) == 1 {
                let buffer = Arc::into_inner(self.buffers.swap_remove(i)).unwrap();
                destroy_buffer_resource(&self.device, buffer);
            }
            else {
                i += 1;
            }
        }


        let mut i = 0;
        while i < self.images.len() {
            if self.images[i].submission_usage.load().is_none_or(|n| n <= last_waited) && Arc::strong_count(&self.images[i]) == 1 {
                let image = Arc::into_inner(self.images.swap_remove(i)).unwrap();
                destroy_image_resource(&self.device, image);
            }
            else {
                i += 1;
            }
        }

        self.descriptor_set_allocator.on_submission_waited(last_waited);

        let mut i = 0;
        while i < self.pipelines.len() {
            if self.pipelines[i].submission_usage.load().is_none_or(|n| n <= last_waited) && Arc::strong_count(&self.pipelines[i]) == 1 {
                let pipeline = Arc::into_inner(self.pipelines.swap_remove(i)).unwrap();
                destroy_pipeline(&self.device, pipeline);
            }
            else {
                i += 1;
            }
        }

        let mut i = 0;
        while i < self.render_passes.len() {
            if self.render_passes[i].submission_usage.load().is_none_or(|n| n <= last_waited) && Arc::strong_count(&self.render_passes[i]) == 1 {
                let render_pass = Arc::into_inner(self.render_passes.swap_remove(i)).unwrap();
                destroy_render_pass(&self.device, render_pass);
            }
            else {
                i += 1;
            }
        }
    }
}

impl Drop for VulkanAllocator {
    fn drop(&mut self) {
        self.destroy_old_resources();

        for (_, descriptor_set_layout) in self.descriptor_set_layouts.drain() {
            unsafe {
                self.device.destroy_descriptor_set_layout(descriptor_set_layout, None);
            }
        }

        // check if something left not allocated
    }
}

/// Event of specific resource usage
#[derive(Copy, Clone, Debug, Default)]
pub struct ResourceUsage {
    pub submission_num: Option<usize>,
    pub stage_flags: PipelineStageFlags,
    pub access_flags: AccessFlags,
}

impl ResourceUsage {
    pub fn new(submission_num: Option<usize>, stage_flags: PipelineStageFlags, access_flags: AccessFlags) -> Self {
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

#[derive(Clone, Debug)]
pub enum LastResourceUsage {
    HasWrite {
        last_write: Option<ResourceUsage>,
        visible_for: AccessFlags,
    },
    None
}

#[derive(Copy, Clone, Debug, Default)]
pub struct RequiredSync {
    pub src_stages: PipelineStageFlags,
    pub dst_stages: PipelineStageFlags,
    pub src_access: AccessFlags,
    pub dst_access: AccessFlags,
}

impl LastResourceUsage {
    pub fn new() -> Self {
        Self::None
    }

    pub fn on_host_waited(&mut self, last_waited_num: usize, had_host_writes: bool) {
        if let Self::HasWrite{ last_write, visible_for } = self
            && let Some(last_write_fr) = last_write
            && let Some(submission_num) = last_write_fr.submission_num
            && last_waited_num >= submission_num {

            *last_write = None;
            if had_host_writes {
                *visible_for = AccessFlags::empty();
            }
        }
        else {
            if self.is_none() {
                *self = Self::HasWrite {
                    last_write: None,
                    visible_for: AccessFlags::empty(),
                }
            }
        }
    }

    /// Add new usage, returning previous usage if a sync barrier is needed.
    /// Returns Some(previous_usage) if we need synchronization, None if no sync needed.
    pub fn add_usage(&mut self, new_usage: ResourceUsage) -> Option<RequiredSync> {
        if let Self::HasWrite {
            last_write,
            visible_for,
        } = self {
            let need_visible = new_usage.access_flags & !*visible_for;
            if let Some(last_write_fr) = last_write {
                let required_sync = RequiredSync {
                    src_stages: last_write_fr.stage_flags,
                    src_access: last_write_fr.access_flags,

                    dst_stages: new_usage.stage_flags,
                    dst_access: need_visible,
                };

                // Update visible_for
                if new_usage.is_readonly() {
                    *last_write = None;
                    *visible_for |= new_usage.access_flags;
                }
                else {
                    // Save new write
                    *last_write_fr = new_usage;
                    *visible_for = AccessFlags::empty();
                }
                Some(required_sync)
            }
            else {
                if !new_usage.is_readonly() {
                    *last_write = Some(new_usage);
                    *visible_for = AccessFlags::empty();
                }
                if !need_visible.is_empty() {
                    // Need sync for new read usages
                    let required_sync = RequiredSync {
                        src_stages: PipelineStageFlags::empty(),
                        src_access: AccessFlags::empty(),

                        dst_stages: new_usage.stage_flags,
                        dst_access: need_visible,
                    };

                    if new_usage.is_readonly() {
                        *visible_for |= new_usage.access_flags;
                    }
                    return Some(required_sync);
                }

                None
            }
        }
        else {
            if !new_usage.is_readonly() {
                *self = LastResourceUsage::HasWrite {
                    last_write: Some(new_usage),
                    visible_for: AccessFlags::empty(),
                };
            }

            None
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

