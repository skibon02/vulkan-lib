use std::fmt::{Debug, Formatter};
use bitflags::bitflags;
use windows_sys::Win32::Foundation::{LPARAM, RECT, WPARAM};
use windows_sys::Win32::System::SystemServices::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use crate::window::Window;

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum MonitorPower {
    PoweringOn,
    GoingLowPower,
    ShuttingOff,

    Unknown,
}

impl MonitorPower {
    pub fn from_raw(lparam: LPARAM) -> Self {
        match lparam {
            -1 => Self::PoweringOn,
            1 => Self::GoingLowPower,
            2 => Self::ShuttingOff,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum SystemCommand {
    Close,
    VScroll,
    HScroll,
    KeyMenu,
    Maximize,
    Minimize,
    MonitorPower(MonitorPower),
    MouseMenu,
    Move,
    NextWindow,
    PrevWindow,
    Restore,
    ScreenSave,
    Size,
    TaskList,
    Hotkey,
    Default,
    ContextHelp,

    Unknown,
}

impl SystemCommand {
    pub fn from_raw(wparam: WPARAM, lparam: LPARAM) -> Self {
        match wparam as u32 & 0xFFF0 {
            SC_CLOSE => SystemCommand::Close,
            SC_VSCROLL => SystemCommand::VScroll,
            SC_HSCROLL => SystemCommand::HScroll,
            SC_KEYMENU => SystemCommand::KeyMenu,
            SC_MAXIMIZE => SystemCommand::Maximize,
            SC_MINIMIZE => SystemCommand::Minimize,
            SC_MONITORPOWER => SystemCommand::MonitorPower(MonitorPower::from_raw(lparam)),
            SC_MOUSEMENU => SystemCommand::MouseMenu,
            SC_MOVE => SystemCommand::Move,
            SC_NEXTWINDOW => SystemCommand::NextWindow,
            SC_PREVWINDOW => SystemCommand::PrevWindow,
            SC_RESTORE => SystemCommand::Restore,
            0xF140 => SystemCommand::ScreenSave,
            SC_SIZE => SystemCommand::Size,
            SC_TASKLIST => SystemCommand::TaskList,
            SC_HOTKEY => SystemCommand::Hotkey,
            SC_DEFAULT => SystemCommand::Default,
            SC_CONTEXTHELP => SystemCommand::ContextHelp,
            _ => SystemCommand::Unknown,
        }
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct KeyStateFlags: usize {
        const LBUTTON  = 0x0001;
        const RBUTTON  = 0x0002;
        const SHIFT    = 0x0004;
        const CONTROL  = 0x0008;
        const MBUTTON  = 0x0010;
        const XBUTTON1 = 0x0020;
        const XBUTTON2 = 0x0040;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTest {
    Error,
    Transparent,
    Nowhere,
    Client,
    Caption,
    SysMenu,
    GrowBox,
    Menu,
    HScroll,
    VScroll,
    MinButton,
    MaxButton,
    Left,
    Right,
    Top,
    TopLeft,
    TopRight,
    Bottom,
    BottomLeft,
    BottomRight,
    Border,
    Close,
    Help,
}

impl HitTest {
    pub fn from_i16(value: i16) -> Self {
        match value  as u32{
            HTNOWHERE => Self::Nowhere,
            HTCLIENT => Self::Client,
            HTCAPTION => Self::Caption,
            HTSYSMENU => Self::SysMenu,
            HTSIZE => Self::GrowBox,
            HTMENU => Self::Menu,
            HTHSCROLL => Self::HScroll,
            HTVSCROLL => Self::VScroll,
            HTMINBUTTON => Self::MinButton,
            HTMAXBUTTON => Self::MaxButton,
            HTLEFT => Self::Left,
            HTRIGHT => Self::Right,
            HTTOP => Self::Top,
            HTTOPLEFT => Self::TopLeft,
            HTTOPRIGHT => Self::TopRight,
            HTBOTTOM => Self::Bottom,
            HTBOTTOMLEFT => Self::BottomLeft,
            HTBOTTOMRIGHT => Self::BottomRight,
            HTBORDER => Self::Border,
            HTCLOSE => Self::Close,
            HTHELP => Self::Help,
            ht if ht == HTERROR as u32=> Self::Error,
            ht if ht == HTTRANSPARENT as u32=> Self::Transparent,
            _ => Self::Client,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Activate {
    Active,
    ClickActive,
    Inactive,
}

impl Activate {
    pub fn from_wparam(wparam: WPARAM) -> Self {
        match wparam as u32 {
            WA_ACTIVE => Activate::Active,
            WA_CLICKACTIVE => Activate::ClickActive,
            WA_INACTIVE => Activate::Inactive,
            _ => Activate::Active,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Size {
    MaxHide,
    Maximized,
    MaxShow,
    Minimized,
    Restored,
    Unknown
}

impl Size {
    pub fn from_wparam(wparam: WPARAM) -> Self {
        match wparam as u32 {
            SIZE_RESTORED => Size::Restored,
            SIZE_MINIMIZED => Size::Minimized,
            SIZE_MAXIMIZED => Size::Maximized,
            SIZE_MAXSHOW => Size::MaxShow,
            SIZE_MAXHIDE => Size::MaxHide,
            _ => Size::Unknown,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SizeEdge {
    Left,
    Right,
    Top,
    TopLeft,
    TopRight,
    Bottom,
    BottomLeft,
    BottomRight,
}

impl SizeEdge {
    pub fn from_wparam(wparam: WPARAM) -> Self {
        match wparam as u32 {
            WMSZ_LEFT => SizeEdge::Left,
            WMSZ_RIGHT => SizeEdge::Right,
            WMSZ_TOP => SizeEdge::Top,
            WMSZ_TOPLEFT => SizeEdge::TopLeft,
            WMSZ_TOPRIGHT => SizeEdge::TopRight,
            WMSZ_BOTTOM => SizeEdge::Bottom,
            WMSZ_BOTTOMLEFT => SizeEdge::BottomLeft,
            WMSZ_BOTTOMRIGHT => SizeEdge::BottomRight,
            _ => SizeEdge::Top,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Icon {
    Big,
    Small,
    Small2
}

impl Icon {
    pub fn from_wparam(wparam: WPARAM) -> Self {
        match wparam as u32 {
            ICON_BIG => Icon::Big,
            ICON_SMALL => Icon::Small,
            ICON_SMALL2 => Icon::Small2,
            _ => Icon::Big,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum NcCalcSize {
    CalcsizeParams(*mut NCCALCSIZE_PARAMS),
    Rect(*mut RECT),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppDevice {
    Mouse,
    Keyboard,
    Oem,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCommand {
    BassBoost,
    BassDown,
    BassUp,
    BrowserBackward,
    BrowserFavorites,
    BrowserForward,
    BrowserRefresh,
    BrowserHome,
    BrowserSearch,
    BrowserStop,
    Close,
    Copy,
    CorrectionList,
    Cut,
    DictateOrCommandControlToggle,
    Find,
    ForwardMail,
    Help,
    LaunchApp1,
    LaunchApp2,
    LaunchMail,
    LaunchMediaSelect,
    MediaChannelDown,
    MediaChannelUp,
    MediaFastForward,
    MediaNextTrack,
    MediaPause,
    MediaPlay,
    MediaPlayPause,
    MediaPreviousTrack,
    MediaRecord,
    MediaRewind,
    MediaStop,
    MicOnOffToggle,
    MicrophoneVolumeDown,
    MicrophoneVolumeMute,
    MicrophoneVolumeUp,
    New,
    Open,
    Paste,
    Print,
    Redo,
    ReplyToMail,
    Save,
    SendMail,
    TrebleDown,
    TrebleUp,
    Undo,
    VolumeMute,
    VolumeDown,
    VolumeUp,

    Unknown,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct AppCommandInfo {
    pub command: AppCommand,
    pub device: AppDevice,
    pub key_state: KeyStateFlags,
}

impl AppCommandInfo {
    pub fn from_lparam(lparam: LPARAM) -> Self {
        let lp = lparam as usize;

        let low_word = (lp & 0xFFFF) as u16;
        let hi_word = ((lp >> 16) & 0xFFFF) as u16;

        let device = match hi_word as u32 & FAPPCOMMAND_MASK {
            FAPPCOMMAND_MOUSE => AppDevice::Mouse,
            FAPPCOMMAND_KEY => AppDevice::Keyboard,
            FAPPCOMMAND_OEM => AppDevice::Oem,
            _ => AppDevice::Unknown,
        };

        let cmd_raw = hi_word & !(FAPPCOMMAND_MASK as u16);
        let command = match cmd_raw as u32 {
            APPCOMMAND_BASS_BOOST => AppCommand::BassBoost,
            APPCOMMAND_BASS_DOWN => AppCommand::BassDown,
            APPCOMMAND_BASS_UP => AppCommand::BassUp,
            APPCOMMAND_BROWSER_BACKWARD => AppCommand::BrowserBackward,
            APPCOMMAND_BROWSER_FAVORITES => AppCommand::BrowserFavorites,
            APPCOMMAND_BROWSER_FORWARD => AppCommand::BrowserForward,
            APPCOMMAND_BROWSER_REFRESH => AppCommand::BrowserRefresh,
            APPCOMMAND_BROWSER_HOME => AppCommand::BrowserHome,
            APPCOMMAND_BROWSER_SEARCH => AppCommand::BrowserSearch,
            APPCOMMAND_BROWSER_STOP => AppCommand::BrowserStop,
            APPCOMMAND_CLOSE => AppCommand::Close,
            APPCOMMAND_COPY => AppCommand::Copy,
            APPCOMMAND_CORRECTION_LIST => AppCommand::CorrectionList,
            APPCOMMAND_CUT => AppCommand::Cut,
            APPCOMMAND_DICTATE_OR_COMMAND_CONTROL_TOGGLE => AppCommand::DictateOrCommandControlToggle,
            APPCOMMAND_FIND => AppCommand::Find,
            APPCOMMAND_FORWARD_MAIL => AppCommand::ForwardMail,
            APPCOMMAND_HELP => AppCommand::Help,
            APPCOMMAND_LAUNCH_APP1 => AppCommand::LaunchApp1,
            APPCOMMAND_LAUNCH_APP2 => AppCommand::LaunchApp2,
            APPCOMMAND_LAUNCH_MAIL => AppCommand::LaunchMail,
            APPCOMMAND_LAUNCH_MEDIA_SELECT => AppCommand::LaunchMediaSelect,
            APPCOMMAND_MEDIA_CHANNEL_DOWN => AppCommand::MediaChannelDown,
            APPCOMMAND_MEDIA_CHANNEL_UP => AppCommand::MediaChannelUp,
            APPCOMMAND_MEDIA_FAST_FORWARD => AppCommand::MediaFastForward,
            APPCOMMAND_MEDIA_NEXTTRACK => AppCommand::MediaNextTrack,
            APPCOMMAND_MEDIA_PAUSE => AppCommand::MediaPause,
            APPCOMMAND_MEDIA_PLAY => AppCommand::MediaPlay,
            APPCOMMAND_MEDIA_PLAY_PAUSE => AppCommand::MediaPlayPause,
            APPCOMMAND_MEDIA_PREVIOUSTRACK => AppCommand::MediaPreviousTrack,
            APPCOMMAND_MEDIA_RECORD => AppCommand::MediaRecord,
            APPCOMMAND_MEDIA_REWIND => AppCommand::MediaRewind,
            APPCOMMAND_MEDIA_STOP => AppCommand::MediaStop,
            APPCOMMAND_MIC_ON_OFF_TOGGLE => AppCommand::MicOnOffToggle,
            APPCOMMAND_MICROPHONE_VOLUME_DOWN => AppCommand::MicrophoneVolumeDown,
            APPCOMMAND_MICROPHONE_VOLUME_MUTE => AppCommand::MicrophoneVolumeMute,
            APPCOMMAND_MICROPHONE_VOLUME_UP => AppCommand::MicrophoneVolumeUp,
            APPCOMMAND_NEW => AppCommand::New,
            APPCOMMAND_OPEN => AppCommand::Open,
            APPCOMMAND_PASTE => AppCommand::Paste,
            APPCOMMAND_PRINT => AppCommand::Print,
            APPCOMMAND_REDO => AppCommand::Redo,
            APPCOMMAND_REPLY_TO_MAIL => AppCommand::ReplyToMail,
            APPCOMMAND_SAVE => AppCommand::Save,
            APPCOMMAND_SEND_MAIL => AppCommand::SendMail,
            APPCOMMAND_TREBLE_DOWN => AppCommand::TrebleDown,
            APPCOMMAND_TREBLE_UP => AppCommand::TrebleUp,
            APPCOMMAND_UNDO => AppCommand::Undo,
            APPCOMMAND_VOLUME_MUTE => AppCommand::VolumeMute,
            APPCOMMAND_VOLUME_DOWN => AppCommand::VolumeDown,
            APPCOMMAND_VOLUME_UP => AppCommand::VolumeUp,

            _ => AppCommand::Unknown,
        };

        let key_state = KeyStateFlags::from_bits_truncate(low_word as usize);

        Self {
            command,
            device,
            key_state,
        }
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct WindowPosFlags: u32 {
        const NO_ZORDER       = SWP_NOZORDER;
        const NO_SIZE         = SWP_NOSIZE;
        const NO_MOVE         = SWP_NOMOVE;
        const NO_ACTIVATE     = SWP_NOACTIVATE;
        const DRAW_FRAME      = SWP_DRAWFRAME;
        const FRAME_CHANGED   = SWP_FRAMECHANGED;
        const SHOW_WINDOW     = SWP_SHOWWINDOW;
        const HIDE_WINDOW     = SWP_HIDEWINDOW;
        const NO_REDRAW       = SWP_NOREDRAW;
        const NO_COPY_BITS    = SWP_NOCOPYBITS;
        const NO_OWNER_ZORDER = SWP_NOOWNERZORDER;
        const NO_SEND_CHANGING = SWP_NOSENDCHANGING;
        const DEFER_ERASE     = SWP_DEFERERASE;
        const ASYNC_WINDOW_POS = SWP_ASYNCWINDOWPOS;
        const NO_REPOSITION   = SWP_NOREPOSITION;
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct WindowPos {
    pub x: i32,
    pub y: i32,
    pub cx: i32,
    pub cy: i32,
    pub flags: WindowPosFlags,
}

impl WindowPos {
    pub fn from_original(winpos: WINDOWPOS) -> Self {
        Self {
            x: winpos.x,
            y: winpos.y,
            cx: winpos.cx,
            cy: winpos.cy,
            flags: WindowPosFlags::from_bits_truncate(winpos.flags as u32),
        }
    }
}

pub struct WindowPosChangingGuard<'a> {
    inner: &'a mut WINDOWPOS,
}

impl<'a> WindowPosChangingGuard<'a> {
    pub unsafe fn from_lparam(lparam: isize) -> Self {
        Self {
            inner: &mut *(lparam as *mut WINDOWPOS),
        }
    }

    pub fn flags(&self) -> WindowPosFlags {
        WindowPosFlags::from_bits_truncate(self.inner.flags)
    }

    pub fn set_flags(&mut self, flags: WindowPosFlags) {
        self.inner.flags = flags.bits();
    }
}

impl<'a> Debug for WindowPosChangingGuard<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowPosChangingGuard")
            .field("x", &self.inner.x)
            .field("y", &self.inner.y)
            .field("cx", &self.inner.cx)
            .field("cy", &self.inner.cy)
            .field("flags", &self.flags())
            .finish()
    }
}

pub struct RectGuard<'a> {
    pub rect: &'a mut RECT,
}

impl<'a> RectGuard<'a> {
    pub unsafe fn from_lparam(lparam: isize) -> Self {
        Self {
            rect: &mut *(lparam as *mut RECT),
        }
    }
}

impl<'a> std::fmt::Debug for RectGuard<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RectGuard")
            .field("left", &self.rect.left)
            .field("top", &self.rect.top)
            .field("right", &self.rect.right)
            .field("bottom", &self.rect.bottom)
            .field("width", &(self.rect.right - self.rect.left))
            .field("height", &(self.rect.bottom - self.rect.top))
            .finish()
    }
}

pub struct CreateStructGuard<'a> {
    pub cs: &'a mut CREATESTRUCTW,
}

impl<'a> CreateStructGuard<'a> {
    pub unsafe fn from_lparam(lparam: isize) -> Self {
        Self {
            cs: &mut *(lparam as *mut CREATESTRUCTW),
        }
    }
}

impl<'a> std::fmt::Debug for CreateStructGuard<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let read_pcwstr = |ptr: windows_sys::core::PCWSTR| {
            if ptr.is_null() { return "NULL".to_string(); }
            let mut len = 0;
            unsafe {
                while *ptr.add(len) != 0 { len += 1; }
                let slice = std::slice::from_raw_parts(ptr, len);
                String::from_utf16_lossy(slice)
            }
        };

        f.debug_struct("CreateStructGuard")
            .field("name", &read_pcwstr(self.cs.lpszName))
            .field("class", &read_pcwstr(self.cs.lpszClass))
            .field("x", &self.cs.x)
            .field("y", &self.cs.y)
            .field("cx", &self.cs.cx)
            .field("cy", &self.cs.cy)
            .field("style", &format_args!("{:#x}", self.cs.style))
            .finish()
    }
}