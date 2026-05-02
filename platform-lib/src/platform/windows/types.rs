use std::fmt;
use std::fmt::Debug;
use bitflags::bitflags;
use windows_sys::Win32::Foundation;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::System::SystemServices::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

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

#[derive(Debug)]
pub enum NcCalcSize<'a> {
    CalcsizeParams(*mut NCCALCSIZE_PARAMS),
    Rect(RectGuard<'a>),
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
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


#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}
pub struct MinMaxInfoGuard<'a> {
    pub info: &'a mut MINMAXINFO,
}

impl<'a> MinMaxInfoGuard<'a> {
    pub unsafe fn from_lparam(lparam: isize) -> Self {
        Self {
            info: &mut *(lparam as *mut MINMAXINFO),
        }
    }

    pub fn set_min_track_size(&mut self, width: i32, height: i32) {
        self.info.ptMinTrackSize.x = width;
        self.info.ptMinTrackSize.y = height;
    }

    pub fn set_max_track_size(&mut self, width: i32, height: i32) {
        self.info.ptMaxTrackSize.x = width;
        self.info.ptMaxTrackSize.y = height;
    }
}

struct PointDebug<'a>(&'a Foundation::POINT);
impl<'a> Debug for PointDebug<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.0.x, self.0.y)
    }
}
impl<'a> Debug for MinMaxInfoGuard<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MinMaxInfoGuard")
            .field("max_size", &PointDebug(&self.info.ptMaxSize))
            .field("max_position", &PointDebug(&self.info.ptMaxPosition))
            .field("min_track_size", &PointDebug(&self.info.ptMinTrackSize))
            .field("max_track_size", &PointDebug(&self.info.ptMaxTrackSize))
            .finish()
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum WindowStatus {
    ParentClosing,
    OtherZoom,
    ParentOpening,
    OtherUnzoom,
}

