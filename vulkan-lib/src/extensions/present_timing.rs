//! Minimal raw-FFI wrapper for VK_EXT_present_timing (registry extension #209,
//! spec v3). Collects the single useful metric NV's preview driver delivers:
//!
//!   q->o = IMAGE_FIRST_PIXEL_OUT − QUEUE_OPERATIONS_END
//!
//! i.e. the time between "driver finished queue operations after vkQueuePresentKHR"
//! and "first pixel of this frame leaves the presentation engine for the display".
//! Everything else (dequeue / first_pixel_visible) is not delivered by current NV
//! drivers, and pure intervals are already covered by VK_NV_low_latency2 host-side
//! markers, so we don't bother accumulating them.
//!
//! The two stage clocks are independent monotonic timelines on NV (queue_op ≈
//! UNIX-epoch ns, pixel_out ≈ since-boot ns), so we calibrate them once via
//! vkGetCalibratedTimestampsKHR + VkSwapchainCalibratedTimestampInfoEXT.

use std::ffi::{c_void, CStr};
use std::ptr;
use std::time::{Duration, Instant};

use ash::vk;
use ash::{Device, Instance};
use log::info;

// === Registry constants (extension #209, base 1000208000) =========================
pub const STRUCTURE_TYPE_PHYSICAL_DEVICE_PRESENT_TIMING_FEATURES_EXT: i32 = 1000208000;
const STRUCTURE_TYPE_SWAPCHAIN_TIME_DOMAIN_PROPERTIES_EXT: i32 = 1000208002;
const STRUCTURE_TYPE_PRESENT_TIMINGS_INFO_EXT: i32 = 1000208003;
const STRUCTURE_TYPE_PRESENT_TIMING_INFO_EXT: i32 = 1000208004;
const STRUCTURE_TYPE_PAST_PRESENTATION_TIMING_INFO_EXT: i32 = 1000208005;
const STRUCTURE_TYPE_PAST_PRESENTATION_TIMING_PROPERTIES_EXT: i32 = 1000208006;
const STRUCTURE_TYPE_PAST_PRESENTATION_TIMING_EXT: i32 = 1000208007;
const STRUCTURE_TYPE_SWAPCHAIN_CALIBRATED_TIMESTAMP_INFO_EXT: i32 = 1000208009;
/// VK_EXT_calibrated_timestamps' VkCalibratedTimestampInfoEXT.
const STRUCTURE_TYPE_CALIBRATED_TIMESTAMP_INFO_EXT: i32 = 1000184000;

const TIME_DOMAIN_PRESENT_STAGE_LOCAL_EXT: i32 = 1000208000;
const TIME_DOMAIN_SWAPCHAIN_LOCAL_EXT: i32 = 1000208001;

/// Bit added to VkSwapchainCreateFlagsKHR to enable timing collection.
pub const SWAPCHAIN_CREATE_PRESENT_TIMING_BIT_EXT: u32 = 1 << 9;

// VkPresentStageFlagBitsEXT — only the two NV's driver actually populates.
const PRESENT_STAGE_QUEUE_OPERATIONS_END: u32 = 1 << 0;
const PRESENT_STAGE_IMAGE_FIRST_PIXEL_OUT: u32 = 1 << 2;
/// What we ask for in VkPresentTimingInfoEXT::presentStageQueries.
const REQUESTED_STAGES: u32 = PRESENT_STAGE_QUEUE_OPERATIONS_END | PRESENT_STAGE_IMAGE_FIRST_PIXEL_OUT;

const PAST_PRESENTATION_TIMING_ALLOW_PARTIAL_RESULTS_BIT_EXT: u32 = 1 << 0;

// === Raw FFI structs ==============================================================

/// Public so the device-creation code can chain it into VkDeviceCreateInfo::pNext.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PhysicalDevicePresentTimingFeaturesEXT {
    pub s_type: vk::StructureType,
    pub p_next: *mut c_void,
    pub present_timing: vk::Bool32,
    pub present_at_absolute_time: vk::Bool32,
    pub present_at_relative_time: vk::Bool32,
}

impl PhysicalDevicePresentTimingFeaturesEXT {
    pub fn enabled() -> Self {
        Self {
            s_type: vk::StructureType::from_raw(
                STRUCTURE_TYPE_PHYSICAL_DEVICE_PRESENT_TIMING_FEATURES_EXT,
            ),
            p_next: ptr::null_mut(),
            present_timing: vk::TRUE,
            present_at_absolute_time: vk::FALSE,
            present_at_relative_time: vk::FALSE,
        }
    }
}

