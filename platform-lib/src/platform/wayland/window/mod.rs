
#[derive(Default)]
pub struct WindowAttributes {
    pub title: String,
    pub initial_pos: Option<(i32, i32)>,
    pub initial_size: Option<(u16, u16)>,
    pub borderless: bool,
}

pub struct Window;

impl Window {
    pub fn new() -> Window {
        Window
    }
    pub fn close_window(mut self) {

    }
}