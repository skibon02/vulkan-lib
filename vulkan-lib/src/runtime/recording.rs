use smallvec::{smallvec, SmallVec};
use ash::vk::BufferCopy;
use crate::runtime::resources::BufferResourceHandle;

pub struct RecordContext<'a> {
    commands: Vec<DeviceCommand<'a>>,
}

impl<'a> RecordContext<'a> {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn buffer_copy<'b>(&'b mut self, src: BufferResourceHandle<'a>, dst: BufferResourceHandle<'a>, regions: SmallVec<[BufferCopy; 1]>) {
        self.commands.push(DeviceCommand::BufferCopy {
            src,
            dst,
            regions
        })
    }
    pub fn buffer_copy_single<'b>(&'b mut self, src: BufferResourceHandle<'a>, dst: BufferResourceHandle<'a>, region: BufferCopy) {
        let regions = smallvec![region];
        self.commands.push(DeviceCommand::BufferCopy {
            src,
            dst,
            regions
        })
    }

    pub(crate) fn take_commands(self) -> Vec<DeviceCommand<'a>> {
        self.commands
    }
}

pub enum DeviceCommand<'a> {
    BufferCopy {
        src: BufferResourceHandle<'a>,
        dst: BufferResourceHandle<'a>,
        regions: SmallVec<[BufferCopy; 1]>,
    }
}