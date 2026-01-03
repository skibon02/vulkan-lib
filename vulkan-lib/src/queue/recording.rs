use strum::EnumDiscriminants;
use std::collections::HashMap;
use std::{iter, mem};
use std::ops::{Deref, DerefMut, Range};
use std::sync::Arc;
use smallvec::{smallvec, SmallVec};
use ash::vk::{self, AccessFlags, BufferCopy, BufferImageCopy, ClearValue, DeviceSize, Format, ImageAspectFlags, ImageLayout, PipelineStageFlags};
use log::{error, warn};
use crate::queue::{FramebufferSet, OptionSeqNumShared};
use crate::queue::queue_local::QueueLocal;
use crate::resources::buffer::{BufferResource, BufferResourceInner};
use crate::resources::descriptor_set::{BoundResource, DescriptorSetResource};
use crate::resources::image::ImageResource;
use crate::resources::pipeline::GraphicsPipelineResource;
use crate::resources::render_pass::RenderPassResource;
use crate::resources::ResourceUsage;
use crate::resources::staging_buffer::{StagingBuffer, StagingBufferRange};
use crate::swapchain_wrapper::SwapchainImages;

pub(crate) enum AnyBufferRange {
    Staging(StagingBufferRange),
    Device(BufferRange),
}

impl From<StagingBufferRange> for AnyBufferRange {
    fn from(range: StagingBufferRange) -> Self {
        AnyBufferRange::Staging(range)
    }
}

impl From<BufferRange> for AnyBufferRange {
    fn from(range: BufferRange) -> Self {
        AnyBufferRange::Device(range)
    }
}

impl AnyBufferRange {
    pub fn buffer(&self) -> vk::Buffer {
        match self {
            AnyBufferRange::Staging(s) => s.buffer.buffer,
            AnyBufferRange::Device(d) => d.buffer.buffer,
        }
    }

    pub fn buffer_size(&self) -> u64 {
        match self {
            AnyBufferRange::Staging(s) => s.buffer.size() as u64,
            AnyBufferRange::Device(s) => s.buffer.size() as u64,
        }
    }

    pub fn offset(&self) -> u64 {
        match self {
            AnyBufferRange::Staging(s) => s.range.start,
            AnyBufferRange::Device(d) => d.custom_range.as_ref()
                .map(|r| r.start as u64)
                .unwrap_or(0),
        }
    }

    pub fn size(&self) -> u64 {
        match self {
            AnyBufferRange::Staging(s) => s.range.end - s.range.start,
            AnyBufferRange::Device(d) => d.custom_range.as_ref()
                .map(|r| (r.end - r.start) as u64)
                .unwrap_or(d.buffer.size() as u64),
        }
    }
    
    pub fn has_host_writes(&self) -> bool {
        matches!(self, AnyBufferRange::Staging(_))
    }
    
    pub fn submission_usage(&self) -> &OptionSeqNumShared {
        match self {
            AnyBufferRange::Staging(s) => &s.buffer.submission_usage,
            AnyBufferRange::Device(d) => &d.buffer.submission_usage,
        }
    }

    pub(crate) fn into_any_buffer(self) -> AnyBuffer {
        match self {
            AnyBufferRange::Staging(s) => AnyBuffer::Staging(s.buffer),
            AnyBufferRange::Device(d) => AnyBuffer::Device(d.buffer),
        }
    }
}

#[derive(Clone)]
pub struct BufferRange {
    pub(crate) buffer: Arc<BufferResource>,
    pub(crate) custom_range: Option<Range<usize>>
}

fn prepare_buffer_copy(src: &AnyBufferRange, dst: &BufferRange) -> BufferCopy {
    let src_offset = src.offset();

    let dst_offset = dst.custom_range.as_ref()
        .map(|r| r.start)
        .unwrap_or(0) as DeviceSize;

    let src_size = src.size();

    let dst_size = dst.custom_range.as_ref()
        .map(|r| r.end - r.start)
        .unwrap_or(dst.buffer.size()) as DeviceSize;

    let size = src_size.min(dst_size);

    if src.buffer_size() != src.size() && dst.custom_range.is_some() && src_size != dst_size {
        warn!(
            "BufferCopy: custom ranges for both buffers, but sizes differ ({} vs {}). Using smallest: {}",
            src_size, dst_size, size
        );
    }

    BufferCopy::default()
        .src_offset(src_offset)
        .dst_offset(dst_offset)
        .size(size)
}

