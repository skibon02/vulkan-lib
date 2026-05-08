use std::cell::Cell;
use std::fs::File;
use std::os::fd::AsFd;
use std::sync::atomic::{AtomicU64, Ordering};
use calloop::{channel, EventLoop};
use calloop::channel::{Event, Sender};
use calloop_wayland_source::WaylandSource;
use log::info;
use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_client::protocol::{wl_buffer, wl_compositor, wl_keyboard, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_keyboard::KeyState;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};
use wayland_protocols::xdg::shell::client::xdg_surface::XdgSurface;
use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
use crate::window::{Window, WindowAttributes};

pub mod window;

pub enum DirectWindowMessage {
    Close,
}

enum WindowMessage {
    CreateWindow(WindowAttributes, oneshot::Sender<Window>),
    Direct(u64, DirectWindowMessage),
}

pub struct WindowManager {
    tx: channel::Sender<WindowMessage>,
}

impl WindowManager {
    pub fn create_window(&mut self, attrib: WindowAttributes) -> Window {
        let (tx, rx) = oneshot::channel();
        self.tx.send(WindowMessage::CreateWindow(attrib, tx)).unwrap();
        rx.recv().unwrap()
    }
}

struct WindowInner {
    base_surface: WlSurface,
    xdg_surface: XdgSurface,
    toplevel: XdgToplevel,
    id: u64
}

pub trait ApplicationLogic {
    fn spawn_logic_task(manager: WindowManager);
}

pub fn start_app<T: ApplicationLogic>() {
    let conn = Connection::connect_to_env().unwrap();

    let event_queue = conn.new_event_queue();
    let qhandle = event_queue.handle();

    let display = conn.display();
    display.get_registry(&qhandle, ());

    let mut state = State {
        running: true,
        compositor: None,
        buffer: None,
        wm_base: None,
        surfaces: vec![],
        last_window_id: 0,
    };

    let (event_tx, rx) = channel::channel();

    println!("Starting the example window app, press <ESC> to quit.");

    let mut event_loop: EventLoop<State> = EventLoop::try_new().unwrap();
    let loop_handle = event_loop.handle();

    loop_handle.insert_source(rx, |msg, _, state| {
        match msg {
            Event::Msg(msg) => match msg {
                WindowMessage::CreateWindow(attrib, tx) => {
                    let window = state.create_surface(&qhandle, attrib, event_tx.clone());
                    tx.send(window).unwrap();
                }
                WindowMessage::Direct(win_id, direct_msg) => match direct_msg {
                    DirectWindowMessage::Close => {
                        for win in state.surfaces.extract_if(.., |s| s.id == win_id) {
                            win.toplevel.destroy();
                            win.xdg_surface.destroy();
                            win.base_surface.destroy();
                            break;
                        }
                        if state.surfaces.is_empty() {
                            state.running = false;
                        }
                    }
                }
            }
            Event::Closed => {
                println!("Logic thread disconnected! closing event loop...");
                state.running = false;
            }
        }
    }).unwrap();

    WaylandSource::new(conn, event_queue).insert(loop_handle).unwrap();

    let mut tx = Some(event_tx.clone());
    while state.running {
        event_loop.dispatch(None, &mut state).unwrap();
        if let Some(tx) = tx.take() && state.is_initialized() {
            T::spawn_logic_task(WindowManager {
                tx
            });
        }
    }
}

#[derive(Default, Copy, Clone)]
struct WindowState {
    id: u64
}

struct State {
    running: bool,
    compositor: Option<WlCompositor>,
    buffer: Option<wl_buffer::WlBuffer>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    surfaces: Vec<WindowInner>,
    last_window_id: u64,
}

impl State {
    pub fn is_initialized(&self) -> bool {
        self.compositor.is_some() && self.buffer.is_some() && self.wm_base.is_some()
    }
    pub fn new_window_id(&mut self) -> u64 {
        let id = self.last_window_id;
        self.last_window_id += 1;
        id
    }
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(state: &mut Self, registry: &WlRegistry, event: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        info!("wl_registry: {:?}", event);
        if let wl_registry::Event::Global { name, interface, .. } = event {
            match &*interface {
                "wl_compositor" => {
                    let compositor =
                        registry.bind::<wl_compositor::WlCompositor, _, _>(name, 1, qh, ());
                    state.compositor = Some(compositor);
                }
                "wl_shm" => {
                    let shm = registry.bind::<wl_shm::WlShm, _, _>(name, 1, qh, ());

                    let (init_w, init_h) = (320, 240);

                    let mut file = tempfile::tempfile().unwrap();
                    draw(&mut file, (init_w, init_h));
                    let pool = shm.create_pool(file.as_fd(), (init_w * init_h * 4) as i32, qh, ());
                    let buffer = pool.create_buffer(
                        0,
                        init_w as i32,
                        init_h as i32,
                        (init_w * 4) as i32,
                        wl_shm::Format::Argb8888,
                        qh,
                        (),
                    );
                    state.buffer = Some(buffer.clone());
                }
                "wl_seat" => {
                    registry.bind::<wl_seat::WlSeat, _, _>(name, 1, qh, ());
                }
                "xdg_wm_base" => {
                    let wm_base = registry.bind::<xdg_wm_base::XdgWmBase, _, _>(name, 1, qh, ());
                    state.wm_base = Some(wm_base);
                }
                _ => {}
            }
        }
    }
}


