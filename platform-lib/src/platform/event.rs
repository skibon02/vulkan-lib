
#[derive(Debug, Clone, Copy)]
pub struct Pos(pub i32, pub i32);

#[derive(Debug, Clone, Copy)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    X1,
    X2,
}
#[derive(Debug, Clone, Copy)]
pub enum Event {
    KeyPressed(u32),
    KeyReleased(u32),
    MouseMoved(Pos),
    MouseButtonPressed(MouseButton, Pos),
}