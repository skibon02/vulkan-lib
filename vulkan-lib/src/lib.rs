use std::ffi::{c_char, CString};
use anyhow::Context;
use ash::vk;
use ash::vk::{make_api_version, ApplicationInfo, CommandPool, Extent2D, PhysicalDevice, Queue};
use log::{debug, info, warn};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use sparkles::range_event_start;
use crate::swapchain_wrapper::SwapchainWrapper;
use crate::wrappers::capabilities_checker::CapabilitiesChecker;
use crate::wrappers::debug_report::VkDebugReport;
use crate::wrappers::device::VkDeviceRef;
use crate::wrappers::surface::{VkSurface, VkSurfaceRef};

pub mod instance;
mod wrappers;
mod swapchain_wrapper;
mod pipeline;
mod descriptor_sets;
pub mod util;
pub mod shaders;

pub struct VulkanRenderer {
    debug_report: VkDebugReport,
    surface: VkSurfaceRef,
    physical_device: PhysicalDevice,
    device: VkDeviceRef,
    queue: Queue,
    command_pool: CommandPool,

    swapchain_wrapper: SwapchainWrapper,
    acq_semaphore: vk::Semaphore,

    // extensions
}
impl VulkanRenderer {
    pub fn new_for_window(window_handle: RawWindowHandle, display_handle: RawDisplayHandle, window_size: (u32, u32)) -> anyhow::Result<Self> {
        let g = range_event_start!("[Vulkan] INIT");
        info!(
            "Vulkan init started! Initializing for size: {:?}",
            window_size
        );

        let app_name = CString::new("Hello Vulkan")?;

        let app_info = ApplicationInfo::default()
            .application_name(&app_name)
            .application_version(make_api_version(0, 1, 0, 0))
            .engine_name(&app_name)
            .engine_version(make_api_version(0, 1, 0, 0))
            .api_version(vk::API_VERSION_1_0);

        //define desired layers
        // 1. Khronos validation layers (optional)
        let mut instance_layers = vec![];
        if cfg!(feature = "validation") {
            instance_layers.push(CString::new("VK_LAYER_KHRONOS_validation")?);
        }
        let mut instance_layers_refs: Vec<*const c_char> =
            instance_layers.iter().map(|l| l.as_ptr()).collect();

        //define desired extensions
        // 1 Debug report
        // 2,3 Required extensions for surface support (platform_specific surface + general surface)
        // 4 Portability enumeration (for moltenvk)
        let surface_required_extensions =
            ash_window::enumerate_required_extensions(display_handle)?;
        let mut instance_extensions: Vec<*const c_char> = surface_required_extensions.to_vec();
        instance_extensions.push(ash::ext::debug_report::NAME.as_ptr());

        let mut debug_report_callback_info = VkDebugReport::get_messenger_create_info();

        let mut caps_checker = CapabilitiesChecker::new();

        // caps_checker will check requested layers and extensions and enable only the
        // supported ones, which can be requested later
        let instance = caps_checker.create_instance(&app_info, &mut instance_layers_refs,
                                                    &mut instance_extensions, &mut debug_report_callback_info)?;

        let surface = VkSurface::new(instance.clone(), display_handle, window_handle)?;

        let debug_report = VkDebugReport::new(instance.clone())?;
        // instance is created. debug report ready

        let physical_devices = unsafe { instance.enumerate_physical_devices()? };

        let physical_device = *physical_devices
            .iter()
            .find(|&d| {
                let properties = unsafe { instance.get_physical_device_properties(*d) };
                properties.device_type == vk::PhysicalDeviceType::DISCRETE_GPU
            })
            .or_else(|| {
                warn!("Discrete GPU was not found!");
                physical_devices.iter().find(|&d| {
                    let properties = unsafe { instance.get_physical_device_properties(*d) };
                    properties.device_type == vk::PhysicalDeviceType::INTEGRATED_GPU
                })
            })
            .or_else(|| {
                warn!("Integrated GPU was not found!");
                physical_devices.iter().find(|&d| {
                    let properties = unsafe { instance.get_physical_device_properties(*d) };
                    properties.device_type == vk::PhysicalDeviceType::CPU
                })
            })
            .unwrap_or_else(|| {
                panic!("No avaliable physical device found");
            });

        //select chosen physical device
        let dev_name_array = unsafe {
            instance
                .get_physical_device_properties(physical_device)
                .device_name
        };
        let dev_name = unsafe { std::ffi::CStr::from_ptr(dev_name_array.as_ptr()) };
        info!("Chosen device: {}", dev_name.to_str().unwrap());

        let queue_family_properties =
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
        let queue_family_index = queue_family_properties
            .iter()
            .enumerate()
            .find(|(_, p)| {
                let support_graphics = p.queue_flags.contains(vk::QueueFlags::GRAPHICS);
                let support_presentation = surface.query_presentation_support(physical_device);

                support_graphics && support_presentation
            })
            .map(|(i, _)| i as u32)
            .unwrap_or_else(|| {
                panic!("No available queue family found");
            });

        let device_extensions = vec![ash::khr::swapchain::NAME.as_ptr(), ash::ext::calibrated_timestamps::NAME.as_ptr()];

        let queue_create_infos = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&[1.0])];
        let mut device_create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_create_infos)
            .enabled_extension_names(&device_extensions);

        let device = caps_checker.create_device(
            instance.clone(),
            physical_device,
            &mut device_create_info,
        )?;

        let device_properties = unsafe { instance.get_physical_device_properties(physical_device) };
        let device_limits = device_properties.limits;

        let queue = unsafe { device.get_device_queue(queue_family_index, 0) };


        let extent = Extent2D {
            width: window_size.0,
            height: window_size.1,
        };
        let swapchain_wrapper = SwapchainWrapper::new(
            device.clone(),
            physical_device,
            extent,
            surface.clone(),
            None,
        )?;

        let command_pool = unsafe { device.create_command_pool(&vk::CommandPoolCreateInfo::default()
            .queue_family_index(queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER), None)
        }.unwrap();

        let acq_semaphore = unsafe {
            device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None).unwrap()
        };
        
        Ok(Self {
            device,
            debug_report,
            surface,
            physical_device,
            queue,
            command_pool,
            swapchain_wrapper,
            acq_semaphore,
        })
    }

    pub fn recreate_resize(&mut self, new_extent: (u32, u32)) {
        let g = range_event_start!("[Vulkan] Recreate swapchain");
        let new_extent = Extent2D {
            width: new_extent.0,
            height: new_extent.1,
        };
        // Submit all commands and wait for idle
        self.wait_idle();

        // 1. Destroy swapchain dependent resources
        // unsafe {
        //     self.render_pass_resources
        //         .destroy(&mut self.resource_manager);
        // }

        // 2. Recreate swapchain
        let old_format = self.swapchain_wrapper.get_surface_format();
        unsafe {
            self.swapchain_wrapper
                .recreate(self.physical_device, new_extent, self.surface.clone())
                .unwrap()
        };
        let new_format = self.swapchain_wrapper.get_surface_format();
        if new_format != old_format {
            unimplemented!("Swapchain returned the wrong format");
        }

        // // 3. Recreate swapchain_dependent resources
        // self.render_pass_resources = self.render_pass.create_render_pass_resources(
        //     self.swapchain_wrapper.get_image_views(),
        //     self.swapchain_wrapper.get_extent(),
        //     &mut self.resource_manager,
        // );

    }

    fn wait_idle(&self) {
        let start = std::time::Instant::now();
        unsafe {
            self.device.queue_wait_idle(self.queue).unwrap();
        }
        let end = std::time::Instant::now();
        debug!("Waited for idle for {:?}", end - start);
    }
    
    pub fn render(&mut self) -> anyhow::Result<()> {
        let (image, is_suboptimal) = unsafe {
            self.swapchain_wrapper.swapchain_loader
                .acquire_next_image(
                    self.swapchain_wrapper.get_swapchain(),
                    u64::MAX,
                    self.acq_semaphore,
                    vk::Fence::null(),
                )
        }.context("acquire next image")?;

        if is_suboptimal {
            warn!("Swapchain is suboptimal!");
        }


        let is_suboptimal = unsafe {
            self.swapchain_wrapper.swapchain_loader
                .queue_present(self.queue, &vk::PresentInfoKHR::default()
                    .wait_semaphores(&[self.acq_semaphore])
                    .swapchains(&[self.swapchain_wrapper.get_swapchain()])
                    .image_indices(&[image]))
        }.context("queue preset")?;

        if is_suboptimal {
            warn!("Swapchain is suboptimal on present!");
        }

        Ok(())
    }
}

impl Drop for VulkanRenderer {
    fn drop(&mut self) {
        // Called before everything is dropped
        info!("vulkan: drop");
        self.wait_idle();

        unsafe {
            self.device.destroy_semaphore(self.acq_semaphore, None);
            self.device.destroy_command_pool(self.command_pool, None);
        }
    }
}
