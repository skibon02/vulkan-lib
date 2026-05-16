use std::os::raw::c_void;
use std::ptr::NonNull;
use calloop::channel::Sender;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle};
use wayland_client::protocol::wl_display::WlDisplay;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::Proxy;
use crate::DirectWindowMessage;
use crate::platform::wayland::WindowMessage;

#[derive(Default)]
pub struct WindowAttributes {
    pub title: String,
    pub initial_pos: Option<(i32, i32)>,
    pub initial_size: Option<(u16, u16)>,
    pub borderless: bool,
}

pub struct Window {
    id: u64,
    tx: Sender<WindowMessage>,
    window: WlSurface,
    display: WlDisplay,
}

impl Window {
    pub(crate) fn new(id: u64, tx: Sender<WindowMessage>, window: WlSurface, display: WlDisplay) -> Window {
        Window {
            id,
            tx,
            window,
            display
        }
    }
    pub fn close_window(mut self) {
        let _ = self.tx.send(WindowMessage::Direct(self.id, DirectWindowMessage::Close));
    }

    pub fn rwh(&self) -> RawWindowHandle {
        let ptr = self.window.id().as_ptr() as *mut c_void;
        let wh = WaylandWindowHandle::new(NonNull::new(ptr).unwrap());
        RawWindowHandle::Wayland(wh)
    }

    pub fn rdh(&self) -> RawDisplayHandle {
        let ptr = self.display.id().as_ptr() as *mut c_void;
        let dh = WaylandDisplayHandle::new(NonNull::new(ptr).unwrap());
        RawDisplayHandle::Wayland(dh)
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let _ = self.tx.send(WindowMessage::Direct(self.id, DirectWindowMessage::Close));
    }
}