// Allows this hand-rolled feature struct to be passed to `DeviceCreateInfo::push_next`.
// Layout matches the Vulkan contract (sType + pNext header), so chain insertion is sound.
unsafe impl vk::ExtendsDeviceCreateInfo for PhysicalDevicePresentTimingFeaturesEXT {}

#[repr(C)]
#[derive(Clone, Copy)]
struct SwapchainCalibratedTimestampInfoEXT {
    s_type: vk::StructureType,
    p_next: *const c_void,
    swapchain: vk::SwapchainKHR,
    present_stage: u32,
    time_domain_id: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CalibratedTimestampInfoEXT {
    s_type: vk::StructureType,
    p_next: *const c_void,
    time_domain: vk::TimeDomainEXT,
}

#[repr(C)]
struct SwapchainTimeDomainPropertiesEXT {
    s_type: vk::StructureType,
    p_next: *mut c_void,
    time_domain_count: u32,
    p_time_domains: *mut vk::TimeDomainKHR,
    p_time_domain_ids: *mut u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PastPresentationTimingInfoEXT {
    s_type: vk::StructureType,
    p_next: *const c_void,
    flags: u32,
    swapchain: vk::SwapchainKHR,
}

#[repr(C)]
struct PastPresentationTimingPropertiesEXT {
    s_type: vk::StructureType,
    p_next: *mut c_void,
    timing_properties_counter: u64,
    time_domains_counter: u64,
    presentation_timing_count: u32,
    p_presentation_timings: *mut PastPresentationTimingEXT,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct PresentStageTimeEXT {
    stage: u32,
    time: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PastPresentationTimingEXT {
    s_type: vk::StructureType,
    p_next: *const c_void,
    present_id: u64,
    target_time: u64,
    present_stage_count: u32,
    p_present_stages: *mut PresentStageTimeEXT,
    time_domain: vk::TimeDomainKHR,
    time_domain_id: u64,
    report_complete: vk::Bool32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PresentTimingInfoEXT {
    pub s_type: vk::StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub target_time: u64,
    pub time_domain_id: u64,
    pub present_stage_queries: u32,
    pub target_time_domain_present_stage: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PresentTimingsInfoEXT {
    pub s_type: vk::StructureType,
    pub p_next: *const c_void,
    pub swapchain_count: u32,
    pub p_timing_infos: *const PresentTimingInfoEXT,
}

// === Function-pointer typedefs ====================================================

type PfnSetQueueSize = unsafe extern "system" fn(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    size: u32,
) -> vk::Result;

type PfnGetTimeDomainProps = unsafe extern "system" fn(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    p_props: *mut SwapchainTimeDomainPropertiesEXT,
    p_counter: *mut u64,
) -> vk::Result;

type PfnGetPastTiming = unsafe extern "system" fn(
    device: vk::Device,
    p_info: *const PastPresentationTimingInfoEXT,
    p_props: *mut PastPresentationTimingPropertiesEXT,
) -> vk::Result;

type PfnGetCalibratedTimestamps = unsafe extern "system" fn(
    device: vk::Device,
    timestamp_count: u32,
    p_timestamp_infos: *const CalibratedTimestampInfoEXT,
    p_timestamps: *mut u64,
    p_max_deviation: *mut u64,
) -> vk::Result;

// === Stats ========================================================================

#[derive(Default, Clone, Copy)]
struct LatencyAccum {
    min: u64,
    max: u64,
    sum: u64,
    count: u64,
}

impl LatencyAccum {
    fn new() -> Self { Self { min: u64::MAX, max: 0, sum: 0, count: 0 } }
    fn add(&mut self, v: u64) {
        if v < self.min { self.min = v; }
        if v > self.max { self.max = v; }
        self.sum += v;
        self.count += 1;
    }
    fn fmt(&self) -> String {
        if self.count == 0 {
            "-".to_string()
        } else {
            format!("min={}µs avg={}µs max={}µs", self.min, self.sum / self.count, self.max)
        }
    }
}

// === Wrapper ======================================================================

pub struct PresentTiming {
    set_queue_size: PfnSetQueueSize,
    get_time_domain_props: PfnGetTimeDomainProps,
    get_past_timing: PfnGetPastTiming,
    get_calibrated_timestamps: PfnGetCalibratedTimestamps,
    device: vk::Device,

    /// Selected time domain id (resolved after swapchain creation).
    time_domain_id: Option<u64>,
    /// Calibration anchors — simultaneous readings of the QUEUE_OPS_END and
    /// IMAGE_FIRST_PIXEL_OUT clocks, used to translate raw cross-stage delta into
    /// a real wall-time delta.
    calib_q_t0: Option<u64>,
    calib_o_t0: Option<u64>,

    /// q->o accumulator; logged once per second.
    queue_to_pixel_out: LatencyAccum,
    last_logged: Instant,

    /// Counter incremented on every present; we only chain a timing request once
    /// per `request_every_n` frames. Capturing IMAGE_FIRST_PIXEL_OUT requires the
    /// driver to synchronise with the actual scanout, so requesting on every frame
    /// throttles the present path to display rate (≈2× framerate drop on NV in
    /// IMMEDIATE/MAILBOX). Sparse sampling keeps the fast path on most frames.
    frames_since_request: u32,
    request_every_n: u32,

    /// Stable storage for the per-present chain. Re-used across frames so we don't
    /// heap-allocate. Addresses are stable for as long as `Self` stays put, which is
    /// the lifetime contract `build_present_chain` relies on.
    timing_info: PresentTimingInfoEXT,
    timings_info: PresentTimingsInfoEXT,
}

// SAFETY: the raw pointers inside `timing_info` / `timings_info` are either null
// (`p_next`) or self-referential into the same `PresentTiming`. The cross-pointer
// `timings_info.p_timing_infos` is re-patched on every sample frame inside
// `build_present_chain` before being read, so a thread move between sample frames
// is fine. No interior mutability is exposed across threads (all mutating methods
// take `&mut self`), so `Sync` is intentionally NOT implemented.
unsafe impl Send for PresentTiming {}

impl PresentTiming {
    pub fn new(instance: &Instance, device: &Device) -> Option<Self> {
        unsafe {
            let dev = device.handle();
            let load = |name: &CStr| -> Option<*const c_void> {
                let raw_ptr = (instance.fp_v1_0().get_device_proc_addr)(dev, name.as_ptr());
                raw_ptr.map(|f| f as *const c_void)
            };
            let s = load(c"vkSetSwapchainPresentTimingQueueSizeEXT")?;
            let gd = load(c"vkGetSwapchainTimeDomainPropertiesEXT")?;
            let gt = load(c"vkGetPastPresentationTimingEXT")?;
            let gct = load(c"vkGetCalibratedTimestampsKHR")
                .or_else(|| load(c"vkGetCalibratedTimestampsEXT"))?;
            Some(Self {
                set_queue_size: std::mem::transmute::<*const c_void, PfnSetQueueSize>(s),
                get_time_domain_props: std::mem::transmute::<*const c_void, PfnGetTimeDomainProps>(gd),
                get_past_timing: std::mem::transmute::<*const c_void, PfnGetPastTiming>(gt),
                get_calibrated_timestamps: std::mem::transmute::<*const c_void, PfnGetCalibratedTimestamps>(gct),
                device: dev,
                time_domain_id: None,
                calib_q_t0: None,
                calib_o_t0: None,
                queue_to_pixel_out: LatencyAccum::new(),
                last_logged: Instant::now(),
                frames_since_request: 0,
                request_every_n: 20,
                timing_info: PresentTimingInfoEXT {
                    s_type: vk::StructureType::from_raw(STRUCTURE_TYPE_PRESENT_TIMING_INFO_EXT),
                    p_next: ptr::null(),
                    flags: 0,
                    target_time: 0,
                    time_domain_id: 0,
                    present_stage_queries: REQUESTED_STAGES,
                    target_time_domain_present_stage: 0,
                },
                timings_info: PresentTimingsInfoEXT {
                    s_type: vk::StructureType::from_raw(STRUCTURE_TYPE_PRESENT_TIMINGS_INFO_EXT),
                    p_next: ptr::null(),
                    swapchain_count: 1,
                    p_timing_infos: ptr::null(),
                },
            })
        }
    }

    /// Initialise the implementation's internal results queue and resolve a stable
    /// time domain. Must be called after every swapchain (re)creation.
    pub fn on_swapchain_created(&mut self, swapchain: vk::SwapchainKHR, image_count: u32) {
        // Generously sized: at high uncapped FPS the implementation may take many
        // frames to deliver a result, so each slot holds for tens of milliseconds.
        let queue_size = (image_count * 16).max(64);
        let r = unsafe { (self.set_queue_size)(self.device, swapchain, queue_size) };
        if r != vk::Result::SUCCESS {
            log::warn!("vkSetSwapchainPresentTimingQueueSizeEXT returned {:?}", r);
        }

        self.time_domain_id = None;
        self.calib_q_t0 = None;
        self.calib_o_t0 = None;
        self.queue_to_pixel_out = LatencyAccum::new();
        self.last_logged = Instant::now();
        self.frames_since_request = 0;

        self.resolve_time_domain(swapchain);
        if self.time_domain_id.is_some() {
            self.calibrate_stage_clocks(swapchain);
        }
    }

    /// On every Nth frame, refresh the pre-allocated chain storage and return a
    /// pointer suitable for `VkPresentInfoKHR::pNext`. Returns `None` on skip frames
    /// and before the time domain has been resolved. The returned pointer is valid
    /// until the next call into `&mut self`.
    pub fn build_present_chain(&mut self) -> Option<*const PresentTimingsInfoEXT> {
        let tdid = self.time_domain_id?;
        let sample = self.frames_since_request == 0;
        self.frames_since_request = (self.frames_since_request + 1) % self.request_every_n;
        if !sample { return None; }

        self.timing_info.time_domain_id = tdid;
        self.timings_info.p_timing_infos = &self.timing_info as *const _;
        Some(&self.timings_info as *const _)
    }

    /// Drain pending timing results, accumulate q->o, log once per second.
    pub fn drain_and_log(&mut self, swapchain: vk::SwapchainKHR) {
        const MAX_RESULTS: usize = 16;
        const STAGES_PER_RESULT: usize = 4;

        let mut stages_buf = [PresentStageTimeEXT { stage: 0, time: 0 }; MAX_RESULTS * STAGES_PER_RESULT];
        let mut timings = [PastPresentationTimingEXT {
            s_type: vk::StructureType::from_raw(STRUCTURE_TYPE_PAST_PRESENTATION_TIMING_EXT),
            p_next: ptr::null(),
            present_id: 0,
            target_time: 0,
            present_stage_count: STAGES_PER_RESULT as u32,
            p_present_stages: ptr::null_mut(),
            time_domain: vk::TimeDomainKHR::default(),
            time_domain_id: 0,
            report_complete: 0,
        }; MAX_RESULTS];
        for i in 0..MAX_RESULTS {
            timings[i].p_present_stages = unsafe { stages_buf.as_mut_ptr().add(i * STAGES_PER_RESULT) };
            timings[i].present_stage_count = STAGES_PER_RESULT as u32;
        }

        let info = PastPresentationTimingInfoEXT {
            s_type: vk::StructureType::from_raw(STRUCTURE_TYPE_PAST_PRESENTATION_TIMING_INFO_EXT),
            p_next: ptr::null(),
            flags: PAST_PRESENTATION_TIMING_ALLOW_PARTIAL_RESULTS_BIT_EXT,
            swapchain,
        };
        let mut props = PastPresentationTimingPropertiesEXT {
            s_type: vk::StructureType::from_raw(STRUCTURE_TYPE_PAST_PRESENTATION_TIMING_PROPERTIES_EXT),
            p_next: ptr::null_mut(),
            timing_properties_counter: 0,
            time_domains_counter: 0,
            presentation_timing_count: MAX_RESULTS as u32,
            p_presentation_timings: timings.as_mut_ptr(),
        };

        let r = unsafe { (self.get_past_timing)(self.device, &info, &mut props) };
        if r != vk::Result::SUCCESS && r != vk::Result::INCOMPLETE {
            return;
        }

        let n = props.presentation_timing_count as usize;
        for t in &timings[..n] {
            let stages = unsafe {
                std::slice::from_raw_parts(t.p_present_stages, t.present_stage_count as usize)
            };
            let find = |bit: u32| stages.iter()
                .find(|s| s.stage == bit && s.time != 0)
                .map(|s| s.time);
            let q = find(PRESENT_STAGE_QUEUE_OPERATIONS_END);
            let o = find(PRESENT_STAGE_IMAGE_FIRST_PIXEL_OUT);

            // Both stages present + calibrated → real q->o delta.
            if let (Some(q), Some(o), Some(q0), Some(o0)) =
                (q, o, self.calib_q_t0, self.calib_o_t0)
            {
                let delta_ns = (o as i128 - o0 as i128) - (q as i128 - q0 as i128);
                if (0..1_000_000_000_i128).contains(&delta_ns) {
                    self.queue_to_pixel_out.add((delta_ns as u64) / 1000);
                }
            }
        }

        if self.last_logged.elapsed() >= Duration::from_secs(1) {
            info!("[present_timing 1s] q->o: {}", self.queue_to_pixel_out.fmt());
            self.queue_to_pixel_out = LatencyAccum::new();
            self.last_logged = Instant::now();
        }
    }

    fn calibrate_stage_clocks(&mut self, swapchain: vk::SwapchainKHR) {
        let Some(tdid) = self.time_domain_id else { return; };

        let q_chain = SwapchainCalibratedTimestampInfoEXT {
            s_type: vk::StructureType::from_raw(STRUCTURE_TYPE_SWAPCHAIN_CALIBRATED_TIMESTAMP_INFO_EXT),
            p_next: ptr::null(),
            swapchain,
            present_stage: PRESENT_STAGE_QUEUE_OPERATIONS_END,
            time_domain_id: tdid,
        };
        let o_chain = SwapchainCalibratedTimestampInfoEXT {
            s_type: vk::StructureType::from_raw(STRUCTURE_TYPE_SWAPCHAIN_CALIBRATED_TIMESTAMP_INFO_EXT),
            p_next: ptr::null(),
            swapchain,
            present_stage: PRESENT_STAGE_IMAGE_FIRST_PIXEL_OUT,
            time_domain_id: tdid,
        };
        let infos = [
            CalibratedTimestampInfoEXT {
                s_type: vk::StructureType::from_raw(STRUCTURE_TYPE_CALIBRATED_TIMESTAMP_INFO_EXT),
                p_next: (&q_chain as *const SwapchainCalibratedTimestampInfoEXT).cast(),
                time_domain: vk::TimeDomainEXT::from_raw(TIME_DOMAIN_PRESENT_STAGE_LOCAL_EXT),
            },
            CalibratedTimestampInfoEXT {
                s_type: vk::StructureType::from_raw(STRUCTURE_TYPE_CALIBRATED_TIMESTAMP_INFO_EXT),
                p_next: (&o_chain as *const SwapchainCalibratedTimestampInfoEXT).cast(),
                time_domain: vk::TimeDomainEXT::from_raw(TIME_DOMAIN_PRESENT_STAGE_LOCAL_EXT),
            },
        ];
        let mut timestamps: [u64; 2] = [0, 0];
        let mut max_dev: u64 = 0;
        let r = unsafe {
            (self.get_calibrated_timestamps)(
                self.device,
                infos.len() as u32,
                infos.as_ptr(),
                timestamps.as_mut_ptr(),
                &mut max_dev,
            )
        };
        if r == vk::Result::SUCCESS {
            self.calib_q_t0 = Some(timestamps[0]);
            self.calib_o_t0 = Some(timestamps[1]);
        }
    }

    fn resolve_time_domain(&mut self, swapchain: vk::SwapchainKHR) {
        // Two-call pattern: first NULL pointers to get the count, then with backing storage.
        let mut probe = SwapchainTimeDomainPropertiesEXT {
            s_type: vk::StructureType::from_raw(STRUCTURE_TYPE_SWAPCHAIN_TIME_DOMAIN_PROPERTIES_EXT),
            p_next: ptr::null_mut(),
            time_domain_count: 0,
            p_time_domains: ptr::null_mut(),
            p_time_domain_ids: ptr::null_mut(),
        };
        let r = unsafe { (self.get_time_domain_props)(self.device, swapchain, &mut probe, ptr::null_mut()) };
        if r != vk::Result::SUCCESS && r != vk::Result::INCOMPLETE { return; }
        let count = probe.time_domain_count as usize;
        if count == 0 { return; }

        let mut domains: Vec<vk::TimeDomainKHR> = vec![vk::TimeDomainKHR::default(); count];
        let mut ids: Vec<u64> = vec![0; count];
        let mut props = SwapchainTimeDomainPropertiesEXT {
            s_type: vk::StructureType::from_raw(STRUCTURE_TYPE_SWAPCHAIN_TIME_DOMAIN_PROPERTIES_EXT),
            p_next: ptr::null_mut(),
            time_domain_count: count as u32,
            p_time_domains: domains.as_mut_ptr(),
            p_time_domain_ids: ids.as_mut_ptr(),
        };
        let r = unsafe { (self.get_time_domain_props)(self.device, swapchain, &mut props, ptr::null_mut()) };
        if r != vk::Result::SUCCESS && r != vk::Result::INCOMPLETE { return; }
        let n = props.time_domain_count as usize;

        let swapchain_local = vk::TimeDomainKHR::from_raw(TIME_DOMAIN_SWAPCHAIN_LOCAL_EXT);
        let stage_local = vk::TimeDomainKHR::from_raw(TIME_DOMAIN_PRESENT_STAGE_LOCAL_EXT);
        let pick = (0..n).find(|&i| domains[i] == swapchain_local)
            .or_else(|| (0..n).find(|&i| domains[i] == stage_local));
        if let Some(i) = pick {
            self.time_domain_id = Some(ids[i]);
        }
    }
}
