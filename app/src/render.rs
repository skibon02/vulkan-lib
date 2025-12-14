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
use rand::Rng;
use sparkles::range_event_start;
use swash::FontRef;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use render_macro::define_layout;
use vulkan_lib::{descriptor_set, use_shader, AttachmentDescription, AttachmentLoadOp, AttachmentStoreOp, BufferCopy, BufferImageCopy, BufferUsageFlags, ClearColorValue, ClearDepthStencilValue, ClearValue, DoubleBuffered, Extent3D, Filter, Format, ImageLayout, ImageSubresourceLayers, ImageUsageFlags, Offset3D, PipelineStageFlags, SampleCountFlags, SamplerCreateInfo, VulkanRenderer};
use vulkan_lib::runtime::resources::AttachmentsDescription;
use vulkan_lib::runtime::resources::images::ImageResourceHandle;
use vulkan_lib::runtime::resources::pipeline::GraphicsPipelineDesc;
use vulkan_lib::shaders::layout::types::{int, vec2, vec3, vec4};

pub enum RenderMessage {
    Redraw { bg_color: [f32; 3] },
    Resize { width: u32, height: u32 },
    Exit,
}

pub struct RenderTask {
    rx: mpsc::Receiver<RenderMessage>,
    vulkan_renderer: VulkanRenderer,
    render_finished: Arc<AtomicBool>,
    resize_finished: Arc<AtomicBool>,
    swapchain_image_handles: SmallVec<[ImageResourceHandle; 3]>,
    swapchain_recreated: bool,
    last_print: Instant,
}

define_layout! {
    pub struct SolidAttributes {
        pub pos: vec3<0>,
        pub size: vec2<0>,
        pub color: vec4<0>,
    }
}

// uniforms
define_layout! {
    pub struct Color {
        pub color: vec4<0>,
    }
}

// descriptor sets
descriptor_set! {
    pub struct GlobalDescriptorSet {
        #[vert]
        0 -> UniformBuffer,
        // #[frag]
        // 1 -> UniformBuffer,
        #[frag]
        2 -> CombinedImageSampler,
    }
}


impl Default for SolidAttributes {
    fn default() -> Self {
        Self {
            color: [1.0, 1.0, 1.0, 1.0].into(),
            pos: [0.0, 0.0, 0.0].into(),
            size: [0.5, 0.5].into(),
        }
    }
}
impl RenderTask {
    pub fn new(vulkan_renderer: VulkanRenderer) -> (Self, mpsc::Sender<RenderMessage>, Arc<AtomicBool>, Arc<AtomicBool>) {
        let (tx, rx) = mpsc::channel::<RenderMessage>();
        let render_finished = Arc::new(AtomicBool::new(true));
        let resize_finished = Arc::new(AtomicBool::new(true));
        let swapchain_image_handles = vulkan_renderer.swapchain_images();

        (Self  {
            rx,
            vulkan_renderer,
            render_finished: render_finished.clone(),
            resize_finished: resize_finished.clone(),
            swapchain_image_handles,
            swapchain_recreated: false,
            last_print: Instant::now(),
        }, tx, render_finished, resize_finished)
    }

