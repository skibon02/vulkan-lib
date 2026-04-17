use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::{mem, thread};
use std::cmp::max;
use std::slice::from_raw_parts;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use log::{error, info, warn};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use sparkles::{instant_event, range_event_start};
use winit::dpi::PhysicalSize;
use winit::event::{ButtonSource, ElementState, MouseButton, PointerSource, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard;
use winit::keyboard::NamedKey;
use winit::monitor::Fullscreen;
use winit::window::Window;
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

    window: Box<dyn Window>,

    // ui
    cursor_pos: (f64, f64),
    component: Component,
    layout_calculator: LayoutCalculator,
    prev_win_size: PhysicalSize<u32>,

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
    pending_resize: Arc<AtomicU64>,

    // some stats
    frame_cnt: usize,
    last_sec: Instant,
}


impl App {
    pub fn new_winit(window: Box<dyn Window>, instances: Vec<SolidAttributes>) -> App {
        let raw_window_handle = window.window_handle().unwrap().as_raw();
        let raw_display_handle = window.display_handle().unwrap().as_raw();
        let inner_size = window.surface_size();
        info!("Window created! ({}x{})", inner_size.width, inner_size.height);

        // try load resource
        let font_data = get_resource(Path::join("fonts".as_ref(), "Ubuntu-Regular.ttf")).unwrap();

        let api_version = vk::API_VERSION_1_1;
        let vulkan_renderer = VulkanInstance::new_for_handle(raw_window_handle, raw_display_handle, (inner_size.width, inner_size.height), api_version).unwrap();
        let shared = vulkan_renderer.shared();
        let mut allocator = vulkan_renderer.new_allocator();
        let pending_resize = Arc::new(AtomicU64::new(0));
        let (render_task, render_tx, render_ready) = render::RenderTask::new(vulkan_renderer, inner_size, pending_resize.clone());
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

            prev_win_size: PhysicalSize::default(),
            frame_counter,
            instances_updated: true,
            instances,
            staging,
            instance_buffers,
            shared,
            allocator,

            render_tx,
            render_ready,
            pending_resize,
            render_thread: Some(render_jh),

            frame_cnt: 0,
            last_sec: Instant::now(),
        }
    }
    pub fn process_touch(&mut self, pos: (f64, f64)) {
        self.instances.push(SolidAttributes {
            pos: [pos.0 as i32 - 20, pos.1 as i32 - 20].into(),
            size: [40, 40].into(),
            d: 0.5.into(),
            color: [1.0, 1.0, 1.0, 1.0].into(),
        });
        self.instances_updated = true;
    }
    pub fn is_finished(&self) -> bool {
        self.app_finished
    }

    pub fn handle_event(&mut self, event_loop: &dyn ActiveEventLoop, event: WindowEvent) -> anyhow::Result<()> {
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
                        .map(|m| (m.size().width, m.refresh_rate_millihertz().map(|v| v.get()).unwrap_or(0), m))
                        .max_by_key(|(w, hz, _)| w * 5000 + hz)
                        .map(|(_, _, m)| m)
                        .unwrap();
                    let hz = mode.refresh_rate_millihertz().map(|v| v.get()).unwrap_or(0);
                    info!("Entering fullscreen mode {:?}, refresh rate: {}", mode.size(), hz as f32 / 1000.0);
                    self.window
                        .set_fullscreen(Some(Fullscreen::Exclusive(monitor, mode)));
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
                    //
                    // if self.instances_updated {
                    //     self.instances_updated = false;
                    //     let g = range_event_start!("instance buffer prepare");
                    //
                    //     // prepare new instances
                    //     let bytes_len = self.instances.len() * size_of::<SolidAttributes>();
                    //     let mut range = self.staging.allocate(&mut self.allocator, bytes_len);
                    //     range.update(|r| {
                    //         let bytes = unsafe { from_raw_parts(self.instances.as_ptr() as *const u8, bytes_len) };
                    //         r[..bytes_len].copy_from_slice(bytes);
                    //     });
                    //     self.render_tx.send(RenderMessage::UpdateInstances {
                    //         staging: range,
                    //         buf: self.instance_buffers.current().clone()
                    //     }).unwrap();
                    // }

                    // Run layout and produce render rects
                    let size = self.window.surface_size();
                    if size != self.prev_win_size {
                        // self.prev_win_size = size;
                        // self.layout_calculator.calculate_layout(size.width, size.height);
                        
                        // let (w, h) = self.layout_calculator.get_min_root_size();
                        // self.window.set_min_inner_size(Some(PhysicalSize::new(w, h)));
                        //
                        // let render_rects = self.layout_calculator.get_render_rects();
                        // self.instances.clear();
                        // for rect in &render_rects {
                        //     self.instances.push(SolidAttributes {
                        //         pos: [rect.x, rect.y].into(),
                        //         size: [rect.w, rect.h].into(),
                        //         d: rect.depth.into(),
                        //         color: [rect.r, rect.g, rect.b, rect.a].into(),
                        //     });
                        // }
                        //
                        // self.instances_updated = true;
                    }

                    self.frame_counter.increment_frame();
                    instant_event!("Send redraw message");
                    let _ = self.render_tx.send(RenderMessage::Redraw {
                        bg_color: [0.15, 0.12, 0.11],
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
            WindowEvent::SurfaceResized(size) => {
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

                    let packed = ((size.width as u64) << 32) | (size.height as u64);
                    self.pending_resize.store(packed, Ordering::Relaxed);
                    self.is_collapsed = false;
                }
            }
            WindowEvent::PointerButton {
                state: ElementState::Pressed,
                button: ButtonSource::Mouse(MouseButton::Left),
                ..
            } => {
                instant_event!("Mouse button press");
                self.process_touch(self.cursor_pos);
            }
            WindowEvent::PointerButton {
                state: ElementState::Pressed,
                button: ButtonSource::Touch { .. },
                position,
                ..
            } => {
                self.process_touch((position.x, position.y))
            }
            WindowEvent::PointerMoved {
                position,
                source: PointerSource::Mouse,
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
