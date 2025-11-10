use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use anyhow::Context;
use ash::vk;
use ash::vk::{AccessFlags, BufferMemoryBarrier, CommandBufferBeginInfo, CommandBufferLevel, CommandPool, DependencyFlags, FenceCreateInfo, Handle, PipelineStageFlags, Queue, WHOLE_SIZE};
use log::warn;
use parking_lot::Mutex;
use slotmap::DefaultKey;
use crate::runtime::recording::{DeviceCommand, RecordContext};
use crate::runtime::resources::{BufferInner, BufferResourceDestroyHandle, ResourceStorage};
use crate::runtime::semaphores::SemaphoreManager;
use crate::runtime::command_buffers::CommandBufferManager;
use crate::wrappers::device::VkDeviceRef;

pub mod resources;
pub mod recording;
pub mod semaphores;
pub mod command_buffers;

pub use semaphores::{SignalSemaphoreRef, WaitSemaphoreRef, WaitSemaphoreStagesRef};
use crate::swapchain_wrapper::SwapchainWrapper;

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
    pub fn poll_completed_fences(&self) {
        self.state.lock().poll_completed_fences();
    }
}


pub struct LocalState {
    device: VkDeviceRef,
    shared_state: SharedState,
    semaphore_manager: SemaphoreManager,
    command_buffer_manager: CommandBufferManager,
    next_submission_num: usize,
    queue: Queue,
    resource_storage: ResourceStorage,
}

