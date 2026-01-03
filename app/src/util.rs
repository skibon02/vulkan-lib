pub struct DoubleBuffered<T> {
    pub buffers: [T; 2],
    current: usize,
}

impl<T> DoubleBuffered<T> {
    pub fn new(mut f: impl FnMut() -> T) -> Self {
        Self {
            buffers: [f(), f()],
            current: 0,
        }
    }

    pub fn new_with_values(a: T, b: T) -> Self {
        Self {
            buffers: [a, b],
            current: 0,
        }
    }

    pub fn current(&self) -> &T {
        &self.buffers[self.current]
    }

    pub fn current_mut(&mut self) -> &mut T {
        &mut self.buffers[self.current]
    }

    pub fn next_frame(&mut self) {
        self.current = (self.current + 1) % 2;
    }
}
