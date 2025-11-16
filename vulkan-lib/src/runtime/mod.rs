use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use anyhow::Context;
use ash::vk;
use ash::vk::{AccessFlags, BufferMemoryBarrier, CommandBufferBeginInfo, CommandPool, DependencyFlags, FenceCreateInfo, Handle, ImageAspectFlags, ImageLayout, ImageMemoryBarrier, ImageSubresourceRange, PipelineStageFlags, Queue, WHOLE_SIZE};
use log::{info, warn};
use parking_lot::Mutex;
use slotmap::DefaultKey;
use smallvec::{smallvec, SmallVec};
use crate::runtime::recording::{DeviceCommand, RecordContext, SpecificResourceUsage};
use crate::runtime::resources::{BufferInner, BufferResourceDestroyHandle, ImageInner, ImageResourceHandle, ResourceStorage, ResourceUsage, ResourceUsages};
use crate::runtime::semaphores::{SemaphoreManager, WaitedOperation};
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
    scheduled_for_destroy_images: Vec<ImageResourceHandle>,
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

    fn schedule_destroy_buffer(&self, handle: BufferResourceDestroyHandle) {
        self.state.lock().schedule_destroy_buffer(handle);
    }
    fn schedule_destroy_image(&self, handle: ImageResourceHandle) {
        self.state.lock().schedule_destroy_image(handle);
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
     
    pub(crate) fn shared(&self) -> SharedState {
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
    
    pub fn wait_prev_submission(&mut self, prev_sub: usize) -> Option<()> {
        let submission_to_wait = self.next_submission_num.checked_sub(1)?.checked_sub(prev_sub)?;
        self.shared_state.wait_submission(submission_to_wait);
        
        
        Some(())
    }
    pub(crate) fn add_buffer(&mut self, buffer: BufferInner) -> DefaultKey {
        self.resource_storage.add_buffer(buffer)
    }

    pub(crate) fn add_image(&mut self, image: ImageInner) -> DefaultKey {
        self.resource_storage.add_image(image)
    }
    pub(crate) fn remove_image(&mut self, image: ImageResourceHandle) {
        self.resource_storage.destroy_image(image.state_key)
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

    fn split_into_barrier_groups<'a>(commands: &'a [DeviceCommand<'a>]) -> Vec<&'a [DeviceCommand<'a>]> {
        if commands.is_empty() {
            return vec![];
        }

        let mut groups = Vec::new();
        let barrier_positions: Vec<usize> = commands
            .iter()
            .enumerate()
            .filter_map(|(i, cmd)| {
                if matches!(cmd, DeviceCommand::Barrier) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        if barrier_positions.is_empty() {
            // no explicit barriers: each command gets its own group
            for i in 0..commands.len() {
                groups.push(&commands[i..i+1]);
            }
        } else {
            // group commands between barrier markers
            let mut start = 0;
            for &barrier_pos in &barrier_positions {
                if start < barrier_pos {
                    groups.push(&commands[start..barrier_pos]);
                }
                // include the Barrier command itself in a group (it does nothing but serves as marker)
                groups.push(&commands[barrier_pos..barrier_pos+1]);
                start = barrier_pos + 1;
            }
            // handle remaining commands after last barrier
            if start < commands.len() {
                groups.push(&commands[start..]);
            }
        }

        groups
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

         let mut wait_semaphore = None;
         if let Some(wait_sem) = wait_ref {
             let stage_flags = wait_sem.stage_flags;
             let (sem, sem_waited_operations) = self.semaphore_manager.get_wait_semaphore(wait_sem, Some(submission_num));

             for waited_op in &sem_waited_operations {
                 if let WaitedOperation::SwapchainImageAcquired(image_handle) = waited_op {
                     let image_inner = self.resource_storage.image(image_handle.state_key);
                     image_inner.usages = ResourceUsages::DeviceUsage(ResourceUsage::new(None, stage_flags, AccessFlags::empty(), true));
                 }
             }
             wait_semaphore = Some((sem, stage_flags, sem_waited_operations));
         }

        let cmd_buffer = self.command_buffer_manager.take_command_buffer(submission_num);

        // begin recording
        unsafe {
            self.device.begin_command_buffer(cmd_buffer, &CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)
            ).unwrap();
        }

        // record commands grouped by barriers
        let commands = record_context.take_commands();
        let groups = Self::split_into_barrier_groups(&commands);

        for (group_num, group) in groups.iter().enumerate() {
            #[cfg(feature = "recording-logs")]
            info!("{{");
            let mut buffer_barriers: Vec<BufferMemoryBarrier> = vec![];
            let mut image_barriers: Vec<ImageMemoryBarrier> = vec![];
            let mut src_stage_mask = PipelineStageFlags::empty();
            let mut dst_stage_mask = PipelineStageFlags::empty();

            // accumulate barriers for all commands in the group
            for cmd in group.iter() {
                for usage in cmd.usages(submission_num) {
                    match usage {
                        SpecificResourceUsage::BufferUsage {
                            usage,
                            handle
                        } => {
                            let buffer_inner = self.resource_storage.buffer(handle.state_key);

                            // 1) update state if waited on host
                            buffer_inner.usages.on_host_waited(last_waited_submission);

                            // 2) update usage information and get prev usage information
                            let prev_usage = buffer_inner.usages.add_usage(usage);

                            let buffer = buffer_inner.buffer;

                            // 3) add memory barrier if usage changed or host write occurred
                            let need_barrier = prev_usage.is_some() ||  handle.host_state.is_some_and(|s| s.has_host_writes.load(Ordering::Relaxed));
                            if need_barrier {
                                if buffer_barriers.iter().any(|b| b.buffer == buffer) {
                                    panic!("Missing required pipeline barrier between same buffer usages! Usage1: {:?}, Usage2: {:?}", prev_usage, usage);
                                }

                                let mut barrier = BufferMemoryBarrier::default()
                                    .buffer(buffer)
                                    .size(WHOLE_SIZE)
                                    .dst_access_mask(usage.access_flags);

                                dst_stage_mask |= usage.stage_flags;

                                // 3.1 add memory barrier if usage changed
                                if let Some(prev_usage) = prev_usage {
                                    barrier = barrier
                                        .src_access_mask(prev_usage.access_flags);

                                    src_stage_mask |= prev_usage.stage_flags;
                                }

                                // 3.2 add host_write dependency if host writes occurred
                                if let Some(host_state) = handle.host_state && host_state.has_host_writes.swap(false, Ordering::Relaxed) {
                                    barrier.src_access_mask |= AccessFlags::HOST_WRITE;

                                    src_stage_mask |= PipelineStageFlags::HOST;
                                }

                                buffer_barriers.push(barrier);
                            }
                        }
                        SpecificResourceUsage::ImageUsage {
                            usage,
                            handle,
                            required_layout,
                            image_aspect,
                        } => {
                            let image_inner = self.resource_storage.image(handle.state_key);
                            let prev_layout = image_inner.layout;

                            // 1) update state if waited on host
                            image_inner.usages.on_host_waited(last_waited_submission);

                            // 2) update usage information and get prev usage information
                            let prev_usage = image_inner.usages.add_usage(usage);

                            let image = image_inner.image;

                            // 3) add memory barrier if usage changed or layout transition required
                            let need_barrier = prev_usage.is_some() || required_layout.is_some_and(|required_layout| prev_layout == ImageLayout::GENERAL || required_layout != prev_layout);
                            if need_barrier {
                                if image_barriers.iter().any(|b| b.image == image) {
                                    panic!("Missing required pipeline barrier between same image usages! Usage1: {:?}, Usage2: {:?}", prev_usage, usage);
                                }

                                let mut barrier = ImageMemoryBarrier::default()
                                    .image(image)
                                    .subresource_range(ImageSubresourceRange::default()
                                        .base_mip_level(0)
                                        .base_array_layer(0)
                                        .layer_count(1)
                                        .level_count(1)
                                        .aspect_mask(image_aspect))
                                    .dst_access_mask(usage.access_flags);
                                dst_stage_mask |= usage.stage_flags;

                                // 3.1 add execution and memory deps if needed
                                if let Some(prev_usage) = prev_usage {
                                    barrier = barrier
                                        .src_access_mask(prev_usage.access_flags);

                                    src_stage_mask |= prev_usage.stage_flags;
                                }

                                // 3.2 add layout transition if needed
                                if let Some(required_layout) = required_layout && (prev_layout == ImageLayout::GENERAL || required_layout != prev_layout) {
                                    barrier = barrier
                                        .old_layout(prev_layout)
                                        .new_layout(required_layout);

                                    image_inner.layout = required_layout;
                                }

                                image_barriers.push(barrier);
                            }
                        }
                    }
                }
            }

            // insert single barrier for entire group if needed
            if !buffer_barriers.is_empty() || !image_barriers.is_empty() {
                #[cfg(feature = "recording-logs")]
                info!("  <- Barrier inserted. SRC: {:?}, DST: {:?}, Buffers: {:?}, Images: {:?}",
                    src_stage_mask,
                    dst_stage_mask,
                    buffer_barriers,
                    image_barriers
                );
                unsafe {
                    self.device.cmd_pipeline_barrier(
                        cmd_buffer,
                        src_stage_mask,
                        dst_stage_mask,
                        DependencyFlags::empty(),
                        &[],
                        &buffer_barriers,
                        &image_barriers
                    )
                }
            }

            // execute all commands in the group
            for cmd in group.iter() {
                #[cfg(feature = "recording-logs")]
                info!("  Recording command: {:?}", cmd.discriminant());
                match cmd {
                    DeviceCommand::CopyBuffer { src, dst, regions } => {
                        let src_buffer = self.resource_storage.buffer(src.state_key).buffer;
                        let dst_buffer = self.resource_storage.buffer(dst.state_key).buffer;
                        if dst.host_state.is_some() {
                            unimplemented!("Copy buffer to host-accessible buffer is not yet implemented");
                        }
                        unsafe {
                            self.device.cmd_copy_buffer(cmd_buffer, src_buffer, dst_buffer, &regions);
                        }
                    }
                    DeviceCommand::CopyBufferToImage {src, dst, regions} => {
                        let src_buffer = self.resource_storage.buffer(src.state_key).buffer;
                        let dst_image = self.resource_storage.image(dst.state_key);
                        unsafe {
                            self.device.cmd_copy_buffer_to_image(cmd_buffer, src_buffer, dst_image.image, dst_image.layout, &regions);
                        }
                    }
                    DeviceCommand::FillBuffer {buffer, offset, size, data} => {
                        let buffer_inner = self.resource_storage.buffer(buffer.state_key);
                        unsafe {
                            self.device.cmd_fill_buffer(cmd_buffer, buffer_inner.buffer, *offset, *size, *data);
                        }
                    }
                    DeviceCommand::Barrier => {} // not a command in particular
                    DeviceCommand::ImageLayoutTransition {..} => {} // not a command in particular
                    DeviceCommand::ClearColorImage {image, clear_color, image_aspect} => {
                        let image_inner = self.resource_storage.image(image.state_key);
                        unsafe {
                            self.device.cmd_clear_color_image(
                                cmd_buffer,
                                image_inner.image,
                                image_inner.layout,
                                clear_color,
                                &[ImageSubresourceRange::default()
                                    .aspect_mask(*image_aspect)
                                    .base_mip_level(0)
                                    .level_count(1)
                                    .base_array_layer(0)
                                    .layer_count(1)],
                            );
                        }
                    },
                    DeviceCommand::ClearDepthStencilImage {
                        image,
                        depth_value,
                        stencil_value,
                    } => {
                        let image_inner = self.resource_storage.image(image.state_key);
                        let mut aspect_mask = ImageAspectFlags::empty();
                        if depth_value.is_some() {
                            aspect_mask |= ImageAspectFlags::DEPTH;
                        }
                        if stencil_value.is_some() {
                            aspect_mask |= ImageAspectFlags::STENCIL;
                        }
                        unsafe {
                            self.device.cmd_clear_depth_stencil_image(
                                cmd_buffer,
                                image_inner.image,
                                image_inner.layout,
                                &vk::ClearDepthStencilValue {
                                    depth: depth_value.unwrap_or(0.0),
                                    stencil: stencil_value.unwrap_or(0),
                                },
                                &[ImageSubresourceRange::default()
                                    .aspect_mask(aspect_mask)
                                    .base_mip_level(0)
                                    .level_count(1)
                                    .base_array_layer(0)
                                    .layer_count(1)],
                            );
                        }
                    }
                }
            }
            #[cfg(feature = "recording-logs")]
            info!("}}");
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
        if let Some((sem, wait_stage_flags, _)) = wait_semaphore {
            wait_semaphores = [sem];
            wait_dst_stage_masks = [wait_stage_flags];
            submit_info = submit_info
                .wait_semaphores(&wait_semaphores)
                .wait_dst_stage_mask(&wait_dst_stage_masks);
        }

        // handle optional signal semaphore
        if let Some(signal) = signal_ref {
            let mut waited_operations = wait_semaphore.map(|(_, _, s)| s).unwrap_or(SmallVec::new());
            // add current submission to waited submissions
            if waited_operations.len() == 1 && let WaitedOperation::Submission(waited_sub_num, PipelineStageFlags::ALL_COMMANDS) = &mut waited_operations[0] {
                // fast path, update submission number
                *waited_sub_num = submission_num;
            }
            else {
                waited_operations.push(WaitedOperation::Submission(submission_num, vk::PipelineStageFlags::ALL_COMMANDS));
            }
            let signal_semaphore = self.semaphore_manager.allocate_signal_semaphore(&signal, waited_operations);
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
    pub(crate) fn acquire_next_image(&mut self, swapchain_wrapper: &mut SwapchainWrapper) -> anyhow::Result<(u32, WaitSemaphoreRef, bool)> {
        let (signal_ref, wait_ref) = self.semaphore_manager.create_semaphore_pair();
        let signal_semaphore = self.semaphore_manager.allocate_signal_semaphore(&signal_ref, smallvec![]);

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
        let image_handle = swapchain_wrapper.get_images()[index as usize];

        #[cfg(feature = "recording-logs")]
        info!("Recording command AcquireNextImage (image_index = {})", index);

        self.semaphore_manager.modify_waited_operations(&wait_ref, smallvec![WaitedOperation::SwapchainImageAcquired(image_handle)]);

        Ok((index, wait_ref, is_suboptimal))
    }

    // Present with semaphore wait
    pub(crate) fn queue_present(&mut self, image_index: u32, wait_ref: WaitSemaphoreRef, swapchain_wrapper: &mut SwapchainWrapper) -> anyhow::Result<bool> {
        // Convert to WaitSemaphoreStagesRef (present doesn't use stages)
        let wait_stages_ref = wait_ref.with_stages(PipelineStageFlags::ALL_COMMANDS);

        // Present operations don't have fence tracking, use None for untracked semaphore
        let (wait_semaphore, waited_operations) = self.semaphore_manager.get_wait_semaphore(wait_stages_ref, None);
        // ensure swapchain image is prepared and is in PRESENT layout
        let image_handle = swapchain_wrapper.get_images()[image_index as usize];
        let image_inner = self.resource_storage.image(image_handle.state_key);
        if image_inner.layout != ImageLayout::GENERAL && image_inner.layout != ImageLayout::PRESENT_SRC_KHR {
            warn!("Image layout for presentable image must be PRESENT or GENERAL!");
        }
        if let ResourceUsages::DeviceUsage(usage) = &mut image_inner.usages {
            let usage_stages = &mut usage.stage_flags;
            if let Some(usage_sub_num) = usage.submission_num {
                for waited_op in waited_operations {
                    if let WaitedOperation::Submission(sub_num, stages) = waited_op {
                        if sub_num >= usage_sub_num {
                            *usage_stages &= !stages;
                        }
                    }
                }
            }

            if !usage_stages.is_empty() {
                warn!("Called Queue present on swapchain image with non-synchronized device usage!")
            }
        }

        #[cfg(feature = "recording-logs")]
        info!("Recording command QueuePresent (image_index = {})", image_index);

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

