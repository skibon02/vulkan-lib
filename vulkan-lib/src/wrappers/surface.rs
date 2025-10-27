use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use std::sync::Arc;
use anyhow::Context;
use ash::Entry;
use ash::vk::{PhysicalDevice, SurfaceKHR};
use ash_window::create_surface;
use sparkles::range_event_start;
use crate::wrappers::instance::VkInstanceRef;

pub type VkSurfaceRef = Arc<VkSurface>;
pub struct VkSurface {
    surface_loader: ash::khr::surface::Instance,
    surface: SurfaceKHR,
    _instance: VkInstanceRef,
}

impl VkSurface {
    pub fn new(instance: VkInstanceRef, display_h: RawDisplayHandle, window_h: RawWindowHandle) -> anyhow::Result<VkSurfaceRef> {
        let entry = Entry::linked();
        let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);
        let surface = unsafe { create_surface(&entry, &instance, display_h, window_h, None).context("Surface creation")? };

        Ok(Arc::new(VkSurface {
            surface_loader,
            surface,
            _instance: instance
        }))
    }
    pub fn query_presentation_support(&self, physical_device: PhysicalDevice) -> bool {
        // TODO: check all queue families, not just first one
        unsafe { self.surface_loader.get_physical_device_surface_support(physical_device, 0, self.surface) }.unwrap()
    }
    pub fn surface(&self) -> &SurfaceKHR {
        &self.surface
    }
    pub fn loader(&self) -> &ash::khr::surface::Instance {
        &self.surface_loader
    }
}

impl Drop for VkSurface {
    fn drop(&mut self) {
        unsafe {
            let g = range_event_start!("[Vulkan] Destroy surface");
            self.surface_loader.destroy_surface(self.surface, None);
        }
    }
}