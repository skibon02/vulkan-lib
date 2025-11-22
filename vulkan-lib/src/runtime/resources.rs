use std::sync::atomic::AtomicBool;
use ash::vk;
use ash::vk::{AccessFlags, AttachmentLoadOp, Buffer, BufferCreateFlags, BufferCreateInfo, BufferUsageFlags, DeviceMemory, DeviceSize, Extent3D, Format, Framebuffer, Image, ImageCreateFlags, ImageCreateInfo, ImageLayout, ImageTiling, ImageType, ImageUsageFlags, MemoryAllocateInfo, PipelineBindPoint, PipelineStageFlags, RenderPass, SampleCountFlags};
use slotmap::{DefaultKey, SlotMap};
use smallvec::SmallVec;
use sparkles::range_event_start;
use crate::runtime::{OptionSeqNumShared, SharedState};
use buffers::BufferResource;
use pipeline::GraphicsPipelineInner;
use render_pass::RenderPassHandle;
use crate::runtime::resources::images::{ImageResource, ImageResourceHandle};
use crate::runtime::resources::render_pass::RenderPassResource;
use crate::wrappers::device::VkDeviceRef;

pub mod pipeline;
pub mod buffers;
pub mod images;
pub mod render_pass;

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

pub(crate) struct RenderPassInner {
    render_pass: RenderPass,
    framebuffers: SmallVec<[Framebuffer; 5]>,
    pub last_used_in: usize,
}

pub(crate) struct ResourceStorage {
    device: VkDeviceRef,
    buffers: SlotMap<DefaultKey, BufferInner>,
    images: SlotMap<DefaultKey, ImageInner>,
    render_passes: SlotMap<DefaultKey, RenderPassInner>,
    pipelines: SlotMap<DefaultKey, GraphicsPipelineInner>,
}

