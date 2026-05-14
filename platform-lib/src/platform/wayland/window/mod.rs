use calloop::channel::Sender;
use crate::DirectWindowMessage;
use crate::platform::wayland::WindowMessage;

#[derive(Default)]
pub struct WindowAttributes {
    pub title: String,
    pub initial_pos: Option<(i32, i32)>,
    pub initial_size: Option<(u16, u16)>,
    pub borderless: bool,
}

pub struct Window {
    id: u64,
    tx: Sender<WindowMessage>
}

impl Window {
    pub(crate) fn new(id: u64, tx: Sender<WindowMessage>) -> Window {
        Window {
            id,
            tx
        }
    }
    pub fn close_window(mut self) {
        let _ = self.tx.send(WindowMessage::Direct(self.id, DirectWindowMessage::Close));
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let _ = self.tx.send(WindowMessage::Direct(self.id, DirectWindowMessage::Close));
    }
}