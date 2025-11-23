use std::mem;
use std::sync::atomic::AtomicBool;
use ash::vk;
use ash::vk::{AccessFlags, AttachmentDescription, AttachmentLoadOp, AttachmentStoreOp, Buffer, BufferCreateFlags, BufferCreateInfo, BufferUsageFlags, DeviceMemory, DeviceSize, Extent3D, Format, Framebuffer, Image, ImageCreateFlags, ImageCreateInfo, ImageLayout, ImageTiling, ImageType, ImageUsageFlags, ImageView, MemoryAllocateInfo, MemoryHeap, MemoryType, Pipeline, PipelineBindPoint, PipelineLayout, PipelineStageFlags, RenderPass, SampleCountFlags};
use slotmap::{DefaultKey, SlotMap};
use smallvec::{smallvec, SmallVec};
use sparkles::range_event_start;
use crate::runtime::{OptionSeqNumShared, SharedState};
use crate::runtime::shared::ScheduledForDestroy;
use buffers::BufferResource;
use render_pass::RenderPassHandle;
use crate::runtime::resources::images::{ImageResource, ImageResourceHandle};
use crate::runtime::resources::render_pass::RenderPassResource;
use crate::runtime::memory_manager::{MemoryManager, MemoryTypeAlgorithm};
use crate::runtime::resources::pipeline::{create_graphics_pipeline, GraphicsPipeline, GraphicsPipelineDesc};
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
    pub image_view: Option<ImageView>,
    pub memory: Option<DeviceMemory>,
    pub usages: ResourceUsages,
    pub layout: ImageLayout,
    pub format: Format,
}

impl ImageInner {
    pub fn get_aspect_flags(&self) -> vk::ImageAspectFlags {
        match self.format {
            Format::D16_UNORM | Format::D32_SFLOAT => vk::ImageAspectFlags::DEPTH,
            Format::S8_UINT => vk::ImageAspectFlags::STENCIL,
            Format::D24_UNORM_S8_UINT | Format::D32_SFLOAT_S8_UINT => vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL,
            _ => vk::ImageAspectFlags::COLOR,
        }
    }
}

pub(crate) struct RenderPassInner {
    attachments_description: AttachmentsDescription,
    render_pass: RenderPass,
    framebuffers: SmallVec<[(Framebuffer, SmallVec<[ImageResource; 5]>); 5]>,
    pub last_used_in: usize,
}

pub(crate) struct GraphicsPipelineInner {
    pipeline: Pipeline, // must not be used in command buffer during destruction (lazy destroy)
    pipeline_layout: PipelineLayout, // vkCmdBindDescriptorSets must not be recorded to any command buffer during destruction (lazy destroy)
}


pub(crate) struct ResourceStorage {
    device: VkDeviceRef,
    memory_manager: MemoryManager,
    buffers: SlotMap<DefaultKey, BufferInner>,
    images: SlotMap<DefaultKey, ImageInner>,
    render_passes: SlotMap<DefaultKey, RenderPassInner>,
    pipelines: SlotMap<DefaultKey, GraphicsPipelineInner>,
}

pub struct AttachmentsDescription {
    color_attachment_desc: AttachmentDescription,
    depth_attachment_desc: Option<AttachmentDescription>,
    resolve_attachment_desc: Option<AttachmentDescription>,
}

impl AttachmentsDescription {
    pub fn new(color_attachment_desc: AttachmentDescription) -> Self {
        Self {
            color_attachment_desc,
            depth_attachment_desc: None,
            resolve_attachment_desc: None,
        }
    }

    pub fn with_depth_attachment(mut self, depth_attachment_desc: AttachmentDescription) -> Self {
        self.depth_attachment_desc = Some(depth_attachment_desc);
        self
    }

    pub fn with_resolve_attachment(mut self, resolve_attachment_desc: AttachmentDescription) -> Self {
        self.resolve_attachment_desc = Some(resolve_attachment_desc);
        self
    }

