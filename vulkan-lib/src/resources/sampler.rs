use std::sync::atomic::{AtomicBool, Ordering};
use ash::vk;
use log::{error, warn};
use crate::try_get_instance;
use crate::queue::OptionSeqNumShared;
use crate::wrappers::device::VkDeviceRef;

pub struct SamplerResource {
    pub(crate) sampler: vk::Sampler,
    pub(crate) submission_usage: OptionSeqNumShared,

    dropped: AtomicBool,
}

impl SamplerResource {
    pub(crate) fn new(
        device: &VkDeviceRef,
        create_info: &ash::vk::SamplerCreateInfo,
    ) -> Self {
        let sampler = unsafe {
            device
                .create_sampler(create_info, None)
                .expect("Failed to create sampler")
        };

        Self {
            sampler,
            submission_usage: OptionSeqNumShared::default(),

            dropped: AtomicBool::new(false),
        }
    }
}

impl Drop for SamplerResource {
    fn drop(&mut self) {
        if !self.dropped.load(Ordering::Relaxed) {
            destroy_sampler(self, false);
        }
    }
}

pub(crate) fn destroy_sampler(sampler: &SamplerResource, no_usages: bool) {
    if !sampler.dropped.swap(true, Ordering::Relaxed) {
        if let Some(instance) = try_get_instance() {
            if !no_usages {
                let last_host_waited = instance.shared_state.last_host_waited_cached().num();
                if sampler.submission_usage.load().is_some_and(|u| u > last_host_waited) {
                    warn!("Trying to destroy sampler resource, but VulkanAllocator was destroyed earlier! Calling device_wait_idle...");
                    unsafe {
                        instance.device.device_wait_idle().unwrap();
                    }
                }
            }
            let device = instance.device.clone();
            unsafe {
                device.destroy_sampler(sampler.sampler, None);
            }
        }
        else {
            error!("VulkanInstance was destroyed! Cannot destroy sampler resource");
        }
    }
}
