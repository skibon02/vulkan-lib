use std::{mem, ptr, thread};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use drop_guard::guard;
use log::{info, warn};
use sparkles::range_event_start;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{DefWindowProcW, DispatchMessageW, GetMessageW, GetWindowLongPtrW, PostQuitMessage, SetWindowLongPtrW, SetWindowLongW, TranslateMessage, CREATESTRUCTW, GWL_USERDATA, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, SC_KEYMENU, WMSZ_BOTTOM, WMSZ_BOTTOMLEFT, WMSZ_BOTTOMRIGHT, WMSZ_LEFT, WMSZ_RIGHT, WMSZ_TOP, WMSZ_TOPLEFT, WMSZ_TOPRIGHT, WM_ACTIVATE, WM_ACTIVATEAPP, WM_APPCOMMAND, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE, WM_CONTEXTMENU, WM_CREATE, WM_DESTROY, WM_DWMNCRENDERINGCHANGED, WM_ENABLE, WM_ENTERIDLE, WM_ENTERMENULOOP, WM_ENTERSIZEMOVE, WM_ERASEBKGND, WM_EXITMENULOOP, WM_EXITSIZEMOVE, WM_GETICON, WM_GETMINMAXINFO, WM_GETOBJECT, WM_IME_CONTROL, WM_IME_NOTIFY, WM_IME_REQUEST, WM_IME_SETCONTEXT, WM_INITMENU, WM_INPUTLANGCHANGE, WM_INPUTLANGCHANGEREQUEST, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MENUSELECT, WM_MOUSEACTIVATE, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_MOVE, WM_MOVING, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMBUTTONDBLCLK, WM_NCMBUTTONDOWN, WM_NCMBUTTONUP, WM_NCMOUSEHOVER, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_NCPAINT, WM_NCRBUTTONDBLCLK, WM_NCRBUTTONDOWN, WM_NCRBUTTONUP, WM_NCXBUTTONDBLCLK, WM_NCXBUTTONDOWN, WM_NCXBUTTONUP, WM_PAINT, WM_QUERYOPEN, WM_QUIT, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_SHOWWINDOW, WM_SIZE, WM_SIZING, WM_STYLECHANGED, WM_STYLECHANGING, WM_SYSCOMMAND, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_THEMECHANGED, WM_USERCHANGED, WM_WINDOWPOSCHANGED, WM_WINDOWPOSCHANGING, WM_XBUTTONDBLCLK, WM_XBUTTONDOWN, WM_XBUTTONUP};
use crate::platform::platform_impl::message::WindowMessage;
use crate::platform::windows::message::{MouseMessage, RawMessage};
use crate::platform::windows::types::SystemCommand;
use crate::window::WindowState;

pub mod window;
mod message;
mod types;

pub fn run_platform_loop() {
    let mut msg: MSG = unsafe { mem::zeroed() };

    thread::spawn(|| {
        let mut cnt = 0;
        loop {
            thread::sleep(Duration::from_secs(1));
            let cur = HANDLED.load(Ordering::Relaxed);
            let new = cur - cnt;

            info!("{} e/s", new);
            cnt = cur;
        }
    });
    loop {
        unsafe {
            let g = range_event_start!("GetMessage");
            if GetMessageW(&mut msg, ptr::null_mut(), 0, 0) == false.into() {
                info!("Exiting from platform loop...");
                break;
            }
            drop(g);


            TranslateMessage(&msg);
            let g = range_event_start!("DispatchMessage");
            DispatchMessageW(&msg);
            drop(g);
        }
    }
}

enum HandleResult {
    Handled,
    Default,
    Custom(LRESULT)
}

