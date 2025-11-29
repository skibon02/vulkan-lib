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
use vulkan_lib::{descriptor_set, use_shader, AttachmentDescription, AttachmentLoadOp, AttachmentStoreOp, BufferUsageFlags, ClearColorValue, ClearDepthStencilValue, ClearValue, DescriptorType, Extent3D, Format, ImageAspectFlags, ImageLayout, ImageSubresourceLayers, Offset3D, PipelineStageFlags, SampleCountFlags, ShaderStageFlags, VulkanRenderer};
use vulkan_lib::runtime::resources::AttachmentsDescription;
use vulkan_lib::runtime::resources::images::ImageResourceHandle;
use vulkan_lib::runtime::resources::pipeline::{GraphicsPipelineDesc, VertexInputDesc};
use vulkan_lib::shaders::layout::types::{int, vec2, vec4};

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
    pub struct Input {
        pub pos: vec4<0>,
        pub norm: vec4<0>,
    }
}
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
        #[frag]
        1 -> UniformBuffer,
        #[frag]
        2 -> CombinedImageSampler,
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
                .final_layout(ImageLayout::DEPTH_ATTACHMENT_OPTIMAL);

            let mut attachments_desc = AttachmentsDescription::new(swapchain_attachment)
                .with_depth_attachment(depth_attachment);

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
            let attributes = CircleAttributes::get_attributes_configuration();

            let pipeline_desc = GraphicsPipelineDesc::new(use_shader!("solid"), attributes, smallvec![GlobalDescriptorSet::bindings()]);
            let pipeline = self.vulkan_renderer.new_pipeline(render_pass.handle(), pipeline_desc);

            let mut global_ds = self.vulkan_renderer.new_descriptor_set(GlobalDescriptorSet::bindings());
            let global_ds_buffer = self.vulkan_renderer.new_device_buffer(BufferUsageFlags::UNIFORM_BUFFER, 16);
            global_ds.bind_buffer(0, global_ds_buffer.handle_static());

            // let mut dev_buffer = self.vulkan_renderer.new_device_buffer(BufferUsageFlags::TRANSFER_DST | BufferUsageFlags::TRANSFER_SRC, 4*swapchain_extent.width as u64 * swapchain_extent.height as u64);
            loop {
                let msg = self.rx.recv();
                let Ok(msg) = msg else {
                    info!("Render thread exiting due to channel close");
                    break;
                };
                match msg {
                    RenderMessage::Redraw { bg_color} => 'render: {
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
                                depth_stencil: ClearDepthStencilValue::default(),
                            },
                            ClearValue {
                                color: bg_clear_color,
                            },
                        ];
                        let present_wait_ref = self.vulkan_renderer.record_device_commands_signal(Some(acquire_wait_ref.with_stages(PipelineStageFlags::TRANSFER)), |ctx| {
                            ctx.render_pass(render_pass.handle(), image_index, clear_values, |ctx| {
                                ctx.bind_pipeline(pipeline.handle());
                                ctx.bind_descriptor_set(0, global_ds.handle());
                                ctx.draw(4, 1, 0, 0);
                            })
                        });

                        let g = range_event_start!("Present");
                        if let Err(e) = self.vulkan_renderer.queue_present(image_index, present_wait_ref) {
                            error!("Present error: {:?}", e);
                        }

                        self.render_finished.store(true, Ordering::Release);
                    }
                    RenderMessage::Resize { width, height } => {
                        let g = range_event_start!("Recreate Resize");
                        self.vulkan_renderer.recreate_resize((width, height));
                        self.swapchain_image_handles = self.vulkan_renderer.swapchain_images();
                        self.swapchain_recreated = true;
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