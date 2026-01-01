use ash::vk;
use log::error;
use crate::queue::OptionSeqNumShared;
use crate::wrappers::device::VkDeviceRef;

pub struct SamplerResource {
    pub(crate) sampler: vk::Sampler,
    pub(crate) submission_usage: OptionSeqNumShared,

    dropped: bool,
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
            
            dropped: false,
        }
    }
}

impl Drop for SamplerResource {
    fn drop(&mut self) {
        if !self.dropped {
            error!("SamplerResource was not destroyed before dropping!");
        }
    }
}

pub(crate) fn destroy_sampler(device: &ash::Device, mut sampler: SamplerResource) {
    if !sampler.dropped {
        unsafe {
            device.destroy_sampler(sampler.sampler, None);
        }
        sampler.dropped = true;
    }
}
