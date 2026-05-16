mod platform;

use std::ffi::c_void;
use wayland_client::{Connection, Proxy};
pub use platform::platform_impl::*;

#[derive(Copy, Clone, Debug)]
pub enum PlatformKind {
    Android,
    Windows,
    Orbital,
    X11,
    Wayland,
    X11OrWayland,
}

pub const fn current_platform() -> PlatformKind {
    #[cfg(windows_platform)]
    return PlatformKind::Windows;

    #[cfg(all(x11_platform, not(wayland_platform)))]
    return PlatformKind::X11;

    #[cfg(all(wayland_platform, not(x11_platform)))]
    return PlatformKind::Wayland;

    #[cfg(all(wayland_platform, x11_platform))]
    return PlatformKind::X11OrWayland;

    #[cfg(any(android_platform, orbital_platform))]
    return PlatformKind::Android;
}


/// If wayland is available, returns raw pointer to wl_display
pub fn wayland_connection() -> Option<*mut c_void> {
    #[cfg(wayland_platform)]
    {
        if let Ok(con) = Connection::connect_to_env() {
            Some(con.display().id().as_ptr() as *mut c_void)
        }
        else {
            None
        }
    }
    #[cfg(not(wayland_platform))]
    {
        None
    }
}