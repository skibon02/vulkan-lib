use std::{mem, ptr, thread};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use log::info;
use sparkles::range_event_start;
use windows_sys::Win32::Foundation::{GetLastError, WAIT_FAILED};
use windows_sys::Win32::System::Threading::INFINITE;
use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, MsgWaitForMultipleObjectsEx, PeekMessageW, TranslateMessage, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT};
use crate::window::HANDLED;

pub mod window;


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