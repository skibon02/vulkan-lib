use std::sync::Arc;

use ash::vk::{self, FenceCreateInfo};
use log::warn;
use parking_lot::Mutex;
use crate::runtime::resources::buffers::BufferResourceDestroyHandle;
use crate::runtime::resources::images::ImageResourceHandle;
use crate::runtime::resources::pipeline::GraphicsPipelineHandle;
use crate::runtime::resources::render_pass::RenderPassHandle;
use crate::wrappers::device::VkDeviceRef;

struct SharedStateInner {
    device: VkDeviceRef,
    host_waited_submission: usize,
    active_fences: Vec<(usize, vk::Fence)>,
    free_fences: Vec<vk::Fence>,

    scheduled_for_destroy_buffers: Vec<BufferResourceDestroyHandle>,
    scheduled_for_destroy_images: Vec<ImageResourceHandle>,
    scheduled_for_destroy_pipelines: Vec<GraphicsPipelineHandle>,
    scheduled_for_destroy_render_passes: Vec<RenderPassHandle>,
}
impl SharedStateInner {
    fn new(device: VkDeviceRef) -> Self {
        Self {
            host_waited_submission: 0,
            active_fences: Vec::new(),
            free_fences: Vec::new(),
            device,

            scheduled_for_destroy_buffers: Vec::new(),
            scheduled_for_destroy_images: Vec::new(),
            scheduled_for_destroy_pipelines: Vec::new(),
            scheduled_for_destroy_render_passes: Vec::new(),
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

    pub fn take_fence_to_wait(&mut self, submission_num: usize) -> Option<(usize, vk::Fence)> {
        if self.host_waited_submission >= submission_num {
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
                    submission_num, self.host_waited_submission);
                None
            }
        }
    }
    pub fn confirm_wait_fence(&mut self, submission_num: usize) {
        self.host_waited_submission = submission_num;

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

    pub fn schedule_destroy_buffer(&mut self, handle: BufferResourceDestroyHandle) {
        self.scheduled_for_destroy_buffers.push(handle);
    }

    pub fn schedule_destroy_image(&mut self, handle: ImageResourceHandle) {
        self.scheduled_for_destroy_images.push(handle);
    }
    
    pub fn schedule_destroy_pipeline(&mut self, handle: GraphicsPipelineHandle) {
        self.scheduled_for_destroy_pipelines.push(handle);
    }
    pub fn schedule_destroy_render_pass(&mut self, handle: RenderPassHandle) {
        self.scheduled_for_destroy_render_passes.push(handle);
    }

    /// Check fences from oldest to newest, updating host_waited_submission
    /// without blocking. Stops at first unsignaled fence.
    pub fn poll_completed_fences(&mut self) {
        if self.active_fences.is_empty() {
            return;
        }

        // sort by submission number to check oldest first
        self.active_fences.sort_by_key(|(num, _)| *num);

        let mut last_signaled_submission = self.host_waited_submission;
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
            self.host_waited_submission = last_signaled_submission;

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
}

impl SharedState {
    pub fn new(device: VkDeviceRef) -> Self {
        Self {
            device: device.clone(),
            state: Arc::new(Mutex::new(SharedStateInner::new(device))),
        }
    }

    pub fn last_host_waited_submission(&self) -> usize {
        self.state.lock().host_waited_submission
    }


    pub fn take_free_fence(&self) -> vk::Fence {
        self.state.lock().take_free_fence()
    }

    pub fn submitted_fence(&self, submission_num: usize, fence: vk::Fence) {
        self.state.lock().submitted_fence(submission_num, fence);
    }

    pub(crate) fn wait_submission(&self, submission_num: usize) {
        let fence_to_wait = self.state.lock().take_fence_to_wait(submission_num);
        if let Some((num, fence)) = fence_to_wait {
            unsafe {
                self.device.wait_for_fences(&[fence], true, u64::MAX).unwrap();
            }
            let mut guard = self.state.lock();
            guard.confirm_wait_fence(num);
            guard.return_free_fence(fence);
        }
    }

    pub fn confirm_all_waited(&self, submission_num: usize) {
        self.state.lock().confirm_wait_fence(submission_num);
    }

    pub fn schedule_destroy_buffer(&self, handle: BufferResourceDestroyHandle) {
        self.state.lock().schedule_destroy_buffer(handle);
    }
    pub fn schedule_destroy_image(&self, handle: ImageResourceHandle) {
        self.state.lock().schedule_destroy_image(handle);
    }
    pub fn schedule_destroy_pipeline(&self, handle: GraphicsPipelineHandle) {
        self.state.lock().schedule_destroy_pipeline(handle);
    }
    pub fn schedule_destroy_render_pass(&self, handle: RenderPassHandle) {
        self.state.lock().schedule_destroy_render_pass(handle);
    }

    pub fn poll_completed_fences(&self) {
        self.state.lock().poll_completed_fences();
    }
    pub fn device(&mut self) -> VkDeviceRef {
        self.state.lock().device.clone()
    }
}