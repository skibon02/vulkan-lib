use strum::EnumDiscriminants;
use std::collections::HashMap;
use std::iter;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::Ordering;
use smallvec::{smallvec, SmallVec};
use ash::vk::{AccessFlags, BufferCopy, BufferImageCopy, ClearValue, DescriptorSetLayoutBinding, Format, ImageAspectFlags, ImageLayout, PipelineStageFlags};
use crate::runtime::resources::buffers::BufferResourceHandle;
use crate::runtime::resources::descriptor_sets::{BoundResource, DescriptorSetHandle};
use crate::runtime::resources::images::ImageResourceHandle;
use crate::runtime::resources::pipeline::GraphicsPipelineHandle;
use crate::runtime::resources::render_pass::RenderPassHandle;
use crate::runtime::resources::{ResourceStorage, ResourceUsage};

pub struct RecordContext<'a> {
    commands: Vec<DeviceCommand<'a>>,
    bound_pipeline: Option<GraphicsPipelineHandle>,
    pipeline_changed: bool,
    bound_descriptor_sets: HashMap<u32, DescriptorSetHandle<'static>>,
    bound_vertex_buffer: Option<BufferResourceHandle<'static>>
}

impl<'a> RecordContext<'a> {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            bound_pipeline: None,
            pipeline_changed: false,
            bound_vertex_buffer: None,
            bound_descriptor_sets: HashMap::new(),
        }
    }

    pub fn bind_pipeline(&mut self, pipeline: GraphicsPipelineHandle) {
        self.bound_pipeline = Some(pipeline);
        self.pipeline_changed = true;
    }

    pub fn bind_descriptor_set(&mut self, set: u32, descriptor_set: DescriptorSetHandle<'static>) {
        self.bound_descriptor_sets.insert(set, descriptor_set);
    }

    pub fn bind_vertex_buffer(&mut self, buf: BufferResourceHandle<'static>) {
        self.bound_vertex_buffer = Some(buf);
    }

    pub fn copy_buffer<'b>(&'b mut self, src: BufferResourceHandle<'a>, dst: BufferResourceHandle<'a>, regions: SmallVec<[BufferCopy; 1]>) {
        self.commands.push(DeviceCommand::CopyBuffer {
            src,
            dst,
            regions
        })
    }
    pub fn copy_buffer_single<'b>(&'b mut self, src: BufferResourceHandle<'a>, dst: BufferResourceHandle<'a>, region: BufferCopy) {
        let regions = smallvec![region];
        self.commands.push(DeviceCommand::CopyBuffer {
            src,
            dst,
            regions
        })
    }

    pub fn copy_buffer_to_image<'b>(&'b mut self, src: BufferResourceHandle<'a>, dst: ImageResourceHandle, regions: SmallVec<[BufferImageCopy; 1]>) {
        self.commands.push(DeviceCommand::CopyBufferToImage {
            src,
            dst,
            regions
        })
    }

    pub fn copy_buffer_to_image_single<'b>(&'b mut self, src: BufferResourceHandle<'a>, dst: ImageResourceHandle, region: BufferImageCopy) {
        let regions = smallvec![region];
        self.commands.push(DeviceCommand::CopyBufferToImage {
            src,
            dst,
            regions
        })
    }
    
    pub fn fill_buffer(&mut self, buffer: BufferResourceHandle<'a>, offset: u64, size: u64, data: u32) {
        self.commands.push(DeviceCommand::FillBuffer {
            buffer,
            offset,
            size,
            data,
        })
    }

    pub fn transition_image_layout(&mut self, image: ImageResourceHandle, new_layout: ImageLayout, image_aspect: ImageAspectFlags) {
        self.commands.push(DeviceCommand::ImageLayoutTransition {
            image,
            new_layout,
            image_aspect,
        })
    }
    
    pub fn clear_color_image(&mut self, image: ImageResourceHandle, clear_color: ash::vk::ClearColorValue, image_aspect: ImageAspectFlags) {
        self.commands.push(DeviceCommand::ClearColorImage {
            image,
            clear_color,
            image_aspect,
        })
    }
    
    pub fn clear_depth_stencil_image(&mut self, image: ImageResourceHandle, depth_value: Option<f32>, stencil_value: Option<u32>) {
        self.commands.push(DeviceCommand::ClearDepthStencilImage {
            image,
            depth_value,
            stencil_value,
        })
    }

    pub fn barrier(&mut self) {
        self.commands.push(DeviceCommand::Barrier)
    }

    pub fn render_pass<F>(&mut self, render_pass: RenderPassHandle, framebuffer_index: u32, clear_values: SmallVec<[ClearValue; 3]>, f: F)
    where
        F: FnOnce(&mut RenderPassContext<'a, '_>)
    {
        self.commands.push(DeviceCommand::RenderPassBegin {
            render_pass,
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

    pub(crate) fn take_commands(self) -> Vec<DeviceCommand<'a>> {
        self.commands
    }
}

pub struct RenderPassContext<'a, 'b> {
    base: &'b mut RecordContext<'a>,
}

impl<'a, 'b> Deref for RenderPassContext<'a, 'b> {
    type Target = RecordContext<'a>;

    fn deref(&self) -> &Self::Target {
        self.base
    }
}

impl<'a, 'b> DerefMut for RenderPassContext<'a, 'b> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.base
    }
}

