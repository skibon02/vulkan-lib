#[cfg(windows_platform)]
pub mod windows;
#[cfg(windows_platform)]
pub use windows as platform_impl;

#[cfg(x11_platform)]
pub mod x11;
#[cfg(all(x11_platform, not(wayland_platform)))]
pub use x11 as platform_impl;

#[cfg(wayland_platform)]
pub mod wayland;
#[cfg(all(wayland_platform, not(x11_platform)))]
pub use wayland as platform_impl;

#[cfg(all(wayland_platform, x11_platform))]
pub mod x11_or_wayland;
#[cfg(all(wayland_platform, x11_platform))]
pub use x11_or_wayland as platform_impl;

#[cfg(any(android_platform, orbital_platform, x11_platform))]
compile_error!("Unsupported platform");