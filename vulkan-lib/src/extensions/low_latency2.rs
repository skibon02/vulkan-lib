use ash::{Instance, vk};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflexMode {
    Off,
    On,
    Boost,
}

pub struct LowLatency2 {
    device: ash::nv::low_latency2::Device,
}

impl LowLatency2 {
    pub fn new(instance: &Instance, device: &ash::Device) -> Self {
        let device = ash::nv::low_latency2::Device::new(instance, device);
        Self { device }
    }

    /// Toggle NVIDIA Reflex (low-latency mode) and clock boost on the given swapchain.
    /// Safe to call any time after swapchain creation; takes effect on subsequent frames.
    /// Requires the swapchain to have been created with `latencyModeEnable = VK_TRUE`.
    pub fn set_mode(&self, swapchain: vk::SwapchainKHR, mode: ReflexMode) {
        let info = vk::LatencySleepModeInfoNV::default()
            .low_latency_mode(matches!(mode, ReflexMode::On | ReflexMode::Boost))
            .low_latency_boost(matches!(mode, ReflexMode::Boost))
            .minimum_interval_us(0);
        unsafe {
            let _ = self.device.set_latency_sleep_mode(swapchain, Some(&info));
        }
    }
}