impl ResourceStorage {
    pub fn new(device: VkDeviceRef) -> Self{
        Self {
            device,
            buffers: SlotMap::new(),
            images: SlotMap::new(),
            pipelines: SlotMap::new(),
            render_passes: SlotMap::new(),
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

    /// Insert an externally created image
    pub fn add_image(&mut self, image: ImageInner) -> DefaultKey {
        self.images.insert(image)
    }

    pub fn create_image(&mut self, usage: ImageUsageFlags, flags: ImageCreateFlags, memory_type: u32, width: u32, height: u32, format: Format, samples: SampleCountFlags, shared_state: SharedState) -> ImageResource {
        // create image
        let image = unsafe {
            self.device.create_image(&ImageCreateInfo::default()
                .usage(usage)
                .flags(flags)
                .extent(Extent3D {
                    width,
                    height,
                    depth: 1
                })
                .tiling(ImageTiling::OPTIMAL)
                .array_layers(1)
                .mip_levels(1)
                .image_type(ImageType::TYPE_2D)
                .initial_layout(ImageLayout::UNDEFINED)
                .format(format)
                .samples(samples)
                                     , None).unwrap()
        };
        let memory_requirements = unsafe { self.device.get_image_memory_requirements(image) };
        let allocation_size = memory_requirements.size;

        //allocate memory
        let memory = unsafe {
            self.device.allocate_memory(&MemoryAllocateInfo::default()
                .allocation_size(allocation_size)
                .memory_type_index(memory_type),
                                        None).unwrap() };

        unsafe {
            self.device.bind_image_memory(image, memory, 0).unwrap();
        }


        let image_inner = ImageInner {
            image,
            usages: ResourceUsages::new(),
            memory: Some(memory),
            layout: ImageLayout::UNDEFINED,
            format,
        };
        let state_key = self.images.insert(image_inner);
        ImageResource::new(shared_state, state_key, memory, width, height)
    }
    pub fn image(&mut self, key: DefaultKey) -> &mut ImageInner {
        self.images.get_mut(key).unwrap()
    }
    pub fn destroy_image(&mut self, handle: ImageResourceHandle) {
        if let Some(image_inner) = self.images.remove(handle.state_key) {
            if let Some(memory) = image_inner.memory {
                unsafe {
                    self.device.destroy_image(image_inner.image, None);
                    self.device.free_memory(memory, None);
                }
            }
        }
    }

    pub fn create_render_pass(&mut self, device: VkDeviceRef, surface_format: Format, msaa_samples: Option<SampleCountFlags>, shared: SharedState, swapchain_images: impl Iterator<Item=ImageResourceHandle>) -> RenderPassResource {
        let g = range_event_start!("Create render pass");

        let intermediate_sample_count = msaa_samples.unwrap_or(SampleCountFlags::TYPE_1);
        let render_pass = {

            let load_op = if msaa_samples.is_some() {
                AttachmentLoadOp::DONT_CARE
            } else {
                AttachmentLoadOp::CLEAR
            };
            let attachments = [
                // 0. final color attachment (resolve attachment)
                vk::AttachmentDescription::default()
                    .format(surface_format)
                    .samples(SampleCountFlags::TYPE_1)
                    .load_op(load_op)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .initial_layout(vk::ImageLayout::UNDEFINED)
                    .final_layout(vk::ImageLayout::PRESENT_SRC_KHR),

                // 1. depth attachment
                vk::AttachmentDescription::default()
                    .format(Format::D16_UNORM)
                    .samples(intermediate_sample_count)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .initial_layout(vk::ImageLayout::UNDEFINED)
                    .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),

                // 2. Color attachment
                vk::AttachmentDescription::default()
                    .format(surface_format)
                    .samples(intermediate_sample_count)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .initial_layout(vk::ImageLayout::UNDEFINED)
                    .final_layout(vk::ImageLayout::PRESENT_SRC_KHR),
            ];

            let resolve_attachment_i = 0;
            let color_attachment_i = if msaa_samples.is_some() {
                2
            }
            else {
                0
            };

            let color_attachment_refs = [vk::AttachmentReference::default()
                .attachment(color_attachment_i)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
            let depth_attachment_ref = vk::AttachmentReference::default()
                .attachment(1)
                .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
            let resolve_attachment_ref = [vk::AttachmentReference::default()
                .attachment(resolve_attachment_i)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];

            let mut subpasses = [vk::SubpassDescription::default()
                .pipeline_bind_point(PipelineBindPoint::GRAPHICS)
                .color_attachments(&color_attachment_refs)
                .depth_stencil_attachment(&depth_attachment_ref)];
            if msaa_samples.is_some() {
                subpasses[0] = subpasses[0].resolve_attachments(&resolve_attachment_ref);
            }
            let dependencies = [vk::SubpassDependency::default()
                .src_subpass(vk::SUBPASS_EXTERNAL)
                .dst_subpass(0)
                .src_stage_mask(PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | PipelineStageFlags::EARLY_FRAGMENT_TESTS)
                .src_access_mask(AccessFlags::empty())
                .dst_stage_mask(PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | PipelineStageFlags::EARLY_FRAGMENT_TESTS)
                .dst_access_mask(AccessFlags::COLOR_ATTACHMENT_WRITE | AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)];

            let render_pass_create_info =
                vk::RenderPassCreateInfo::default()
                    .subpasses(&subpasses)
                    .dependencies(&dependencies);
            if msaa_samples.is_some() {
                let render_pass_create_info = render_pass_create_info.attachments(&attachments);
                unsafe { device.create_render_pass(&render_pass_create_info, None).unwrap() }
            }
            else {
                let render_pass_create_info = render_pass_create_info.attachments(&attachments[..2]);
                unsafe { device.create_render_pass(&render_pass_create_info, None).unwrap() }
            }
        };
        let framebuffers = ;

        let render_pass_inner = RenderPassInner {
            render_pass,
            framebuffers,
            last_used_in: 0,
        };
        let key = self.render_passes.insert(render_pass_inner);

        RenderPassResource::new(key, shared)
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