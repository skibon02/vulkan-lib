use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use ash::vk::{self, FenceCreateInfo, Framebuffer};
use log::{info, warn};
use parking_lot::Mutex;
use sparkles::range_event_start;
use crate::wrappers::device::VkDeviceRef;

#[derive(Copy, Clone)]
pub struct HostWaitedNum(usize);

impl HostWaitedNum {
    pub fn num(&self) -> usize {
        self.0
    }
}

struct SharedStateInner {
    device: VkDeviceRef,
    active_fences: Vec<(usize, vk::Fence)>,
    free_fences: Vec<vk::Fence>,
}
impl SharedStateInner {
    fn new(device: VkDeviceRef) -> Self {
        Self {
            active_fences: Vec::new(),
            free_fences: Vec::new(),
            device,
        }
    }
}

impl SharedStateInner {
    pub fn take_free_fence(&mut self) -> vk::Fence {
        self.free_fences.pop().unwrap_or_else(|| {
            unsafe { self.device.create_fence(&FenceCreateInfo::default(), None).unwrap() }
        })
    }
    pub fn submitted_fence(&mut self, submission_num: usize, fence: vk::Fence) {
        self.active_fences.push((submission_num, fence));
    }

    pub fn return_free_fence(&mut self, fence: vk::Fence) {
        self.free_fences.push(fence);
    }

    pub fn take_fence_to_wait(&mut self, submission_num: usize, host_waited: &AtomicUsize) -> Option<(usize, vk::Fence)> {
        if host_waited.load(Ordering::Relaxed) >= submission_num {
            return None;
        }

        if let Some(i) = self.active_fences.iter().position(|(n, _) | *n == submission_num) {
            let (num, f) = self.active_fences.swap_remove(i);
            Some((num, f))
        }
        else {
            // try find anything bigger
            let mut best_fence_index = None;
            let mut min_available_submission = usize::MAX;

            for (i, (num, _)) in self.active_fences.iter().enumerate() {
                if *num > submission_num {
                    if *num < min_available_submission {
                        min_available_submission = *num;
                        best_fence_index = Some(i);
                    }
                }
            }

            if let Some(i) = best_fence_index {
                let (num, fence) = self.active_fences.swap_remove(i);
                Some((num, fence))
            } else {
                warn!("Unexpected situation! Cannot find fence to wait on host for submission {} (host waited for {})",
                    submission_num, host_waited.load(Ordering::Relaxed));
                None
            }
        }
    }
    pub fn confirm_wait_fence(&mut self, submission_num: usize, host_waited: &AtomicUsize) {
        host_waited.store(submission_num, Ordering::Release);
        if cfg!(feature="recording-logs") {
            info!("Host waited for submission {}", submission_num);
        }

        let mut i = 0;
        while i < self.active_fences.len() {
            if self.active_fences[i].0 <= submission_num {
                let (_, fence) = self.active_fences.swap_remove(i);
                self.free_fences.push(fence);
            } else {
                i += 1;
            }
        }
    }


    /// Check fences from oldest to newest, updating host_waited_submission
    /// without blocking. Stops at first unsignaled fence.
    pub fn poll_completed_fences(&mut self, host_waited: &AtomicUsize) {
        if self.active_fences.is_empty() {
            return;
        }

        // sort by submission number to check oldest first
        self.active_fences.sort_by_key(|(num, _)| *num);

        let mut last_signaled_submission = host_waited.load(Ordering::Relaxed);
        let mut completed_count = 0;

        for i in 0..self.active_fences.len() {
            let (num, fence) = self.active_fences[i];

            // check fence status without blocking (timeout = 0)
            let status = unsafe {
                self.device.wait_for_fences(&[fence], true, 0)
            };

            match status {
                Ok(_) => {
                    // fence is signaled
                    last_signaled_submission = num;
                    completed_count += 1;
                }
                Err(_) => {
                    // fence not signaled yet, stop checking
                    break;
                }
            }
        }

        if completed_count > 0 {
            // update host_waited_submission
            host_waited.store(last_signaled_submission, Ordering::Release);

            // remove and recycle completed fences
            let completed_fences: Vec<_> = self.active_fences.drain(0..completed_count).collect();
            for (_, fence) in completed_fences {
                self.free_fences.push(fence);
            }
        }
    }
}

impl Drop for SharedStateInner {
    fn drop(&mut self) {
        unsafe {
            for fence in self.free_fences.drain(..) {
                self.device.destroy_fence(fence, None);
            }

            for (_, fence) in self.active_fences.drain(..) {
                self.device.destroy_fence(fence, None);
            }
        }
    }
}

#[derive(Clone)]
pub struct SharedState {
    device: VkDeviceRef,
    state: Arc<Mutex<SharedStateInner>>,
    last_submission_num: Arc<AtomicUsize>,
    last_host_waited_submission: Arc<AtomicUsize>,
}

impl SharedState {
    pub fn new(device: VkDeviceRef) -> Self {
        let last_submission_num = Arc::new(AtomicUsize::new(0));
        let last_host_waited_submission = Arc::new(AtomicUsize::new(0));
        Self {
            device: device.clone(),
            state: Arc::new(Mutex::new(SharedStateInner::new(device))),
            last_submission_num,
            last_host_waited_submission,
        }
    }

    /// Submission number of last recorded and submitted commands
    pub fn last_submission_num(&self) -> usize {
        self.last_submission_num.load(Ordering::Relaxed)
    }

    pub(crate) fn increment_and_get_submission_num(&self) -> usize {
        self.last_submission_num.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Returns cached value of last host-waited submission without polling fences
    pub fn last_host_waited_cached(&self) -> HostWaitedNum {
        HostWaitedNum(self.last_host_waited_submission.load(Ordering::Acquire))
    }

    /// Polls fences to update and return the actual last host-waited submission
    pub fn last_host_waited_submission(&self) -> HostWaitedNum {
        self.poll_completed_fences();
        HostWaitedNum(self.last_host_waited_submission.load(Ordering::Acquire))
    }


    pub(crate) fn take_free_fence(&self) -> vk::Fence {
        self.state.lock().take_free_fence()
    }

    pub(crate) fn submitted_fence(&self, submission_num: usize, fence: vk::Fence) {
        self.state.lock().submitted_fence(submission_num, fence);
    }

    pub fn wait_submission(&self, submission_num: usize) -> HostWaitedNum {
        let last_submitted_num = self.last_submission_num.load(Ordering::Relaxed);
        let submission_num = submission_num.min(last_submitted_num);
        let g = range_event_start!("[Vulkan] Wait for fence");
        let fence_to_wait = self.state.lock().take_fence_to_wait(submission_num, &self.last_host_waited_submission);
        if let Some((num, fence)) = fence_to_wait {
            let g = range_event_start!("Actual wait");
            unsafe {
                self.device.wait_for_fences(&[fence], true, u64::MAX).unwrap();
            }
            drop(g);
            let mut guard = self.state.lock();
            guard.confirm_wait_fence(num, &self.last_host_waited_submission);
            guard.return_free_fence(fence);
        }

        HostWaitedNum(submission_num)
    }

    pub(crate) fn confirm_all_waited(&self, submission_num: usize) {
        self.state.lock().confirm_wait_fence(submission_num, &self.last_host_waited_submission);
    }

    pub(crate) fn poll_completed_fences(&self) {
        self.state.lock().poll_completed_fences(&self.last_host_waited_submission);
    }

    pub fn device(&mut self) -> VkDeviceRef {
        self.state.lock().device.clone()
    }
}