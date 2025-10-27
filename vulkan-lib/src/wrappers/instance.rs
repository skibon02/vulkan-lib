use std::ops::Deref;
use std::sync::Arc;
use ash::Instance;
use sparkles::range_event_start;

pub type VkInstanceRef = Arc<VkInstance>;

/// RAII vulakn instance
pub struct VkInstance {
    instance: Instance
}

impl Deref for VkInstance {
    type Target = Instance;
    fn deref(&self) -> &Self::Target {
        &self.instance
    }
}

impl VkInstance {
    pub fn new(instance: Instance) -> VkInstance {
        VkInstance {
            instance,
        }
    }
}

impl Drop for VkInstance {
    fn drop(&mut self) {
        let g = range_event_start!("[Vulkan] Destroy instance");
        unsafe { self.instance.destroy_instance(None); }
    }
}