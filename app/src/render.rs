use std::sync::{mpsc, Arc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::thread::JoinHandle;
use log::{error, info};
use sparkles::range_event_start;
use vulkan_lib::VulkanRenderer;

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
            loop {
                let msg = self.rx.recv();
                if msg.is_err() {
                    info!("Render thread exiting due to channel close");
                    break;
                }
                match msg {
                    Ok(RenderMessage::Redraw { bg_color}) => {
                        let g = range_event_start!("Render");
                        if let Err(e) = self.vulkan_renderer.render() {
                            error!("Render error: {:?}", e);
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