pub struct RecordContext {
    commands: Vec<DeviceCommand>,
    bound_pipeline: Option<Arc<GraphicsPipelineResource>>,
    pipeline_changed: bool,
    bound_descriptor_sets: HashMap<u32, Arc<DescriptorSetResource>>,
    bound_vertex_buffer: Option<BufferRange>
}

impl RecordContext {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            bound_pipeline: None,
            pipeline_changed: false,
            bound_vertex_buffer: None,
            bound_descriptor_sets: HashMap::new(),
        }
    }

    pub fn bind_pipeline(&mut self, pipeline: Arc<GraphicsPipelineResource>) {
        self.bound_pipeline = Some(pipeline);
        self.pipeline_changed = true;
    }

    pub fn bind_descriptor_set(&mut self, set: u32, descriptor_set: Arc<DescriptorSetResource>) {
        descriptor_set.lock_updates();
        if let Some(prev) = self.bound_descriptor_sets.insert(set, descriptor_set) {
            prev.unlock_updates();
        }
    }

    pub fn bind_vertex_buffer(&mut self, buf: BufferRange) {
        self.bound_vertex_buffer = Some(buf);
    }

    pub fn copy_buffer(&mut self, src: impl Into<AnyBufferRange>, dst: BufferRange) {
        let src = src.into();
        let region = prepare_buffer_copy(&src, &dst);
        let regions = smallvec![region];
        self.commands.push(DeviceCommand::CopyBuffer {
            src,
            dst: dst.buffer,
            regions
        })
    }

    /// Copy data from buffer range to full contents of the image.
    /// Safety:
    /// - bytes per texel must correctly represent texel block size in bytes.
    /// - Texel block size of image format must be 1x1
    pub fn copy_buffer_to_image_full(&mut self, src: impl Into<AnyBufferRange>, dst: Arc<ImageResource>, bytes_per_texel: usize) {
        let src = src.into();
        let buffer_offset = src.offset();

        let extent = dst.extent();
        let image_size_bytes = (extent.width * extent.height) as usize * bytes_per_texel;

        let buffer_size = src.size();

        if buffer_size < image_size_bytes as u64 {
            error!(
                "Buffer range size ({} bytes) is too small for image ({}x{} with {} bytes/texel = {} bytes). Copy may fail!",
                buffer_size, extent.width, extent.height, bytes_per_texel, image_size_bytes
            );
        }

        let region = BufferImageCopy::default()
            .buffer_offset(buffer_offset)
            .image_subresource(vk::ImageSubresourceLayers::default()
                .aspect_mask(dst.get_aspect_flags())
                .layer_count(1))
            .image_extent(vk::Extent3D {
                width: extent.width,
                height: extent.height,
                depth: 1
            });

        let regions = smallvec![region];
        self.commands.push(DeviceCommand::CopyBufferToImage {
            src,
            dst,
            regions
        })
    }

    pub fn fill_buffer(&mut self, buffer: BufferRange, data: u32) {
        let (offset, size) = if let Some(range) = buffer.custom_range {
            (range.start, range.end - range.start)
        }
        else {
            (0, buffer.buffer.size())
        };
        self.commands.push(DeviceCommand::FillBuffer {
            buffer: buffer.buffer,
            offset: offset as u64,
            size: size as u64,
            data,
        })
    }

    pub fn transition_image_layout(&mut self, image: Arc<ImageResource>, new_layout: ImageLayout, image_aspect: ImageAspectFlags) {
        self.commands.push(DeviceCommand::ImageLayoutTransition {
            image,
            new_layout,
            image_aspect,
        })
    }
    
    pub fn clear_color_image(&mut self, image: Arc<ImageResource>, clear_color: ash::vk::ClearColorValue, image_aspect: ImageAspectFlags) {
        self.commands.push(DeviceCommand::ClearColorImage {
            image,
            clear_color,
            image_aspect,
        })
    }
    
    pub fn clear_depth_stencil_image(&mut self, image: Arc<ImageResource>, depth_value: Option<f32>, stencil_value: Option<u32>) {
        self.commands.push(DeviceCommand::ClearDepthStencilImage {
            image,
            depth_value,
            stencil_value,
        })
    }

    pub fn barrier(&mut self) {
        self.commands.push(DeviceCommand::Barrier)
    }

    pub fn render_pass<F>(&mut self, render_pass: Arc<RenderPassResource>, framebuffer_index: u32, clear_values: SmallVec<[ClearValue; 3]>, f: F)
    where
        F: FnOnce(&mut RenderPassContext<'_>)
    {
        self.commands.push(DeviceCommand::RenderPassBegin {
            render_pass: render_pass.clone(),
            framebuffer_index,
            clear_values
        });
        let mut render_pass_ctx = RenderPassContext {
            base: &mut *self,
        };
        f(&mut render_pass_ctx);
        self.commands.push(DeviceCommand::RenderPassEnd {
            render_pass,
            framebuffer_index,
        });
    }

    pub(crate) fn take_commands(&mut self) -> Vec<DeviceCommand> {
        mem::take(&mut self.commands)
    }
    pub(crate) fn unlock_descriptor_sets(&self) {
        for ds in self.bound_descriptor_sets.values() {
            ds.unlock_updates();
        }
    }
}

