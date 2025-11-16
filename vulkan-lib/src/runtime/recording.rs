use std::iter;
use smallvec::{smallvec, SmallVec};
use ash::vk::{AccessFlags, BufferCopy, BufferImageCopy, ImageAspectFlags, ImageLayout, PipelineStageFlags};
use crate::runtime::resources::{BufferResourceHandle, ImageResourceHandle, ResourceUsage};

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

pub enum SpecificResourceUsage<'a> {
    BufferUsage {
        usage: ResourceUsage,
        handle: BufferResourceHandle<'a>
    },
    ImageUsage {
        usage: ResourceUsage,
        handle: ImageResourceHandle,
        required_layout: Option<ImageLayout>,
        image_aspect: ImageAspectFlags
    }
}

pub enum DeviceCommand<'a> {
    BufferCopy {
        src: BufferResourceHandle<'a>,
        dst: BufferResourceHandle<'a>,
        regions: SmallVec<[BufferCopy; 1]>,
    },
    BufferToImageCopy {
        src: BufferResourceHandle<'a>,
        dst: ImageResourceHandle,
        regions: SmallVec<[BufferImageCopy; 1]>,
    },
    Barrier,
}

impl<'a> DeviceCommand<'a> {
    pub fn usages(&self, submission_num: usize, group_num: usize) -> Box<dyn Iterator<Item=SpecificResourceUsage<'a>> + 'a> {
        match self {
            DeviceCommand::BufferCopy {
                src,
                dst,
                regions
            } => {
                Box::new(
                    [
                        SpecificResourceUsage::BufferUsage {
                            usage: ResourceUsage::new(
                                submission_num,
                                group_num,
                                PipelineStageFlags::TRANSFER,
                                AccessFlags::TRANSFER_READ,
                                true
                                ),
                            handle: *src
                        },
                        SpecificResourceUsage::BufferUsage {
                            usage: ResourceUsage::new(
                                submission_num,
                                group_num,
                                PipelineStageFlags::TRANSFER,
                                AccessFlags::TRANSFER_WRITE,
                                false
                            ),
                            handle: *dst
                        },
                    ].into_iter()
                )
            }

            DeviceCommand::BufferToImageCopy {
                src,
                dst,
                regions,
            } => {
                let combined_aspect = regions.iter()
                    .fold(ImageAspectFlags::empty(), |acc, region| acc | region.image_subresource.aspect_mask);
                Box::new(
                    [
                        SpecificResourceUsage::BufferUsage {
                            usage: ResourceUsage::new(
                                submission_num,
                                group_num,
                                PipelineStageFlags::TRANSFER,
                                AccessFlags::TRANSFER_READ,
                                true
                            ),
                            handle: *src
                        },
                        SpecificResourceUsage::ImageUsage {
                            usage: ResourceUsage::new(
                                submission_num,
                                group_num,
                                PipelineStageFlags::TRANSFER,
                                AccessFlags::TRANSFER_WRITE,
                                false
                            ),
                            handle: *dst,
                            required_layout: None,
                            image_aspect: combined_aspect
                        },
                    ].into_iter()
                )
            }
            
            DeviceCommand::Barrier => Box::new(iter::empty())
        }
    }
}