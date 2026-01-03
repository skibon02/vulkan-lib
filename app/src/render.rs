use std::f64::consts::PI;
use vulkan_lib::vk::{DescriptorType, Extent2D};
use vulkan_lib::vk::{BufferCopy, ClearColorValue, ClearDepthStencilValue, ClearValue, Filter, ImageUsageFlags, PipelineStageFlags};
use vulkan_lib::vk::{AttachmentDescription, AttachmentLoadOp, AttachmentStoreOp, BufferUsageFlags, Format, ImageLayout, SampleCountFlags};
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
use swash::FontRef;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use winit::dpi::PhysicalSize;
use render_macro::define_layout;
use vulkan_lib::{descriptor_set, use_shader};
use vulkan_lib::vk::{BufferCreateFlags, ImageCreateFlags};
use vulkan_lib::queue::GraphicsQueue;
use vulkan_lib::resources::image::ImageResource;
use vulkan_lib::resources::pipeline::GraphicsPipelineDesc;
use vulkan_lib::resources::render_pass::{AttachmentsDescription, FrameBufferAttachment};
use vulkan_lib::resources::staging_buffer::StagingBufferRange;
use vulkan_lib::resources::VulkanAllocator;
use vulkan_lib::shaders::layout::types::{vec2, vec3, vec4, ivec2};
use crate::resources::get_resource;
use crate::util::DoubleBuffered;

pub enum RenderMessage {
    Redraw { bg_color: [f32; 3] },
    Resize { width: u32, height: u32 },
    Exit,
}

pub struct RenderTask {
    rx: mpsc::Receiver<RenderMessage>,
    vulkan_renderer: GraphicsQueue,
    render_finished: Arc<AtomicBool>,
    resize_finished: Arc<AtomicBool>,
    swapchain_recreated: bool,
    last_print: Instant,
    extent: [i32; 2],
}

define_layout! {
    pub struct SolidAttributes {
        pub pos: ivec2<0>,
        pub size: ivec2<0>,
        pub d: float<0>,
    }
}

// uniforms
define_layout! {
    pub struct Global {
        pub aspect: ivec2<0>,
    }
}

// descriptor sets
descriptor_set! {
    pub struct GlobalDescriptorSet {
        #[vert]
        0 -> UniformBuffer, // global UB
        #[frag]
        1 -> CombinedImageSampler,
    }
}

fn load_font_texture(allocator: &mut VulkanAllocator) -> (StagingBufferRange, Arc<ImageResource>, Extent2D) {
    // load font
    let font_data = get_resource("fonts/Ubuntu-Regular.ttf".into()).unwrap();
    let font = FontRef::from_index(&font_data, 0).unwrap();

    println!("attributes: {}", font.attributes());

    let mut context = ScaleContext::new();
    let mut scaler = context.builder(font)
        .size(24.0)
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

    // let img_file = fs::File::create("output.png").unwrap();
    // let encoder = image::codecs::png::PngEncoder::new(img_file);
    // // prepare bigger image
    // let mut big_img = Vec::with_capacity(img.data.len() * 9);
    // for i in img.data.chunks(img.placement.width as usize) {
    //     for _ in 0..3 {
    //         for byte in i {
    //             for _ in 0..9 {
    //                 big_img.push(*byte);
    //             }
    //         }
    //     }
    // }
    // encoder.write_image(
    //     &big_img,
    //     img.placement.width * 3,
    //     img.placement.height * 3,
    //     ExtendedColorType::Rgb8,
    // ).unwrap();

    info!("img placement: {:?}", img.placement);

    // Create texture
    let texture = allocator.new_image(
        ImageUsageFlags::SAMPLED | ImageUsageFlags::TRANSFER_DST,
        ImageCreateFlags::empty(),
        img.placement.width,
        img.placement.height,
        Format::R8G8B8A8_UNORM,
        SampleCountFlags::TYPE_1,
    );

    // Upload texture using staging buffer
    let staging_texture = allocator.new_staging_buffer(
        img.data.len() as u64,
    );

    let mut tex_range = staging_texture.try_freeze(img.data.len()).expect("Should be empty");
    tex_range.update(|data| {
        data.copy_from_slice(&img.data);
    });

    (tex_range, texture, Extent2D {
        width: img.placement.width,
        height: img.placement.height,
    })
}

