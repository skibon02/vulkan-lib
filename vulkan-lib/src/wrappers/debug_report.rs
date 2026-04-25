use std::ffi::{c_char, c_void};
use ash::{vk, Entry};
use ash::vk::{DebugReportCallbackCreateInfoEXT, DebugReportFlagsEXT, DebugReportObjectTypeEXT, DebugUtilsMessengerCreateInfoEXT};
use crate::wrappers::instance::VkInstanceRef;

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";          // dimmed body
const RED: &str = "\x1b[1;31m";       // bold red
const YELLOW: &str = "\x1b[1;33m";    // bold yellow
const CYAN: &str = "\x1b[1;36m";      // bold cyan
const GRAY: &str = "\x1b[1;90m";      // bold gray

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
    let msg = unsafe { std::ffi::CStr::from_ptr(p_message) }.to_string_lossy();
    // `flags` is a bitmask; messages may carry multiple bits, so dispatch by
    // priority rather than equality match.
    let (color, tag) = if flags.contains(DebugReportFlagsEXT::ERROR) {
        (RED, "VK ERR ")
    } else if flags.contains(DebugReportFlagsEXT::WARNING)
        || flags.contains(DebugReportFlagsEXT::PERFORMANCE_WARNING) {
        (YELLOW, "VK WARN")
    } else if flags.contains(DebugReportFlagsEXT::INFORMATION) {
        (CYAN, "VK INFO")
    } else if flags.contains(DebugReportFlagsEXT::DEBUG) {
        (GRAY, "VK DBG ")
    } else {
        return vk::FALSE;
    };
    // Coloured tag, dim body. eprintln so we share stderr with env_logger but the
    // distinct format makes it easy to pick out validation output at a glance.
    eprintln!("{color}[{tag}]{RESET} {DIM}{:?}: {msg}{RESET}", object_type);
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
        let mut flags = DebugReportFlagsEXT::ERROR
            | DebugReportFlagsEXT::WARNING
            | DebugReportFlagsEXT::PERFORMANCE_WARNING;
        if cfg!(feature = "validation-verbose") {
            flags |= DebugReportFlagsEXT::INFORMATION | DebugReportFlagsEXT::DEBUG;
        }
        vk::DebugReportCallbackCreateInfoEXT::default()
            .flags(flags)
            .pfn_callback(Some(vulkan_debug_callback))
    }
}

impl Drop for VkDebugReport {
    fn drop(&mut self) {
        unsafe { self.debug_report_h.destroy_debug_report_callback(self.debug_report_callback_h, None) };
    }
}
