use ash::vk::Sampler;
use slotmap::DefaultKey;
use crate::runtime::shared::SharedState;
use crate::wrappers::device::VkDeviceRef;

pub struct SamplerResource {
    shared: SharedState,
    sampler: Sampler,
}

impl SamplerResource {
    pub fn handle(&self) -> SamplerHandle {
        SamplerHandle(self.sampler)
    }
}

#[derive(Copy, Clone, PartialEq)]
pub struct SamplerHandle(pub(crate) Sampler);

impl Drop for SamplerResource {
    fn drop(&mut self) {
        self.shared.schedule_destroy_sampler(SamplerHandle(self.sampler));
    }
}

pub(crate) fn create_sampler(
    device: &VkDeviceRef,
    shared: SharedState,
    create_info: &ash::vk::SamplerCreateInfo,
) -> SamplerResource {
    let sampler = unsafe {
        device
            .create_sampler(create_info, None)
            .expect("Failed to create sampler")
    };

    SamplerResource { shared, sampler }
}