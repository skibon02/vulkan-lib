use std::{io, ptr};
use std::cell::Cell;
use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::prelude::*;
use log::info;
use sparkles::range_event_start;
use windows_sys::Win32::Foundation::{HMODULE, HWND, LPARAM, WPARAM};
use windows_sys::Win32::System::SystemServices::IMAGE_DOS_HEADER;
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use crate::platform::windows::public_window_callback;

pub fn encode_wide(string: impl AsRef<OsStr>) -> Vec<u16> {
    string.as_ref().encode_wide().chain(once(0)).collect()
}

pub struct InitData {
}

impl InitData {
    pub fn create_state(&mut self) -> Box<WindowState> {
        Box::new(
            WindowState::new()
        )
    }
}

pub struct WindowState {
    minimized: Cell<bool>,
    size: Cell<(u16, u16)>,
    pos: Cell<(u16, u16)>,
}

impl WindowState {
    fn new() -> WindowState {
        WindowState {
            minimized: Cell::new(false),
            size: Cell::new((100, 100)), // will be overwritten very soon
            pos: Cell::new((100, 100)), // will be overwritten very soon
        }
    }

    pub fn set_minimized(&self, is_minimized: bool) {
        self.minimized.set(is_minimized);
    }
    pub fn is_minimized(&self) -> bool {
        self.minimized.get()
    }

    pub fn set_size(&self, size: (u16, u16)) {
        self.size.set(size);
    }
    pub fn get_size(&self) -> (u16, u16) {
        self.size.get()
    }

    pub fn set_pos(&self, pos: (u16, u16)) {
        self.pos.set(pos);
    }
    pub fn get_pos(&self) -> (u16, u16) {
        self.pos.get()
    }
}

#[derive(Default)]
pub struct WindowAttributes {
    pub title: String,
    pub initial_pos: Option<(i32, i32)>,
    pub initial_size: Option<(u16, u16)>,
    pub borderless: bool
}

pub struct Window {
    hwnd: usize
}

impl Window {
    /// can be only created from inside input thread, so limit to pub(crate)
    /// Other api is safe to use from any thread
    /// We assume window can only be destroyed only after dropping this type
    pub(crate) fn new(attrib: WindowAttributes) -> Self {
        let class_name = "GeneralWindowClass";
        let (style, ex_style) = if attrib.borderless {
            // Only WS_POPUP is strictly necessary for the window to exist.
            // WS_VISIBLE is kept so it actually shows up.
            let style = WS_POPUP | WS_VISIBLE | WS_CLIPSIBLINGS;

            // Clear the extended styles that force 3D edges or borders.
            let ex_style = WS_EX_LEFT;

            (style, ex_style)
        }
        else {
            let ex_style = WS_EX_WINDOWEDGE | WS_EX_ACCEPTFILES | WS_EX_APPWINDOW;
            let style = WS_CAPTION | WS_BORDER | WS_CLIPSIBLINGS | WS_SYSMENU
                | WS_SIZEBOX | WS_MAXIMIZEBOX | WS_MINIMIZEBOX | WS_VISIBLE;

            (style, ex_style)
        };

        let window_init_data = Box::new(InitData {
        });
        Window {
            hwnd: new_window_raw(Some(&attrib.title), class_name, window_init_data, attrib.initial_pos, attrib.initial_size, style, ex_style, Some(public_window_callback)) as usize
        }
    }

    pub fn close_window(self) {
    }
    pub fn resize(&self, width: u16, height: u16) {
        self.send_message(UserWindowMessage::Resize(width, height))
    }

    fn send_message(&self, msg: UserWindowMessage) {
        let boxed = Box::new(msg);
        let addr = Box::into_raw(boxed) as u64;
        unsafe { PostMessageW(self.hwnd as HWND, WM_USER, addr as WPARAM, 0); }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        self.send_message(UserWindowMessage::Close);
    }
}

#[derive(Debug)]
pub enum UserWindowMessage {
    Close,
    Resize(u16, u16),
}

pub(crate) fn new_window_raw<T: Sized>(title: Option<&str>, class_name: &str, window_init_data: Box<T>,
                                       initial_pos: Option<(i32, i32)>,
                                       initial_size: Option<(u16, u16)>,
                                       style: WINDOW_STYLE, ex_style: WINDOW_EX_STYLE,
                                       callback: WNDPROC) -> HWND {
    let class_name = encode_wide(class_name);
    unsafe { register_window_class(&class_name, callback) };

    let title_encoded = if let Some(title) = title {
        encode_wide(title)
    }
    else {
        Vec::new()
    };
    let title_ptr = if title.is_some() {
        title_encoded.as_ptr()
    }
    else {
        ptr::null()
    };

    let (x, y) = if let Some(initial_pos) = initial_pos {
        (initial_pos.0, initial_pos.1)
    }
    else {
        (CW_USEDEFAULT, CW_USEDEFAULT)
    };
    let (width, height) = if let Some(initial_size) = initial_size {
        (initial_size.0 as i32, initial_size.1 as i32)
    }
    else {
        (CW_USEDEFAULT, CW_USEDEFAULT)
    };
    let g = range_event_start!("CreateWindow");
    let handle = unsafe {
        CreateWindowExW(
            ex_style,
            class_name.as_ptr(),
            title_ptr,
            style,
            x,
            y,
            width,
            height,
            ptr::null_mut(),
            ptr::null_mut(),
            get_instance_handle(),
            Box::into_raw(window_init_data) as *mut _,
        )
    };
    drop(g);

    if handle.is_null() {
        let err = io::Error::last_os_error();
        panic!("Failed to create window: {:?}!", err);
    }
    handle
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

unsafe fn register_window_class(class_name: &[u16], callback: WNDPROC) {
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: callback,
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: get_instance_handle(),
        hIcon: ptr::null_mut(),
        hCursor: ptr::null_mut(), // must be null in order for cursor state to work properly
        hbrBackground: ptr::null_mut(),
        // hbrBackground: unsafe { GetSysColorBrush(COLOR_WINDOW) },
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
