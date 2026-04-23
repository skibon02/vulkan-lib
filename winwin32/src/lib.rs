use std::{mem, ptr};
use windows_sys::Win32::Foundation::{GetLastError, WAIT_FAILED};
use windows_sys::Win32::System::Threading::INFINITE;
use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MsgWaitForMultipleObjectsEx, PeekMessageW, TranslateMessage, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT};

pub mod window;

pub fn run_platform_loop() {
    loop {
        wait_for_message();
        dispatch_messages();
    }
}

fn wait_for_message() {
    unsafe {
        let result = MsgWaitForMultipleObjectsEx(
            0,
            [ptr::null_mut()].as_ptr() as *const _,
            INFINITE,
            QS_ALLINPUT,
            MWMO_INPUTAVAILABLE,
        );
        if result == WAIT_FAILED {
            log::warn!("Failed to MsgWaitForMultipleObjectsEx: error code {}", GetLastError());
        }
    }
}

fn dispatch_messages() {
    let mut msg: MSG = unsafe { mem::zeroed() };

    loop {
        unsafe {
            if PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) == false.into() {
                break;
            }


            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

}