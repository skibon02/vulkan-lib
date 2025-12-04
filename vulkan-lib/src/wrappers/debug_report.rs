use std::ffi::{c_char, c_void};
use ash::{vk, Entry};
use ash::vk::{DebugReportCallbackCreateInfoEXT, DebugReportFlagsEXT, DebugReportObjectTypeEXT, DebugUtilsMessengerCreateInfoEXT};
use log::{debug, error, info, warn};
use crate::wrappers::instance::VkInstanceRef;

pub struct VkDebugReport {
    debug_report_h: ash::ext::debug_report::Instance,
    debug_report_callback_h: vk::DebugReportCallbackEXT,
    instance: VkInstanceRef
}

unsafe extern "system" fn vulkan_debug_callback(
    flags: DebugReportFlagsEXT,
    object_type: DebugReportObjectTypeEXT,
    object: u64,
    location: usize,
    message_code: i32,
    p_layer_prefix: *const c_char,
    p_message: *const c_char,
    p_user_data: *mut c_void,
) -> vk::Bool32 {
    let msg = unsafe { std::ffi::CStr::from_ptr(p_message) };
    match flags {
        DebugReportFlagsEXT::ERROR => {
            error!("{:?}: {}", object_type, msg.to_str().unwrap());
        },
        DebugReportFlagsEXT::INFORMATION => {
            info!("{:?}: {}", object_type, msg.to_str().unwrap());
        },
        DebugReportFlagsEXT::WARNING | DebugReportFlagsEXT::PERFORMANCE_WARNING => {
            warn!("{:?}: {}", object_type, msg.to_str().unwrap());
        },
            DebugReportFlagsEXT::DEBUG => {
            debug!("{:?}: {}", object_type, msg.to_str().unwrap());
        },
        _ => {}
    }
    vk::FALSE
}

impl VkDebugReport {
    /// Can be used AFTER instance is created
    pub fn new(entry: &Entry, instance: VkInstanceRef) -> anyhow::Result<VkDebugReport> {
        let debug_report_h = ash::ext::debug_report::Instance::new(entry, &instance);

        let debug_report_callback_h = unsafe {
            debug_report_h.create_debug_report_callback(
                &Self::get_messenger_create_info(), None) }?;


        Ok(VkDebugReport {
            debug_report_callback_h,
            debug_report_h,
            instance
        })
    }

    /// Can be used during instance creation
    pub fn get_messenger_create_info() -> DebugReportCallbackCreateInfoEXT<'static> {
        let debug_messenger_create_info = vk::DebugReportCallbackCreateInfoEXT::default()
            .pfn_callback(Some(vulkan_debug_callback));
        debug_messenger_create_info
    }
}

impl Drop for VkDebugReport {
    fn drop(&mut self) {
        unsafe { self.debug_report_h.destroy_debug_report_callback(self.debug_report_callback_h, None) };
    }
}
