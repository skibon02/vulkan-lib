use strum::EnumDiscriminants;
use std::collections::HashMap;
use std::iter;
use std::ops::{Deref, DerefMut};
use smallvec::{smallvec, SmallVec};
use ash::vk::{AccessFlags, BufferCopy, BufferImageCopy, ImageAspectFlags, ImageLayout, PipelineStageFlags};
use crate::runtime::resources::buffers::BufferResourceHandle;
use crate::runtime::resources::descriptor_sets::DescriptorSetHandle;
use crate::runtime::resources::images::ImageResourceHandle;
use crate::runtime::resources::pipeline::GraphicsPipelineHandle;
use crate::runtime::resources::render_pass::RenderPassHandle;
use crate::runtime::resources::ResourceUsage;

pub struct RecordContext<'a> {
    commands: Vec<DeviceCommand<'a>>,
    bound_pipeline: Option<GraphicsPipelineHandle>,
    // Map from set number to descriptor set handle
    bound_descriptor_sets: HashMap<u32, DescriptorSetHandle>,
}

impl<'a> RecordContext<'a> {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            bound_pipeline: None,
            bound_descriptor_sets: HashMap::new(),
        }
    }

    // Binding methods (change state, don't add commands)
    pub fn bind_pipeline(&mut self, pipeline: GraphicsPipelineHandle) {
        // When binding a new pipeline, invalidate all descriptor sets
        // because the pipeline layout may be incompatible
        self.bound_pipeline = Some(pipeline);
        self.bound_descriptor_sets.clear();
    }

    pub fn bind_descriptor_set(&mut self, set: u32, descriptor_set: DescriptorSetHandle) {
        // Bind the descriptor set at the specific set number
        self.bound_descriptor_sets.insert(set, descriptor_set);

        // Invalidate all descriptor sets with higher set numbers
        // because they may not be compatible with the new binding
        self.bound_descriptor_sets.retain(|&k, _| k <= set);
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

    pub fn render_pass<F>(&mut self, render_pass: RenderPassHandle, framebuffer_index: u32, f: F)
    where
        F: FnOnce(&mut RenderPassContext<'a, '_>)
    {
        let draw_commands = {
            let mut render_pass_ctx = RenderPassContext {
                base: &mut *self,
                draw_commands: Vec::new(),
            };

            f(&mut render_pass_ctx);
            render_pass_ctx.draw_commands
        };

        self.commands.push(DeviceCommand::RenderPass {
            render_pass,
            framebuffer_index,
            draw_commands,
        });
    }

    pub(crate) fn take_commands(self) -> Vec<DeviceCommand<'a>> {
        self.commands
    }
}

pub struct RenderPassContext<'a, 'b> {
    base: &'b mut RecordContext<'a>,
    draw_commands: Vec<DrawCommand>,
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
    // Draw command methods (render pass specific)
    pub fn draw(&mut self, vertex_count: u32, instance_count: u32, first_vertex: u32, first_instance: u32) {
        self.draw_commands.push(DrawCommand::Draw {
            vertex_count,
            instance_count,
            first_vertex,
            first_instance,
        });
    }
}

pub enum DrawCommand {
    Draw {
        vertex_count: u32,
        instance_count: u32,
        first_vertex: u32,
        first_instance: u32,
    },
    // More draw commands will be added later
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
    RenderPass {
        render_pass: RenderPassHandle,
        framebuffer_index: u32,
        draw_commands: Vec<DrawCommand>,
    }
}

impl<'a> DeviceCommand<'a> {
    pub fn usages(&self, submission_num: usize) -> Box<dyn Iterator<Item=SpecificResourceUsage<'a>> + 'a> {
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
                                true
                                ),
                            handle: *src
                        },
                        SpecificResourceUsage::BufferUsage {
                            usage: ResourceUsage::new(
                                Some(submission_num),
                                PipelineStageFlags::TRANSFER,
                                AccessFlags::TRANSFER_WRITE,
                                false
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
                                true
                            ),
                            handle: *src
                        },
                        SpecificResourceUsage::ImageUsage {
                            usage: ResourceUsage::new(
                                Some(submission_num),
                                PipelineStageFlags::TRANSFER,
                                AccessFlags::TRANSFER_WRITE,
                                false
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
                            false
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
                        true
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
                        false
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
                        false
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
            DeviceCommand::RenderPass { .. } => {
                // Render pass resource usage will be handled when we implement draw commands
                // For now, no resource usage from the render pass itself
                Box::new(iter::empty())
            }
        }
    }
}