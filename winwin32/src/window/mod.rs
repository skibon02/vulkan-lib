use std::{io, mem, ptr};
use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use drop_guard::guard;
use log::{info, warn};
use sparkles::{instant_event, range_event_start};
use windows_sys::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::SystemServices::IMAGE_DOS_HEADER;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub fn encode_wide(string: impl AsRef<OsStr>) -> Vec<u16> {
    string.as_ref().encode_wide().chain(once(0)).collect()
}

struct InitData {
    v: u32,
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
        let mut shared = Box::new(InitData {
            v: 262,
        });
        let g = range_event_start!("CreateWindow");
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
                Box::into_raw(shared) as *mut _,
            )
        };
        drop(g);
        info!("Create window finished!");

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
        let g = range_event_start!("DestroyWindow");
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
        style: 0,
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


static DEPTH: AtomicUsize = AtomicUsize::new(0);
pub static HANDLED: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn public_window_callback(
    window: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DEPTH.fetch_add(1, Ordering::Relaxed);
    let g = guard((), |_| {
        DEPTH.fetch_sub(1, Ordering::Relaxed);
    });

    let depth = DEPTH.load(Ordering::Relaxed);
    if depth > 7 {
        info!("callback recursion depth is {}", depth)
    }
    HANDLED.fetch_add(1, Ordering::Relaxed);
    match msg {
        WM_NCCREATE => {
            let g = range_event_start!("NCCREATE");
            return unsafe { DefWindowProcW(window, msg, wparam, lparam) };
        },

        WM_CREATE => {
            let g = range_event_start!("CREATE");
            let createstruct_ptr = lparam as *mut CREATESTRUCTW;
            let Some(createstruct) = createstruct_ptr.as_mut() else {
                panic!("CREATESTRUCT address is null!");
            };

            let init_data_ptr = createstruct.lpCreateParams as *mut InitData;
            let Some(init_data) = init_data_ptr.as_mut() else {
                panic!("INIT_DATA address is null!");
            };

            return unsafe { DefWindowProcW(window, msg, wparam, lparam) }
        },
        _ => {}
    };


    // handle message, specific cases that return raw value and doesn't require DefWindowProc
    // match msg {
    //     WM_DPICHANGED |
    // }
    let g = match msg {
        WM_ACTIVATE => {
            range_event_start!("WM_ACTIVATE")
        }
        WM_ACTIVATEAPP => {
            range_event_start!("WM_ACTIVATEAPP")
        }
        WM_NCACTIVATE => {
            range_event_start!("WM_NCACTIVATE")
        }
        WM_DESTROY => {
            let g = range_event_start!("WM_DESTROY");
            unsafe { PostQuitMessage(0); }
            g
        }
        WM_NCDESTROY => {
            range_event_start!("WM_NCDESTROY")
        }
        WM_CLOSE => {
            range_event_start!("WM_CLOSE")
        }
        WM_SIZE => {
            range_event_start!("WM_SIZE")
        }
        WM_SIZING => {
            range_event_start!("WM_SIZING")
        }
        WM_QUERYOPEN => {
            range_event_start!("WM_QUERYOPEN")
        }
        WM_ENABLE => {
            range_event_start!("WM_ENABLE")
        }
        WM_ENTERSIZEMOVE => {
            range_event_start!("WM_ENTERSIZEMOVE")
        }
        WM_EXITSIZEMOVE => {
            range_event_start!("WM_EXITSIZEMOVE")
        }
        WM_GETICON => {
            range_event_start!("WM_GETICON")
        }
        WM_GETMINMAXINFO => {
            range_event_start!("WM_GETMINMAXINFO")
        }
        WM_INPUTLANGCHANGE => {
            range_event_start!("WM_INPUTLANGCHANGE")
        }
        WM_INPUTLANGCHANGEREQUEST => {
            range_event_start!("WM_INPUTLANGCHANGEREQUEST")
        }
        WM_MOVE => {
            range_event_start!("WM_MOVE")
        }
        WM_MOVING => {
            range_event_start!("WM_MOVING")
        }
        WM_QUIT => {
            range_event_start!("WM_QUIT")
        }
        WM_SHOWWINDOW => {
            range_event_start!("WM_SHOWWINDOW")
        }
        WM_STYLECHANGING => {
            range_event_start!("WM_STYLECHANGING")
        }
        WM_STYLECHANGED => {
            range_event_start!("WM_STYLECHANGED")
        }
        WM_THEMECHANGED => {
            range_event_start!("WM_THEMECHANGED")
        }
        WM_USERCHANGED => {
            range_event_start!("WM_USERCHANGED")
        }
        WM_WINDOWPOSCHANGED => {
            range_event_start!("WM_WINDOWPOSCHANGED")
        }
        WM_WINDOWPOSCHANGING => {
            range_event_start!("WM_WINDOWPOSCHANGING")
        }
        WM_SETCURSOR => {
            range_event_start!("WM_SETCURSOR")
        }
        WM_NCMOUSEMOVE => {
            range_event_start!("WM_NCMOUSEMOVE")
        }
        WM_MOUSEMOVE => {
            range_event_start!("WM_MOUSEMOVE")
        }
        WM_NCMOUSEHOVER => {
            range_event_start!("WM_NCMOUSEHOVER")
        }
        WM_NCMOUSELEAVE => {
            range_event_start!("WM_NCMOUSELEAVE")
        }
        WM_MOUSEWHEEL => {
            range_event_start!("WM_MOUSEWHEEL")
        }
        WM_MOUSEHWHEEL => {
            range_event_start!("WM_MOUSEWHEEL")
        }
        WM_MOUSEACTIVATE => {
            range_event_start!("WM_MOUSEACTIVATE")
        }
        WM_CONTEXTMENU => {
            range_event_start!("WM_CONTEXTMENU")
        }
        WM_ENTERMENULOOP => {
            warn!("WM_ENTERMENULOOP!");
            range_event_start!("WM_ENTERMENULOOP")
        }
        WM_INITMENU => {
            warn!("WM_INITMENU!");
            range_event_start!("WM_INITMENU")
        }
        WM_MENUSELECT => {
            warn!("WM_MENUSELECT!");
            range_event_start!("WM_MENUSELECT")
        }
        WM_ENTERIDLE => {
            warn!("WM_ENTERIDLE!");
            range_event_start!("WM_ENTERIDLE")
        }
        WM_EXITMENULOOP => {
            warn!("WM_EXITMENULOOP");
            range_event_start!("WM_EXITMENULOOP")
        }
        WM_CAPTURECHANGED => {
            range_event_start!("WM_CAPTURECHANGED")
        }
        WM_NCHITTEST => {
            range_event_start!("WM_NCHITTEST")
        }

        WM_SYSKEYDOWN => {
            range_event_start!("WM_SYSKEYDOWN")
        }
        WM_SYSKEYUP => {
            range_event_start!("WM_SYSKEYUP")
        }
        WM_SYSCOMMAND => {
            info!("SYSCOMMAND!");
            if wparam as u32 & 0xFFF0 == SC_KEYMENU {
                return 0;
            }
            range_event_start!("WM_SYSCOMMAND")
        }
        WM_KEYDOWN => {
            range_event_start!("WM_KEYDOWN")
        }
        WM_KEYUP => {
            range_event_start!("WM_KEYUP")
        }
        WM_CHAR => {
            range_event_start!("WM_CHAR")
        }
        WM_NCCALCSIZE => {
            range_event_start!("WM_NCCALCSIZE")
        }
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => {
            range_event_start!("WM_*BUTTONDOWN")
        }
        WM_NCLBUTTONDOWN | WM_NCRBUTTONDOWN | WM_NCMBUTTONDOWN | WM_NCXBUTTONDOWN => {
            range_event_start!("WM_NC*BUTTONDOWN")
        }
        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP=> {
            range_event_start!("WM_*BUTTONUP")
        }
        WM_NCLBUTTONUP | WM_NCRBUTTONUP | WM_NCMBUTTONUP | WM_NCXBUTTONUP=> {
            range_event_start!("WM_NC*BUTTONUP")
        }
        WM_LBUTTONDBLCLK | WM_RBUTTONDBLCLK | WM_MBUTTONDBLCLK | WM_XBUTTONDBLCLK => {
            range_event_start!("WM_*BUTTONDBLCLK")
        }
        WM_NCLBUTTONDBLCLK | WM_NCRBUTTONDBLCLK | WM_NCMBUTTONDBLCLK | WM_NCXBUTTONDBLCLK => {
            range_event_start!("WM_NC*BUTTONDBLCLK")
        }
        WM_APPCOMMAND => {
            range_event_start!("WM_APPCOMMAND")
        }
        WM_GETOBJECT => {
            range_event_start!("WM_ERASEBKGND")
        }

        WM_PAINT => {
            range_event_start!("WM_PAINT")
        }
        WM_NCPAINT => {
            range_event_start!("WM_NCPAINT")
        }
        WM_ERASEBKGND => {
            range_event_start!("WM_ERASEBKGND")
        }
        WM_DWMNCRENDERINGCHANGED => {
            range_event_start!("WM_DWMNCRENDERINGCHANGED")
        }

        // ime
        WM_IME_SETCONTEXT => {
            range_event_start!("WM_IME_SETCONTEXT")
        }
        WM_IME_NOTIFY => {
            range_event_start!("WM_IME_NOTIFY")
        }
        WM_IME_REQUEST => {
            range_event_start!("WM_IME_REQUEST")
        }
        WM_IME_CONTROL => {
            range_event_start!("WM_IME_CONTROL")
        }

        // wmsz
        WMSZ_BOTTOMLEFT | WMSZ_BOTTOM | WMSZ_BOTTOMRIGHT |
        WMSZ_LEFT | WMSZ_RIGHT | WMSZ_TOP | WMSZ_TOPLEFT | WMSZ_TOPRIGHT => {
            range_event_start!("WMSZ_*")
        }

        _ => {
            warn!("unknown message {:?}", msg);
            range_event_start!("UNKNOW")
        }
    };

    // info!("Handling message: {:?}", msg);

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