impl LocalState {
    pub fn new(device: VkDeviceRef, queue_family_index: u32, queue: Queue, resource_storage: ResourceStorage) -> Self {
        let shared_state = SharedState::new(device.clone());
        Self {
            device: device.clone(),
            shared_state,
            semaphore_manager: SemaphoreManager::new(device.clone()),
            command_buffer_manager: CommandBufferManager::new(device, queue_family_index),
            next_submission_num: 1,
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

        self.semaphore_manager.on_wait_idle();
        self.command_buffer_manager.on_wait_idle();
    }
    pub fn add_buffer(&mut self, buffer: BufferInner) -> DefaultKey {
        self.resource_storage.add_buffer(buffer)
    }

    pub fn record_device_commands<'a, 'b, F>(&'a mut self, wait_ref: Option<WaitSemaphoreStagesRef>, f: F)
    where
        F: FnOnce(&mut RecordContext<'b>) {
        self.record_device_commands_impl(f, wait_ref, None)
    }

    pub fn record_device_commands_signal<'a, 'b, F>(&'a mut self, wait_ref: Option<WaitSemaphoreStagesRef>, f: F) -> WaitSemaphoreRef
    where
        F: FnOnce(&mut RecordContext<'b>) {
        let (signal_ref, new_wait_ref) = self.semaphore_manager.create_semaphore_pair();

        self.record_device_commands_impl(f, wait_ref, Some(signal_ref));

        new_wait_ref
    }

    fn record_device_commands_impl<'a, 'b, F>(&'a mut self, f: F, wait_ref: Option<WaitSemaphoreStagesRef>, signal_ref: Option<semaphores::SignalSemaphoreRef>)
    where
        F: FnOnce(&mut RecordContext<'b>),
         {
        let mut record_context = RecordContext::new(); // lives for 'c
        f(&mut record_context);

        let submission_num = self.next_submission_num;
        self.next_submission_num += 1;

        self.shared_state.poll_completed_fences();
        let last_waited_submission = self.shared_state.last_host_waited_submission();
        self.semaphore_manager.on_last_waited_submission(last_waited_submission);
        self.command_buffer_manager.on_last_waited_submission(last_waited_submission);

        let cmd_buffer = self.command_buffer_manager.take_command_buffer(submission_num);

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
                    let mut buffer_barriers = vec![];
                    let mut src_stage_mask = PipelineStageFlags::empty();
                    let mut dst_stage_mask = PipelineStageFlags::empty();

                    let src_inner = self.resource_storage.buffer(src.state_key);
                    // 1) update state if waited on host
                    src_inner.usages.on_host_waited(last_waited_submission);

                    // 2) update usage information and get prev usage information
                    let usage = resources::ResourceUsage::new(
                        submission_num,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::AccessFlags::TRANSFER_READ,
                        true,
                    );
                    let prev_usage = src_inner.usages.add_usage(usage);

                    let src_buffer = src_inner.buffer;
                    // 3) add memory barrier
                    if let Some(prev_usage) = prev_usage {
                        buffer_barriers.push(BufferMemoryBarrier::default()
                            .src_access_mask(prev_usage.access_flags)
                            .dst_access_mask(usage.access_flags)
                            .buffer(src_buffer)
                            .size(WHOLE_SIZE)
                        );

                        src_stage_mask |= prev_usage.stage_flags;
                        dst_stage_mask |= usage.stage_flags;
                    }
                    else if let Some(host_state) = src.host_state && host_state.has_host_writes.swap(false, Ordering::Relaxed) {
                        buffer_barriers.push(BufferMemoryBarrier::default()
                            .src_access_mask(AccessFlags::HOST_WRITE)
                            .dst_access_mask(usage.access_flags)
                            .buffer(src_buffer)
                            .size(WHOLE_SIZE)
                        );

                        src_stage_mask |= PipelineStageFlags::HOST;
                        dst_stage_mask |= usage.stage_flags;
                    }

                    let dst_inner = self.resource_storage.buffer(dst.state_key);
                    // 1) update state if waited on host
                    dst_inner.usages.on_host_waited(last_waited_submission);

                    // 2) update usage information and get prev usage information
                    let usage = resources::ResourceUsage::new(
                        submission_num,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::AccessFlags::TRANSFER_WRITE,
                        false,
                    );
                    let prev_usage = dst_inner.usages.add_usage(usage);

                    let dst_buffer = dst_inner.buffer;
                    // 3) add memory barrier
                    if let Some(prev_usage) = prev_usage {
                        buffer_barriers.push(BufferMemoryBarrier::default()
                            .src_access_mask(prev_usage.access_flags)
                            .dst_access_mask(usage.access_flags)
                            .buffer(dst_buffer)
                            .size(WHOLE_SIZE)
                        );

                        src_stage_mask |= prev_usage.stage_flags;
                        dst_stage_mask |= usage.stage_flags;
                    }
                    else if let Some(host_state) = dst.host_state && host_state.has_host_writes.swap(false, Ordering::Relaxed) {
                        buffer_barriers.push(BufferMemoryBarrier::default()
                            .src_access_mask(AccessFlags::HOST_WRITE)
                            .dst_access_mask(usage.access_flags)
                            .buffer(dst_buffer)
                            .size(WHOLE_SIZE)
                        );

                        src_stage_mask |= PipelineStageFlags::HOST;
                        dst_stage_mask |= usage.stage_flags;
                    }

                    unsafe {
                        if !buffer_barriers.is_empty() {
                            self.device.cmd_pipeline_barrier(
                                cmd_buffer,
                                src_stage_mask,
                                dst_stage_mask,
                                DependencyFlags::empty(),
                                &[],
                                &buffer_barriers,
                                &[]
                            )
                        }

                        self.device.cmd_copy_buffer(cmd_buffer, src_buffer, dst_buffer, &regions);
                    }
                }
            }
        }

        // end recording
        unsafe {
            self.device.end_command_buffer(cmd_buffer).unwrap();
        }

        // get fence
        let fence = self.shared_state.take_free_fence();
         unsafe {
             self.device.reset_fences(&[fence]).unwrap();
         }

        // prepare submit info with optional wait/signal semaphores
        let cmd_buffers = [cmd_buffer];
        let wait_semaphores;
        let wait_dst_stage_masks;
        let signal_semaphores;

        let mut submit_info = vk::SubmitInfo::default()
            .command_buffers(&cmd_buffers);

        // handle optional wait semaphore
        if let Some(wait) = wait_ref {
            let wait_stage_flags = wait.stage_flags;
            let sem = self.semaphore_manager.get_wait_semaphore(wait, Some(submission_num));
            wait_semaphores = [sem];
            wait_dst_stage_masks = [wait_stage_flags];
            submit_info = submit_info
                .wait_semaphores(&wait_semaphores)
                .wait_dst_stage_mask(&wait_dst_stage_masks);
        }

        // handle optional signal semaphore
        if let Some(signal) = signal_ref {
            let signal_semaphore = self.semaphore_manager.allocate_signal_semaphore(&signal);
            signal_semaphores = [signal_semaphore];

            submit_info = submit_info.signal_semaphores(&signal_semaphores);
        }

        // submit
        unsafe {
            self.device.queue_submit(self.queue, &[submit_info], fence).unwrap();
        }

        // register fence
        self.shared_state.submitted_fence(submission_num, fence);
    }

    // Acquire next swapchain image with semaphore signaling
    pub fn acquire_next_image(&mut self, swapchain_wrapper: &mut SwapchainWrapper) -> anyhow::Result<(u32, WaitSemaphoreRef, bool)> {
        let (signal_ref, wait_ref) = self.semaphore_manager.create_semaphore_pair();
        let signal_semaphore = self.semaphore_manager.allocate_signal_semaphore(&signal_ref);

        let (index, is_suboptimal) = unsafe {
            swapchain_wrapper
                .swapchain_loader
                .acquire_next_image(
                    swapchain_wrapper.get_swapchain(),
                    u64::MAX,
                    signal_semaphore,
                    vk::Fence::null(),
                )
                .context("acquire_next_image")?
        };

        Ok((index, wait_ref, is_suboptimal))
    }

    // Present with semaphore wait
    pub fn queue_present(&mut self, image_index: u32, wait_ref: WaitSemaphoreRef, swapchain_wrapper: &mut SwapchainWrapper) -> anyhow::Result<bool> {
        // Convert to WaitSemaphoreStagesRef (present doesn't use stages)
        let wait_stages_ref = wait_ref.with_stages(vk::PipelineStageFlags::empty());

        // Present operations don't have fence tracking, use None for untracked semaphore
        let wait_semaphore = self.semaphore_manager.get_wait_semaphore(wait_stages_ref, None);

        unsafe {
            swapchain_wrapper
                .swapchain_loader
                .queue_present(
                    self.queue,
                    &vk::PresentInfoKHR::default()
                        .wait_semaphores(&[wait_semaphore])
                        .swapchains(&[swapchain_wrapper.get_swapchain()])
                        .image_indices(&[image_index]),
                )
                .context("queue_present")
        }
    }
}

pub struct OptionSeqNumShared(AtomicUsize);
impl Default for OptionSeqNumShared {
    fn default() -> Self {
        Self(AtomicUsize::new(usize::MAX))
    }
}
impl OptionSeqNumShared {
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

