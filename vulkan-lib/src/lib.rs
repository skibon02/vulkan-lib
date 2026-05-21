use std::ffi::{c_char, CString};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use anyhow::bail;
use ash::Entry;
use ash::khr::{wayland_surface, win32_surface};
use ash::vk::{make_api_version, wl_display, ApplicationInfo, BufferCreateInfo, Extent2D, PhysicalDevice, PhysicalDeviceType, API_VERSION_1_0};
use log::{info, warn};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use sparkles::range_event_start;
use crate::wrappers::capabilities_checker::CapabilitiesChecker;
use crate::wrappers::debug_report::VkDebugReport;
use crate::wrappers::device::VkDeviceRef;
use crate::wrappers::surface::{VkSurface, VkSurfaceRef};
use crate::extensions::calibrated_timestamps::CalibratedTimestamps;
use crate::extensions::low_latency2::LowLatency2;
pub use crate::extensions::low_latency2::ReflexMode;
use crate::extensions::present_timing::{
    PhysicalDevicePresentTimingFeaturesEXT, PresentTiming,
};
use crate::wrappers::timestamp_pool::TimestampPool;

use crate::queue::GraphicsQueue;
pub use ash::vk;
pub use vk::{DescriptorType, ShaderStageFlags};
use platform_lib::{current_platform, PlatformKind};
use crate::queue::shared::SharedState;

mod wrappers;
mod swapchain_wrapper;
mod util;
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
    shared_state: SharedState,
    device: VkDeviceRef,

    entry: Entry,
}