impl WindowStatus {
    pub fn from_lparam(lparam: LPARAM) -> Self {
        match lparam as u32 {
            SW_PARENTCLOSING => WindowStatus::ParentClosing,
            SW_OTHERZOOM => WindowStatus::OtherZoom,
            SW_PARENTOPENING => WindowStatus::ParentOpening,
            SW_OTHERUNZOOM => WindowStatus::OtherUnzoom,
            _ => WindowStatus::OtherZoom,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum StyleKind {
    Style,
    ExStyle
}

impl StyleKind {
    pub fn from_wparam(wparam: WPARAM) -> Self {
        match wparam as i32 {
            GWL_STYLE => StyleKind::Style,
            GWL_EXSTYLE => StyleKind::ExStyle,
            _ => StyleKind::Style,
        }
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct WindowStyle: u32 {
        const OVERLAPPED       = WS_OVERLAPPED;
        const POPUP            = WS_POPUP;
        const CHILD            = WS_CHILD;
        const MINIMIZE         = WS_MINIMIZE;
        const VISIBLE          = WS_VISIBLE;
        const DISABLED         = WS_DISABLED;
        const CLIP_SIBLINGS    = WS_CLIPSIBLINGS;
        const CLIP_CHILDREN    = WS_CLIPCHILDREN;
        const MAXIMIZE         = WS_MAXIMIZE;
        const CAPTION          = WS_CAPTION; // BORDER | DLGFRAME
        const BORDER           = WS_BORDER;
        const DLG_FRAME        = WS_DLGFRAME;
        const VSCROLL          = WS_VSCROLL;
        const HSCROLL          = WS_HSCROLL;
        const SYSMENU          = WS_SYSMENU;
        const THICK_FRAME      = WS_THICKFRAME;
        const GROUP            = WS_GROUP;
        const TAB_STOP         = WS_TABSTOP;
        const MINIMIZE_BOX     = WS_MINIMIZEBOX;
        const MAXIMIZE_BOX     = WS_MAXIMIZEBOX;
        const TILED            = WS_TILED;
        const ICONIC           = WS_ICONIC;
        const SIZE_BOX         = WS_SIZEBOX;

        const ACTIVE_CAPTION    = WS_ACTIVECAPTION;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct WindowExStyle: u32 {
        const DLG_MODAL_FRAME       = WS_EX_DLGMODALFRAME;
        const NO_PARENT_NOTIFY      = WS_EX_NOPARENTNOTIFY;
        const TOPMOST               = WS_EX_TOPMOST;
        const ACCEPT_FILES          = WS_EX_ACCEPTFILES;
        const TRANSPARENT           = WS_EX_TRANSPARENT;
        const MDI_CHILD             = WS_EX_MDICHILD;
        const TOOL_WINDOW           = WS_EX_TOOLWINDOW;
        const WINDOW_EDGE           = WS_EX_WINDOWEDGE;
        const CLIENT_EDGE           = WS_EX_CLIENTEDGE;
        const CONTEXT_HELP          = WS_EX_CONTEXTHELP;
        const RIGHT                 = WS_EX_RIGHT;
        const LEFT                  = WS_EX_LEFT;
        const RTL_READING           = WS_EX_RTLREADING;
        const LTR_READING           = WS_EX_LTRREADING;
        const LEFTSCROLLBAR         = WS_EX_LEFTSCROLLBAR;
        const RIGHTSCROLLBAR        = WS_EX_RIGHTSCROLLBAR;
        const CONTROL_PARENT        = WS_EX_CONTROLPARENT;
        const STATIC_EDGE           = WS_EX_STATICEDGE;
        const APP_WINDOW            = WS_EX_APPWINDOW;
        const LAYERED               = WS_EX_LAYERED;
        const NO_INHERIT_LAYOUT     = WS_EX_NOINHERITLAYOUT;
        const LAYOUT_RTL            = WS_EX_LAYOUTRTL;
        const COMPOSITED            = WS_EX_COMPOSITED;
        const NO_ACTIVATE           = WS_EX_NOACTIVATE;
        const NO_REDIRECTION_BITMAP = WS_EX_NOREDIRECTIONBITMAP;
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Style {
    Style(WindowStyle),
    ExStyle(WindowExStyle),
}

impl Style {
    pub fn from_params(wparam: WPARAM, lparam: LPARAM) -> Self {
        match StyleKind::from_wparam(wparam) {
            StyleKind::Style => {
                Self::Style(WindowStyle::from_bits_truncate(lparam as u32))
            }
            StyleKind::ExStyle => {
                Self::ExStyle(WindowExStyle::from_bits_truncate(lparam as u32))
            }
        }
    }
}

pub struct StyleStructGuard<'a> {
    pub inner: &'a mut STYLESTRUCT,
    pub style_kind: StyleKind,
}

impl<'a> StyleStructGuard<'a> {
    pub unsafe fn from_params(wparam: WPARAM, lparam: LPARAM) -> Self {
        Self {
            inner: &mut *(lparam as *mut STYLESTRUCT),
            style_kind: StyleKind::from_wparam(wparam),
        }
    }

    pub fn set_new_style(&mut self, style: u32) {
        self.inner.styleNew = style;
    }
}

impl<'a> std::fmt::Debug for StyleStructGuard<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut ds = f.debug_struct("StyleStructGuard");

        match self.style_kind {
            StyleKind::Style => {
                ds.field("old", &WindowStyle::from_bits_truncate(self.inner.styleOld))
                    .field("new", &WindowStyle::from_bits_truncate(self.inner.styleNew));
            }
            StyleKind::ExStyle => {
                ds.field("old", &WindowExStyle::from_bits_truncate(self.inner.styleOld))
                    .field("new", &WindowExStyle::from_bits_truncate(self.inner.styleNew));
            }
        }

        ds.finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeystrokeFlags {
    pub repeat_count: u16,
    pub scan_code: u8,
    pub is_extended: bool,
    pub context_code: bool,
    pub previous_state: bool,
    pub transition_state: bool,
}

impl KeystrokeFlags {
    pub fn from_lparam(lparam: isize) -> Self {
        let lp = lparam as u32;

        Self {
            repeat_count: (lp & 0xFFFF) as u16,
            scan_code: ((lp >> 16) & 0xFF) as u8,
            is_extended: (lp & (1 << 24)) != 0,
            context_code: (lp & (1 << 29)) != 0,
            previous_state: (lp & (1 << 30)) != 0,
            transition_state: (lp & (1 << 31)) != 0,
        }
    }

    pub fn is_autorepeat(&self) -> bool {
        self.previous_state && !self.transition_state
    }

    pub fn event_type(&self) -> KeyEventType {
        if self.transition_state {
            KeyEventType::Release
        } else if self.previous_state {
            KeyEventType::Repeat
        } else {
            KeyEventType::Press
        }
    }
}
#[derive(Debug, PartialEq)]
pub enum KeyEventType {
    Press,
    Repeat,
    Release,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    X,
}

impl MouseButton {
    pub fn from_msg(msg: u16) -> Self {
        match msg as u32 {
            WM_LBUTTONDOWN => MouseButton::Left,
            WM_NCLBUTTONDOWN => MouseButton::Left,
            WM_MBUTTONDOWN => MouseButton::Middle,
            WM_NCMBUTTONDOWN => MouseButton::Right,
            WM_RBUTTONDOWN => MouseButton::Right,
            WM_NCRBUTTONDOWN => MouseButton::Right,
            WM_XBUTTONDOWN => MouseButton::X,
            WM_NCXBUTTONDOWN => MouseButton::X,
            _ => MouseButton::Left
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct MouseActivateResult {
    activate: bool,
    eat: bool
}
impl MouseActivateResult {
    pub fn new() -> Self {
        Self {
            activate: true,
            eat: false
        }
    }

    pub fn as_num(&self) -> LRESULT {
        let res = match (self.activate, self.eat) {
            (false, false) => MA_NOACTIVATE,
            (false, true) => MA_NOACTIVATEANDEAT,
            (true, false) => MA_ACTIVATE,
            (true, true) => MA_ACTIVATEANDEAT,
        };
        res as LRESULT
    }
}
