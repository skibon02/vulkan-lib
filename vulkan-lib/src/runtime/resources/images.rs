use ash::vk::{DeviceMemory, Extent3D};
use slotmap::DefaultKey;
use crate::runtime::shared::SharedState;

pub struct ImageResource {
    shared: SharedState,

    state_key: DefaultKey,
    width: u32,
    height: u32
}

impl ImageResource {
    pub fn new(shared: SharedState, state_key: DefaultKey, memory: DeviceMemory, width: u32, height: u32) -> Self {
        Self {
            shared,

            state_key,
            width,
            height
        }
    }
    pub fn handle(&self) -> ImageResourceHandle {
        ImageResourceHandle {
            state_key: self.state_key,
            width: self.width,
            height: self.height
        }
    }
}

impl Drop for ImageResource {
    fn drop(&mut self) {
        self.shared.schedule_destroy_image(self.handle())
    }
}

#[derive(Copy, Clone, PartialEq)]
pub struct ImageResourceHandle {
    pub(crate) state_key: DefaultKey,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl ImageResourceHandle {
    pub fn extent(&self) -> Extent3D {
        Extent3D {
            width: self.width,
            height: self.height,
            depth: 1,
        }
    }
}