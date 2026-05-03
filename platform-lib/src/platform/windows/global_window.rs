use log::{info, warn};
use sparkles::{instant_event, range_event_start};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{DefWindowProcW, DestroyWindow, GetWindowLongPtrW, SetWindowLongPtrW, GWL_USERDATA, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_OVERLAPPED};
use crate::GlobalUserMessage;
use crate::platform::platform_impl::{HandleResult, EVENT_LOOP_DATA};
use crate::platform::platform_impl::message::{RawMessage, WindowMessage};
use crate::window::{new_window_raw, Window};

pub struct GlobalInitData {
}

impl GlobalInitData {
    pub fn create_state(&mut self) -> Box<GlobalWindowState> {
        Box::new(
            GlobalWindowState::new()
        )
    }
}

pub struct GlobalWindowState {
}

impl GlobalWindowState {
    fn new() -> GlobalWindowState {
        GlobalWindowState {
        }
    }
}


pub(crate) struct GlobalWindow {
    handle: HWND
}

impl GlobalWindow {
    pub fn new() -> Self {
        let class_name = "GlobalWindowClass";
        let ex_style = WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_TOOLWINDOW;
        let style = WS_OVERLAPPED;
        let window_init_data = Box::new(GlobalInitData {
        });
        Self {
            handle: new_window_raw(None, class_name, window_init_data, None, None, style, ex_style, Some(global_window_callback))
        }
    }
    pub fn hwnd(&self) -> u64 {
        self.handle as u64
    }
}

impl Drop for GlobalWindow {
    fn drop(&mut self) {
        let g = range_event_start!("DestroyWindow");
        unsafe {
            DestroyWindow(self.handle);
        }
    }
}

pub fn handle_global_window_message(window: HWND, msg: RawMessage, state: &GlobalWindowState) -> HandleResult {
    if let RawMessage::UserMessage(data1, data2) = msg {
        let msg = unsafe { Box::from_raw(data1 as *mut GlobalUserMessage) };
        match *msg {
            GlobalUserMessage::CreateWindow(attrib) => {
                let wnd_tx= unsafe { Box::from_raw(data2 as *mut oneshot::Sender<Window>) };
                info!("Creating window {}", &attrib.title);
                let window = Window::new(attrib);
                wnd_tx.send(window).unwrap();
            }
        }
    }
    else {
        info!("-> GlobalWindow: {:?}", msg);
    }
    HandleResult::Default
}
unsafe extern "system" fn global_window_callback(
    window: HWND,
    raw_msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
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
        let init_data_ptr = createstruct.cs.lpCreateParams as *mut GlobalInitData;
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

    let state = unsafe { GetWindowLongPtrW(window, GWL_USERDATA) } as *const GlobalWindowState;
    let res = if state.is_null() {
        warn!("GlobalWindowState not yet initialized! Message: {:?}. Running default handler", &msg);
        HandleResult::Default
    }
    else {
        let state = unsafe {&*state};
        handle_global_window_message(window, msg, state)
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
