use std::ops::Deref;
use std::sync::Arc;
use ash::{Device, Instance};
use ash::vk::PhysicalDevice;
use sparkles::range_event_start;
use crate::wrappers::instance::VkInstanceRef;

pub type VkDeviceRef = Arc<VkDevice>;

/// Reference to the vulkan Device.
/// When last reference is destroyed, device is destroyed as well
#[derive(Clone)]
pub struct VkDevice {
    physical_device: PhysicalDevice,
    device: Device,
    instance: VkInstanceRef,
}
impl VkDevice {
    pub fn new(physical_device: PhysicalDevice, device: Device, instance: VkInstanceRef) -> VkDevice {
        VkDevice {
            physical_device,
            device,
            instance
        }
    }
    pub(crate) fn instance(&self) -> &Instance {
        &self.instance
    }
    pub(crate) fn physical_device(&self) -> PhysicalDevice {
        self.physical_device
    }
}

impl Deref for VkDevice {
    type Target = Device;
    fn deref(&self) -> &Self::Target {
        &self.device
    }
}

impl Drop for VkDevice {
    fn drop(&mut self) {
        let g = range_event_start!("[Vulkan] Destroy device");
        // Safety: We use raii and ensure that everyon who use device
        unsafe { self.device.destroy_device(None); }
    }
}