fn draw(tmp: &mut File, (buf_x, buf_y): (u32, u32)) {
    use std::{cmp::min, io::Write};
    let mut buf = std::io::BufWriter::new(tmp);
    for y in 0..buf_y {
        for x in 0..buf_x {
            let a = 0xFF;
            let r = min(((buf_x - x) * 0xFF) / buf_x, ((buf_y - y) * 0xFF) / buf_y);
            let g = min((x * 0xFF) / buf_x, ((buf_y - y) * 0xFF) / buf_y);
            let b = min(((buf_x - x) * 0xFF) / buf_x, (y * 0xFF) / buf_y);
            buf.write_all(&[b as u8, g as u8, r as u8, a as u8]).unwrap();
        }
    }
    buf.flush().unwrap();
}

impl State {
    fn create_surface(&mut self, qh: &QueueHandle<State>, attrib: WindowAttributes, tx: Sender<WindowMessage>) -> Window {
        let window_id = self.new_window_id();
        let window_state = WindowState {
            id: window_id
        };

        let compositor = self.compositor.as_ref().unwrap();
        let wm_base = self.wm_base.as_ref().unwrap();
        let base_surface = compositor.create_surface(qh, window_state);

        let xdg_surface = wm_base.get_xdg_surface(&base_surface, qh, window_state);
        let toplevel = xdg_surface.get_toplevel(qh, window_state);
        toplevel.set_title("A fantastic window!".into());
        base_surface.commit();

        self.surfaces.push(WindowInner {
            base_surface,
            xdg_surface,
            toplevel,
            id: window_id
        });

        Window::new(window_id, tx)
    }
}

delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wl_buffer::WlBuffer);

impl Dispatch<wl_surface::WlSurface, WindowState> for State {
    fn event(state: &mut Self, proxy: &WlSurface, event: wl_surface::Event, win: &WindowState, conn: &Connection, qhandle: &QueueHandle<Self>) {
        let id = win.id;
        info!("wl_surface({id}): {:?}", event);
    }
}
impl Dispatch<xdg_wm_base::XdgWmBase, ()> for State {
    fn event(
        _: &mut Self,
        wm_base: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        info!("xdg_wm_base: {:?}", event);
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, WindowState> for State {
    fn event(
        state: &mut Self,
        xdg_surface: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        win: &WindowState,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = win.id;
        info!("xdg_surface ({id}): {:?}", event);
        if let xdg_surface::Event::Configure { serial, .. } = event {
            xdg_surface.ack_configure(serial);
            for surface in &state.surfaces {
                if let Some(ref buffer) = state.buffer {
                    surface.base_surface.attach(Some(buffer), 0, 0);
                    surface.base_surface.commit();
                }
            }
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, WindowState> for State {
    fn event(
        state: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        win: &WindowState,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = win.id;
        info!("xdg_toplevel ({id}): {:?}", event);
        if let xdg_toplevel::Event::Close = event {
            state.running = false;
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        info!("wl_seat: {:?}", event);
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(capabilities) } = event {
            if capabilities.contains(wl_seat::Capability::Keyboard) {
                seat.get_keyboard(qh, KbState {
                    entered: AtomicU64::new(0),
                });
            }
        }
    }
}

pub struct KbState {
    entered: AtomicU64
}
impl Dispatch<wl_keyboard::WlKeyboard, KbState> for State {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        kb: &KbState,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Key { key, state: key_state, .. } = event {
            let id = kb.entered.load(Ordering::Relaxed);
            info!("wl_keyboard (win {id}): {:?}", event);
            if key_state == WEnum::Value(KeyState::Pressed) {
                if key == 1 {
                    // ESC key
                    state.running = false;
                }
                // else if key == 33 {
                //     state.create_surface(qh);
                // }
                // else if key == 32 {
                //     if let Some(win) = state.surfaces.pop() {
                //         win.toplevel.destroy();
                //         win.xdg_surface.destroy()
                //         win.base_surface.destroy();
                //     }
                //     if state.surfaces.is_empty() {
                //         state.running = false;
                //     }
                // }
            }
        }
        else {
            info!("wl_keyboard: {:?}", event);
        }
        if let wl_keyboard::Event::Enter {surface, ..} = event {
            let win: &WindowState = surface.data().unwrap();
            let id = win.id;
            kb.entered.store(id, Ordering::Relaxed)
        }
    }
}