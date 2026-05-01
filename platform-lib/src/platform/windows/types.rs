use bitflags::bitflags;
use windows_sys::Win32::Foundation::{LPARAM, WPARAM};
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
    #[derive(Debug, Clone, Copy)]
    pub struct MouseKeys: usize {
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