impl Default for SolidAttributes {
    fn default() -> Self {
        Self {
            pos: [0, 0].into(),
            d: 0.0.into(),
            size: [0, 0].into(),
        }
    }
}
impl RenderTask {
    pub fn new(vulkan_renderer: GraphicsQueue, initial_size: PhysicalSize<u32>) -> (Self, mpsc::Sender<RenderMessage>, Arc<AtomicBool>, Arc<AtomicBool>) {
        let (tx, rx) = mpsc::channel::<RenderMessage>();
        let render_finished = Arc::new(AtomicBool::new(true));
        let resize_finished = Arc::new(AtomicBool::new(true));

        // Create depth image for render pass
        (Self  {
            rx,
            vulkan_renderer,
            render_finished: render_finished.clone(),
            resize_finished: resize_finished.clone(),
            swapchain_recreated: false,
            last_print: Instant::now(),
            extent: [initial_size.width as i32, initial_size.height as i32],
        }, tx, render_finished, resize_finished)
    }

    pub fn spawn(mut self) -> JoinHandle<()> {
        thread::Builder::new().name("Render".into()).spawn(move || {
            info!("Render thread spawned!");

            let start_tm = Instant::now();

            // Create allocator
            let mut allocator = self.vulkan_renderer.new_allocator();

            const NUM_INSTANCES: u32 = 1;
            let bytes_per_instance = SolidAttributes::SIZE as u64;
            let total_bytes = bytes_per_instance * NUM_INSTANCES as u64;

            // Create double-buffered staging buffers for instance data
            let staging_a = allocator.new_staging_buffer(
                total_bytes,
            );
            let staging_b = allocator.new_staging_buffer(
                total_bytes,
            );

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

            let pixel_perfect_sampler = allocator.new_sampler(|i| {
                i
                    .min_filter(Filter::NEAREST)
                    .mag_filter(Filter::NEAREST)
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

            // Depth/color images are now created automatically per-framebuffer in the queue
            // This allows automatic handling of swapchain recreation with different extents
            let swapchain_format = self.vulkan_renderer.swapchain_format();
            let render_pass = allocator.new_render_pass(
                attachments_desc.clone(),
                swapchain_format,
            );

            let attributes = SolidAttributes::get_attributes_configuration();

            // Create double-buffered vertex buffers
            let mut vertex_buffer = DoubleBuffered::new(|| {
                allocator.new_buffer(
                    BufferUsageFlags::VERTEX_BUFFER | BufferUsageFlags::TRANSFER_DST,
                    BufferCreateFlags::empty(),
                    total_bytes,
                )
            });

            let pipeline_desc = GraphicsPipelineDesc::new(use_shader!("solid"), attributes, smallvec![GlobalDescriptorSet::bindings()]);
            let pipeline = allocator.new_pipeline(render_pass.clone(), pipeline_desc);

            let (font_staging, font_texture, font_size) = load_font_texture(&mut allocator);

            self.vulkan_renderer.record_device_commands(None, |ctx| {
                ctx.copy_buffer_to_image_full(
                    font_staging,
                    font_texture.clone(),
                    4, // bytes_per_texel for RGBA8
                );
            });

            // Create descriptor sets
            let descriptor_set = allocator.allocate_descriptor_set(GlobalDescriptorSet::bindings());
            let ds_b = allocator.allocate_descriptor_set(GlobalDescriptorSet::bindings());

            // Create uniform buffers
            let global_ds_buffer = allocator.new_buffer(
                BufferUsageFlags::UNIFORM_BUFFER | BufferUsageFlags::TRANSFER_DST,
                BufferCreateFlags::empty(),
                16,
            );

            // Bind resources
            descriptor_set.try_bind_buffer(0, global_ds_buffer.clone()).unwrap();
            descriptor_set.try_bind_image_sampler(1, font_texture.clone(), pixel_perfect_sampler.clone()).unwrap();

            // Upload initial uniform data
            let global_staging = allocator.new_staging_buffer(
                16,
            );

            let mut global = Global {
                aspect: [self.extent[0], self.extent[1]].into(),
            };

            let mut staging_global_range = global_staging.try_freeze(16).unwrap();
            staging_global_range.update(|data| data.copy_from_slice(global.as_bytes()));

            self.vulkan_renderer.record_device_commands(None, |ctx| {
                ctx.copy_buffer(staging_global_range, global_ds_buffer.full());
            });

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

                            let g = range_event_start!("Wait previous submission");
                            let waited = self.vulkan_renderer.wait_prev_submission(1);
                            drop(g);

                            // Freeze a range from current staging buffer for vertex data
                            let vertex_staging = if staging_a.try_unfreeze(waited).is_some() {
                                &staging_a
                            }
                            else if staging_b.try_unfreeze(waited).is_some() {
                                &staging_b
                            } else {
                                panic!("Both stagings were frozen!");
                            };
                            let mut vertex_staging_range = vertex_staging
                                .try_freeze(bytes_per_instance as usize)
                                .expect("Staging buffer should be unfrozen by now");

                            let t = (start_tm.elapsed().as_secs_f64() % 4.0) / 4.0 * (2.0 * PI);
                            let width = self.extent[0];
                            let height = self.extent[1];
                            let x = width as f64 * (0.5 + t.sin() * 0.4);
                            let y = height as f64 * (0.5 + t.cos() * 0.4);
                            vertex_staging_range.update(|data| {
                                let square = unsafe {
                                    &mut *(data.as_mut_ptr() as *mut SolidAttributes)
                                };

                                *square = SolidAttributes {
                                    pos: [x as i32, y as i32].into(),
                                    size: [font_size.width as i32, font_size.height as i32].into(),
                                    d: 0.5.into(),
                                };
                            });

                            // Handle global uniform update on resize
                            let global_range = if self.swapchain_recreated {
                                global.aspect = self.extent.into();
                                let sub_num = self.vulkan_renderer.wait_prev_submission(0);
                                global_staging.try_unfreeze(sub_num).unwrap();
                                let mut range = global_staging.try_freeze(16).expect("Global staging should have space");
                                range.update(|data| data.copy_from_slice(global.as_bytes()));
                                Some(range)
                            } else {
                                None
                            };

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
                                if let Some(range) = global_range {
                                    ctx.copy_buffer(range, global_ds_buffer.full());
                                }

                                ctx.copy_buffer(vertex_staging_range, vertex_buffer.current().full());
                                ctx.render_pass(render_pass.clone(), image_index, clear_values, |ctx| {
                                    ctx.bind_pipeline(pipeline.clone());
                                    ctx.bind_descriptor_set(0, descriptor_set.clone());
                                    ctx.bind_vertex_buffer(vertex_buffer.current().full());
                                    ctx.draw(4, NUM_INSTANCES, 0, 0);
                                })
                            });

                            let g = range_event_start!("Present");
                            if let Err(e) = self.vulkan_renderer.queue_present(image_index, present_wait_ref) {
                                error!("Present error: {:?}", e);
                            }
                        }


                        if self.swapchain_recreated {
                            self.swapchain_recreated = false;
                        }
                        
                        allocator.destroy_old_resources();

                        vertex_buffer.next_frame();
                        self.render_finished.store(true, Ordering::Release);
                    }
                    RenderMessage::Resize { width, height } => {
                        let g = range_event_start!("Recreate Resize");
                        self.vulkan_renderer.recreate_resize((width, height));
                        self.swapchain_recreated = true;
                        self.resize_finished.store(true, Ordering::Release);
                        self.extent = [width as i32, height as i32];
                    }
                    RenderMessage::Exit => {
                        info!("Render thread exiting");
                        break;
                    }
                }

                if self.last_print.elapsed().as_secs() >= 3 {
                    allocator.dump_resource_usage();
                    self.last_print = Instant::now();
                }
            }
        }).unwrap()
    }
}