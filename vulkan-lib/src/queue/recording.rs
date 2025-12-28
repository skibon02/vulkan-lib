use strum::EnumDiscriminants;
use std::collections::HashMap;
use std::iter;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use smallvec::{smallvec, SmallVec};
use ash::vk::{AccessFlags, BufferCopy, BufferImageCopy, ClearValue, Format, ImageAspectFlags, ImageLayout, PipelineStageFlags};
use crate::resources::buffer::BufferResource;
use crate::resources::descriptor_set::{BoundResource, DescriptorSetResource};
use crate::resources::image::ImageResource;
use crate::resources::pipeline::GraphicsPipelineResource;
use crate::resources::render_pass::{FrameBufferAttachment, RenderPassResource};
use crate::resources::ResourceUsage;
use crate::swapchain_wrapper::SwapchainImages;

pub struct RecordContext {
    commands: Vec<DeviceCommand>,
    bound_pipeline: Option<Arc<GraphicsPipelineResource>>,
    pipeline_changed: bool,
    bound_descriptor_sets: HashMap<u32, Arc<DescriptorSetResource>>,
    bound_vertex_buffer: Option<Arc<BufferResource>>
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

    pub fn bind_vertex_buffer(&mut self, buf: Arc<BufferResource>) {
        self.bound_vertex_buffer = Some(buf);
    }