    pub fn fill_defaults(&mut self, swapchain_format: Format) {
        self.color_attachment_desc.format = swapchain_format;
        self.color_attachment_desc.load_op = AttachmentLoadOp::CLEAR;
        self.color_attachment_desc.store_op = AttachmentStoreOp::STORE;
        if let Some(depth_attachment) = &mut self.depth_attachment_desc {
            depth_attachment.stencil_load_op = AttachmentLoadOp::DONT_CARE;
            depth_attachment.stencil_store_op = AttachmentStoreOp::DONT_CARE;
            depth_attachment.load_op = AttachmentLoadOp::CLEAR;
            depth_attachment.store_op = AttachmentStoreOp::DONT_CARE;
        }
        if let Some(resolve_attachment) = &mut self.resolve_attachment_desc {
            resolve_attachment.format = swapchain_format;
            resolve_attachment.load_op = AttachmentLoadOp::DONT_CARE;
            resolve_attachment.store_op = AttachmentStoreOp::STORE;
        }
    }
}

impl ResourceStorage {
    pub fn new(device: VkDeviceRef, memory_types: Vec<MemoryType>, memory_heaps: Vec<MemoryHeap>) -> Self{
        let memory_manager = MemoryManager::new(device.clone(), memory_types, memory_heaps);
        Self {
            device,
            memory_manager,
            buffers: SlotMap::new(),
            images: SlotMap::new(),
            pipelines: SlotMap::new(),
            render_passes: SlotMap::new(),
        }
    }

    pub fn memory_manager(&mut self) -> &mut MemoryManager {
        &mut self.memory_manager
    }

