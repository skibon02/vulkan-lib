use ash::vk::{Buffer, DeviceMemory};
use slotmap::{DefaultKey, SlotMap};
use crate::wrappers::device::VkDeviceRef;

#[derive(Copy, Clone)]
pub struct BufferResource {
    state_key: DefaultKey,
    buffer: Buffer,
    memory: DeviceMemory,
    size: usize,
}

pub struct BufferInner {
    used_in: Vec<usize>,

}

pub struct ResourceStorage {
    device: VkDeviceRef,
    host_buffers: SlotMap<DefaultKey, BufferInner>,
}
impl ResourceStorage {
    pub fn new(device: VkDeviceRef) -> Self{

        Self {
            device,

            host_buffers: SlotMap::new(),
        }
    }
}