impl VulkanInstance {
    #[track_caller]
    pub fn new_1_0(app_name: &str) -> anyhow::Result<GraphicsQueue> {
        let api_version = API_VERSION_1_0;
        let platform = current_platform();
        info!("Vulkan: Initializing for: {:?}", platform);

        let Ok(entry) = (unsafe { Entry::load() }) else {
            bail!("Failed to load Vulkan entry");
        };

        let g = range_event_start!("[Vulkan] INIT");
        let app_name = CString::new(app_name)?;

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
        let mut instance_extensions = Vec::<*const c_char>::new();
        instance_extensions.push(ash::ext::debug_report::NAME.as_ptr());
        // Instance-level dependency of VK_EXT_present_timing.
        instance_extensions.push(ash::khr::get_surface_capabilities2::NAME.as_ptr());
        // for MoltenVK
        instance_extensions.push(ash::khr::portability_enumeration::NAME.as_ptr());
        if cfg!(feature = "validation") {
            instance_extensions.push(ash::ext::validation_features::NAME.as_ptr());
        }
        // platform-dependent surface extensions
        instance_extensions.push(ash::khr::surface::NAME.as_ptr());
        match platform {
            PlatformKind::Windows => {
                instance_extensions.push(ash::khr::win32_surface::NAME.as_ptr());
            }
            PlatformKind::Android => {
                instance_extensions.push(ash::khr::android_surface::NAME.as_ptr());
            }
            PlatformKind::X11 => {
                instance_extensions.push(ash::khr::xcb_surface::NAME.as_ptr());
            }
            PlatformKind::Wayland => {
                instance_extensions.push(ash::khr::wayland_surface::NAME.as_ptr());
            }
            PlatformKind::X11OrWayland => {
                instance_extensions.push(ash::khr::wayland_surface::NAME.as_ptr());
                instance_extensions.push(ash::khr::xcb_surface::NAME.as_ptr());
            }
            PlatformKind::Orbital => {
                panic!("Orbital platform not supported!")
            }
            other => {
                panic!("Unsupported platform {:?}", other)
            }
        };


        let mut debug_report_callback_info = VkDebugReport::get_messenger_create_info();

        let enabled_validation_features = [
            vk::ValidationFeatureEnableEXT::BEST_PRACTICES,
            vk::ValidationFeatureEnableEXT::SYNCHRONIZATION_VALIDATION,
        ];
        let validation_features = vk::ValidationFeaturesEXT::default()
            .enabled_validation_features(&enabled_validation_features);
        if cfg!(feature = "validation") {
            debug_report_callback_info.p_next =
                (&validation_features as *const vk::ValidationFeaturesEXT).cast();
        }

        let mut caps_checker = CapabilitiesChecker::new();

        // caps_checker will check requested layers and extensions and enable only the
        // supported ones, which can be requested later
        let instance = caps_checker.create_instance(&entry, &app_info, &mut instance_layers_refs,
                                                    &mut instance_extensions, &mut debug_report_callback_info)?;

        let debug_report = VkDebugReport::new(&entry, instance.clone())?;
        // instance is created. debug report ready

        let mut physical_devices = unsafe { instance.enumerate_physical_devices()? }.into_iter().map(|physical_device| {
            let props = unsafe {
                instance.get_physical_device_properties(physical_device)
            };
            let qf_props =
                unsafe { instance.get_physical_device_queue_family_properties(physical_device) };

            (physical_device, props, qf_props)
        }).filter_map(|(d, prop, qf_prop)| {
            // fill in presentation support information, filter out devices without presentation support on all qf
            // android: presentation is always supported to any surface
            // win32: f(device, q_family) -> bool
            // wayland (needs wl_display connection): f(device, q_family) -> bool
            // x11 (needs xcb connection): f(visualID, device, q_family) -> bool

            let qf_presentation =  match platform {
                PlatformKind::Android => {
                    qf_prop.iter().map(|_| true).collect::<Vec<_>>()
                }
                PlatformKind::Windows => {
                    let mut win32_surface = win32_surface::Instance::new(&entry, &instance);
                    qf_prop.iter().enumerate()
                        .map(|(i, qf_properties)| {
                            unsafe { win32_surface.get_physical_device_win32_presentation_support(d, i as u32) }
                        }).collect::<Vec<_>>()
                }
                PlatformKind::Wayland => {
                    let con = platform_lib::wayland_connection().unwrap() as *mut wl_display;
                    let mut wl_surface = wayland_surface::Instance::new(&entry, &instance);
                    qf_prop.iter().enumerate()
                        .map(|(i, qf_properties)| {
                            unsafe { wl_surface.get_physical_device_wayland_presentation_support(d, i as u32, &mut *con) }
                        }).collect::<Vec<_>>()
                }
                other => {
                    todo!("queue family filtering not implemented on platform {:?}", other)
                }
            };

            if qf_presentation.iter().all(|p| !*p) {
                None
            }
            else {
                Some((d, prop, qf_prop, qf_presentation))
            }
        }).collect::<Vec<_>>();

        // sort by gpu type: discrete -> integrated -> other
        physical_devices.sort_by_key(|(_, prop, _, _)| {
            match prop.device_type {
                PhysicalDeviceType::DISCRETE_GPU => 1,
                PhysicalDeviceType::INTEGRATED_GPU => 2,
                _ => 3,
            }
        });

        //select chosen physical device
        let Some((physical_device, physical_device_properties, queue_family_properties, qf_presentation_support)) = physical_devices.into_iter().next() else {
            bail!("Could not find Vulkan Devices with presentation support!")
        };

        let dev_name = unsafe { std::ffi::CStr::from_ptr(physical_device_properties.device_name.as_ptr()) };
        info!("Chosen device: {}", dev_name.to_str().unwrap());

        let queue_family_index = queue_family_properties
            .iter()
            .enumerate()
            .zip(qf_presentation_support.iter())
            .find(|((_, props), presentation_supported)| {
                let support_graphics = props.queue_flags.contains(vk::QueueFlags::GRAPHICS);
                support_graphics && **presentation_supported
            })
            .map(|((i, _), _)| i as u32)
            .unwrap_or_else(|| {
                panic!("No available queue family found");
            });

        let present_timing_name = CString::new("VK_EXT_present_timing")?;
        let calibrated_timestamps_khr_name = CString::new("VK_KHR_calibrated_timestamps")?;
        let timeline_sem_name = CString::new("VK_KHR_timeline_semaphore")?;
        let mut device_extensions = vec![
            ash::khr::swapchain::NAME.as_ptr(),
            ash::ext::calibrated_timestamps::NAME.as_ptr(),
            ash::nv::low_latency2::NAME.as_ptr(),
            timeline_sem_name.as_ptr(),
        ];
        if cfg!(feature = "present-timing") {
            device_extensions.push(calibrated_timestamps_khr_name.as_ptr());
            device_extensions.push(present_timing_name.as_ptr());
        }

        let queue_create_infos = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&[1.0])];
        let mut device_create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_create_infos)
            .enabled_extension_names(&device_extensions);

        let mut pt_features = PhysicalDevicePresentTimingFeaturesEXT::enabled();
        if cfg!(feature = "present-timing") {
            device_create_info = caps_checker.try_chain_device_feature(
                &instance,
                physical_device,
                device_create_info,
                &present_timing_name,
                &mut pt_features,
            )?;
        }

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
        let low_latency2 = if caps_checker.is_device_extension_enabled(ash::nv::low_latency2::NAME) {
            Some(LowLatency2::new(instance.as_ref(), device.as_ref()))
        } else {
            warn!("VK_NV_low_latency2 not available on this device");
            None
        };
        let present_timing = if caps_checker.is_device_extension_enabled(present_timing_name.as_c_str()) {
            match PresentTiming::new(instance.as_ref(), device.as_ref()) {
                Some(pt) => {
                    Some(pt)
                }
                None => {
                    warn!("VK_EXT_present_timing reported as supported but proc addrs failed to load");
                    None
                }
            }
        } else {
            None
        };

        let queue = unsafe { device.get_device_queue(queue_family_index, 0) };

        // let surface = VkSurface::new(&entry, instance.clone(), display_handle, window_handle)?;
        // let extent = Extent2D {
        //     width: initial_size.0,
        //     height: initial_size.1,
        // };
        //
        // let swapchain_wrapper = SwapchainWrapper::new(
        //     device.clone(),
        //     physical_device,
        //     extent,
        //     surface,
        //     None,
        //     low_latency2.is_some(),
        //     present_timing.is_some(),
        // )?;

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
            calibrated_timestamps,
            timestamp_pool,
            low_latency2,
            present_timing,
        ))
    }
}