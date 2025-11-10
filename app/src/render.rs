use std::sync::{mpsc, Arc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::thread::JoinHandle;
use log::{error, info, warn};
use sparkles::range_event_start;
use vulkan_lib::{BufferCopy, BufferUsageFlags, PipelineStageFlags, VulkanRenderer};

pub enum RenderMessage {
    Redraw { bg_color: [f32; 3] },
    Resize { width: u32, height: u32 },
    Exit,
}

pub struct RenderTask {
    rx: mpsc::Receiver<RenderMessage>,
    vulkan_renderer: VulkanRenderer,
    render_finished: Arc<AtomicBool>,
}

impl RenderTask {
    pub fn new(vulkan_renderer: VulkanRenderer) -> (Self, mpsc::Sender<RenderMessage>, Arc<AtomicBool>) {
        let (tx, rx) = mpsc::channel::<RenderMessage>();
        let render_finished = Arc::new(AtomicBool::new(true));

        (Self  {
            rx,
            vulkan_renderer,
            render_finished: render_finished.clone(),
        }, tx, render_finished)
    }

    pub fn spawn(mut self) -> JoinHandle<()> {
        thread::Builder::new().name("Render".into()).spawn(move || {
            let mut staging_buffer = self.vulkan_renderer.new_host_buffer(2048);
            staging_buffer.map_write(0, &[1,2,3,4,5]);

            let dev_buffer = self.vulkan_renderer.new_device_buffer(BufferUsageFlags::VERTEX_BUFFER | BufferUsageFlags::TRANSFER_DST, 2048);
            loop {
                let msg = self.rx.recv();
                if msg.is_err() {
                    info!("Render thread exiting due to channel close");
                    break;
                }
                match msg {
                    Ok(RenderMessage::Redraw { bg_color}) => {
                        let g = range_event_start!("Render");

                        // Map and update, host wait
                        staging_buffer.map_write(0, &[1,2,3,4,5]);
                        let staging_handle = staging_buffer.handle();
                        let dev_handle = dev_buffer.handle_static();
                        self.vulkan_renderer.runtime_state().record_device_commands(None, |ctx| {
                            ctx.buffer_copy_single(staging_handle, dev_handle, BufferCopy {
                                size: 5,
                                src_offset: 0,
                                dst_offset: 0
                            });
                        });

                        // Acquire next swapchain image
                        let (image_index, acquire_wait_ref, is_suboptimal) = match self.vulkan_renderer.acquire_next_image() {
                            Ok(result) => result,
                            Err(e) => {
                                error!("Failed to acquire next image: {:?}", e);
                                continue;
                            }
                        };

                        if is_suboptimal {
                            warn!("Swapch ain is suboptimal after acquire");
                        }

                        let present_wait_ref = self.vulkan_renderer.runtime_state().record_device_commands_signal(
                            Some(acquire_wait_ref.with_stages(PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)),
                            |_ctx| {
                            }
                        );

                        if let Err(e) = self.vulkan_renderer.queue_present(image_index, present_wait_ref) {
                            error!("Present error: {:?}", e);
                        }

                        self.render_finished.store(true, Ordering::Release);
                    }
                    Ok(RenderMessage::Resize { width, height }) => {
                        let g = range_event_start!("Recreate Resize");
                        self.vulkan_renderer.recreate_resize((width, height));
                    }
                    Ok(RenderMessage::Exit) | Err(_) => {
                        info!("Render thread exiting");
                        break;
                    }
                }
            }
        }).unwrap()
    }
}