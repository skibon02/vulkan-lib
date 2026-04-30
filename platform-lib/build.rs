use cfg_aliases::cfg_aliases;

fn main() {
    cfg_aliases! {
        android_platform: { target_os="android" },
        windows_platform: { target_os = "windows" },
        free_unix: { all(unix, not(target_vendor = "apple"), not(android_platform), not(target_os = "emscripten")) },
        orbital_platform: { target_os = "redox" },

        x11_platform: { all(free_unix, not(orbital_platform), not(feature = "only_wayland")) },
        wayland_platform: { all(free_unix, not(orbital_platform), not(feature = "only_x11")) },
    }
    println!("cargo:rerun-if-changed=build.rs");
}