pub struct RenderPassContext<'a> {
    base: &'a mut RecordContext,
}

impl<'a> Deref for RenderPassContext<'a> {
    type Target = RecordContext;

    fn deref(&self) -> &Self::Target {
        self.base
    }
}

impl<'a> DerefMut for RenderPassContext<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.base
    }
}

impl<'a> RenderPassContext<'a> {
    pub fn barrier(&mut self) {
        panic!("Pipeline barriers are not allowed inside render passes! Barriers must be placed before RenderPassBegin.");
    }

    pub fn draw(&mut self, vertex_count: u32, instance_count: u32, first_vertex: u32, first_instance: u32) {
        let mut new_descriptor_set_bindings = SmallVec::new();
        for (i, descriptor_set) in &self.bound_descriptor_sets {
            new_descriptor_set_bindings.push((*i, descriptor_set.clone()));
        }
        self.bound_descriptor_sets.clear();
        let new_vertex_buffer = self.bound_vertex_buffer.take();
        let pipeline = self.bound_pipeline.clone().expect("You must bind pipeline before draw command");
        let pipeline_changed = self.pipeline_changed;
        self.pipeline_changed = false;

        self.commands.push(DeviceCommand::DrawCommand(DrawCommand::Draw {
            vertex_count,
            instance_count,
            first_vertex,
            first_instance,
            new_vertex_buffer,
            new_descriptor_set_bindings,
            pipeline,
            pipeline_changed,
        }));
    }
}

pub enum DrawCommand {
    Draw {
        vertex_count: u32,
        instance_count: u32,
        first_vertex: u32,
        first_instance: u32,
        new_vertex_buffer: Option<BufferRange>,
        pipeline: Arc<GraphicsPipelineResource>,
        pipeline_changed: bool,
        new_descriptor_set_bindings: SmallVec<[(u32, Arc<DescriptorSetResource>); 4]>,
    },
}

pub(crate) enum AnyBuffer {
    Device(Arc<BufferResource>),
    Staging(Arc<StagingBuffer>),
}

impl AnyBuffer {
    pub fn buffer(&self) -> vk::Buffer {
        match self {
            AnyBuffer::Device(b) => b.buffer,
            AnyBuffer::Staging(s) => s.buffer,
        }
    }

    pub fn buffer_inner(&self) -> &QueueLocal<BufferResourceInner> {
        match self {
            AnyBuffer::Device(b) => &b.inner,
            AnyBuffer::Staging(s) => &s.inner,
        }
    }

    pub fn submission_usage(&self) -> &OptionSeqNumShared {
        match self {
            AnyBuffer::Device(b) => &b.submission_usage,
            AnyBuffer::Staging(s) => &s.submission_usage,
        }
    }

    pub fn has_host_writes(&self) -> bool {
        matches!(self, AnyBuffer::Staging(_))
    }
}

pub(crate) enum SpecificResourceUsage {
    BufferUsage {
        usage: ResourceUsage,
        buffer: AnyBuffer
    },
    ImageUsage {
        usage: ResourceUsage,
        image: Arc<ImageResource>,
        required_layout: Option<ImageLayout>,
        image_aspect: ImageAspectFlags
    }
}