static CREATED_WINDOWS: AtomicUsize = AtomicUsize::new(0);
fn handle_message_inner(window: HWND, msg: RawMessage, state: &WindowState) -> HandleResult {
    match msg {
        RawMessage::WindowMessage(win) => match win {
            WindowMessage::NcCreate { .. } => {
                CREATED_WINDOWS.fetch_add(1, Ordering::Relaxed);
                HandleResult::Default
            },
            WindowMessage::Destroy => {
                info!("Window message[{:x}]: {:?}", window as usize, win);
                if CREATED_WINDOWS.fetch_sub(1, Ordering::Relaxed) == 1 {
                    info!("All windows closed! Will exit now");
                    // should exit
                    unsafe { PostQuitMessage(0); }
                }
                HandleResult::Default
            }
            
            
            WindowMessage::SystemCommand(sc, (x, y)) => {
                info!("Window message[{:x}]: {:?}", window as usize, win);
                if sc == SystemCommand::KeyMenu {
                    HandleResult::Handled
                } else {
                    HandleResult::Default
                }
            }

            WindowMessage::SetCursor(..) => {
                HandleResult::Default
            }
            _ => {
                info!("Window messag[{:x}]: {:?}", window as usize, win);
                HandleResult::Default
            }
        },
        RawMessage::MouseMessage(mouse) => match mouse {
            MouseMessage::MouseMove(..) | MouseMessage::NCMouseMove(..) | MouseMessage::NcHitTest(_, _)=> {
                // don't print them
                HandleResult::Default
            }
            m => {
                info!("Mouse: {:?}", m);
                HandleResult::Default
            }
        }
        RawMessage::KeyboardMessage(keyboard) => match keyboard {
            m => {
                info!("Keyboard: {:?}", m);
                HandleResult::Default
            }
        }
    }
}

#[derive(Default)]
struct EventLoopWindowLocal {
    last_surrogate: u16
}
struct EventLoopData {
    windows: HashMap<HWND, EventLoopWindowLocal>,
}

impl EventLoopData {
    fn new() -> EventLoopData {
        EventLoopData {
            windows: HashMap::new()
        }
    }

    pub fn push_u16_char(&mut self, hwnd: HWND, unit: u16) -> Option<char> {
        let state = self.windows.entry(hwnd).or_insert_with(Default::default);

        match (state.last_surrogate, unit) {
            (0, u) if (0xD800..=0xDBFF).contains(&u) => {
                state.last_surrogate = u;
                None
            }
            (high, low) if (0xDC00..=0xDFFF).contains(&low) && high != 0 => {
                state.last_surrogate = 0;
                char::decode_utf16([high, low]).next()?.ok()
            }
            (_, u) => {
                state.last_surrogate = 0;
                char::decode_utf16([u]).next()?.ok()
            }
        }
    }
    fn add_window(&mut self, window: HWND) {
        self.windows.insert(window, Default::default());
    }
    fn remove_window(&mut self, window: HWND) {
        self.windows.remove(&window);
    }
}

thread_local! {
    static EVENT_LOOP_DATA: RefCell<EventLoopData> = RefCell::new(EventLoopData::new());
}

static DEPTH: AtomicUsize = AtomicUsize::new(0);
pub static HANDLED: AtomicUsize = AtomicUsize::new(0);

/// TODO: handle panics in this callback correctly!
unsafe extern "system" fn public_window_callback(
    window: HWND,
    raw_msg: u32,
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

    let msg = EVENT_LOOP_DATA.with_borrow_mut(|event_loop_data| {
        unsafe {RawMessage::try_parse(raw_msg, wparam, lparam, window, event_loop_data)}
    });
    
    let Some(msg) = msg else {
        warn!("unknown message {:?}", raw_msg);
        let g = range_event_start!("UNKNOW");
        return unsafe { DefWindowProcW(window, raw_msg, wparam, lparam) };
    };

    if let RawMessage::WindowMessage(WindowMessage::NcCreate { createstruct }) = &msg {
        EVENT_LOOP_DATA.with_borrow_mut(|data| {
            data.add_window(window);
        });
        let init_data_ptr = createstruct.cs.lpCreateParams as *mut window::InitData;
        let Some(init_data) = (unsafe{init_data_ptr.as_mut()}) else {
            panic!("INIT_DATA address is null!");
        };
        let state = init_data.create_state();
        unsafe { SetWindowLongPtrW(window, GWL_USERDATA, Box::into_raw(state) as isize) };
    }

    if let RawMessage::WindowMessage(WindowMessage::NcDestroy) = &msg {
        EVENT_LOOP_DATA.with_borrow_mut(|data| {
            data.remove_window(window);
        });
    }

    let state = unsafe { GetWindowLongPtrW(window, GWL_USERDATA) } as *const WindowState;
    let res = if state.is_null() {
        warn!("WindowState not yet initialized! Message: {:?}. Running default handler", &msg);
        HandleResult::Default
    }
    else {
        let state = unsafe {&*state};
        handle_message_inner(window, msg, state)
    };

    match res {
        HandleResult::Handled => {
            0
        }
        HandleResult::Default => {
            unsafe { DefWindowProcW(window, raw_msg, wparam, lparam) }
        }
        HandleResult::Custom(val) => {
            val
        }
    }
}