    pub fn create_buffer(&mut self, usage: BufferUsageFlags, flags: BufferCreateFlags, size: DeviceSize, algorithm: MemoryTypeAlgorithm, shared: SharedState) -> (BufferResource, DeviceMemory) {
        let (_, memory_type_bits) = self.memory_manager.get_buffer_memory_requirements(usage, flags);
        let memory_type = self.memory_manager.select_memory_type(memory_type_bits, algorithm);

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
    pub fn add_image(&mut self, image: Image, format: Format, width: u32, height: u32) -> ImageResourceHandle {
        let image_inner = ImageInner {
            image,
            image_view: None,
            usages: ResourceUsages::new(),
            memory: None,
            layout: ImageLayout::UNDEFINED,
            format,
        };
        let state_key = self.images.insert(image_inner);
        ImageResourceHandle {
            state_key,
            width,
            height,
        }
    }

    pub fn create_image(&mut self, usage: ImageUsageFlags, flags: ImageCreateFlags, algorithm: MemoryTypeAlgorithm, width: u32, height: u32, format: Format, samples: SampleCountFlags, shared_state: SharedState) -> ImageResource {
        let memory_type_bits = self.memory_manager.get_image_memory_requirements(format, ImageTiling::OPTIMAL, usage, flags);
        let memory_type = self.memory_manager.select_memory_type(memory_type_bits, algorithm);

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
                .samples(samples),
            None).unwrap()
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
            image_view: None,
            usages: ResourceUsages::new(),
            memory: Some(memory),
            layout: ImageLayout::UNDEFINED,
            format,
        };
        let state_key = self.images.insert(image_inner);
        ImageResource::new(shared_state, state_key, memory, width, height)
    }
    fn create_image_view(&mut self, key: DefaultKey) -> ImageView {
        let image_inner = self.images.get_mut(key).unwrap();
        let image_view_create_info = vk::ImageViewCreateInfo::default()
            .image(image_inner.image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(image_inner.format)
            .subresource_range(vk::ImageSubresourceRange::default()
                .aspect_mask(image_inner.get_aspect_flags())
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1));
        let image_view = unsafe {
            self.device.create_image_view(&image_view_create_info, None).unwrap()
        };

        image_inner.image_view = Some(image_view);
        image_view
    }
    pub fn image(&mut self, key: DefaultKey) -> &mut ImageInner {
        self.images.get_mut(key).unwrap()
    }
    pub fn image_view(&mut self, key: DefaultKey) -> ImageView {
        if let Some(iv) = self.images.get(key).unwrap().image_view {
            return iv;
        }

        let image_view = self.create_image_view(key);
        image_view
    }
    pub fn destroy_image(&mut self, handle: ImageResourceHandle) {
        if let Some(image_inner) = self.images.remove(handle.state_key) {
            if let Some(memory) = image_inner.memory {
                unsafe {
                    self.device.destroy_image(image_inner.image, None);
                    self.device.free_memory(memory, None);
                }
            }
            if let Some(image_view) = image_inner.image_view {
                unsafe {
                    self.device.destroy_image_view(image_view, None);
                }
            }
        }
    }

    fn create_framebuffers(&mut self, device: VkDeviceRef, render_pass: RenderPass, swapchain_images: &SmallVec<[ImageResourceHandle; 3]>,
                           swapchain_extent: Extent3D, attachments: &SmallVec<[AttachmentDescription; 5]>, swapchain_format: Format, shared: SharedState) -> SmallVec<[(Framebuffer, SmallVec<[ImageResource; 5]>); 5]> {
        let mut framebuffers = smallvec![];
        for swapchain_image in swapchain_images {
            let mut owned_images: SmallVec<[ImageResource; 5]> = smallvec![];
            for attachment_image in attachments.iter().skip(1) {
                let usage = if attachment_image.format == swapchain_format {
                    ImageUsageFlags::COLOR_ATTACHMENT | ImageUsageFlags::TRANSIENT_ATTACHMENT
                } else {
                    ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | ImageUsageFlags::TRANSIENT_ATTACHMENT
                };

                let image_resource = self.create_image(
                    usage,
                    ImageCreateFlags::empty(),
                    MemoryTypeAlgorithm::Device,
                    swapchain_extent.width,
                    swapchain_extent.height,
                    attachment_image.format,
                    attachment_image.samples,
                    shared.clone(),
                );
                self.image_view(image_resource.handle().state_key); // create image view
                owned_images.push(image_resource);
            }

            let mut views: SmallVec<[ImageView; 5]> = smallvec![];
            views.push(self.image_view(swapchain_image.state_key));
            for owned_image in owned_images.iter() {
                views.push(self.image_view(owned_image.handle().state_key));
            }
            let framebuffer_create_info = vk::FramebufferCreateInfo::default()
                .render_pass(render_pass)
                .attachments(&views)
                .width(swapchain_extent.width)
                .height(swapchain_extent.height)
                .layers(1);
            let framebuffer = unsafe {
                device.create_framebuffer(&framebuffer_create_info, None).unwrap()
            };

            framebuffers.push((framebuffer, owned_images));
        }

        framebuffers
    }

    /// Index 0 is always target swapchain 1 layer color attachment.
    pub fn create_render_pass(&mut self, device: VkDeviceRef, shared: SharedState,
                              swapchain_images: SmallVec<[ImageResourceHandle; 3]>, mut attachments_description: AttachmentsDescription) -> RenderPassResource {
        let g = range_event_start!("Create render pass");

        let swapchain_format = self.image(swapchain_images[0].state_key).format;

        attachments_description.fill_defaults(swapchain_format);
        let mut attachments: SmallVec<[AttachmentDescription; 5]> = smallvec![attachments_description.color_attachment_desc];
        let mut attachment_i = 1;
        let mut subpass = vk::SubpassDescription::default()
            .pipeline_bind_point(PipelineBindPoint::GRAPHICS);

        let depth_attachment_ref;
        if let Some(attachment) = attachments_description.depth_attachment_desc {
            attachments.push(attachment);
            depth_attachment_ref = vk::AttachmentReference::default()
                .attachment(attachment_i)
                .layout(ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
            subpass = subpass.depth_stencil_attachment(&depth_attachment_ref);
            attachment_i += 1;
        }
        let color_attachment_refs;
        let resolve_attachment_refs;
        if let Some(attachment) = attachments_description.resolve_attachment_desc {
            attachments.push(attachment);
            resolve_attachment_refs = [vk::AttachmentReference::default()
                .attachment(0)
                .layout(ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];

            color_attachment_refs = [vk::AttachmentReference::default()
                .attachment(attachment_i)
                .layout(ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
            subpass = subpass.resolve_attachments(&resolve_attachment_refs);
            subpass = subpass.color_attachments(&color_attachment_refs);
            attachment_i += 1;
        }
        else {
            color_attachment_refs = [vk::AttachmentReference::default()
                .attachment(0)
                .layout(ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];

            subpass = subpass.color_attachments(&color_attachment_refs);
        }

        let dependencies = [vk::SubpassDependency::default()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | PipelineStageFlags::EARLY_FRAGMENT_TESTS)
            .src_access_mask(AccessFlags::empty())
            .dst_stage_mask(PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | PipelineStageFlags::EARLY_FRAGMENT_TESTS)
            .dst_access_mask(AccessFlags::COLOR_ATTACHMENT_WRITE | AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)];

        let subpasses = [subpass];
        let render_pass_create_info =
            vk::RenderPassCreateInfo::default()
                .subpasses(&subpasses)
                .dependencies(&dependencies);
        let render_pass_create_info = render_pass_create_info.attachments(&attachments);
        let render_pass = unsafe { device.create_render_pass(&render_pass_create_info, None).unwrap() };

        // Create images for framebuffer and framebuffers
        let swapchain_extent = vk::Extent2D {
            width: swapchain_images[0].width,
            height: swapchain_images[0].height,
        };

        // Create images and image views for attachments (except swapchain image)
        let framebuffers = self.create_framebuffers(
            device.clone(),
            render_pass,
            &swapchain_images,
            Extent3D {
                width: swapchain_extent.width,
                height: swapchain_extent.height,
                depth: 1,
            },
            &attachments,
            swapchain_format,
            shared.clone(),
        );

        let render_pass_inner = RenderPassInner {
            attachments_description,
            render_pass,
            framebuffers,
            last_used_in: 0,
        };
        let key = self.render_passes.insert(render_pass_inner);

        RenderPassResource::new(key, shared)
    }
    pub fn render_passes(&self) -> SmallVec<[RenderPassHandle; 5]> {
        self.render_passes.keys().map(|k| RenderPassHandle(k)).collect()
    }
    pub fn destroy_render_pass_resources(&mut self, render_pass_handle: RenderPassHandle, shared: SharedState) {
        let render_pass_inner = self.render_passes.get_mut(render_pass_handle.0).unwrap();
        let used_in = render_pass_inner.last_used_in;
        for (framebuffer, owned_images) in mem::take(&mut render_pass_inner.framebuffers) {
            for image in owned_images {
                self.destroy_image(image.handle());
            }
            shared.schedule_destroy_framebuffer(framebuffer, used_in);
        }
    }
    pub fn recreate_render_pass_resources(&mut self, render_pass_handle: RenderPassHandle, device: VkDeviceRef, shared: SharedState,
                                          swapchain_images: &SmallVec<[ImageResourceHandle; 3]>) {
        let render_pass_inner = self.render_passes.get_mut(render_pass_handle.0).unwrap();
        assert!(render_pass_inner.framebuffers.is_empty(), "Render pass resources must be destroyed using `destroy_render_pass_resources` before recreation");

        let render_pass = render_pass_inner.render_pass;
        let attachments_description = &render_pass_inner.attachments_description;
        let mut attachments = SmallVec::<[AttachmentDescription; 5]>::new();
        attachments.push(attachments_description.color_attachment_desc);
        if let Some(depth_attachment) = &attachments_description.depth_attachment_desc {
            attachments.push(*depth_attachment);
        }
        if let Some(resolve_attachment) = &attachments_description.resolve_attachment_desc {
            attachments.push(*resolve_attachment);
        }
        let swapchain_format = self.image(swapchain_images[0].state_key).format;

        let framebuffers = self.create_framebuffers(
            device.clone(),
            render_pass,
            swapchain_images,
            Extent3D {
                width: swapchain_images[0].width,
                height: swapchain_images[0].height,
                depth: 1,
            },
            &attachments,
            swapchain_format,
            shared.clone(),
        );

        let render_pass_inner = self.render_passes.get_mut(render_pass_handle.0).unwrap();
        render_pass_inner.framebuffers = framebuffers;
    }

    pub fn destroy_render_pass(&mut self, render_pass_handle: RenderPassHandle) {
        if let Some(render_pass_inner) = self.render_passes.remove(render_pass_handle.0) {
            unsafe {
                for (framebuffer, _) in render_pass_inner.framebuffers {
                    self.device.destroy_framebuffer(framebuffer, None);
                }
                self.device.destroy_render_pass(render_pass_inner.render_pass, None);
            }
        }
    }

    pub fn create_graphics_pipeline(&mut self, render_pass_handle: RenderPassHandle, desc: GraphicsPipelineDesc, shared: SharedState) -> GraphicsPipeline {
        let render_pass = self.render_passes.get(render_pass_handle.0).unwrap().render_pass;
        let (inner, mut handle) = create_graphics_pipeline(self.device.clone(), render_pass, desc);
        let key = self.pipelines.insert(inner);
        handle.key = key;
        GraphicsPipeline {
            shared,
            handle
        }
    }
    pub fn pipeline(&mut self, key: DefaultKey) -> &mut GraphicsPipelineInner {
        self.pipelines.get_mut(key).unwrap()
    }
    pub fn destroy_pipeline(&mut self, key: DefaultKey) {
        if let Some(pipeline) = self.pipelines.remove(key) {
            unsafe {
                self.device.destroy_pipeline(pipeline.pipeline, None);
                self.device.destroy_pipeline_layout(pipeline.pipeline_layout, None);
            }
        }
    }

    pub fn destroy_scheduled_resources(&mut self, scheduled: ScheduledForDestroy) {
        for (buffer_handle, _) in scheduled.buffers {
            self.destroy_buffer(buffer_handle.state_key);
        }

        for (image_handle, _) in scheduled.images {
            self.destroy_image(image_handle);
        }

        for (pipeline_handle, _) in scheduled.pipelines {
            self.destroy_pipeline(pipeline_handle.key);
        }

        for (render_pass_handle, _) in scheduled.render_passes {
            self.destroy_render_pass(render_pass_handle);
        }

        unsafe {
            for (framebuffer, _) in scheduled.framebuffers {
                self.device.destroy_framebuffer(framebuffer, None);
            }
        }
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
}

impl Drop for ResourceStorage {
    fn drop(&mut self) {
        unsafe {
            // render pass may own images, but they will be destroyed below
            for (_, render_pass_inner) in self.render_passes.drain() {
                for (framebuffer, _) in render_pass_inner.framebuffers {
                    self.device.destroy_framebuffer(framebuffer, None);
                }
                self.device.destroy_render_pass(render_pass_inner.render_pass, None);
            }
            
            for (_, pipeline_inner) in self.pipelines.drain() {
                self.device.destroy_pipeline(pipeline_inner.pipeline, None);
                self.device.destroy_pipeline_layout(pipeline_inner.pipeline_layout, None);
            }

            for (_, buffer_inner) in self.buffers.drain() {
                self.device.destroy_buffer(buffer_inner.buffer, None);
                self.device.free_memory(buffer_inner.memory, None);
            }

            for (_, image_inner) in self.images.drain() {
                if let Some(memory) = image_inner.memory {
                    self.device.destroy_image(image_inner.image, None);
                    self.device.free_memory(memory, None);
                }
                if let Some(image_view) = image_inner.image_view {
                    self.device.destroy_image_view(image_view, None);
                }
            }
        }
    }
}