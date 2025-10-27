use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use log::{info, warn};
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use sparkles::{instant_event, range_event_start};
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard;
use winit::keyboard::NamedKey;
use winit::window::{Fullscreen, Window};
use vulkan_lib::VulkanRenderer;

pub struct App {
    app_finished: bool,
    // do not render while in collapsed state
    is_collapsed: bool,
    vulkan_renderer: VulkanRenderer,
    window: Window,


    // some stats
    frame_cnt: usize,
    last_sec: Instant,
}


impl App {
    pub fn new_winit(window: Window) -> App {
        let raw_window_handle = window.raw_window_handle().unwrap();
        let raw_display_handle = window.raw_display_handle().unwrap();
        let inner_size = window.inner_size();
        let vulkan_renderer = VulkanRenderer::new_for_window(raw_window_handle, raw_display_handle, (inner_size.width, inner_size.height)).unwrap();

        Self {
            is_collapsed: false,
            app_finished: false,
            vulkan_renderer,
            window,

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

            WindowEvent::RedrawRequested => {
                let g = range_event_start!("[APP] Redraw requested");
                if !self.app_finished && !self.is_collapsed {
                    self.vulkan_renderer.render();

                    // handle fps
                    self.frame_cnt += 1;
                    let elapsed_secs = self.last_sec.elapsed().as_secs_f32();
                    if elapsed_secs >= 1.0 {
                        let fps = self.frame_cnt as f32 / elapsed_secs;
                        info!("FPS: {:.0}", fps);
                        self.frame_cnt = 0;
                        self.last_sec = Instant::now();
                    }
                    
                    // schedule another redraw requested call
                    self.window.request_redraw();
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
                    self.vulkan_renderer.recreate_resize((size.width, size.height));
                    self.is_collapsed = false;
                }
            }
            _ => {

            }
        }

        Ok(())
    }
}