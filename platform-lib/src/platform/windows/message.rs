use log::{info, warn};
use sparkles::instant_event;
use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{*};
use crate::platform::windows::types::{Activate, AppCommandInfo, CreateStructGuard, HitTest, Icon, KeyStateFlags, KeystrokeFlags, MinMaxInfoGuard, MouseButton, NcCalcSize, RectGuard, Size, SizeEdge, Style, StyleStructGuard, SystemCommand, WindowPos, WindowPosChangingGuard, WindowStatus};
use crate::platform::windows::EventLoopData;


#[derive(Debug)]
pub enum WindowMessage<'a> {
    NcCreate {
        createstruct: CreateStructGuard<'a>,
    },
    Create {
        createstruct: CreateStructGuard<'a>,
    },
    SystemCommand(SystemCommand, (i16, i16)),
    SetCursor(HitTest, u32),
    Move(i16, i16),
    /// Can modify fields in RECT. return TRUE if message is handled and RECT changed
    Moving(RectGuard<'a>),
    Size(Size, u16, u16),
    /// Can modify fields in RECT. return TRUE if message is handled and RECT changed
    Sizing(SizeEdge, RectGuard<'a>),
    EnterSizeMove,
    ExitSizeMove,
    WindowPosChanged(WindowPos),
    WindowPosChanging(WindowPosChangingGuard<'a>),
    GetMinMaxInfo(MinMaxInfoGuard<'a>),
    InputLangChange,
    InputLangChangeRequest,
    ShowWindow(bool, WindowStatus),
    StyleChanging(StyleStructGuard<'a>),
    StyleChanged(Style),

    /// active
    ActivateApp{
        active: bool,
    },
    /// Default processing is recommended
    NcActivate{
        active: bool,
        nc_repaint: bool,
    },
    NcCalcSize(NcCalcSize<'a>),

    Enable(bool),
    Close,
    Destroy,
    NcDestroy,
    QueryOpen,
    /// Return type: HICON
    GetIcon(Icon, u32),
    Paint,
    NcPaint,
    /// Should return non-zero if application erases the background
    EraseBkgnd,
    UserChanged,
    Quit(usize),

    DwmNcRenderingChanged,
    DwmColorizationColorChanged(u8, u8, u8, u8, bool),
}

#[derive(Debug)]
pub enum KeyboardMessage {
    Activate {
        active: Activate,
        is_minimized: bool
    },
    KeyUp {
        vk: u32,
        flags: KeystrokeFlags,
    },
    KeyDown {
        vk: u32,
        flags: KeystrokeFlags,
    },
    Char {
        code: Option<char>,
        flags: KeystrokeFlags,
    },
    AppCommand(AppCommandInfo)
}

#[derive(Debug)]
pub enum MouseMessage {
    ButtonDown(MouseButton, i16, i16, KeyStateFlags),
    ButtonUp(MouseButton, i16, i16, KeyStateFlags),
    ButtonDoubleClk(MouseButton, i16, i16, KeyStateFlags),
    NcHitTest(i16, i16),
    MouseMove(i16, i16, KeyStateFlags),
    NcMouseHover(i16, i16, HitTest),
    NcMouseLeave,
    CaptureChanged,
    MouseWheel(i16, i16, i16, KeyStateFlags),
    /// Return value: MouseActivateResult::as_num
    MouseActivate(MouseButton),

    NcButtonDown(MouseButton, i16, i16, KeyStateFlags),
    NcButtonUp(MouseButton, i16, i16, KeyStateFlags),
    NcButtonDoubleClk(MouseButton, i16, i16, KeyStateFlags),
    NcMouseMove(i16, i16, HitTest),
}

#[derive(Debug)]
pub enum RawInputMessage {
    Input,
    InputDeviceChange
}

#[derive(Debug)]
pub enum ImeMessage {
    SetContext,
    Select,
    Request,
    Notify,
    KeyUp,
    KeyDown,
    Control,
}

use WindowMessage::*;
use KeyboardMessage::*;
use MouseMessage::*;
use RawInputMessage::*;
use ImeMessage::*;

#[derive(Debug)]
pub enum RawMessage<'a> {
    WindowMessage(WindowMessage<'a>),
    KeyboardMessage(KeyboardMessage),
    MouseMessage(MouseMessage),
    RawInputMessage(RawInputMessage),
    ImeMessage(ImeMessage),
}

impl<'a> RawMessage<'a> {
    pub unsafe fn try_parse(msg: u32, wparam: WPARAM, lparam: LPARAM, window: HWND, event_loop_data: &mut EventLoopData) -> Option<RawMessage<'a>> {
        match msg {
            WM_NCCREATE => Some(RawMessage::WindowMessage(NcCreate{
                createstruct: unsafe { CreateStructGuard::from_lparam(lparam) },
            })),
            WM_CREATE => Some(RawMessage::WindowMessage(Create{
                createstruct: unsafe { CreateStructGuard::from_lparam(lparam) },
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

                Some(RawMessage::WindowMessage(WindowMessage::SetCursor(hit_test, trig_msg)))
            }
            WM_MOVE => {
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                Some(RawMessage::WindowMessage(Move(x, y)))
            }
            WM_MOVING => {
                Some(RawMessage::WindowMessage(Moving(RectGuard::from_lparam(lparam))))
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
                Some(RawMessage::WindowMessage(Sizing(size_edge, RectGuard::from_lparam(lparam))))
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
            WM_ERASEBKGND => Some(RawMessage::WindowMessage(EraseBkgnd)),
            WM_NCCALCSIZE => {
                let need_indicate = wparam != 0;
                let res = if need_indicate {
                    NcCalcSize::CalcsizeParams(lparam as *mut NCCALCSIZE_PARAMS)
                }
                else {
                    NcCalcSize::Rect(RectGuard::from_lparam(lparam))
                };

                Some(RawMessage::WindowMessage(WindowMessage::NcCalcSize(res)))
            }
            WM_ENTERSIZEMOVE => Some(RawMessage::WindowMessage(EnterSizeMove)),
            WM_EXITSIZEMOVE => Some(RawMessage::WindowMessage(ExitSizeMove)),
            WM_WINDOWPOSCHANGED => {
                let pos = unsafe {*(lparam as *const WINDOWPOS)};
                Some(RawMessage::WindowMessage(WindowPosChanged(WindowPos::from_original(pos))))
            }
            WM_WINDOWPOSCHANGING => {
                Some(RawMessage::WindowMessage(WindowPosChanging(unsafe { WindowPosChangingGuard::from_lparam(lparam) })))
            }
            WM_GETMINMAXINFO => Some(RawMessage::WindowMessage(GetMinMaxInfo(unsafe {MinMaxInfoGuard::from_lparam(lparam)}))),
            WM_INPUTLANGCHANGE => Some(RawMessage::WindowMessage(InputLangChange)),
            WM_INPUTLANGCHANGEREQUEST => Some(RawMessage::WindowMessage(InputLangChangeRequest)),
            WM_SHOWWINDOW => Some(RawMessage::WindowMessage(WindowMessage::ShowWindow(wparam != 0, WindowStatus::from_lparam(lparam)))),
            WM_STYLECHANGING => Some(RawMessage::WindowMessage(StyleChanging(unsafe { StyleStructGuard::from_params(wparam, lparam) }))),
            WM_STYLECHANGED => Some(RawMessage::WindowMessage(StyleChanged(Style::from_params(wparam, lparam)))),
            WM_USERCHANGED => Some(RawMessage::WindowMessage(UserChanged)),
            WM_DWMNCRENDERINGCHANGED => Some(RawMessage::WindowMessage(DwmNcRenderingChanged)),
            WM_DWMCOLORIZATIONCOLORCHANGED => {
                let b = wparam as u8;
                let g = (wparam >> 8) as u8;
                let r = (wparam >> 16) as u8;
                let a = (wparam >> 24) as u8;
                let blended = lparam != 0;
                Some(RawMessage::WindowMessage(DwmColorizationColorChanged(r,g,b,a,blended)))
            },


            // Keyboard messages
            WM_KEYUP | WM_SYSKEYUP => Some(RawMessage::KeyboardMessage(KeyboardMessage::KeyUp {
                vk: wparam as u32,
                flags: KeystrokeFlags::from_lparam(lparam)
            })),
            WM_KEYDOWN | WM_SYSKEYDOWN => Some(RawMessage::KeyboardMessage(KeyboardMessage::KeyDown {
                vk: wparam as u32,
                flags: KeystrokeFlags::from_lparam(lparam)
            })),
            WM_ACTIVATE => {
                let active = Activate::from_wparam(wparam & 0xFFFF);
                let is_minimized = (wparam >> 16) != 0;
                Some(RawMessage::KeyboardMessage(KeyboardMessage::Activate{active, is_minimized}))
            }
            WM_APPCOMMAND => {
                // wparam, lparam
                let info = AppCommandInfo::from_lparam(lparam);
                Some(RawMessage::KeyboardMessage(AppCommand(info)))
            }
            WM_CHAR => {
                Some(RawMessage::KeyboardMessage(Char {
                    code: event_loop_data.push_u16_char(window, wparam as u16),
                    flags: KeystrokeFlags::from_lparam(lparam)
                }))
            },

            // Mouse messages
            WM_NCMOUSEMOVE => {
                let hit_test = HitTest::from_i16((wparam & 0xFFFF) as i16);
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                Some(RawMessage::MouseMessage(NcMouseMove(x, y, hit_test)))
            }
            WM_MOUSEMOVE => {
                let keys = KeyStateFlags::from_bits_truncate(wparam);
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                Some(RawMessage::MouseMessage(MouseMove(x, y, keys)))
            }
            WM_NCHITTEST => {
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                Some(RawMessage::MouseMessage(NcHitTest(x, y)))
            }
            WM_NCMOUSEHOVER => {
                let hit_test = HitTest::from_i16((wparam & 0xFFFF) as i16);
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                Some(RawMessage::MouseMessage(NcMouseHover(x, y, hit_test)))
            }
            WM_NCMOUSELEAVE => Some(RawMessage::MouseMessage(NcMouseLeave)),
            WM_CAPTURECHANGED => Some(RawMessage::MouseMessage(CaptureChanged)),
            WM_MOUSEWHEEL => {
                let keys = KeyStateFlags::from_bits_truncate(wparam & 0xFFFF);
                let dist = (wparam >> 16) as i16;
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                Some(RawMessage::MouseMessage(MouseWheel(x, y, dist, keys)))
            },
            WM_MOUSEACTIVATE => {
                let btn = MouseButton::from_msg((lparam >> 16) as u16);
                Some(RawMessage::MouseMessage(MouseActivate(btn)))
            },
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => {
                let flags = KeyStateFlags::from_bits_truncate(wparam);
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                let button = match msg {
                    WM_LBUTTONDOWN => MouseButton::Left,
                    WM_RBUTTONDOWN => MouseButton::Right,
                    WM_MBUTTONDOWN => MouseButton::Middle,
                    WM_XBUTTONDOWN => MouseButton::X,
                    _ => unreachable!()
                };
                Some(RawMessage::MouseMessage(ButtonDown(button, x, y, flags)))
            }
            WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP=> {
                let flags = KeyStateFlags::from_bits_truncate(wparam);
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                let button = match msg {
                    WM_LBUTTONUP => MouseButton::Left,
                    WM_RBUTTONUP => MouseButton::Right,
                    WM_MBUTTONUP => MouseButton::Middle,
                    WM_XBUTTONUP => MouseButton::X,
                    _ => unreachable!()
                };
                Some(RawMessage::MouseMessage(ButtonUp(button, x, y, flags)))
            }
            WM_LBUTTONDBLCLK | WM_RBUTTONDBLCLK | WM_MBUTTONDBLCLK | WM_XBUTTONDBLCLK => {
                let flags = KeyStateFlags::from_bits_truncate(wparam);
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                let button = match msg {
                    WM_LBUTTONDBLCLK => MouseButton::Left,
                    WM_RBUTTONDBLCLK => MouseButton::Right,
                    WM_MBUTTONDBLCLK => MouseButton::Middle,
                    WM_XBUTTONDBLCLK => MouseButton::X,
                    _ => unreachable!()
                };
                Some(RawMessage::MouseMessage(ButtonDoubleClk(button, x, y, flags)))
            }
            WM_NCLBUTTONDOWN | WM_NCRBUTTONDOWN | WM_NCMBUTTONDOWN | WM_NCXBUTTONDOWN => {
                let flags = KeyStateFlags::from_bits_truncate(wparam);
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                let button = match msg {
                    WM_NCLBUTTONDOWN => MouseButton::Left,
                    WM_NCRBUTTONDOWN => MouseButton::Right,
                    WM_NCMBUTTONDOWN => MouseButton::Middle,
                    WM_NCXBUTTONDOWN => MouseButton::X,
                    _ => unreachable!()
                };
                Some(RawMessage::MouseMessage(NcButtonDown(button, x, y, flags)))
            }
            WM_NCLBUTTONUP | WM_NCRBUTTONUP | WM_NCMBUTTONUP | WM_NCXBUTTONUP=> {
                let flags = KeyStateFlags::from_bits_truncate(wparam);
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                let button = match msg {
                    WM_NCLBUTTONUP => MouseButton::Left,
                    WM_NCRBUTTONUP => MouseButton::Right,
                    WM_NCMBUTTONUP => MouseButton::Middle,
                    WM_NCXBUTTONUP => MouseButton::X,
                    _ => unreachable!()
                };
                Some(RawMessage::MouseMessage(NcButtonUp(button, x, y, flags)))
            }
            WM_NCLBUTTONDBLCLK | WM_NCRBUTTONDBLCLK | WM_NCMBUTTONDBLCLK | WM_NCXBUTTONDBLCLK => {
                let flags = KeyStateFlags::from_bits_truncate(wparam);
                let x = (lparam & 0xFFFF) as i16;
                let y = (lparam >> 16) as i16;
                let button = match msg {
                    WM_NCLBUTTONDBLCLK => MouseButton::Left,
                    WM_NCRBUTTONDBLCLK => MouseButton::Right,
                    WM_NCMBUTTONDBLCLK => MouseButton::Middle,
                    WM_NCXBUTTONDBLCLK => MouseButton::X,
                    _ => unreachable!()
                };
                Some(RawMessage::MouseMessage(NcButtonDoubleClk(button, x, y, flags)))
            }

            // ime
            WM_IME_SETCONTEXT => Some(RawMessage::ImeMessage(SetContext)),
            WM_IME_NOTIFY => Some(RawMessage::ImeMessage(Notify)),
            WM_IME_REQUEST => Some(RawMessage::ImeMessage(Request)),
            WM_IME_CONTROL => Some(RawMessage::ImeMessage(Control)),
            _ => None
        }
    }
}