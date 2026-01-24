use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::{mem, thread};
use std::slice::from_raw_parts;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use log::{error, info, warn};
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use sparkles::{instant_event, range_event_start};
use winit::event::{ElementState, MouseButton, TouchPhase, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard;
use winit::keyboard::NamedKey;
use winit::window::{Fullscreen, Window};
use vulkan_lib::{vk, VulkanInstance};
use vulkan_lib::queue::shared::SharedState;
use vulkan_lib::resources::buffer::BufferResource;
use vulkan_lib::resources::VulkanAllocator;
use vulkan_lib::vk::{BufferCreateFlags, BufferUsageFlags};
use crate::component::Component;
use crate::layout::calculator::LayoutCalculator;
use crate::render;
use crate::render::{RenderMessage, SolidAttributes};
use crate::resources::get_resource;
use crate::util::{DoubleBuffered, FrameCounter, TrippleAutoStaging};

pub struct App {
    app_finished: bool,
    // do not render while in collapsed state
    is_collapsed: bool,
    start_time: Instant,

    window: Window,

    // ui
    cursor_pos: (f64, f64),
    component: Component,
    layout_calculator: LayoutCalculator,

    // Rendering thread
    shared: SharedState,
    frame_counter: FrameCounter,
    instances_updated: bool,
    pub instances: Vec<SolidAttributes>,
    staging: TrippleAutoStaging,
    instance_buffers: DoubleBuffered<Arc<BufferResource>>,

    allocator: VulkanAllocator,
    render_tx: mpsc::Sender<RenderMessage>,
    render_thread: Option<JoinHandle<()>>,
    render_ready: Arc<AtomicBool>,
    resize_ready: Arc<AtomicBool>,

    // some stats
    frame_cnt: usize,
    last_sec: Instant,
}


impl App {
    pub fn new_winit(window: Window, instances: Vec<SolidAttributes>) -> App {
        let raw_window_handle = window.raw_window_handle().unwrap();
        let raw_display_handle = window.raw_display_handle().unwrap();
        let inner_size = window.inner_size();

        // try load resource
        let font_data = get_resource(Path::join("fonts".as_ref(), "Ubuntu-Regular.ttf")).unwrap();

        let api_version = vk::API_VERSION_1_1;
        let vulkan_renderer = VulkanInstance::new_for_handle(raw_window_handle, raw_display_handle, (inner_size.width, inner_size.height), api_version).unwrap();
        let shared = vulkan_renderer.shared();
        let mut allocator = vulkan_renderer.new_allocator();
        let (render_task, render_tx, render_ready, resize_ready) = render::RenderTask::new(vulkan_renderer, inner_size);
        let render_jh = render_task.spawn();
        
        // create UI component
        let mut component = Component::new();
        let mut layout_calculator =  LayoutCalculator::new();

        info!("Initializing UI component...");
        component.init(&mut layout_calculator);
        info!("Done!");

        let frame_counter = FrameCounter::new();

        let staging = TrippleAutoStaging::new(&frame_counter, &mut allocator, 4096);
        let instance_buffers = DoubleBuffered::new(&frame_counter, || {
            allocator.new_buffer(BufferUsageFlags::VERTEX_BUFFER | BufferUsageFlags::TRANSFER_DST, BufferCreateFlags::empty(), 100_000)
        });

        Self {
            is_collapsed: false,
            app_finished: false,
            start_time: Instant::now(),
            
            component,
            layout_calculator,
            cursor_pos: (0.0, 0.0),

            window,

            frame_counter,
            instances_updated: true,
            instances,
            staging,
            instance_buffers,
            shared,
            allocator,

            render_tx,
            render_ready,
            resize_ready,
            render_thread: Some(render_jh),

            frame_cnt: 0,
            last_sec: Instant::now(),
        }
    }
    pub fn process_touch(&mut self, pos: (f64, f64)) {
        self.instances.push(SolidAttributes {
            pos: [pos.0 as i32 - 20, pos.1 as i32 - 20].into(),
            size: [40, 40].into(),
            d: 0.5.into()
        });
        self.instances_updated = true;
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

                    if self.instances_updated {
                        self.instances_updated = false;
                        let g = range_event_start!("instance buffer prepare");

                        // prepare new instances
                        let bytes_len = self.instances.len() * size_of::<SolidAttributes>();
                        let mut range = self.staging.allocate(&mut self.allocator, bytes_len);
                        range.update(|r| {
                            let bytes = unsafe { from_raw_parts(self.instances.as_ptr() as *const u8, bytes_len) };
                            r[..bytes_len].copy_from_slice(bytes);
                        });
                        self.render_tx.send(RenderMessage::UpdateInstances {
                            staging: range,
                            buf: self.instance_buffers.current().clone()
                        }).unwrap();
                    }

                    // poll UI logic
                    self.component.poll(&mut self.layout_calculator);

                    let size = self.window.inner_size();
                    self.layout_calculator.calculate_layout(size.width, size.height);

                    // take UI elements to render
                    let elements = self.layout_calculator.get_elements();
                    // convert into primitive elements, fill instance buffer (text -> list of symbols, img/box -> rects)

                    self.frame_counter.increment_frame();
                    instant_event!("Send redraw message");
                    let _ = self.render_tx.send(RenderMessage::Redraw {
                        bg_color: [0.7, 0.3, 0.9],
                    });

                    self.allocator.destroy_old_resources();

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
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                instant_event!("Mouse button press");
                self.process_touch(self.cursor_pos);
            }
            WindowEvent::Touch(t) => {
                if t.phase == TouchPhase::Started {
                    self.process_touch((t.location.x, t.location.y))
                }
            }
            WindowEvent::CursorMoved {
                position,
                ..
            } => {
                self.cursor_pos = (position.x, position.y);
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
