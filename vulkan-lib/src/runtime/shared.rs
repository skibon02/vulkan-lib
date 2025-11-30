use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use ash::vk::{self, FenceCreateInfo, Framebuffer};
use log::{info, warn};
use parking_lot::Mutex;
use sparkles::range_event_start;
use crate::runtime::resources::buffers::BufferResourceDestroyHandle;
use crate::runtime::resources::descriptor_sets::{DescriptorSetDestroyHandle, DescriptorSetHandle};
use crate::runtime::resources::images::ImageResourceHandle;
use crate::runtime::resources::pipeline::{GraphicsPipelineDestroyHandle, GraphicsPipelineHandle};
use crate::runtime::resources::render_pass::RenderPassHandle;
use crate::runtime::resources::sampler::SamplerHandle;
use crate::wrappers::device::VkDeviceRef;

#[derive(Default)]
pub struct ScheduledForDestroy {
    pub buffers: Vec<(BufferResourceDestroyHandle, usize)>,
    pub images: Vec<(ImageResourceHandle, usize)>,
    pub pipelines: Vec<(GraphicsPipelineDestroyHandle, usize)>,
    pub render_passes: Vec<(RenderPassHandle, usize)>,
    pub framebuffers: Vec<(Framebuffer, usize)>,
    pub descriptor_sets: Vec<(DescriptorSetDestroyHandle, usize)>,
    pub samplers: Vec<(SamplerHandle, usize)>,
}

impl ScheduledForDestroy {
    pub fn take_ready_for_destroy(&mut self, last_waited_submission: usize) -> Self {
        let mut result = Self::default();

        // Use swap_remove for efficiency since order doesn't matter
        let mut i = 0;
        while i < self.buffers.len() {
            if self.buffers[i].1 <= last_waited_submission {
                result.buffers.push(self.buffers.swap_remove(i));
            } else {
                i += 1;
            }
        }

        i = 0;
        while i < self.images.len() {
            if self.images[i].1 <= last_waited_submission {
                result.images.push(self.images.swap_remove(i));
            } else {
                i += 1;
            }
        }

        i = 0;
        while i < self.pipelines.len() {
            if self.pipelines[i].1 <= last_waited_submission {
                result.pipelines.push(self.pipelines.swap_remove(i));
            } else {
                i += 1;
            }
        }

        i = 0;
        while i < self.render_passes.len() {
            if self.render_passes[i].1 <= last_waited_submission {
                result.render_passes.push(self.render_passes.swap_remove(i));
            } else {
                i += 1;
            }
        }

        i = 0;
        while i < self.framebuffers.len() {
            if self.framebuffers[i].1 <= last_waited_submission {
                result.framebuffers.push(self.framebuffers.swap_remove(i));
            } else {
                i += 1;
            }
        }

        i = 0;
        while i < self.descriptor_sets.len() {
            if self.descriptor_sets[i].1 <= last_waited_submission {
                result.descriptor_sets.push(self.descriptor_sets.swap_remove(i));
            } else {
                i += 1;
            }
        }
        
        i = 0;
        while i < self.samplers.len() {
            if self.samplers[i].1 <= last_waited_submission {
                result.samplers.push(self.samplers.swap_remove(i));
            } else {
                i += 1;
            }
        }

        if !result.framebuffers.is_empty() {
            info!("Destroying {} framebuffers", result.framebuffers.len());
        }
        if !result.render_passes.is_empty() {
            info!("Destroying {} render passes", result.render_passes.len());
        }
        if !result.pipelines.is_empty() {
            info!("Destroying {} pipelines", result.pipelines.len());
        }
        if !result.images.is_empty() {
            info!("Destroying {} images", result.images.len());
        }
        if !result.buffers.is_empty() {
            info!("Destroying {} buffers", result.buffers.len());
        }
        if !result.descriptor_sets.is_empty() {
            info!("Recycling {} descriptor sets", result.descriptor_sets.len());
        }
        if !result.samplers.is_empty() {
            info!("Destroying {} samplers", result.samplers.len());
        }

        result
    }

    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty() &&
        self.images.is_empty() &&
        self.pipelines.is_empty() &&
        self.render_passes.is_empty() &&
        self.framebuffers.is_empty() &&
        self.descriptor_sets.is_empty() &&
        self.samplers.is_empty()
    }
}

struct SharedStateInner {
    device: VkDeviceRef,
    host_waited_submission: usize,
    active_fences: Vec<(usize, vk::Fence)>,
    free_fences: Vec<vk::Fence>,

