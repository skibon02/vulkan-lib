mod platform;

use std::ffi::c_void;
use std::sync::OnceLock;
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


#[cfg(wayland_platform)]
use wayland_client::{Connection, Proxy};

// todo: consider using mutex for cases where wayland connection got available dynamically
#[cfg(wayland_platform)]
static DUMMY_CONNECTION: OnceLock<Option<Connection>> = OnceLock::new();

/// If wayland is available, returns raw pointer to wl_display
pub fn wayland_connection() -> Option<*mut c_void> {
    #[cfg(wayland_platform)]
    {
        let con = DUMMY_CONNECTION.get_or_init(|| {
            Connection::connect_to_env().ok()
        });
        if let Some(con) = con {
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