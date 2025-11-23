use vulkan_lib::shaders::layout::types::{float, GlslType};
use vulkan_lib::shaders::layout::LayoutInfo;
use vulkan_lib::shaders::layout::MemberMeta;
use std::mem::offset_of;
use vulkan_lib::shaders::layout::types::GlslTypeVariant;
use smallvec::{smallvec, SmallVec};
use std::sync::{mpsc, Arc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::thread::JoinHandle;
use std::time::Instant;
use log::{error, info, warn};
use sparkles::range_event_start;
use render_macro::define_layout;
use vulkan_lib::{use_shader, AttachmentDescription, AttachmentLoadOp, AttachmentStoreOp, BufferUsageFlags, ClearColorValue, Extent3D, Format, ImageAspectFlags, ImageLayout, ImageSubresourceLayers, Offset3D, PipelineStageFlags, SampleCountFlags, VulkanRenderer};
use vulkan_lib::runtime::resources::AttachmentsDescription;
use vulkan_lib::runtime::resources::images::ImageResourceHandle;
use vulkan_lib::runtime::resources::pipeline::{GraphicsPipelineDesc, VertexInputDesc};
use vulkan_lib::shaders::layout::types::{int, vec2, vec4};
use vulkan_lib::shaders::{DescriptorBindingsDesc, UniformBindingType};

pub enum RenderMessage {
    Redraw { bg_color: [f32; 3] },
    Resize { width: u32, height: u32 },
    Exit,
}

pub struct RenderTask {
    rx: mpsc::Receiver<RenderMessage>,
    vulkan_renderer: VulkanRenderer,
    render_finished: Arc<AtomicBool>,
    swapchain_image_handles: SmallVec<[ImageResourceHandle; 3]>,
    swapchain_recreated: bool,
    last_print: Instant,
}

define_layout! {
    pub struct CircleAttributes {
        pub color: vec4<0>,
        pub pos: vec2<0>,
        pub trig_time: int<0>,
    }
}

// uniforms
define_layout! {
    pub struct Time {
        pub time: int<0>
    }
}

define_layout! {
    pub struct MapStats {
        pub r: float<0>,
        pub ar: float<0>,
        pub aspect: float<0>
    }
}


impl Default for CircleAttributes {
    fn default() -> Self {
        Self {
            color: [1.0, 1.0, 1.0, 0.0].into(),
            pos: [0.0, 0.0].into(),
            trig_time: 0.into(),
        }
    }
}
impl RenderTask {
    pub fn new(vulkan_renderer: VulkanRenderer) -> (Self, mpsc::Sender<RenderMessage>, Arc<AtomicBool>) {
        let (tx, rx) = mpsc::channel::<RenderMessage>();
        let render_finished = Arc::new(AtomicBool::new(true));
        let swapchain_image_handles = vulkan_renderer.swapchain_images();

        (Self  {
            rx,
            vulkan_renderer,
            render_finished: render_finished.clone(),
            swapchain_image_handles,
            swapchain_recreated: false,
            last_print: Instant::now(),
        }, tx, render_finished)
    }

    pub fn spawn(mut self) -> JoinHandle<()> {
        thread::Builder::new().name("Render".into()).spawn(move || {
            info!("Render thread spawned!");

            let swapchain_extent = self.swapchain_image_handles[0].extent();
            let mut staging_buffer = self.vulkan_renderer.new_host_buffer((4 * swapchain_extent.width * swapchain_extent.height) as u64);

            // Create render pass
            let msaa_samples = SampleCountFlags::TYPE_1;

            let need_resolve = msaa_samples != SampleCountFlags::TYPE_1;

            let load_op = if need_resolve {
                AttachmentLoadOp::CLEAR
            } else {
                AttachmentLoadOp::DONT_CARE
            };

            let color_attachment = AttachmentDescription::default()
                .samples(SampleCountFlags::TYPE_1)
                .load_op(load_op)
                .store_op(AttachmentStoreOp::STORE)
                .initial_layout(ImageLayout::UNDEFINED)
                .final_layout(ImageLayout::PRESENT_SRC_KHR);

            let depth_attachment = AttachmentDescription::default()
                .format(Format::D16_UNORM)
                .samples(msaa_samples)
                .load_op(AttachmentLoadOp::CLEAR)
                .store_op(AttachmentStoreOp::DONT_CARE)
                .stencil_load_op(AttachmentLoadOp::DONT_CARE)
                .stencil_store_op(AttachmentStoreOp::DONT_CARE)
                .initial_layout(ImageLayout::UNDEFINED)
                .final_layout(ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);

            let mut attachments_desc = AttachmentsDescription::new(color_attachment)
                .with_depth_attachment(depth_attachment);

            if need_resolve {
                // Add resolve attachment
                let resolve_attachment = AttachmentDescription::default()
                    .samples(SampleCountFlags::TYPE_1)
                    .load_op(AttachmentLoadOp::DONT_CARE)
                    .store_op(AttachmentStoreOp::STORE)
                    .initial_layout(ImageLayout::UNDEFINED)
                    .final_layout(ImageLayout::PRESENT_SRC_KHR);

                attachments_desc = attachments_desc.with_resolve_attachment(resolve_attachment);
            }

            let render_pass = self.vulkan_renderer.new_render_pass(attachments_desc);
            let attributes = CircleAttributes::get_attributes_configuration();
            let bindings_desc = smallvec![
                (0, UniformBindingType::UniformBuffer),
                (1, UniformBindingType::UniformBuffer),
                (2, UniformBindingType::CombinedImageSampler)
            ];
            let pipeline_desc = GraphicsPipelineDesc::new(use_shader!("circle"), attributes, bindings_desc);
            let pipeline = self.vulkan_renderer.new_pipeline(render_pass.handle(), pipeline_desc);


            // let mut dev_buffer = self.vulkan_renderer.new_device_buffer(BufferUsageFlags::TRANSFER_DST | BufferUsageFlags::TRANSFER_SRC, 4*swapchain_extent.width as u64 * swapchain_extent.height as u64);
            loop {
                let msg = self.rx.recv();
                if msg.is_err() {
                    info!("Render thread exiting due to channel close");
                    break;
                }
                match msg {
                    Ok(RenderMessage::Redraw { bg_color}) => {
                        let g = range_event_start!("Render");

                        let bg_clear_color = ClearColorValue {
                            float32: [bg_color[2], bg_color[1], bg_color[0], 1.0],
                        };
                        // let bg_color_u32 = ((bg_color[2].clamp(0.0, 1.0) * 255.0) as u32) |
                        //     ((bg_color[1].clamp(0.0, 1.0) * 255.0) as u32) << 8 |
                        //     ((bg_color[0].clamp(0.0, 1.0) * 255.0) as u32) << 16 |
                        //     (255u32 << 24);

                        let swapchain_extent = self.swapchain_image_handles[0].extent();
                        if self.swapchain_recreated {
                            self.swapchain_recreated = false;

                            staging_buffer = self.vulkan_renderer.new_host_buffer((4 * swapchain_extent.width * swapchain_extent.height) as u64);
                            // dev_buffer = self.vulkan_renderer.new_device_buffer(BufferUsageFlags::TRANSFER_DST | BufferUsageFlags::TRANSFER_SRC, 4*swapchain_extent.width as u64 * swapchain_extent.height as u64);
                        }

                        let g = range_event_start!("Wait previous submission");
                        self.vulkan_renderer.wait_prev_submission(2);
                        drop(g);

                        // Acquire next swapchain image
                        let g = range_event_start!("Acquire next image");
                        let (image_index, acquire_wait_ref, is_suboptimal) = match self.vulkan_renderer.acquire_next_image() {
                            Ok(result) => result,
                            Err(e) => {
                                error!("Failed to acquire next image: {:?}", e);
                                continue;
                            }
                        };
                        drop(g);

                        if is_suboptimal {
                            warn!("Swapchain is suboptimal after acquire");
                        }

                        let present_wait_ref = self.vulkan_renderer.record_device_commands_signal(Some(acquire_wait_ref.with_stages(PipelineStageFlags::TRANSFER)), |ctx| {
                            // ctx.fill_buffer(
                            //     dev_buffer.handle_static(),
                            //     0,
                            //     (4 * swapchain_extent.width * swapchain_extent.height) as u64,
                            //     bg_color_u32,
                            // );
                            // ctx.copy_buffer_to_image_single(
                            //     dev_buffer.handle_static(),
                            //     self.swapchain_image_handles[image_index as usize],
                            //     BufferImageCopy {
                            //         buffer_offset: 0,
                            //         buffer_row_length: 0,
                            //         buffer_image_height: 0,
                            //         image_subresource: ImageSubresourceLayers {
                            //             aspect_mask: ImageAspectFlags::COLOR,
                            //             mip_level: 0,
                            //             base_array_layer: 0,
                            //             layer_count: 1,
                            //         },
                            //         image_offset: Offset3D { x: 0, y: 0, z: 0 },
                            //         image_extent: Extent3D {
                            //             width: swapchain_extent.width,
                            //             height: swapchain_extent.height,
                            //             depth: 1,
                            //         },
                            //     },
                            // );
                            ctx.clear_color_image(
                                self.swapchain_image_handles[image_index as usize],
                                bg_clear_color,
                                ImageAspectFlags::COLOR,
                            );
                            ctx.transition_image_layout(
                                self.swapchain_image_handles[image_index as usize],
                                ImageLayout::PRESENT_SRC_KHR,
                                ImageAspectFlags::COLOR,
                            );
                        });

                        let g = range_event_start!("Present");
                        if let Err(e) = self.vulkan_renderer.queue_present(image_index, present_wait_ref) {
                            error!("Present error: {:?}", e);
                        }

                        self.render_finished.store(true, Ordering::Release);
                    }
                    Ok(RenderMessage::Resize { width, height }) => {
                        let g = range_event_start!("Recreate Resize");
                        self.vulkan_renderer.recreate_resize((width, height));
                        self.swapchain_image_handles = self.vulkan_renderer.swapchain_images();
                        self.swapchain_recreated = true;
                    }
                    Ok(RenderMessage::Exit) | Err(_) => {
                        info!("Render thread exiting");
                        break;
                    }
                }

                if self.last_print.elapsed().as_secs() >= 3 {
                    self.vulkan_renderer.dump_resource_usage();
                    self.last_print = Instant::now();
                }
            }
        }).unwrap()
    }
}