#[derive(EnumDiscriminants)]
pub enum DeviceCommand {
    CopyBuffer {
        src: AnyBufferRange,
        dst: Arc<BufferResource>,
        regions: SmallVec<[BufferCopy; 1]>,
    },
    CopyBufferToImage {
        src: AnyBufferRange,
        dst: Arc<ImageResource>,
        regions: SmallVec<[BufferImageCopy; 1]>,
    },
    FillBuffer {
        buffer: Arc<BufferResource>,
        offset: u64,
        size: u64,
        data: u32,
    },
    Barrier,
    ImageLayoutTransition {
        image: Arc<ImageResource>,
        new_layout: ImageLayout,
        image_aspect: ImageAspectFlags,
    },
    ClearColorImage {
        image: Arc<ImageResource>,
        clear_color: ash::vk::ClearColorValue,
        image_aspect: ImageAspectFlags,
    },
    ClearDepthStencilImage {
        image: Arc<ImageResource>,
        depth_value: Option<f32>,
        stencil_value: Option<u32>,
    },
    RenderPassBegin {
        render_pass: Arc<RenderPassResource>,
        framebuffer_index: u32,
        clear_values: SmallVec<[ClearValue; 3]>,
    },
    DrawCommand(DrawCommand),
    RenderPassEnd {
        render_pass: Arc<RenderPassResource>,
        framebuffer_index: u32,
    },
}

