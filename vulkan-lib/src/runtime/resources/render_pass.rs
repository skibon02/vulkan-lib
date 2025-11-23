use slotmap::DefaultKey;
use crate::runtime::shared::SharedState;

#[derive(Clone)]
pub struct RenderPassResource {
    state_key: DefaultKey,
    shared: SharedState,
}

impl RenderPassResource {
    pub fn new(state_key: DefaultKey, shared: SharedState) -> Self {
        Self {
            state_key,
            shared
        }
    }
    pub fn handle(&self) -> RenderPassHandle {
        RenderPassHandle(self.state_key)
    }
}

#[derive(Copy, Clone)]
pub struct RenderPassHandle(pub(crate) DefaultKey);

impl Drop for RenderPassResource {
    fn drop(&mut self) {
        self.shared.schedule_destroy_render_pass(self.handle());
    }
}