    pub fn copy_buffer<'b>(&'b mut self, src: Arc<BufferResource>, dst: Arc<BufferResource>, regions: SmallVec<[BufferCopy; 1]>) {
        self.commands.push(DeviceCommand::CopyBuffer {
            src,
            dst,
            regions
        })
    }
    pub fn copy_buffer_single<'b>(&'b mut self, src: Arc<BufferResource>, dst: Arc<BufferResource>, region: BufferCopy) {
        let regions = smallvec![region];
        self.commands.push(DeviceCommand::CopyBuffer {
            src,
            dst,
            regions
        })
    }

    pub fn copy_buffer_to_image<'b>(&'b mut self, src: Arc<BufferResource>, dst: Arc<ImageResource>, regions: SmallVec<[BufferImageCopy; 1]>) {
        self.commands.push(DeviceCommand::CopyBufferToImage {
            src,
            dst,
            regions
        })
    }

    pub fn copy_buffer_to_image_single<'b>(&'b mut self, src: Arc<BufferResource>, dst: Arc<ImageResource>, region: BufferImageCopy) {
        let regions = smallvec![region];
        self.commands.push(DeviceCommand::CopyBufferToImage {
            src,
            dst,
            regions
        })
    }
    
    pub fn fill_buffer(&mut self, buffer: Arc<BufferResource>, offset: u64, size: u64, data: u32) {
        self.commands.push(DeviceCommand::FillBuffer {
            buffer,
            offset,
            size,
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

    pub(crate) fn take_commands(self) -> Vec<DeviceCommand> {
        self.commands
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
        for (i, binding) in &self.bound_descriptor_sets {
            new_descriptor_set_bindings.push((*i, binding.clone()));
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
        new_vertex_buffer: Option<Arc<BufferResource>>,
        pipeline: Arc<GraphicsPipelineResource>,
        pipeline_changed: bool,
        new_descriptor_set_bindings: SmallVec<[(u32, Arc<DescriptorSetResource>); 4]>,
    },
}

pub enum SpecificResourceUsage {
    BufferUsage {
        usage: ResourceUsage,
        buffer: Arc<BufferResource>
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
        src: Arc<BufferResource>,
        dst: Arc<BufferResource>,
        regions: SmallVec<[BufferCopy; 1]>,
    },
    CopyBufferToImage {
        src: Arc<BufferResource>,
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
    pub fn usages(&self, submission_num: usize, swapchain_images: &SwapchainImages) -> Box<dyn Iterator<Item=SpecificResourceUsage>> {
        match self {
            DeviceCommand::CopyBuffer {
                src,
                dst,
                regions
            } => {
                src.submission_usage.store(Some(submission_num));
                dst.submission_usage.store(Some(submission_num));
                Box::new(
                    [
                        SpecificResourceUsage::BufferUsage {
                            usage: ResourceUsage::new(
                                Some(submission_num),
                                PipelineStageFlags::TRANSFER,
                                AccessFlags::TRANSFER_READ,
                                ),
                            buffer: src.clone()
                        },
                        SpecificResourceUsage::BufferUsage {
                            usage: ResourceUsage::new(
                                Some(submission_num),
                                PipelineStageFlags::TRANSFER,
                                AccessFlags::TRANSFER_WRITE,
                            ),
                            buffer: dst.clone()
                        },
                    ].into_iter()
                )
            }

            DeviceCommand::CopyBufferToImage {
                src,
                dst,
                regions,
            } => {
                src.submission_usage.store(Some(submission_num));
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
                            buffer: src.clone()
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
                        buffer: buffer.clone()
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

                let mut next_image_i = 0;
                if let Some(depth_desc) = attachments.get_depth_attachment_desc() {
                    let attachment = render_pass.attachment(swapchain_images, *framebuffer_index as usize, next_image_i);
                    let format = depth_desc.format;
                    let contains_stencil = matches!(format, Format::S8_UINT | Format::D16_UNORM_S8_UINT | Format::D24_UNORM_S8_UINT | Format::D32_SFLOAT_S8_UINT);
                    let contains_depth = !matches!(format, Format::S8_UINT);
                    let mut aspect_mask = ImageAspectFlags::empty();
                    if contains_depth {
                        aspect_mask |= ImageAspectFlags::DEPTH
                    }
                    if contains_stencil {
                        aspect_mask |= ImageAspectFlags::STENCIL
                    }
                    let required_layout = if depth_desc.initial_layout == ImageLayout::UNDEFINED {
                        None
                    }
                    else {
                        Some(depth_desc.initial_layout)
                    };
                    usages.push(SpecificResourceUsage::ImageUsage {
                        image: attachment,
                        usage: ResourceUsage::new(
                            Some(submission_num),
                            PipelineStageFlags::EARLY_FRAGMENT_TESTS | PipelineStageFlags::LATE_FRAGMENT_TESTS,
                            AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ | AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                        ),
                        required_layout,
                        image_aspect: aspect_mask,
                    });

                    next_image_i += 1;
                }

                if let Some(color_desc) = attachments.get_color_attachment_desc() {
                    let attachment = render_pass.attachment(swapchain_images, *framebuffer_index as usize, next_image_i);
                    let required_layout = if color_desc.initial_layout == ImageLayout::UNDEFINED {
                        None
                    }
                    else {
                        Some(color_desc.initial_layout)
                    };
                    usages.push(SpecificResourceUsage::ImageUsage {
                        image: attachment,
                        usage: ResourceUsage::new(
                            Some(submission_num),
                            PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                            AccessFlags::COLOR_ATTACHMENT_READ | AccessFlags::COLOR_ATTACHMENT_WRITE,
                        ),
                        required_layout,
                        image_aspect: ImageAspectFlags::COLOR,
                    });

                    next_image_i += 1;
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
                        buffer: v_buf.clone(),
                        usage: ResourceUsage::new(
                            Some(submission_num),
                            PipelineStageFlags::VERTEX_INPUT,
                            AccessFlags::VERTEX_ATTRIBUTE_READ,
                        ),
                    });
                    v_buf.submission_usage.store(Some(submission_num));
                }
                for (set_index, descriptor_set) in new_descriptor_set_bindings {
                    // collect usage for bound resources
                    for binding in descriptor_set.bindings().lock().unwrap().iter() {
                        match binding.resource.as_ref().expect("all descriptor set resources must be bound") {
                            BoundResource::Buffer(buf) => {
                                usages.push(SpecificResourceUsage::BufferUsage {
                                    buffer: buf.clone(),
                                    usage: ResourceUsage::new(
                                        Some(submission_num),
                                        PipelineStageFlags::VERTEX_SHADER | PipelineStageFlags::FRAGMENT_SHADER,
                                        AccessFlags::UNIFORM_READ,
                                    ),
                                })
                            }
                            BoundResource::Image(img) | BoundResource::CombinedImageSampler {image: img, ..} => {
                                usages.push(SpecificResourceUsage::ImageUsage {
                                    image: img.clone(),
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