impl<'a, 'b> RenderPassContext<'a, 'b> {
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
        let pipeline_handle = self.bound_pipeline.clone().unwrap();
        let pipeline_handle_changed = self.pipeline_changed;
        self.pipeline_changed = false;

        self.commands.push(DeviceCommand::DrawCommand(DrawCommand::Draw {
            vertex_count,
            instance_count,
            first_vertex,
            first_instance,
            new_vertex_buffer,
            new_descriptor_set_bindings,
            pipeline_handle,
            pipeline_handle_changed
        }));
    }
}

pub enum DrawCommand {
    Draw {
        vertex_count: u32,
        instance_count: u32,
        first_vertex: u32,
        first_instance: u32,
        new_vertex_buffer: Option<BufferResourceHandle<'static>>,
        pipeline_handle: GraphicsPipelineHandle,
        pipeline_handle_changed: bool,
        new_descriptor_set_bindings: SmallVec<[(u32, DescriptorSetHandle<'static>); 4]>,
    },
}

pub enum SpecificResourceUsage<'a> {
    BufferUsage {
        usage: ResourceUsage,
        handle: BufferResourceHandle<'a>
    },
    ImageUsage {
        usage: ResourceUsage,
        handle: ImageResourceHandle,
        required_layout: Option<ImageLayout>,
        image_aspect: ImageAspectFlags
    }
}

#[derive(EnumDiscriminants)]
pub enum DeviceCommand<'a> {
    CopyBuffer {
        src: BufferResourceHandle<'a>,
        dst: BufferResourceHandle<'a>,
        regions: SmallVec<[BufferCopy; 1]>,
    },
    CopyBufferToImage {
        src: BufferResourceHandle<'a>,
        dst: ImageResourceHandle,
        regions: SmallVec<[BufferImageCopy; 1]>,
    },
    FillBuffer {
        buffer: BufferResourceHandle<'a>,
        offset: u64,
        size: u64,
        data: u32,
    },
    Barrier,
    ImageLayoutTransition {
        image: ImageResourceHandle,
        new_layout: ImageLayout,
        image_aspect: ImageAspectFlags,
    },
    ClearColorImage {
        image: ImageResourceHandle,
        clear_color: ash::vk::ClearColorValue,
        image_aspect: ImageAspectFlags,
    },
    ClearDepthStencilImage {
        image: ImageResourceHandle,
        depth_value: Option<f32>,
        stencil_value: Option<u32>,
    },
    RenderPassBegin {
        render_pass: RenderPassHandle,
        framebuffer_index: u32,
        clear_values: SmallVec<[ClearValue; 3]>,
    },
    DrawCommand(DrawCommand),
    RenderPassEnd {
        render_pass: RenderPassHandle,
        framebuffer_index: u32,
    },
}

