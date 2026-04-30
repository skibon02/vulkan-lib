
#[derive(Debug, PartialEq)]
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

#[derive(Debug, PartialEq)]
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

use bitflags::bitflags;
use windows_sys::Win32::Foundation::{LPARAM, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{SC_CLOSE, SC_CONTEXTHELP, SC_DEFAULT, SC_HOTKEY, SC_HSCROLL, SC_KEYMENU, SC_MAXIMIZE, SC_MINIMIZE, SC_MONITORPOWER, SC_MOUSEMENU, SC_MOVE, SC_NEXTWINDOW, SC_PREVWINDOW, SC_RESTORE, SC_SIZE, SC_TASKLIST, SC_VSCROLL};

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
#[repr(i32)]
pub enum HitTest {
    Error       = -2,
    Transparent = -1,
    Nowhere     =  0,
    Client      =  1,
    Caption     =  2,
    SysMenu     =  3,
    GrowBox     =  4, // same as Size
    Menu        =  5,
    HScroll     =  6,
    VScroll     =  7,
    MinButton   =  8, // same as Reduce
    MaxButton   =  9, // same as Zoom
    Left        = 10,
    Right       = 11,
    Top         = 12,
    TopLeft     = 13,
    TopRight    = 14,
    Bottom      = 15,
    BottomLeft  = 16,
    BottomRight = 17,
    Border      = 18,
    Close       = 20,
    Help        = 21,
}

impl HitTest {
    pub fn from_isize(value: isize) -> Option<Self> {
        match value {
            -2 => Some(Self::Error),
            -1 => Some(Self::Transparent),
            0 => Some(Self::Nowhere),
            1 => Some(Self::Client),
            2 => Some(Self::Caption),
            3 => Some(Self::SysMenu),
            4 => Some(Self::GrowBox),
            5 => Some(Self::Menu),
            6 => Some(Self::HScroll),
            7 => Some(Self::VScroll),
            8 => Some(Self::MinButton),
            9 => Some(Self::MaxButton),
            10 => Some(Self::Left),
            11 => Some(Self::Right),
            12 => Some(Self::Top),
            13 => Some(Self::TopLeft),
            14 => Some(Self::TopRight),
            15 => Some(Self::Bottom),
            16 => Some(Self::BottomLeft),
            17 => Some(Self::BottomRight),
            18 => Some(Self::Border),
            20 => Some(Self::Close),
            21 => Some(Self::Help),
            _ => None,
        }
    }
}