use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use ash::vk;
use ash::vk::{CommandBuffer, CommandBufferAllocateInfo, CommandBufferBeginInfo, CommandBufferLevel, CommandPool, FenceCreateInfo, Queue};
use log::warn;
use parking_lot::Mutex;
use slotmap::DefaultKey;
use crate::runtime::recording::{DeviceCommand, RecordContext};
use crate::runtime::resources::{BufferInner, BufferResourceDestroyHandle, BufferResourceHandle, ResourceStorage};
use crate::wrappers::device::VkDeviceRef;

pub mod resources;
pub mod recording;

struct SharedStateInner {
    device: VkDeviceRef,
    host_waited_submission: usize,
    active_fences: Vec<(usize, vk::Fence)>,
    free_fences: Vec<vk::Fence>,
    scheduled_for_destroy_buffers: Vec<BufferResourceDestroyHandle>,
}
impl SharedStateInner {
    fn new(device: VkDeviceRef) -> Self {
        Self {
            host_waited_submission: 0,
            active_fences: Vec::new(),
            free_fences: Vec::new(),
            device,
            scheduled_for_destroy_buffers: Vec::new(),
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

    pub fn confirm_all_waited(&self, submission_num: usize) {
        self.state.lock().confirm_wait_fence(submission_num);
    }

    fn schedule_destroy_buffer(&self, handle: BufferResourceDestroyHandle) {
        self.state.lock().schedule_destroy_buffer(handle);
    }
}


pub struct LocalState {
    device: VkDeviceRef,
    shared_state: SharedState,
    free_semaphores: Vec<vk::Semaphore>,
    active_semaphores: Vec<(usize, vk::Semaphore)>,
    next_submission_num: usize,
    command_pool: CommandPool,
    queue: Queue,
    resource_storage: ResourceStorage,
}

impl LocalState {
    pub fn new(device: VkDeviceRef, command_pool: CommandPool, queue: Queue, resource_storage: ResourceStorage) -> Self {
        let shared_state = SharedState::new(device.clone());
        Self {
            device,
            shared_state,
            free_semaphores: Vec::new(),
            active_semaphores: Vec::new(),
            next_submission_num: 1,
            command_pool,
            queue,
            resource_storage,
        }
    }
     
    pub fn shared(&self) -> SharedState {
        self.shared_state.clone()
    }

    pub fn wait_idle(&mut self) {
        unsafe {
            self.device.queue_wait_idle(self.queue).unwrap();
        }

        // after wait_idle, all submissions up to next_submission_num-1 are done
        let last_submitted = self.next_submission_num - 1;
        if last_submitted > 0 {
            self.shared_state.confirm_all_waited(last_submitted);
        }
    }
    pub fn add_buffer(&mut self, buffer: BufferInner) -> DefaultKey {
        self.resource_storage.add_buffer(buffer)
    }

    pub fn queue(&self) -> Queue {
        self.queue
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

    pub fn record_device_commands<F: FnOnce(&mut RecordContext)>(&mut self, f: F) {
        let mut record_context = RecordContext::new();
        f(&mut record_context);

        let submission_num = self.next_submission_num;
        self.next_submission_num += 1;

        // allocate command buffer
        let cmd_buffer = unsafe {
            self.device.allocate_command_buffers(&CommandBufferAllocateInfo::default()
                .command_pool(self.command_pool)
                .level(CommandBufferLevel::PRIMARY)
                .command_buffer_count(1)
            ).unwrap()[0]
        };

        // begin recording
        unsafe {
            self.device.begin_command_buffer(cmd_buffer, &CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)
            ).unwrap();
        }

        // record commands
        for cmd in record_context.take_commands() {
            match cmd {
                DeviceCommand::BufferCopy { src, dst, regions } => {
                    let src_buffer = {
                        let src_inner = self.resource_storage.buffer(src.state_key);
                        src_inner.used_in.push(submission_num);
                        src_inner.buffer
                    };
                    let dst_buffer = {
                        let dst_inner = self.resource_storage.buffer(dst.state_key);
                        dst_inner.used_in.push(submission_num);
                        dst_inner.buffer
                    };

                    unsafe {
                        self.device.cmd_copy_buffer(cmd_buffer, src_buffer, dst_buffer, &regions);
                    }
                }
            }
        }

        // end recording
        unsafe {
            self.device.end_command_buffer(cmd_buffer).unwrap();
        }

        // get fence and semaphore for signaling
        let fence = self.shared_state.take_free_fence();
        let signal_semaphore = self.take_free_semaphore();

        // submit
        unsafe {
            self.device.queue_submit(self.queue, &[vk::SubmitInfo::default()
                .command_buffers(&[cmd_buffer])
                .signal_semaphores(&[signal_semaphore])
            ], fence).unwrap();
        }

        // register fence and semaphore
        self.shared_state.submitted_fence(submission_num, fence);
        self.submitted_semaphore(submission_num, signal_semaphore);

        // free command buffer after submission (will be actually freed when fence signals)
        unsafe {
            self.device.free_command_buffers(self.command_pool, &[cmd_buffer]);
        }
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

