use ash::{Entry, Instance};
use ash::vk::{CalibratedTimestampInfoEXT, PhysicalDevice, TimeDomainEXT};
use log::info;

pub struct CalibratedTimestamps {
    instance: ash::ext::calibrated_timestamps::Instance,
    device: ash::ext::calibrated_timestamps::Device,

    time_domains: Vec<TimeDomainEXT>,
}

impl CalibratedTimestamps {
    pub fn new(instance: &Instance, physical_device: PhysicalDevice, device: &ash::Device) -> Self {
        let entry = Entry::linked();
        let device = ash::ext::calibrated_timestamps::Device::new(instance, device);
        let instance = ash::ext::calibrated_timestamps::Instance::new(&entry, instance);
        unsafe {
            let time_domains = instance.get_physical_device_calibrateable_time_domains(physical_device).unwrap();
            info!("Calibrated timestamps time domains: {:?}", time_domains);
            Self {
                instance,
                device,
                time_domains
            }
        }
    }

    pub fn get_timestamps(&self) -> (Vec<(TimeDomainEXT, u64)>, u64) {
        let mut res = Vec::new();

        let calibrated_timestamps_info: Vec<_> = self.time_domains.iter().map(|d| {
            CalibratedTimestampInfoEXT {
                time_domain: *d,
                ..Default::default()
            }
        }).collect();
        unsafe {
            let (timestamps, max_deviation) = self.device.get_calibrated_timestamps(&calibrated_timestamps_info).unwrap();
            for (tm, domain) in timestamps.into_iter().zip(self.time_domains.iter()) {
                res.push((*domain, tm));
            }
            (res, max_deviation)
        }
    }

    pub fn get_timestamps_pair(&self) -> Option<(u64, u64)> {
        let (tms, max_dev) = self.get_timestamps();
        let mut gpu_tm = None;
        let mut host_tm = None;
        for (source, tm) in tms {
            if source == TimeDomainEXT::CLOCK_MONOTONIC {
                host_tm = Some(tm);
            }
            else if source == TimeDomainEXT::DEVICE {
                gpu_tm = Some(tm);
            }
        }
        if let (Some(gpu_tm), Some(host_tm)) = (gpu_tm, host_tm) {
            Some((gpu_tm, host_tm))
        }
        else {
            None
        }
    }
}
