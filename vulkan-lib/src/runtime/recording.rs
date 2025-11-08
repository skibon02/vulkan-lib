use smallvec::SmallVec;
use ash::vk::BufferCopy;
use crate::runtime::resources::BufferResourceHandle;

pub struct RecordContext {

}

impl RecordContext {
    pub fn new() -> Self {
        Self {

        }
    }
}

pub enum DeviceCommand<'a> {
    BufferCopy {
        src: BufferResourceHandle<'a>,
        dst: BufferResourceHandle<'a>,
        regions: SmallVec<[BufferCopy; 1]>,
    }
}