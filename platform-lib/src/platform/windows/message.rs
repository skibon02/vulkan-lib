use log::{info, warn};
use sparkles::instant_event;
use windows_sys::Win32::Foundation::{LPARAM, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{*};
use crate::platform::windows::types::{HitTest, MouseKeys, SystemCommand};

#[derive(Debug)]
pub enum RawMessage {
    KeyUp {
        vk: u32
    },
    KeyDown {
        vk: u32
    },
    Create {
        createstruct: *mut CREATESTRUCTW,
    },
    SystemCommand(SystemCommand, (i16, i16)),
    NCMouseMove(i16, i16, Option<HitTest>),
    MouseMove(i16, i16, MouseKeys),
    NcHitTest(i16, i16),
    SetCursor(Option<HitTest>, u32),
    Close,
}

impl RawMessage {
    pub fn try_parse(msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<RawMessage> {
        match msg {
            WM_CREATE => Some(RawMessage::Create {
                createstruct: lparam as *mut CREATESTRUCTW,
            }),
            WM_KEYUP => Some(RawMessage::KeyUp {
                vk: wparam as u32
            }),
            WM_KEYDOWN => Some(RawMessage::KeyDown {
                vk: wparam as u32
            }),
            WM_CLOSE => Some(RawMessage::Close),
            WM_SYSCOMMAND => {
                info!("SYSCOMMAND!");
                let sc = SystemCommand::from_raw(wparam, lparam);

                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                Some(RawMessage::SystemCommand(sc, (x, y)))
            }
            WM_NCMOUSEMOVE => {
                let hit_test = HitTest::from_isize((lparam & 0xFFFF) as i16 as isize);
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                Some(RawMessage::NCMouseMove(x, y, hit_test))
            }
            WM_MOUSEMOVE => {
                let keys = MouseKeys::from_bits_truncate(wparam);
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                Some(RawMessage::MouseMove(x, y, keys))
            }
            WM_NCHITTEST => {
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                Some(RawMessage::NcHitTest(x, y))
            }
            WM_SETCURSOR => {
                let hit_test = HitTest::from_isize((lparam & 0xFFFF) as i16 as isize);
                let trig_msg = ((lparam >> 16) & 0xFFFF) as u32;

                Some(RawMessage::SetCursor(hit_test, trig_msg))
            }



            WM_MOVE => {
                instant_event!("WM_MOVE");
                None
            }
            WM_MOVING => {
                instant_event!("WM_MOVING");
                None
            }
            WM_ACTIVATE => {
                instant_event!("WM_ACTIVATE");
                None
            }
            WM_ACTIVATEAPP => {
                instant_event!("WM_ACTIVATEAPP");
                None
            }
            WM_NCACTIVATE => {
                instant_event!("WM_NCACTIVATE");
                None
            }
            WM_DESTROY => {
                instant_event!("WM_DESTROY");
                unsafe { PostQuitMessage(0); }
                None
            }
            WM_NCDESTROY => {
                instant_event!("WM_NCDESTROY");
                None
            }
            WM_SIZE => {
                instant_event!("WM_SIZE");
                None
            }
            WM_SIZING => {
                instant_event!("WM_SIZING");
                None
            }
            WM_QUERYOPEN => {
                instant_event!("WM_QUERYOPEN");
                None
            }
            WM_ENABLE => {
                instant_event!("WM_ENABLE");
                None
            }
            WM_ENTERSIZEMOVE => {
                instant_event!("WM_ENTERSIZEMOVE");
                None
            }
            WM_EXITSIZEMOVE => {
                instant_event!("WM_EXITSIZEMOVE");
                None
            }
            WM_GETICON => {
                instant_event!("WM_GETICON");
                None
            }
            WM_GETMINMAXINFO => {
                instant_event!("WM_GETMINMAXINFO");
                None
            }
            WM_INPUTLANGCHANGE => {
                instant_event!("WM_INPUTLANGCHANGE");
                None
            }
            WM_INPUTLANGCHANGEREQUEST => {
                instant_event!("WM_INPUTLANGCHANGEREQUEST");
                None
            }
            WM_QUIT => {
                instant_event!("WM_QUIT");
                None
            }
            WM_SHOWWINDOW => {
                instant_event!("WM_SHOWWINDOW");
                None
            }
            WM_STYLECHANGING => {
                instant_event!("WM_STYLECHANGING");
                None
            }
            WM_STYLECHANGED => {
                instant_event!("WM_STYLECHANGED");
                None
            }
            WM_THEMECHANGED => {
                instant_event!("WM_THEMECHANGED");
                None
            }
            WM_USERCHANGED => {
                instant_event!("WM_USERCHANGED");
                None
            }
            WM_WINDOWPOSCHANGED => {
                instant_event!("WM_WINDOWPOSCHANGED");
                None
            }
            WM_WINDOWPOSCHANGING => {
                instant_event!("WM_WINDOWPOSCHANGING");
                None
            }
            WM_NCMOUSEHOVER => {
                instant_event!("WM_NCMOUSEHOVER");
                None
            }
            WM_NCMOUSELEAVE => {
                instant_event!("WM_NCMOUSELEAVE");
                None
            }
            WM_MOUSEWHEEL => {
                instant_event!("WM_MOUSEWHEEL");
                None
            }
            WM_MOUSEHWHEEL => {
                instant_event!("WM_MOUSEWHEEL");
                None
            }
            WM_MOUSEACTIVATE => {
                instant_event!("WM_MOUSEACTIVATE");
                None
            }
            WM_CONTEXTMENU => {
                instant_event!("WM_CONTEXTMENU");
                None
            }
            WM_ENTERMENULOOP => {
                warn!("WM_ENTERMENULOOP!");
                instant_event!("WM_ENTERMENULOOP");
                None
            }
            WM_INITMENU => {
                warn!("WM_INITMENU!");
                instant_event!("WM_INITMENU");
                None
            }
            WM_MENUSELECT => {
                warn!("WM_MENUSELECT!");
                instant_event!("WM_MENUSELECT");
                None
            }
            WM_ENTERIDLE => {
                warn!("WM_ENTERIDLE!");
                instant_event!("WM_ENTERIDLE");
                None
            }
            WM_EXITMENULOOP => {
                warn!("WM_EXITMENULOOP");
                instant_event!("WM_EXITMENULOOP");
                None
            }
            WM_CAPTURECHANGED => {
                instant_event!("WM_CAPTURECHANGED");
                None
            }
            WM_SYSKEYDOWN => {
                instant_event!("WM_SYSKEYDOWN");
                None
            }
            WM_SYSKEYUP => {
                instant_event!("WM_SYSKEYUP");
                None
            }
            WM_CHAR => {
                instant_event!("WM_CHAR");
                None
            }
            WM_NCCALCSIZE => {
                instant_event!("WM_NCCALCSIZE");
                None
            }
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => {
                instant_event!("WM_*BUTTONDOWN");
                None
            }
            WM_NCLBUTTONDOWN | WM_NCRBUTTONDOWN | WM_NCMBUTTONDOWN | WM_NCXBUTTONDOWN => {
                instant_event!("WM_NC*BUTTONDOWN");
                None
            }
            WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP=> {
                instant_event!("WM_*BUTTONUP");
                None
            }
            WM_NCLBUTTONUP | WM_NCRBUTTONUP | WM_NCMBUTTONUP | WM_NCXBUTTONUP=> {
                instant_event!("WM_NC*BUTTONUP");
                None
            }
            WM_LBUTTONDBLCLK | WM_RBUTTONDBLCLK | WM_MBUTTONDBLCLK | WM_XBUTTONDBLCLK => {
                instant_event!("WM_*BUTTONDBLCLK");
                None
            }
            WM_NCLBUTTONDBLCLK | WM_NCRBUTTONDBLCLK | WM_NCMBUTTONDBLCLK | WM_NCXBUTTONDBLCLK => {
                instant_event!("WM_NC*BUTTONDBLCLK");
                None
            }
            WM_APPCOMMAND => {
                instant_event!("WM_APPCOMMAND");
                None
            }
            WM_GETOBJECT => {
                instant_event!("WM_ERASEBKGND");
                None
            }
            WM_PAINT => {
                instant_event!("WM_PAINT");
                None
            }
            WM_NCPAINT => {
                instant_event!("WM_NCPAINT");
                None
            }
            WM_ERASEBKGND => {
                instant_event!("WM_ERASEBKGND");
                None
            }
            WM_DWMNCRENDERINGCHANGED => {
                instant_event!("WM_DWMNCRENDERINGCHANGED");
                None
            }

            // ime
            WM_IME_SETCONTEXT => {
                instant_event!("WM_IME_SETCONTEXT");
                None
            }
            WM_IME_NOTIFY => {
                instant_event!("WM_IME_NOTIFY");
                None
            }
            WM_IME_REQUEST => {
                instant_event!("WM_IME_REQUEST");
                None
            }
            WM_IME_CONTROL => {
                instant_event!("WM_IME_CONTROL");
                None
            }

            // wmsz
            WMSZ_BOTTOMLEFT | WMSZ_BOTTOM | WMSZ_BOTTOMRIGHT |
             WMSZ_LEFT | WMSZ_RIGHT | WMSZ_TOP | WMSZ_TOPLEFT | WMSZ_TOPRIGHT => {
                instant_event!("WMSZ_*");
                None
            }
            _ => None
        }
    }
}