    pub fn spawn(mut self) -> JoinHandle<()> {
        thread::Builder::new().name("Render".into()).spawn(move || {
            info!("Render thread spawned!");

            const NUM_INSTANCES: u32 = 1;
            let bytes_per_instance = SolidAttributes::SIZE as u64;
            let total_bytes = bytes_per_instance * NUM_INSTANCES as u64;
            let mut staging_buffers = DoubleBuffered::new(|| {
                self.vulkan_renderer.new_host_buffer(total_bytes)
            });

            // Create render pass
            let msaa_samples = SampleCountFlags::TYPE_1;

            let need_resolve = msaa_samples != SampleCountFlags::TYPE_1;

            let load_op = if need_resolve {
                AttachmentLoadOp::DONT_CARE
            } else {
                AttachmentLoadOp::CLEAR
            };
            let swapchain_attachment = AttachmentDescription::default()
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

            let mut attachments_desc = AttachmentsDescription::new(swapchain_attachment)
                .with_depth_attachment(depth_attachment);

            let sampler = self.vulkan_renderer.new_sampler(|i| {
                i.mag_filter(Filter::NEAREST)
            });

            if need_resolve {
                // Add resolve attachment
                let color_attachment = AttachmentDescription::default()
                    .samples(msaa_samples)
                    .load_op(AttachmentLoadOp::DONT_CARE)
                    .store_op(AttachmentStoreOp::DONT_CARE)
                    .initial_layout(ImageLayout::UNDEFINED)
                    .final_layout(ImageLayout::COLOR_ATTACHMENT_OPTIMAL);

                attachments_desc = attachments_desc.with_color_attachment(color_attachment);
            }

            let render_pass = self.vulkan_renderer.new_render_pass(attachments_desc);
            let attributes = SolidAttributes::get_attributes_configuration();

            let mut vertex_buffer = DoubleBuffered::new(|| {
                let buf = self.vulkan_renderer.new_device_buffer(
                    BufferUsageFlags::VERTEX_BUFFER | BufferUsageFlags::TRANSFER_DST,
                    total_bytes
                );
                buf
            });

            let pipeline_desc = GraphicsPipelineDesc::new(use_shader!("solid"), attributes, smallvec![GlobalDescriptorSet::bindings()]);
            let pipeline = self.vulkan_renderer.new_pipeline(render_pass.handle(), pipeline_desc);

            // load font
            let font_data = std::fs::read(String::from("Ubuntu-Regular.ttf")).unwrap();
            let font = FontRef::from_index(&font_data, 0).unwrap();

            println!("attributes: {}", font.attributes());

            let mut context = ScaleContext::new();
            let mut scaler = context.builder(font)
                .size(90.)
                .build();
            let mut font_rnd = Render::new(&[
                // Color outline with the first palette
                Source::ColorOutline(0),
                // Color bitmap with best fit selection mode
                Source::ColorBitmap(StrikeWith::BestFit),
                // Standard scalable outline
                Source::Outline,
            ]);
            let glyph = font.charmap().map('ы');
            let img = font_rnd.format(swash::zeno::Format::Subpixel)
                .render(&mut scaler, glyph).unwrap();

            info!("img placement: {:?}", img.placement);

            let texture = self.vulkan_renderer.new_image(Format::R8G8B8A8_UNORM, ImageUsageFlags::SAMPLED | ImageUsageFlags::TRANSFER_DST, SampleCountFlags::TYPE_1, img.placement.width, img.placement.height);

            // write to staging
            let mut staging_texture_buffer = self.vulkan_renderer.new_host_buffer(img.data.len() as u64);
            staging_texture_buffer.map_update(0..img.data.len() as u64, |data| {
                data[..].copy_from_slice(&img.data);
            });
            // copy to device local image
            self.vulkan_renderer.record_device_commands(None, |ctx| {
                ctx.copy_buffer_to_image(
                    staging_texture_buffer.handle(),
                    texture.handle(),
                    smallvec![
                        BufferImageCopy::default()
                            .image_extent(Extent3D::default().width(img.placement.width).height(img.placement.height).depth(1))
                            .image_subresource(
                                ImageSubresourceLayers::default()
                                    .aspect_mask(vulkan_lib::ImageAspectFlags::COLOR)
                                    .mip_level(0)
                                    .base_array_layer(0)
                                    .layer_count(1)
                            )
                    ],
                );
            });

            let mut global_ds = self.vulkan_renderer.new_double_buffered_descriptor_sets(
                GlobalDescriptorSet::bindings(),
                |ds, renderer| {
                    let buffer = renderer.new_device_buffer(BufferUsageFlags::UNIFORM_BUFFER, 16);
                    ds.bind_buffer(0, buffer.handle_static());
                    ds.bind_image_and_sampler(2, texture.handle(), sampler.handle());
                    buffer
                },
            );

            loop {
                let msg = self.rx.recv();
                let Ok(msg) = msg else {
                    info!("Render thread exiting due to channel close");
                    break;
                };
                match msg {
                    RenderMessage::Redraw { bg_color} => {
                        let g = range_event_start!("Render");

                        'render: {
                            let bg_clear_color = ClearColorValue {
                                float32: [bg_color[2], bg_color[1], bg_color[0], 1.0],
                            };

                            if self.swapchain_recreated {
                                self.swapchain_recreated = false;
                            }

                            let g = range_event_start!("Wait previous submission");
                            self.vulkan_renderer.wait_prev_submission(1);
                            drop(g);

                            staging_buffers.current_mut().map_update(0..bytes_per_instance, |data| {
                                let square = unsafe {
                                    &mut *(data.as_mut_ptr() as *mut SolidAttributes)
                                };

                                *square = SolidAttributes {
                                    pos: [-1.0, -1.0, 0.0].into(),
                                    size: [2.0, 2.0].into(),
                                    color: [1.0, 1.0, 1.0, 1.0].into(),
                                };
                            });


                            // Acquire next swapchain image
                            let g = range_event_start!("Acquire next image");
                            let (image_index, acquire_wait_ref, is_suboptimal) = match self.vulkan_renderer.acquire_next_image() {
                                Ok(result) => result,
                                Err(e) => {
                                    error!("Failed to acquire next image: {:?}", e);
                                    break 'render;
                                }
                            };
                            drop(g);

                            if is_suboptimal {
                                warn!("Swapchain is suboptimal after acquire");
                            }

                            let clear_values = smallvec![
                                ClearValue {
                                    color: bg_clear_color,
                                },
                                ClearValue {
                                    depth_stencil: ClearDepthStencilValue::default().depth(1.0)
                                },
                                ClearValue {
                                    color: bg_clear_color,
                                },
                            ];

                            let region = BufferCopy::default()
                                .src_offset(0)
                                .dst_offset(0)
                                .size(total_bytes);


                            let present_wait_ref = self.vulkan_renderer.record_device_commands_signal(Some(acquire_wait_ref.with_stages(PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)), |ctx| {
                                ctx.copy_buffer(staging_buffers.current().handle(), vertex_buffer.current().handle_static(), smallvec![region]);
                                ctx.render_pass(render_pass.handle(), image_index, clear_values, |ctx| {
                                    ctx.bind_pipeline(pipeline.handle());
                                    ctx.bind_descriptor_set(0, global_ds.current().handle());


                                    ctx.bind_vertex_buffer(vertex_buffer.current().handle_static());
                                    ctx.draw(4, NUM_INSTANCES, 0, 0);
                                })
                            });

                            let g = range_event_start!("Present");
                            if let Err(e) = self.vulkan_renderer.queue_present(image_index, present_wait_ref) {
                                error!("Present error: {:?}", e);
                            }
                        }

                        global_ds.next_frame();
                        vertex_buffer.next_frame();
                        staging_buffers.next_frame();
                        self.render_finished.store(true, Ordering::Release);
                    }
                    RenderMessage::Resize { width, height } => {
                        let g = range_event_start!("Recreate Resize");
                        self.vulkan_renderer.recreate_resize((width, height));
                        self.swapchain_image_handles = self.vulkan_renderer.swapchain_images();
                        self.swapchain_recreated = true;
                        self.resize_finished.store(true, Ordering::Release);
                    }
                    RenderMessage::Exit => {
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