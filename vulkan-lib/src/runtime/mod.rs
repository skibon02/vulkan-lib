use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use ash::vk;
use ash::vk::FenceCreateInfo;
use log::warn;
use parking_lot::Mutex;
use crate::runtime::recording::RecordContext;
use crate::runtime::resources::BufferResourceHandle;
use crate::wrappers::device::VkDeviceRef;

pub mod resources;
pub mod recording;

struct SharedStateInner {
    device: VkDeviceRef,
    host_waited_submission: usize,
    active_fences: Vec<(usize, vk::Fence)>,
    free_fences: Vec<vk::Fence>,
}
impl SharedStateInner {
    fn new(device: VkDeviceRef) -> Self {
        Self {
            host_waited_submission: 0,
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

    pub fn wait_submission(&self, submission_num: usize) {
        if let Some((num, fence)) = self.state.lock().take_fence_to_wait(submission_num) {
            unsafe {
                self.device.wait_for_fences(&[fence], true, u64::MAX).unwrap();
            }
            self.state.lock().confirm_wait_fence(num);
            self.state.lock().return_free_fence(fence);
        }
    }

    fn schedule_destroy_buffer(&self, handle: BufferResourceHandle) {

    }
}


pub struct LocalState {
    device: VkDeviceRef,
    shared_state: SharedState,
    free_semaphores: Vec<vk::Semaphore>,
    active_semaphores: Vec<(usize, vk::Semaphore)>,
}

impl LocalState {
    pub fn new(device: VkDeviceRef, shared_state: SharedState) -> Self {
        Self {
            device,
            shared_state,
            free_semaphores: Vec::new(),
            active_semaphores: Vec::new(),
        }
    }
     
    pub fn shared(&self) -> SharedState {
        self.shared_state.clone()
    }

    fn cleanup_old_semaphores(&mut self) {
        let host_waited = self.shared_state.last_host_waited_submission();

        let mut i = 0;
        while i < self.active_semaphores.len() {
            if self.active_semaphores[i].0 <= host_waited {
                let (_, semaphore) = self.active_semaphores.swap_remove(i);
                self.free_semaphores.push(semaphore);
            } else {
                i += 1;
            }
        }
    }

    pub fn take_free_semaphore(&mut self) -> vk::Semaphore {
        self.cleanup_old_semaphores();

        self.free_semaphores.pop().unwrap_or_else(|| {
            unsafe { self.device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None).unwrap() }
        })
    }

    pub fn submitted_semaphore(&mut self, submission_num: usize, semaphore: vk::Semaphore) {
        self.active_semaphores.push((submission_num, semaphore));
    }

    pub fn get_semaphore_to_wait(&mut self, submission_num: usize) -> Option<vk::Semaphore> {
        self.cleanup_old_semaphores();

        if self.shared_state.last_host_waited_submission() >= submission_num {
            return None;
        }

        if let Some((_, sem)) = self.active_semaphores.iter().find(|(n, _)| *n == submission_num) {
            Some(*sem)
        } else {
            let mut best_sem = None;
            let mut min_available_submission = usize::MAX;

            for (num, sem) in &self.active_semaphores {
                if *num > submission_num && *num < min_available_submission {
                    min_available_submission = *num;
                    best_sem = Some(*sem);
                }
            }

            best_sem
        }
    }

    pub fn record_device_commands<F: FnOnce(&mut RecordContext)>(&mut self, mut f: F) {
        let mut record_context = RecordContext::new();
        f(&mut record_context);
    }
}

pub struct OptionSeqNumShared(AtomicUsize);
impl OptionSeqNumShared {
    pub fn new() -> Self {
        OptionSeqNumShared(AtomicUsize::new(usize::MAX))
    }

    pub fn load(&self) -> Option<usize> {
        let v = self.0.load(Ordering::Relaxed);
        if v == usize::MAX {
            None
        }
        else {
            Some(v)
        }
    }

    pub fn store(&self, val: Option<usize>) {
        if let Some(v) = val {
            self.0.store(v, Ordering::Relaxed);
        }
        else {
            self.0.store(usize::MAX, Ordering::Relaxed);
        }
    }
}

