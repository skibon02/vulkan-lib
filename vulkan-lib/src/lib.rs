use ash::vk::{CommandPool, PhysicalDevice, Queue};
use log::{debug, info};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use crate::render_pass::RenderPassWrapper;
use crate::swapchain_wrapper::SwapchainWrapper;
use crate::wrappers::debug_report::VkDebugReport;
use crate::wrappers::device::VkDeviceRef;
use crate::wrappers::surface::VkSurfaceRef;

pub mod instance;
mod wrappers;
mod swapchain_wrapper;
mod render_pass;
mod pipeline;
mod descriptor_sets;
mod util;

pub struct VulkanRenderer {
    debug_report: VkDebugReport,
    surface: VkSurfaceRef,
    physical_device: PhysicalDevice,
    device: VkDeviceRef,
    queue: Queue,
    command_pool: CommandPool,

    swapchain_wrapper: SwapchainWrapper,

    // extensions

    // Rendering stuff
    render_pass: RenderPassWrapper,
}
impl VulkanRenderer {
    pub fn new_for_window(window_handle: RawWindowHandle, display_handle: RawDisplayHandle, window_size: (u32, u32)) -> anyhow::Result<Self> {

        Ok(Self {

        })
    }

    pub fn recreate_resize(&mut self, new_extent: (u32, u32)) {

    }

    fn wait_idle(&self) {
        let start = std::time::Instant::now();
        unsafe {
            self.device.queue_wait_idle(self.queue).unwrap();
        }
        let end = std::time::Instant::now();
        debug!("Waited for idle for {:?}", end - start);
    }
}

impl Drop for VulkanRenderer {
    fn drop(&mut self) {
        info!("vulkan: drop");
        self.wait_idle();
        unsafe {
            self.render_pass_resources
                .destroy(&mut self.resource_manager);
        }
    }
}
