use log::{info, warn};
use sparkles::instant_event;
use windows_sys::Win32::Foundation::{LPARAM, RECT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{*};
use crate::platform::windows::types::{Activate, HitTest, Icon, MouseKeys, Size, SizeEdge, SystemCommand};

#[derive(Debug)]
pub enum MouseMessage {
    NcHitTest(i16, i16),
    NCMouseMove(i16, i16, HitTest),
    MouseMove(i16, i16, MouseKeys),
}

#[derive(Debug)]
pub enum KeyboardMessage {
    Activate {
        active: Activate,
        is_minimized: bool
    },
    KeyUp {
        vk: u32
    },
    KeyDown {
        vk: u32
    },
}


#[derive(Debug)]
pub enum RawInputMessage {
    Input,
    InputDeviceChange
}

#[derive(Debug)]
pub enum WindowMessage {
    Create {
        createstruct: *mut CREATESTRUCTW,
    },
    SystemCommand(SystemCommand, (i16, i16)),
    SetCursor(HitTest, u32),
    Move(i16, i16),
    /// Can modify fields in RECT. return TRUE if message is handled and RECT changed
    Moving(*mut RECT),
    Size(Size, u16, u16),
    /// Can modify fields in RECT. return TRUE if message is handled and RECT changed
    Sizing(SizeEdge, *mut RECT),

    /// active
    ActivateApp{
        active: bool,
    },
    /// Default processing is recommended
    NcActivate{
        active: bool,
        nc_repaint: bool,
    },

    Enable(bool),
    Close,
    Destroy,
    NcDestroy,
    QueryOpen,
    /// Return type: HICON
    GetIcon(Icon, u32),
    Paint,
    NcPaint,
    Quit(usize),
}

use WindowMessage::*;
use KeyboardMessage::*;
use MouseMessage::*;
use RawInputMessage::*;
#[derive(Debug)]
pub enum RawMessage {
    WindowMessage(WindowMessage),
    KeyboardMessage(KeyboardMessage),
    MouseMessage(MouseMessage),
}

impl RawMessage {
    pub fn try_parse(msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<RawMessage> {
        match msg {
            WM_CREATE => Some(RawMessage::WindowMessage(Create{
                createstruct: lparam as *mut CREATESTRUCTW,
            })),
            WM_SYSCOMMAND => {
                let sc = SystemCommand::from_raw(wparam, lparam);

                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                Some(RawMessage::WindowMessage(SystemCommand(sc, (x, y))))
            }
            WM_SETCURSOR => {
                let hit_test = HitTest::from_i16((lparam & 0xFFFF) as i16);
                let trig_msg = ((lparam >> 16) & 0xFFFF) as u32;

                Some(RawMessage::WindowMessage(SetCursor(hit_test, trig_msg)))
            }
            WM_MOVE => {
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                Some(RawMessage::WindowMessage(Move(x, y)))
            }
            WM_MOVING => {
                let rect = lparam as *mut RECT;
                Some(RawMessage::WindowMessage(Moving(rect)))
            }
            WM_ACTIVATEAPP => {
                let active = wparam != 0;
                Some(RawMessage::WindowMessage(ActivateApp{active}))
            }
            WM_NCACTIVATE => {
                let active = wparam != 0;
                let nc_repaint = lparam != -1;

                Some(RawMessage::WindowMessage(NcActivate{active, nc_repaint}))
            }
            WM_SIZE => {
                let size = Size::from_wparam(wparam);
                let width = (lparam & 0xFFFF) as u16;
                let height = (lparam >> 16) as u16;
                Some(RawMessage::WindowMessage(Size(size, width, height)))
            }
            WM_SIZING => {
                let size_edge = SizeEdge::from_wparam(wparam);
                let rect = lparam as *mut RECT;
                Some(RawMessage::WindowMessage(Sizing(size_edge, rect)))
            }
            WM_ENABLE => {
                let enabled = wparam != 0;
                Some(RawMessage::WindowMessage(Enable(enabled)))
            }
            WM_CLOSE => Some(RawMessage::WindowMessage(Close)),
            WM_DESTROY => Some(RawMessage::WindowMessage(Destroy)),
            WM_NCDESTROY => Some(RawMessage::WindowMessage(NcDestroy)),
            WM_QUERYOPEN => Some(RawMessage::WindowMessage(QueryOpen)),
            WM_GETICON => {
                let icon = Icon::from_wparam(wparam);
                let dpi = lparam as u32;
                Some(RawMessage::WindowMessage(GetIcon(icon, dpi)))
            }
            WM_PAINT => Some(RawMessage::WindowMessage(Paint)),
            WM_NCPAINT => Some(RawMessage::WindowMessage(NcPaint)),
            WM_QUIT => {
                let exit_code = wparam as usize;
                Some(RawMessage::WindowMessage(Quit(exit_code)))
            }


            // Keyboard messages
            WM_KEYUP => Some(RawMessage::KeyboardMessage(KeyUp {
                vk: wparam as u32
            })),
            WM_ACTIVATE => {
                let activate = Activate::from_wparam(wparam & 0xFFFF);
                let is_minimized = (wparam >> 16) != 0;
                Some(RawMessage::KeyboardMessage(KeyboardMessage::Activate{active, is_minimized}))
            }
            WM_KEYDOWN => Some(RawMessage::KeyboardMessage(KeyDown {
                vk: wparam as u32
            })),

            // Mouse messages
            WM_NCMOUSEMOVE => {
                let hit_test = HitTest::from_i16((lparam & 0xFFFF) as i16);
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                Some(RawMessage::MouseMessage(NCMouseMove(x, y, hit_test)))
            }
            WM_MOUSEMOVE => {
                let keys = MouseKeys::from_bits_truncate(wparam);
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                Some(RawMessage::MouseMessage(MouseMove(x, y, keys)))
            }
            WM_NCHITTEST => {
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                Some(RawMessage::MouseMessage(NcHitTest(x, y)))
            }



            WM_ENTERSIZEMOVE => {
                instant_event!("WM_ENTERSIZEMOVE");
                None
            }
            WM_EXITSIZEMOVE => {
                instant_event!("WM_EXITSIZEMOVE");
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
            _ => None
        }
    }
}