impl DeviceCommand {
    /// Get usages for command, update last_used_in
    pub fn usages<'a>(&'a self, submission_num: usize, swapchain_images: &'a SwapchainImages, framebuffer_sets: &'a HashMap<vk::RenderPass, FramebufferSet>) -> Box<dyn Iterator<Item=SpecificResourceUsage> + 'a> {
        match self {
            DeviceCommand::CopyBuffer {
                src,
                dst,
                regions
            } => {
                src.submission_usage().store(Some(submission_num));
                dst.submission_usage.store(Some(submission_num));
                Box::new(
                    [
                        SpecificResourceUsage::BufferUsage {
                            usage: ResourceUsage::new(
                                Some(submission_num),
                                PipelineStageFlags::TRANSFER,
                                AccessFlags::TRANSFER_READ,
                                ),
                            buffer: match src {
                                AnyBufferRange::Staging(s) => AnyBuffer::Staging(s.buffer.clone()),
                                AnyBufferRange::Device(d) => AnyBuffer::Device(d.buffer.clone()),
                            },
                        },
                        SpecificResourceUsage::BufferUsage {
                            usage: ResourceUsage::new(
                                Some(submission_num),
                                PipelineStageFlags::TRANSFER,
                                AccessFlags::TRANSFER_WRITE,
                            ),
                            buffer: AnyBuffer::Device(dst.clone()),
                        },
                    ].into_iter()
                )
            }

            DeviceCommand::CopyBufferToImage {
                src,
                dst,
                regions,
            } => {
                src.submission_usage().store(Some(submission_num));
                dst.submission_usage.store(Some(submission_num));
                let combined_aspect = regions.iter()
                    .fold(ImageAspectFlags::empty(), |acc, region| acc | region.image_subresource.aspect_mask);
                Box::new(
                    [
                        SpecificResourceUsage::BufferUsage {
                            usage: ResourceUsage::new(
                                Some(submission_num),
                                PipelineStageFlags::TRANSFER,
                                AccessFlags::TRANSFER_READ,
                            ),
                            buffer: match src {
                                AnyBufferRange::Staging(s) => AnyBuffer::Staging(s.buffer.clone()),
                                AnyBufferRange::Device(d) => AnyBuffer::Device(d.buffer.clone()),
                            },
                        },
                        SpecificResourceUsage::ImageUsage {
                            usage: ResourceUsage::new(
                                Some(submission_num),
                                PipelineStageFlags::TRANSFER,
                                AccessFlags::TRANSFER_WRITE,
                            ),
                            image: dst.clone(),
                            required_layout: Some(ImageLayout::TRANSFER_DST_OPTIMAL),
                            image_aspect: combined_aspect
                        },
                    ].into_iter()
                )
            }
            DeviceCommand::FillBuffer { buffer, .. } => {
                buffer.submission_usage.store(Some(submission_num));
                Box::new(iter::once(
                    SpecificResourceUsage::BufferUsage {
                        usage: ResourceUsage::new(
                            Some(submission_num),
                            PipelineStageFlags::TRANSFER,
                            AccessFlags::TRANSFER_WRITE,
                        ),
                        buffer: AnyBuffer::Device(buffer.clone()),
                    },
                ))
            }
            DeviceCommand::Barrier => Box::new(iter::empty()),
            DeviceCommand::ImageLayoutTransition {image, new_layout, image_aspect} => {
                image.submission_usage.store(Some(submission_num));
                Box::new(iter::once(
                    SpecificResourceUsage::ImageUsage {
                        usage: ResourceUsage::new(
                            Some(submission_num),
                            PipelineStageFlags::TRANSFER, // keep non-empty stage flag for execution dependency
                            AccessFlags::empty(),
                        ),
                        image: image.clone(),
                        required_layout: Some(*new_layout),
                        image_aspect: *image_aspect
                    },
                ))
            },
            DeviceCommand::ClearColorImage {image, image_aspect, ..} => {
                image.submission_usage.store(Some(submission_num));
                Box::new(iter::once(
                    SpecificResourceUsage::ImageUsage {
                        usage: ResourceUsage::new(
                            Some(submission_num),
                            PipelineStageFlags::TRANSFER,
                            AccessFlags::TRANSFER_WRITE,
                        ),
                        image: image.clone(),
                        required_layout: Some(ImageLayout::TRANSFER_DST_OPTIMAL),
                        image_aspect: *image_aspect
                    },
                ))
            },
            DeviceCommand::ClearDepthStencilImage {image, depth_value, stencil_value} => {
                image.submission_usage.store(Some(submission_num));
                Box::new(iter::once(
                    SpecificResourceUsage::ImageUsage {
                        usage: ResourceUsage::new(
                            Some(submission_num),
                            PipelineStageFlags::TRANSFER,
                            AccessFlags::TRANSFER_WRITE,
                        ),
                        image: image.clone(),
                        required_layout: Some(ImageLayout::TRANSFER_DST_OPTIMAL),
                        image_aspect: match (depth_value, stencil_value) {
                            (Some(_), Some(_)) => ImageAspectFlags::DEPTH | ImageAspectFlags::STENCIL,
                            (Some(_), None) => ImageAspectFlags::DEPTH,
                            (None, Some(_)) => ImageAspectFlags::STENCIL,
                            (None, None) => ImageAspectFlags::empty(),
                        }
                    },
                ))
            },
            DeviceCommand::RenderPassBegin { render_pass, framebuffer_index, .. } => {
                render_pass.submission_usage.store(Some(submission_num));
                // usages for attachments
                let attachments = render_pass.attachments_desc();
                let swapchain_desc = attachments.get_swapchain_desc();
                let framebuffer_attachment = swapchain_images[*framebuffer_index as usize].clone();
                let required_layout = if swapchain_desc.initial_layout == ImageLayout::UNDEFINED {
                    None
                }
                else {
                    Some(swapchain_desc.initial_layout)
                };
                let mut usages: SmallVec<[_; 4]> = smallvec![
                    SpecificResourceUsage::ImageUsage {
                        image: framebuffer_attachment,
                        usage: ResourceUsage::new(
                            Some(submission_num),
                            PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                            AccessFlags::COLOR_ATTACHMENT_READ | AccessFlags::COLOR_ATTACHMENT_WRITE,
                        ),
                        required_layout,
                        image_aspect: ImageAspectFlags::COLOR,
                    }
                    // render pass declared single subpass with some attachments
                ];

                // Use iterator for non-swapchain attachments
                if let Some(framebuffer_set) = framebuffer_sets.get(&render_pass.render_pass) {
                    let framebuffer_data = &framebuffer_set.framebuffers[*framebuffer_index as usize];

                    for (_, slot, desc) in attachments.iter_non_swapchain_attachments() {
                        let attachment_option = match slot {
                            crate::resources::render_pass::AttachmentSlot::Depth => &framebuffer_data.depth_image,
                            crate::resources::render_pass::AttachmentSlot::ColorMSAA => &framebuffer_data.color_image,
                            crate::resources::render_pass::AttachmentSlot::Swapchain => unreachable!("Swapchain should not be in non-swapchain iterator"),
                        };

                        if let Some(attachment) = attachment_option {
                            match slot {
                                crate::resources::render_pass::AttachmentSlot::Depth => {
                                    let format = desc.format;
                                    let contains_stencil = matches!(format, Format::S8_UINT | Format::D16_UNORM_S8_UINT | Format::D24_UNORM_S8_UINT | Format::D32_SFLOAT_S8_UINT);
                                    let contains_depth = !matches!(format, Format::S8_UINT);
                                    let mut aspect_mask = ImageAspectFlags::empty();
                                    if contains_depth {
                                        aspect_mask |= ImageAspectFlags::DEPTH
                                    }
                                    if contains_stencil {
                                        aspect_mask |= ImageAspectFlags::STENCIL
                                    }
                                    let required_layout = if desc.initial_layout == ImageLayout::UNDEFINED {
                                        None
                                    }
                                    else {
                                        Some(desc.initial_layout)
                                    };
                                    usages.push(SpecificResourceUsage::ImageUsage {
                                        image: attachment.clone(),
                                        usage: ResourceUsage::new(
                                            Some(submission_num),
                                            PipelineStageFlags::EARLY_FRAGMENT_TESTS | PipelineStageFlags::LATE_FRAGMENT_TESTS,
                                            AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ | AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                                        ),
                                        required_layout,
                                        image_aspect: aspect_mask,
                                    });
                                },
                                crate::resources::render_pass::AttachmentSlot::ColorMSAA => {
                                    let required_layout = if desc.initial_layout == ImageLayout::UNDEFINED {
                                        None
                                    }
                                    else {
                                        Some(desc.initial_layout)
                                    };
                                    usages.push(SpecificResourceUsage::ImageUsage {
                                        image: attachment.clone(),
                                        usage: ResourceUsage::new(
                                            Some(submission_num),
                                            PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                                            AccessFlags::COLOR_ATTACHMENT_READ | AccessFlags::COLOR_ATTACHMENT_WRITE,
                                        ),
                                        required_layout,
                                        image_aspect: ImageAspectFlags::COLOR,
                                    });
                                },
                                crate::resources::render_pass::AttachmentSlot::Swapchain => unreachable!("Swapchain should not be in non-swapchain iterator"),
                            }
                        }
                    }
                }

                Box::new(usages.into_iter())
            }
            DeviceCommand::DrawCommand(
                DrawCommand::Draw {
                    new_vertex_buffer,
                    new_descriptor_set_bindings,
                    pipeline,
                    pipeline_changed,
                    ..
                }
            ) => {
                let mut usages: SmallVec<[_; 10]> = smallvec![];
                if let Some(v_buf) = new_vertex_buffer {
                    usages.push(SpecificResourceUsage::BufferUsage {
                        buffer: AnyBuffer::Device(v_buf.buffer.clone()),
                        usage: ResourceUsage::new(
                            Some(submission_num),
                            PipelineStageFlags::VERTEX_INPUT,
                            AccessFlags::VERTEX_ATTRIBUTE_READ,
                        ),
                    });
                    v_buf.buffer.submission_usage.store(Some(submission_num));
                }
                for (set_index, descriptor_set) in new_descriptor_set_bindings {
                    // collect usage for bound resources
                    for binding in descriptor_set.bindings().lock().unwrap().iter() {
                        match binding.resource.as_ref().expect("all descriptor set resources must be bound") {
                            BoundResource::Buffer(buf) => {
                                buf.submission_usage.store(Some(submission_num));
                                usages.push(SpecificResourceUsage::BufferUsage {
                                    buffer: AnyBuffer::Device(buf.clone()),
                                    usage: ResourceUsage::new(
                                        Some(submission_num),
                                        PipelineStageFlags::VERTEX_SHADER | PipelineStageFlags::FRAGMENT_SHADER,
                                        AccessFlags::UNIFORM_READ,
                                    ),
                                })
                            }
                            BoundResource::CombinedImageSampler {image, sampler} => {
                                image.submission_usage.store(Some(submission_num));
                                sampler.submission_usage.store(Some(submission_num));
                                usages.push(SpecificResourceUsage::ImageUsage {
                                    image: image.clone(),
                                    usage: ResourceUsage::new(
                                        Some(submission_num),
                                        PipelineStageFlags::FRAGMENT_SHADER,
                                        AccessFlags::SHADER_READ,
                                    ),
                                    required_layout: Some(ImageLayout::SHADER_READ_ONLY_OPTIMAL),
                                    image_aspect: ImageAspectFlags::COLOR,
                                });
                                
                            }
                            BoundResource::Image(image) => {
                                image.submission_usage.store(Some(submission_num));
                                usages.push(SpecificResourceUsage::ImageUsage {
                                    image: image.clone(),
                                    usage: ResourceUsage::new(
                                        Some(submission_num),
                                        PipelineStageFlags::FRAGMENT_SHADER,
                                        AccessFlags::SHADER_READ,
                                    ),
                                    required_layout: Some(ImageLayout::SHADER_READ_ONLY_OPTIMAL),
                                    image_aspect: ImageAspectFlags::COLOR,
                                })
                            }
                            
                        }
                    }

                    // mark descriptor sets used
                    descriptor_set.submission_usage.store(Some(submission_num));
                }

                if *pipeline_changed {
                    // mark pipeline used
                    pipeline.submission_usage.store(Some(submission_num))
                }
                Box::new(usages.into_iter())
            }
            DeviceCommand::RenderPassEnd { .. } => {
                Box::new(iter::empty())
            }
        }
    }
}