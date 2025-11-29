use std::ffi::{c_char, CString};
use ash::vk;
use ash::vk::{make_api_version, ApplicationInfo, BufferCreateInfo, Extent2D};
use log::{info, warn};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use sparkles::range_event_start;
use crate::swapchain_wrapper::SwapchainWrapper;
use crate::wrappers::capabilities_checker::CapabilitiesChecker;
use crate::wrappers::debug_report::VkDebugReport;
use crate::wrappers::device::VkDeviceRef;
use crate::wrappers::surface::{VkSurface, VkSurfaceRef};
use crate::runtime::RuntimeState;

pub use vk::BufferUsageFlags;
pub use vk::PipelineStageFlags;
pub use vk::BufferCopy;
pub use vk::BufferImageCopy;
pub use vk::Extent3D;
pub use vk::Offset3D;
pub use vk::ImageLayout;
pub use vk::ImageAspectFlags;
pub use vk::ImageSubresourceLayers;
pub use vk::ClearColorValue;
pub use vk::SampleCountFlags;
pub use vk::AttachmentLoadOp;
pub use vk::{AttachmentStoreOp, AttachmentDescription, Format, DescriptorType, ShaderStageFlags, ClearValue, ClearDepthStencilValue};
pub use crate::runtime::{DoubleBufferedDescriptorSets, DoubleBuffered};
use crate::extensions::calibrated_timestamps::CalibratedTimestamps;
use crate::wrappers::timestamp_pool::TimestampPool;

pub mod instance;
mod wrappers;
mod swapchain_wrapper;
pub mod util;
pub mod shaders;
pub mod runtime;
mod extensions;

#[cfg(target_os = "android")]
pub mod android;

pub struct VulkanRenderer {
    debug_report: VkDebugReport,
    surface: VkSurfaceRef,
    device: VkDeviceRef,

    // runtime state
    runtime_state: RuntimeState,
}

impl VulkanRenderer {
    #[track_caller]
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


        // extensions
        let timestamp_query_support = device_limits.timestamp_period != 0.0 && device_limits.timestamp_compute_and_graphics != 0
            && queue_family_properties[queue_family_index as usize].timestamp_valid_bits != 0;
        let timestamp_pool = if !timestamp_query_support {
            warn!("Timestamp query is not supported!");
            None
        }
        else {
            let res = TimestampPool::new(device.clone(), 10, device_limits.timestamp_period);
            res
        };
        let calibrated_timestamps = if caps_checker.is_device_extension_enabled(ash::ext::calibrated_timestamps::NAME) {
            Some(CalibratedTimestamps::new(instance.as_ref(), physical_device, device.as_ref()))
        }
        else {
            warn!("Calibrated timestamps extension is supported");
            None
        };


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

        let memory_properties = unsafe {
            device
                .instance()
                .get_physical_device_memory_properties(physical_device)
        };

        let memory_heaps = memory_properties.memory_heaps_as_slice().to_vec();
        let memory_types = memory_properties.memory_types_as_slice().to_vec();

        let runtime_state = RuntimeState::new(
            device.clone(),
            queue_family_index,
            queue,
            physical_device,
            memory_types,
            memory_heaps,
            swapchain_wrapper,
            surface.clone(),
            calibrated_timestamps,
            timestamp_pool,
        );

        let mut res = Self {
            device,
            debug_report,
            surface,
            runtime_state,
        };

        res.runtime_state.update_swapchain_image_handles();

        Ok(res)
    }

    pub fn test_buffer_sizes(&mut self, usage: vk::BufferUsageFlags) {
        info!("Test buffer sizes for usage {:?}", usage);

        let buffer = unsafe {
            self.device.create_buffer(&BufferCreateInfo::default()
                .usage(usage)
                .size(256), None).unwrap()
        };

        let memory_requirements = unsafe { self.device.get_buffer_memory_requirements(buffer) };

        let alignment = memory_requirements.alignment;
        let memory_types = memory_requirements.memory_type_bits;

        // if memory_requirements.size != i {
        //     info!("{} -> {}", i, memory_requirements.size);
        // }

        unsafe {
            self.device.destroy_buffer(buffer, None);
        }
        info!("Alignment: {}. Memory types: {:b}", alignment, memory_types);
    } 
}

impl std::ops::Deref for VulkanRenderer {
    type Target = RuntimeState;

    fn deref(&self) -> &Self::Target {
        &self.runtime_state
    }
}

impl std::ops::DerefMut for VulkanRenderer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.runtime_state
    }
}

impl Drop for VulkanRenderer {
    fn drop(&mut self) {
        // Called before everything is dropped
        info!("vulkan: drop");
        self.wait_idle();
    }
}