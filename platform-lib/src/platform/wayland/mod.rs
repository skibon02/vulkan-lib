use std::sync::atomic::{AtomicU64, Ordering};
use calloop::{channel, EventLoop};
use calloop::channel::Event;
use calloop_wayland_source::WaylandSource;
use log::info;
use tokio::sync::mpsc;
use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_client::protocol::{wl_compositor, wl_keyboard, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_display::WlDisplay;
use wayland_client::protocol::wl_keyboard::KeyState;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};
use wayland_protocols::xdg::shell::client::xdg_surface::XdgSurface;
use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
use wayland_protocols::xdg::decoration::zv1::client::{zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1};
use crate::platform;
use crate::window::{Window, WindowAttributes};

pub mod window;

enum EventLoopCommand {
    CreateWindow(WindowAttributes, oneshot::Sender<Window>),
    WindowCommand(u64, WindowCommand),
}

pub enum WindowCommand {
    Close,
}

pub struct WindowManager {
    tx: channel::Sender<EventLoopCommand>,
    notification_rx: mpsc::Receiver<platform::event::Notification>,
}

impl WindowManager {
    /// Create a new window. Returned handle should be preserved. When you drop `Window`, it automatically closes
    #[must_use]
    pub fn create_window(&mut self, attrib: WindowAttributes) -> Window {
        let (tx, rx) = oneshot::channel();
        self.tx.send(EventLoopCommand::CreateWindow(attrib, tx)).unwrap();
        rx.recv().unwrap()
    }

    pub async fn read_notification(&mut self) -> Option<platform::event::Notification> {
        self.notification_rx.recv().await
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
    let conn = Connection::connect_to_env().expect("Connection to wayland compositor");

    let event_queue = conn.new_event_queue();
    let qhandle = event_queue.handle();

    let display = conn.display();
    display.get_registry(&qhandle, ());

    let (cmd_tx, cmd_rx) = channel::channel();
    let (notification_tx, notification_rx) = mpsc::channel(100);

    let mut state = State {
        running: true,
        configured: false,
        compositor: None,
        wm_base: None,
        display: display.clone(),
        surfaces: vec![],
        decoration_manager: None,
        last_window_id: 0,
        notification_tx,
    };

    println!("Starting the example window app, press <ESC> to quit.");

    let mut event_loop: EventLoop<State> = EventLoop::try_new().unwrap();
    let loop_handle = event_loop.handle();

    loop_handle.insert_source(cmd_rx, |msg, _, state| {
        match msg {
            Event::Msg(msg) => match msg {
                EventLoopCommand::CreateWindow(attrib, tx) => {
                    let window = state.create_surface(&qhandle, attrib, cmd_tx.clone());
                    tx.send(window).unwrap();
                }
                EventLoopCommand::WindowCommand(win_id, direct_msg) => match direct_msg {
                    WindowCommand::Close => {
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

    let mut cmd_tx = Some(cmd_tx.clone());
    let mut notification_rx = Some(notification_rx);
    while state.running {
        event_loop.dispatch(None, &mut state).unwrap();
        if state.is_initialized() &&
            let Some(tx) = cmd_tx.take() &&
            let Some(notification_rx) = notification_rx.take() {
            T::spawn_logic_task(WindowManager {
                tx,
                notification_rx,
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
    configured: bool,
    compositor: Option<WlCompositor>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    display: WlDisplay,
    surfaces: Vec<WindowInner>,
    decoration_manager: Option<zxdg_decoration_manager_v1::ZxdgDecorationManagerV1>,
    last_window_id: u64,
    notification_tx: mpsc::Sender<platform::event::Notification>,
}

impl State {
    pub fn is_initialized(&self) -> bool {
        self.compositor.is_some() && self.wm_base.is_some()
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
                "wl_seat" => {
                    registry.bind::<wl_seat::WlSeat, _, _>(name, 1, qh, ());
                }
                "xdg_wm_base" => {
                    let wm_base = registry.bind::<xdg_wm_base::XdgWmBase, _, _>(name, 1, qh, ());
                    state.wm_base = Some(wm_base);
                }
                "zxdg_decoration_manager_v1" => {
                    let manager = registry.bind::<zxdg_decoration_manager_v1::ZxdgDecorationManagerV1, _, _>(name, 1, qh, ());
                    state.decoration_manager = Some(manager);
                }
                _ => {}
            }
        }
    }
}

use wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::Mode;

impl State {
    fn create_surface(&mut self, qh: &QueueHandle<State>, attrib: WindowAttributes, tx: channel::Sender<EventLoopCommand>) -> Window {
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

        if let Some(ref manager) = self.decoration_manager {
            let decoration = manager.get_toplevel_decoration(&toplevel, qh, window_state);
            decoration.set_mode(Mode::ServerSide);
        }

        base_surface.commit();

        self.surfaces.push(WindowInner {
            base_surface: base_surface.clone(),
            xdg_surface,
            toplevel,
            id: window_id
        });

        Window::new(window_id, tx, base_surface, self.display.clone())
    }
}

delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore WlDisplay);
delegate_noop!(State: ignore zxdg_decoration_manager_v1::ZxdgDecorationManagerV1);

impl Dispatch<zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1, WindowState> for State {
    fn event(
        _: &mut Self,
        _: &zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1,
        event: zxdg_toplevel_decoration_v1::Event,
        win: &WindowState,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = win.id;
        info!("zxdg_toplevel_decoration ({id}): {:?}", event);
    }
}

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
            state.configured = true;
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