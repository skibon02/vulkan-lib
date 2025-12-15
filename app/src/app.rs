use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use log::{error, info, warn};
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use sparkles::range_event_start;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard;
use winit::keyboard::NamedKey;
use winit::window::{Fullscreen, Window};
use vulkan_lib::{BufferUsageFlags, VulkanRenderer};
use crate::component::Component;
use crate::layout::calculator::LayoutCalculator;
use crate::render;
use crate::render::RenderMessage;

pub struct App {
    app_finished: bool,
    // do not render while in collapsed state
    is_collapsed: bool,
    start_time: Instant,

    window: Window,

    // ui
    component: Component,
    layout_calculator: LayoutCalculator,

    // Rendering thread
    render_tx: mpsc::Sender<RenderMessage>,
    render_thread: Option<JoinHandle<()>>,
    render_ready: Arc<AtomicBool>,
    resize_ready: Arc<AtomicBool>,

    // some stats
    frame_cnt: usize,
    last_sec: Instant,
}


impl App {
    pub fn new_winit(window: Window) -> App {
        let raw_window_handle = window.raw_window_handle().unwrap();
        let raw_display_handle = window.raw_display_handle().unwrap();
        let inner_size = window.inner_size();

        let mut vulkan_renderer = VulkanRenderer::new_for_window(raw_window_handle, raw_display_handle, (inner_size.width, inner_size.height)).unwrap();
        vulkan_renderer.test_buffer_sizes(BufferUsageFlags::TRANSFER_DST);
        vulkan_renderer.test_buffer_sizes(BufferUsageFlags::TRANSFER_SRC);
        vulkan_renderer.test_buffer_sizes(BufferUsageFlags::VERTEX_BUFFER);
        vulkan_renderer.test_buffer_sizes(BufferUsageFlags::UNIFORM_BUFFER);
        let (render_task, render_tx, render_ready, resize_ready) = render::RenderTask::new(vulkan_renderer);
        let render_jh = render_task.spawn();
        
        // create UI component
        let mut component = Component::new();
        let mut layout_calculator =  LayoutCalculator::new();

        info!("Initializing UI component...");
        component.init(&mut layout_calculator);
        info!("Done!");

        Self {
            is_collapsed: false,
            app_finished: false,
            start_time: Instant::now(),
            
            component,
            layout_calculator,

            window,

            render_tx,
            render_ready,
            resize_ready,
            render_thread: Some(render_jh),

            frame_cnt: 0,
            last_sec: Instant::now(),
        }
    }
    pub fn is_finished(&self) -> bool {
        self.app_finished
    }

    pub fn handle_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) -> anyhow::Result<()> {
        match &event {
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event:
                winit::event::KeyEvent {
                    logical_key: keyboard::Key::Named(NamedKey::GoBack | NamedKey::BrowserBack),
                    state: ElementState::Pressed,
                    ..
                },
                ..
            } => {
                let g = range_event_start!("[APP] Close requested");
                info!("Close requested...");
                self.app_finished = true;
            }

            WindowEvent::KeyboardInput {
                event:
                winit::event::KeyEvent {
                    logical_key: keyboard::Key::Named(NamedKey::F11),
                    state: ElementState::Pressed,
                    ..
                },
                ..
            } => {
                if self.window.fullscreen().is_none() {
                    let g = range_event_start!("[APP] Enable fullscreen");
                    let monitor = self.window.current_monitor().unwrap();
                    // find max by width and refresh rate
                    let mode = monitor
                        .video_modes()
                        .map(|m| (m.size().width, m.refresh_rate_millihertz(), m))
                        .max_by_key(|(w, hz, m)| w * 5000 + * hz)
                        .map(|(_, _, m)| m)
                        .unwrap();
                    info!("Entering fullscreen mode {:?}, refresh rate: {}", mode.size(), mode.refresh_rate_millihertz() as f32 / 1000.0);
                    self.window
                        .set_fullscreen(Some(Fullscreen::Exclusive(mode)));
                } else {
                    let g = range_event_start!("[APP] Exit fullscreen mode");
                    self.window.set_fullscreen(None);
                }
            }

            WindowEvent::RedrawRequested => 'handling: {
                let g = range_event_start!("[APP] Redraw requested");
                let g = range_event_start!("[APP] window.request_redraw call");
                self.window.request_redraw();
                drop(g);
                if !self.app_finished && !self.is_collapsed {
                    if !self.render_ready.swap(false, Ordering::Acquire) {
                        break 'handling;
                    }
                    
                    // poll UI logic
                    self.component.poll(&mut self.layout_calculator);

                    let size = self.window.inner_size();
                    self.layout_calculator.calculate_layout(size.width, size.height);

                    // take UI elements to render
                    let elements = self.layout_calculator.get_elements();
                    // convert into primitive elements, fill instance buffer (text -> list of symbols, img/box -> rects)

                    let _ = self.render_tx.send(RenderMessage::Redraw {
                        bg_color: [0.0, 0.0, 0.0],
                    });

                    // handle fps
                    self.frame_cnt += 1;
                    let elapsed_secs = self.last_sec.elapsed().as_secs_f32();
                    if elapsed_secs >= 1.0 {
                        let fps = self.frame_cnt as f32 / elapsed_secs;
                        info!("FPS: {:.0}", fps);
                        self.frame_cnt = 0;
                        self.last_sec = Instant::now();
                    }
                }
            }
            WindowEvent::Resized(size) => {
                static FIRST_RESIZE: AtomicBool = AtomicBool::new(true);
                if FIRST_RESIZE.swap(false, Ordering::Relaxed) {
                    return Ok(());
                }

                info!("Resized to {}x{}", size.width, size.height);
                if size.width == 0 || size.height == 0 {
                    warn!("One of dimensions is 0! Suspending rendering...");
                    self.is_collapsed = true;
                } else {
                    if self.is_collapsed {
                        info!("Continue rendering...");
                    }

                    // wait until previous resize operation is finished
                    while !self.resize_ready.swap(false, Ordering::Relaxed) {
                        thread::sleep(Duration::from_millis(1));
                    }

                    let _ = self.render_tx.send(RenderMessage::Resize {
                        width: size.width,
                        height: size.height,
                    });
                    self.is_collapsed = false;
                }
            }
            _ => {

            }
        }

        Ok(())
    }
}

impl Drop for App {
    fn drop(&mut self) {
        info!("AppState dropping, sending exit message to render thread");
        let _ = self.render_tx.send(RenderMessage::Exit);

        if let Some(thread) = self.render_thread.take() {
            info!("Waiting for render thread to finish...");
            if let Err(e) = thread.join() {
                error!("Error joining render thread: {:?}", e);
            } else {
                info!("Render thread finished successfully");
            }
        }
    }
}