    scheduled_for_destroy: ScheduledForDestroy,
    last_submission_num: Arc<AtomicUsize>,
}
impl SharedStateInner {
    fn new(device: VkDeviceRef, last_submission_num: Arc<AtomicUsize>) -> Self {
        Self {
            host_waited_submission: 0,
            active_fences: Vec::new(),
            free_fences: Vec::new(),
            device,

            scheduled_for_destroy: ScheduledForDestroy::default(),
            last_submission_num,
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

    pub fn schedule_destroy_buffer(&mut self, handle: BufferResourceDestroyHandle, submission_num: usize) {
        self.scheduled_for_destroy.buffers.push((handle, submission_num));
    }

    pub fn schedule_destroy_image(&mut self, handle: ImageResourceHandle, submission_num: usize) {
        self.scheduled_for_destroy.images.push((handle, submission_num));
    }

    pub fn schedule_destroy_pipeline(&mut self, handle: GraphicsPipelineHandle, submission_num: usize) {
        unsafe {
            // we can destroy these immediately
            self.device.destroy_pipeline_cache(handle.pipeline_cache, None);
        }
    self.scheduled_for_destroy.pipelines.push((handle.into(), submission_num));
    }

    pub fn schedule_destroy_render_pass(&mut self, handle: RenderPassHandle, submission_num: usize) {
        self.scheduled_for_destroy.render_passes.push((handle, submission_num));
    }

    pub fn schedule_destroy_framebuffer(&mut self, framebuffer: Framebuffer, submission_num: usize) {
        self.scheduled_for_destroy.framebuffers.push((framebuffer, submission_num));
    }
    
    pub fn schedule_destroy_sampler(&mut self, sampler: SamplerHandle, submission_num: usize) {
        self.scheduled_for_destroy.samplers.push((sampler, submission_num));
    }

    pub fn schedule_recycle_descriptor_set(&mut self, handle: DescriptorSetDestroyHandle, submission_num: usize) {
        self.scheduled_for_destroy.descriptor_sets.push((handle, submission_num));
    }

    pub fn take_ready_for_destroy(&mut self) -> ScheduledForDestroy {
        self.scheduled_for_destroy.take_ready_for_destroy(self.host_waited_submission)
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
    last_submission_num: Arc<AtomicUsize>,
}

impl SharedState {
    pub fn new(device: VkDeviceRef) -> Self {
        let last_submission_num = Arc::new(AtomicUsize::new(0));
        Self {
            device: device.clone(),
            state: Arc::new(Mutex::new(SharedStateInner::new(device, last_submission_num.clone()))),
            last_submission_num,
        }
    }

    pub fn last_submission_num(&self) -> usize {
        self.last_submission_num.load(Ordering::Relaxed)
    }

    pub(crate) fn increment_and_get_submission_num(&self) -> usize {
        self.last_submission_num.fetch_add(1, Ordering::Relaxed) + 1
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
        let g = range_event_start!("[Vulkan] Wait for fence");
        let fence_to_wait = self.state.lock().take_fence_to_wait(submission_num);
        if let Some((num, fence)) = fence_to_wait {
            let g = range_event_start!("Actual wait");
            unsafe {
                self.device.wait_for_fences(&[fence], true, u64::MAX).unwrap();
            }
            drop(g);
            let mut guard = self.state.lock();
            guard.confirm_wait_fence(num);
            guard.return_free_fence(fence);
        }
    }

    pub fn confirm_all_waited(&self, submission_num: usize) {
        self.state.lock().confirm_wait_fence(submission_num);
    }

    pub fn schedule_destroy_buffer(&self, handle: BufferResourceDestroyHandle) {
        // Use host_used_in from handle if available, otherwise use last submission
        let submission_num = handle.host_used_in.unwrap_or_else(|| self.last_submission_num());
        self.state.lock().schedule_destroy_buffer(handle, submission_num);
    }

    pub fn schedule_destroy_image(&self, handle: ImageResourceHandle) {
        let submission_num = self.last_submission_num();
        self.state.lock().schedule_destroy_image(handle, submission_num);
    }

    pub fn schedule_destroy_pipeline(&self, handle: GraphicsPipelineHandle) {
        let submission_num = self.last_submission_num();
        self.state.lock().schedule_destroy_pipeline(handle, submission_num);
    }

    pub fn schedule_destroy_render_pass(&self, handle: RenderPassHandle) {
        let submission_num = self.last_submission_num();
        self.state.lock().schedule_destroy_render_pass(handle, submission_num);
    }

    pub fn schedule_destroy_framebuffer(&self, framebuffer: Framebuffer, submission_num: usize) {
        self.state.lock().schedule_destroy_framebuffer(framebuffer, submission_num);
    }

    pub fn schedule_recycle_descriptor_set(&self, handle: DescriptorSetDestroyHandle) {
        let submission_num = self.last_submission_num();
        self.state.lock().schedule_recycle_descriptor_set(handle, submission_num);
    }

    pub fn schedule_destroy_sampler(&self, handle: SamplerHandle) {
        let submission_num = self.last_submission_num();
        self.state.lock().schedule_destroy_sampler(handle, submission_num);
    }

    pub fn poll_completed_fences(&self) {
        self.state.lock().poll_completed_fences();
    }

    pub fn take_ready_for_destroy(&self) -> ScheduledForDestroy {
        self.state.lock().take_ready_for_destroy()
    }

    pub fn device(&mut self) -> VkDeviceRef {
        self.state.lock().device.clone()
    }
}