impl<'a> DeviceCommand<'a> {
    pub fn usages(&self, submission_num: usize, resource_storage: &mut ResourceStorage, swapchain_images: SmallVec<[ImageResourceHandle; 3]>) -> Box<dyn Iterator<Item=SpecificResourceUsage<'a>> + 'a> {
        match self {
            DeviceCommand::CopyBuffer {
                src,
                dst,
                regions
            } => {
                Box::new(
                    [
                        SpecificResourceUsage::BufferUsage {
                            usage: ResourceUsage::new(
                                Some(submission_num),
                                PipelineStageFlags::TRANSFER,
                                AccessFlags::TRANSFER_READ,
                                ),
                            handle: *src
                        },
                        SpecificResourceUsage::BufferUsage {
                            usage: ResourceUsage::new(
                                Some(submission_num),
                                PipelineStageFlags::TRANSFER,
                                AccessFlags::TRANSFER_WRITE,
                            ),
                            handle: *dst
                        },
                    ].into_iter()
                )
            }

            DeviceCommand::CopyBufferToImage {
                src,
                dst,
                regions,
            } => {
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
                            handle: *src
                        },
                        SpecificResourceUsage::ImageUsage {
                            usage: ResourceUsage::new(
                                Some(submission_num),
                                PipelineStageFlags::TRANSFER,
                                AccessFlags::TRANSFER_WRITE,
                            ),
                            handle: *dst,
                            required_layout: Some(ImageLayout::TRANSFER_DST_OPTIMAL),
                            image_aspect: combined_aspect
                        },
                    ].into_iter()
                )
            }
            DeviceCommand::FillBuffer { buffer, .. } => {
                Box::new(iter::once(
                    SpecificResourceUsage::BufferUsage {
                        usage: ResourceUsage::new(
                            Some(submission_num),
                            PipelineStageFlags::TRANSFER,
                            AccessFlags::TRANSFER_WRITE,
                        ),
                        handle: *buffer
                    },
                ))
            }
            DeviceCommand::Barrier => Box::new(iter::empty()),
            DeviceCommand::ImageLayoutTransition {image, new_layout, image_aspect} => Box::new(iter::once(
                SpecificResourceUsage::ImageUsage {
                    usage: ResourceUsage::new(
                        Some(submission_num),
                        PipelineStageFlags::TRANSFER, // keep non-empty stage flag for execution dependency
                        AccessFlags::empty(),
                    ),
                    handle: *image,
                    required_layout: Some(*new_layout),
                    image_aspect: *image_aspect
                },
            )),
            DeviceCommand::ClearColorImage {image, image_aspect, ..} => Box::new(iter::once(
                SpecificResourceUsage::ImageUsage {
                    usage: ResourceUsage::new(
                        Some(submission_num),
                        PipelineStageFlags::TRANSFER,
                        AccessFlags::TRANSFER_WRITE,
                    ),
                    handle: *image,
                    required_layout: Some(ImageLayout::TRANSFER_DST_OPTIMAL),
                    image_aspect: *image_aspect
                },
            )),
            DeviceCommand::ClearDepthStencilImage {image, depth_value, stencil_value} => Box::new(iter::once(
                SpecificResourceUsage::ImageUsage {
                    usage: ResourceUsage::new(
                        Some(submission_num),
                        PipelineStageFlags::TRANSFER,
                        AccessFlags::TRANSFER_WRITE,
                    ),
                    handle: *image,
                    required_layout: Some(ImageLayout::TRANSFER_DST_OPTIMAL),
                    image_aspect: match (depth_value, stencil_value) {
                        (Some(_), Some(_)) => ImageAspectFlags::DEPTH | ImageAspectFlags::STENCIL,
                        (Some(_), None) => ImageAspectFlags::DEPTH,
                        (None, Some(_)) => ImageAspectFlags::STENCIL,
                        (None, None) => ImageAspectFlags::empty(),
                    }
                },
            )),
            DeviceCommand::RenderPassBegin { render_pass, framebuffer_index, .. } => {
                // usages for attachments
                let attachments = resource_storage.render_pass(render_pass.0).attachments_description.clone();
                let swapchain_desc = attachments.get_swapchain_desc();
                let swapchain_image_handle = swapchain_images[*framebuffer_index as usize];
                let required_layout = if swapchain_desc.initial_layout == ImageLayout::UNDEFINED {
                    None
                }
                else {
                    Some(swapchain_desc.initial_layout)
                };
                let mut usages: SmallVec<[_; 4]> = smallvec![
                    SpecificResourceUsage::ImageUsage {
                        handle: swapchain_image_handle,
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
                    let image_handle = resource_storage.render_pass(render_pass.0).framebuffers[*framebuffer_index as usize].1[next_image_i].handle();
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
                        handle: image_handle,
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
                    let image_handle = resource_storage.render_pass(render_pass.0).framebuffers[*framebuffer_index as usize].1[next_image_i].handle();
                    let required_layout = if color_desc.initial_layout == ImageLayout::UNDEFINED {
                        None
                    }
                    else {
                        Some(color_desc.initial_layout)
                    };
                    usages.push(SpecificResourceUsage::ImageUsage {
                        handle: image_handle,
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
                    pipeline_handle,
                    pipeline_handle_changed,
                    ..
                }
            ) => {
                let mut usages: SmallVec<[_; 10]> = smallvec![];
                if let Some(v_buf) = new_vertex_buffer {
                    usages.push(SpecificResourceUsage::BufferUsage {
                        handle: v_buf.clone(),
                        usage: ResourceUsage::new(
                            Some(submission_num),
                            PipelineStageFlags::VERTEX_INPUT,
                            AccessFlags::VERTEX_ATTRIBUTE_READ,
                        ),
                    })
                }
                for (set_index, descriptor_set_handle) in new_descriptor_set_bindings {
                    // collect usage for bound resources
                    for binding in &descriptor_set_handle.bindings {
                        match binding.resource.expect("all descriptor set resources must be bound") {
                            BoundResource::Buffer(buf) => {
                                usages.push(SpecificResourceUsage::BufferUsage {
                                    handle: buf.clone(),
                                    usage: ResourceUsage::new(
                                        Some(submission_num),
                                        PipelineStageFlags::VERTEX_SHADER | PipelineStageFlags::FRAGMENT_SHADER,
                                        AccessFlags::UNIFORM_READ,
                                    ),
                                })
                            }
                            BoundResource::Image(img) => {
                                usages.push(SpecificResourceUsage::ImageUsage {
                                    handle: img.clone(),
                                    usage: ResourceUsage::new(
                                        Some(submission_num),
                                        PipelineStageFlags::FRAGMENT_SHADER,
                                        AccessFlags::SHADER_READ,
                                    ),
                                    required_layout: Some(ImageLayout::SHADER_READ_ONLY_OPTIMAL),
                                    image_aspect: ImageAspectFlags::COLOR,
                                })
                            }
                            _ => {

                            }
                        }
                    }

                    // mark descriptor set used
                }

                if *pipeline_handle_changed {
                    // mark pipeline used
                }
                Box::new(usages.into_iter())
            }
            DeviceCommand::RenderPassEnd { .. } => {
                Box::new(iter::empty())
            }
        }
    }
}