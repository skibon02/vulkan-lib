use std::ffi::{c_char, CString};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use anyhow::bail;
use ash::Entry;
use ash::vk::{make_api_version, ApplicationInfo, BufferCreateInfo, Extent2D, PhysicalDevice};
use log::{info, warn};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use sparkles::range_event_start;
use crate::swapchain_wrapper::SwapchainWrapper;
use crate::wrappers::capabilities_checker::CapabilitiesChecker;
use crate::wrappers::debug_report::VkDebugReport;
use crate::wrappers::device::VkDeviceRef;
use crate::wrappers::surface::{VkSurface, VkSurfaceRef};
use crate::extensions::calibrated_timestamps::CalibratedTimestamps;
use crate::wrappers::timestamp_pool::TimestampPool;

use crate::queue::GraphicsQueue;
pub use ash::vk;
pub use vk::{DescriptorType, ShaderStageFlags};
use crate::queue::shared::SharedState;

mod wrappers;
mod swapchain_wrapper;
pub mod util;
pub mod shaders;
mod extensions;
pub mod queue;

#[cfg(target_os = "android")]
pub mod android;
pub mod resources;

static INSTANCE_SLOT: Mutex<Weak<VulkanInstance>> = Mutex::new(Weak::new());

pub(crate) fn try_get_instance() -> Option<Arc<VulkanInstance>> {
    INSTANCE_SLOT.lock().unwrap().upgrade()
}

pub struct VulkanInstance {
    debug_report: VkDebugReport,
    physical_device: PhysicalDevice,
    device: VkDeviceRef,
    shared_state: SharedState,

    entry: Entry,
}

impl VulkanInstance {
    #[track_caller]
    pub fn new_for_handle(window_handle: RawWindowHandle, display_handle: RawDisplayHandle, initial_size: (u32, u32), api_version: u32) -> anyhow::Result<GraphicsQueue> {
        let Ok(entry) = (unsafe { Entry::load() }) else {
            bail!("Failed to load Vulkan entry");
        };

        let g = range_event_start!("[Vulkan] INIT");
        let app_name = CString::new("Hello Vulkan")?;

        let app_info = ApplicationInfo::default()
            .application_name(&app_name)
            .application_version(make_api_version(0, 1, 0, 0))
            .engine_name(&app_name)
            .engine_version(make_api_version(0, 1, 0, 0))
            .api_version(api_version);

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
        let instance = caps_checker.create_instance(&entry, &app_info, &mut instance_layers_refs,
                                                    &mut instance_extensions, &mut debug_report_callback_info)?;

        let surface = VkSurface::new(&entry, instance.clone(), display_handle, window_handle)?;

        let debug_report = VkDebugReport::new(&entry, instance.clone())?;
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
                panic!("No available physical device found");
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

        // desired device extensions to be enabled
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
            Some(CalibratedTimestamps::new(&entry, instance.as_ref(), physical_device, device.as_ref()))
        }
        else {
            warn!("Calibrated timestamps extension is supported");
            None
        };


        let queue = unsafe { device.get_device_queue(queue_family_index, 0) };



        let memory_properties = unsafe {
            device
                .instance()
                .get_physical_device_memory_properties(physical_device)
        };

        let memory_heaps = memory_properties.memory_heaps_as_slice().to_vec();
        let memory_types = memory_properties.memory_types_as_slice().to_vec();

        let extent = Extent2D {
            width: initial_size.0,
            height: initial_size.1,
        };
        let swapchain_wrapper = SwapchainWrapper::new(
            device.clone(),
            physical_device,
            extent,
            surface,
            None,
        )?;

        let shared_state = SharedState::new(device.clone());
        let res = Arc::new(Self {
            entry,
            physical_device,
            device: device.clone(),
            debug_report,
            shared_state,
        });
        {
            let mut slot = INSTANCE_SLOT.lock().unwrap();
            *slot = Arc::downgrade(&res);
        }


        Ok(GraphicsQueue::new(
            res,
            queue_family_index,
            queue,
            physical_device,
            swapchain_wrapper,
            calibrated_timestamps,
            timestamp_pool,
            memory_types,
            memory_heaps,
        ))
    }
}