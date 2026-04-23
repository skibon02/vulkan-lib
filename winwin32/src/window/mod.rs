use std::{io, mem, ptr};
use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::prelude::*;
use log::info;
use windows_sys::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::SystemServices::IMAGE_DOS_HEADER;
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DefWindowProcW, DestroyWindow, PostMessageW, RegisterClassExW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWL_USERDATA, WINDOW_LONG_PTR_INDEX, WNDCLASSEXW, WM_NCCREATE, WM_CREATE, WS_EX_WINDOWEDGE, WS_EX_ACCEPTFILES, WS_CAPTION, WS_BORDER, WS_CLIPSIBLINGS, WS_SYSMENU, WS_SIZEBOX, WS_VISIBLE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_EX_APPWINDOW};

pub fn encode_wide(string: impl AsRef<OsStr>) -> Vec<u16> {
    string.as_ref().encode_wide().chain(once(0)).collect()
}

struct WindowData {

}

pub struct Window {
    handle: HWND
}

impl Window {
    pub fn new() -> Self {
        let title = encode_wide("*_*");
        let class_name = encode_wide(&"MyWindowclass");
        unsafe { register_window_class(&class_name) };

        let ex_style = WS_EX_WINDOWEDGE | WS_EX_ACCEPTFILES | WS_EX_APPWINDOW;
        let style = WS_CAPTION | WS_BORDER | WS_CLIPSIBLINGS | WS_SYSMENU
            | WS_SIZEBOX | WS_MAXIMIZEBOX | WS_MINIMIZEBOX | WS_VISIBLE;
        let mut shared = WindowData {};
        let handle = unsafe {
            CreateWindowExW(
                ex_style,
                class_name.as_ptr(),
                title.as_ptr(),
                style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                ptr::null_mut(),
                ptr::null_mut(),
                get_instance_handle(),
                &mut shared as *mut _ as *mut _,
            )
        };

        if handle.is_null() {
            let err = io::Error::last_os_error();
            panic!("Failed to create window: {:?}!", err);
        }
        Window {
            handle
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.handle);
        }
    }
}
pub fn get_instance_handle() -> HMODULE {
    // Gets the instance handle by taking the address of the
    // pseudo-variable created by the microsoft linker:
    // https://devblogs.microsoft.com/oldnewthing/20041025-00/?p=37483

    // This is preferred over GetModuleHandle(NULL) because it also works in DLLs:
    // https://stackoverflow.com/questions/21718027/getmodulehandlenull-vs-hinstance

    unsafe extern "C" {
        static __ImageBase: IMAGE_DOS_HEADER;
    }

    unsafe { &__ImageBase as *const _ as _ }
}
unsafe fn register_window_class(class_name: &[u16]) {
    let class = WNDCLASSEXW {
        cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(public_window_callback),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: get_instance_handle(),
        hIcon: ptr::null_mut(),
        hCursor: ptr::null_mut(), // must be null in order for cursor state to work properly
        hbrBackground: ptr::null_mut(),
        lpszMenuName: ptr::null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: ptr::null_mut(),
    };

    // We ignore errors because registering the same window class twice would trigger
    //  an error, and because errors here are detected during CreateWindowEx anyway.
    // Also since there is no weird element in the struct, there is no reason for this
    //  call to fail.
    unsafe { RegisterClassExW(&class) };
}

unsafe extern "system" fn public_window_callback(
    window: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let userdata = unsafe { get_window_long(window, GWL_USERDATA) };

    // match (userdata, msg) {
    //     (0, WM_NCCREATE) => {
    //         info!("NC Create!")
    //         return unsafe { DefWindowProcW(window, msg, wparam, lparam) };
    //     },
    //     // Getting here should quite frankly be impossible,
    //     // but we'll make window creation fail here just in case.
    //     (0, WM_CREATE) => return -1,
    //     (_, WM_CREATE) => return unsafe { DefWindowProcW(window, msg, wparam, lparam) },
    //     (0, _) => return unsafe { DefWindowProcW(window, msg, wparam, lparam) },
    //     _ => {}
    // };


    // handle message, specific cases that return raw value and doesn't require DefWindowProc
    // match msg {
    //     WM_DPICHANGED |
    // }

    info!("Handling message: {:?}", msg);

    unsafe { DefWindowProcW(window, msg, wparam, lparam) }
}

pub(crate) unsafe fn get_window_long(hwnd: HWND, nindex: WINDOW_LONG_PTR_INDEX) -> isize {
    #[cfg(target_pointer_width = "64")]
    return unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, nindex) };
    #[cfg(target_pointer_width = "32")]
    return unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongW(hwnd, nindex) as isize
    };
}
