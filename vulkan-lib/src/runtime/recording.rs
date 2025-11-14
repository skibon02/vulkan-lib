use std::iter;
use smallvec::{smallvec, SmallVec};
use ash::vk::{AccessFlags, BufferCopy, PipelineStageFlags};
use crate::runtime::resources::{BufferResourceHandle, ResourceUsage};

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

    pub fn barrier(&mut self) {
        self.commands.push(DeviceCommand::Barrier)
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
    },
    Barrier,
}

impl<'a> DeviceCommand<'a> {
    pub fn usages(&self, submission_num: usize) -> Box<dyn Iterator<Item=(ResourceUsage, BufferResourceHandle<'a>)> + 'a> {
        match self {
            DeviceCommand::BufferCopy {
                src,
                dst,
                regions
            } => {
                Box::new(
                    [
                        (ResourceUsage::new(
                            submission_num, 
                            PipelineStageFlags::TRANSFER, 
                            AccessFlags::TRANSFER_READ, 
                            true
                        ), *src), 
                        (ResourceUsage::new(
                            submission_num,
                            PipelineStageFlags::TRANSFER,
                            AccessFlags::TRANSFER_WRITE,
                            false
                        ), *dst)
                    ].into_iter()
                )
            }
            
            DeviceCommand::Barrier => Box::new(iter::empty())
        }
    }
}