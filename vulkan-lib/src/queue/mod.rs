pub mod queue_local;
pub mod command_buffers;
pub mod memory_manager;
pub mod recording;
pub mod semaphores;
pub mod shared;

use std::collections::HashMap;
use std::sync;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use anyhow::Context;
use ash::vk;
use ash::vk::{AccessFlags, BufferMemoryBarrier, CommandBufferBeginInfo, DependencyFlags, Extent2D, ImageAspectFlags, ImageCreateFlags, ImageLayout, ImageMemoryBarrier, ImageSubresourceRange, ImageUsageFlags, ImageView, MemoryHeap, MemoryType, PhysicalDevice, PipelineBindPoint, PipelineStageFlags, Queue, Rect2D, RenderPassBeginInfo, SubpassContents, Viewport, WHOLE_SIZE};
use log::{info, warn};
use smallvec::{smallvec, SmallVec};
use sparkles::monotonic::get_perf_frequency;
use sparkles::{range_event_start, static_name};
use sparkles::external_events::ExternalEventsSource;
use strum::IntoDiscriminant;
use crate::extensions::calibrated_timestamps::CalibratedTimestamps;
use crate::resources::image::ImageResource;
use crate::resources::render_pass::{AttachmentUsage, FrameBufferAttachment, RenderPassResource};
use crate::resources::VulkanAllocator;
use command_buffers::CommandBufferManager;
use shared::{HostWaitedNum, SharedState};
use crate::queue::memory_manager::MemoryManager;
use crate::queue::queue_local::QueueLocalToken;
use crate::queue::recording::{DeviceCommand, DrawCommand, RecordContext, SpecificResourceUsage};
use crate::queue::semaphores::{SemaphoreManager, WaitSemaphoreRef, WaitSemaphoreStagesRef, SemaphoreWaitOperation};
use crate::resources::{LastResourceUsage, RequiredSync, ResourceUsage};
use crate::swapchain_wrapper::SwapchainWrapper;
use crate::VulkanInstance;
use crate::wrappers::device::VkDeviceRef;
use crate::wrappers::surface::VkSurfaceRef;
use crate::wrappers::timestamp_pool::TimestampPool;

/// Data for a single framebuffer and its attachments
pub(crate) struct FramebufferData {
    pub(crate) framebuffer: vk::Framebuffer,
    pub(crate) depth_image: Option<Arc<ImageResource>>,
    pub(crate) color_image: Option<Arc<ImageResource>>,
}

impl FramebufferData {
    pub(crate) fn attachment(&self, idx: usize) -> Arc<ImageResource> {
        if idx == 1 {
            if let Some(depth) = &self.depth_image {
                depth.clone()
            } else if let Some(color) = &self.color_image {
                color.clone()
            } else {
                panic!("Framebuffer has no additional attachments");
            }
        }
        else if idx == 2 {
            if self.depth_image.is_some() && let Some(color) = &self.color_image {
                color.clone()
            } else {
                panic!("Framebuffer has no color attachment");
            }
        }
        else {
            panic!("Invalid attachment index");
        }
    }
}

/// Set of framebuffers for a render pass
/// Each framebuffer has its own depth/color attachment images
pub(crate) struct FramebufferSet {
    pub(crate) render_pass: sync::Weak<RenderPassResource>,
    pub(crate) framebuffers: SmallVec<[FramebufferData; 4]>,
}

pub struct GraphicsQueue {
    physical_device: PhysicalDevice,
    device: VkDeviceRef,
    queue: vk::Queue,
    surface: VkSurfaceRef,
    swapchain_wrapper: SwapchainWrapper,
    framebuffers: HashMap<vk::RenderPass, FramebufferSet>,
    recycled_framebuffer_sets: HashMap<usize, Vec<FramebufferSet>>,
    recycled_swapchain_images: HashMap<usize, Vec<Arc<ImageResource>>>,
    token: QueueLocalToken,

    semaphore_manager: SemaphoreManager,
    command_buffer_manager: CommandBufferManager,

    last_time_sync_tm: Option<Instant>,
    sparkles_gpu_channel: ExternalEventsSource,


    // queries
    timestamp_pool: Option<TimestampPool>,

    // extensions
    calibrated_timestamps: Option<CalibratedTimestamps>,


    memory_manager: MemoryManager,
    // Last to be dropped
    instance: Arc<VulkanInstance>,
}
impl GraphicsQueue {
    pub fn new(
        instance: Arc<VulkanInstance>,
        queue_family_index: u32,
        queue: Queue,
        physical_device: PhysicalDevice,
        swapchain_wrapper: SwapchainWrapper,
        calibrated_timestamps: Option<CalibratedTimestamps>,
        timestamp_pool: Option<TimestampPool>,
        memory_types: Vec<MemoryType>,
        memory_heaps: Vec<MemoryHeap>,
    ) -> Self {
        let surface = swapchain_wrapper.surface();

        let mut sparkles_gpu_channel = ExternalEventsSource::new("Vulkan GPU".to_string());
        if let Some(calibrated_timestamps) = &calibrated_timestamps {
            if let Some((gpu_tm, host_tm, provider)) = calibrated_timestamps.get_timestamps_pair() {
                sparkles_gpu_channel.push_sync_point(host_tm * (1_000_000_000 / get_perf_frequency()), gpu_tm);
            }
        }

        let device = instance.device.clone();
        GraphicsQueue {
            device: device.clone(),
            instance,
            physical_device,
            queue,
            surface,
            swapchain_wrapper,
            framebuffers: HashMap::new(),
            recycled_framebuffer_sets: HashMap::new(),
            recycled_swapchain_images: HashMap::new(),
            token: QueueLocalToken::try_new().unwrap(),

            semaphore_manager: SemaphoreManager::new(device.clone()),
            command_buffer_manager: CommandBufferManager::new(device.clone(), queue_family_index),

            last_time_sync_tm: None,
            sparkles_gpu_channel,
            timestamp_pool,
            calibrated_timestamps,

            memory_manager: MemoryManager::new(
                device.clone(),
                memory_types,
                memory_heaps,
            ),
        }
    }

    pub fn new_allocator(&self) -> VulkanAllocator {
        VulkanAllocator::new(
            self.instance.clone(),
            self.memory_manager.clone(),
        )
    }

    fn get_or_create_framebuffers(&mut self, render_pass: &Arc<RenderPassResource>) -> SmallVec<[vk::Framebuffer; 4]> {
        let framebuffer_set = self.framebuffers
            .entry(render_pass.render_pass)
            .or_insert_with(|| {
                let swapchain_images = self.swapchain_wrapper.get_images();
                let swapchain_image_count = swapchain_images.len();
                let swapchain_extent = self.swapchain_wrapper.get_extent();

                let attachments_desc = render_pass.attachments_desc();
                let mut framebuffer_data_vec = smallvec![];

                // Create one framebuffer per swapchain image
                for framebuffer_index in 0..swapchain_image_count {
                    let mut views: SmallVec<[ImageView; 5]> = smallvec![];
                    let mut depth_image = None;
                    let mut color_image = None;

                    // Attachment 0: swapchain image
                    views.push(swapchain_images[framebuffer_index].image_view);

                    // Create depth/color images for this framebuffer
                    for (idx, slot, desc, _) in attachments_desc.iter_attachments() {
                        if idx == 0 {
                            continue; // skip swapchain attachment
                        }
                        match slot {
                            AttachmentUsage::Depth => {
                                let image = Arc::new(ImageResource::new(
                                    &self.device,
                                    &mut self.memory_manager,
                                    ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                                    ImageCreateFlags::empty(),
                                    swapchain_extent.width,
                                    swapchain_extent.height,
                                    desc.format,
                                    desc.samples,
                                ));
                                views.push(image.image_view);
                                depth_image = Some(image);
                            },
                            AttachmentUsage::Color | AttachmentUsage::Resolve => {
                                let image = Arc::new(ImageResource::new(
                                    &self.device,
                                    &mut self.memory_manager,
                                    ImageUsageFlags::COLOR_ATTACHMENT,
                                    ImageCreateFlags::empty(),
                                    swapchain_extent.width,
                                    swapchain_extent.height,
                                    desc.format,
                                    desc.samples,
                                ));
                                views.push(image.image_view);
                                color_image = Some(image);
                            },
                        }
                    }

                    // Create the framebuffer
                    let framebuffer_create_info = vk::FramebufferCreateInfo::default()
                        .render_pass(render_pass.render_pass)
                        .attachments(&views)
                        .width(swapchain_extent.width)
                        .height(swapchain_extent.height)
                        .layers(1);

                    let framebuffer = unsafe {
                        self.device.create_framebuffer(&framebuffer_create_info, None).unwrap()
                    };

                    framebuffer_data_vec.push(FramebufferData {
                        framebuffer,
                        depth_image,
                        color_image,
                    });
                }

                FramebufferSet {
                    render_pass: Arc::downgrade(render_pass),
                    framebuffers: framebuffer_data_vec,
                }
            });

        // Extract framebuffer handles to return
        framebuffer_set.framebuffers.iter().map(|fd| fd.framebuffer).collect()
    }


    pub fn recycle_old_resources(&mut self) {
        // destroy framebuffers if their render pass was destroyed
        let keys = self.framebuffers.keys().cloned().collect::<SmallVec<[_; 5]>>();
        for key in keys {
            if sync::Weak::upgrade(&self.framebuffers.get(&key).unwrap().render_pass).is_none() {
                let framebuffer_set = self.framebuffers.remove(&key).unwrap();
                for fb_data in framebuffer_set.framebuffers {
                    unsafe {
                        self.device.destroy_framebuffer(fb_data.framebuffer, None);
                    }
                    // Destroy depth/color images
                    if let Some(depth_image) = fb_data.depth_image {
                        crate::resources::image::destroy_image_resource(&depth_image, true);
                    }
                    if let Some(color_image) = fb_data.color_image {
                        crate::resources::image::destroy_image_resource(&color_image, true);
                    }
                }
            }
        }

        // destroy recycled framebuffer sets (with images) when their submission is waited
        let last_waited_num = self.instance.shared_state.last_host_waited_submission().num();
        for (_, framebuffer_sets) in self.recycled_framebuffer_sets.extract_if(|s, _| *s <= last_waited_num) {
            for framebuffer_set in framebuffer_sets {
                for fb_data in framebuffer_set.framebuffers {
                    unsafe {
                        self.device.destroy_framebuffer(fb_data.framebuffer, None);
                    }
                    if let Some(depth_image) = fb_data.depth_image {
                        crate::resources::image::destroy_image_resource(&depth_image, true);
                    }
                    if let Some(color_image) = fb_data.color_image {
                        crate::resources::image::destroy_image_resource(&color_image, true);
                    }
                }
            }
        }

        // destroy recycled swapchain images when their submission is waited
        for (_, swapchain_images) in self.recycled_swapchain_images.extract_if(|s, _| *s <= last_waited_num) {
            for image in swapchain_images {
                crate::resources::image::destroy_image_resource(&image, true);
            }
        }
    }

    // Swapchain methods
    pub fn recreate_resize(&mut self, new_extent: (u32, u32)) {
        let g = range_event_start!("[Vulkan] Recreate swapchain");
        let new_extent = Extent2D {
            width: new_extent.0,
            height: new_extent.1,
        };

        // 1. Destroy old framebuffers and their depth/color images
        for (_, framebuffer_set) in self.framebuffers.drain() {
            if let Some(render_pass) = sync::Weak::upgrade(&framebuffer_set.render_pass) {
                let last_used = render_pass.submission_usage.load();
                // If recently used, delay destruction of entire set
                if let Some(seq_num) = last_used {
                    self.recycled_framebuffer_sets.entry(seq_num).or_default()
                        .push(framebuffer_set);
                }
                else {
                    // Destroy immediately
                    for fb_data in framebuffer_set.framebuffers {
                        unsafe {
                            self.device.destroy_framebuffer(fb_data.framebuffer, None);
                        }
                        if let Some(depth_image) = fb_data.depth_image {
                            crate::resources::image::destroy_image_resource(&depth_image, true);
                        }
                        if let Some(color_image) = fb_data.color_image {
                            crate::resources::image::destroy_image_resource(&color_image, true);
                        }
                    }
                }
            }
            else {
                // Render pass already destroyed, clean up immediately
                for fb_data in framebuffer_set.framebuffers {
                    unsafe {
                        self.device.destroy_framebuffer(fb_data.framebuffer, None);
                    }
                    if let Some(depth_image) = fb_data.depth_image {
                        crate::resources::image::destroy_image_resource(&depth_image, true);
                    }
                    if let Some(color_image) = fb_data.color_image {
                        crate::resources::image::destroy_image_resource(&color_image, true);
                    }
                }
            }
        }

        // 2. Recreate swapchain and recycle old swapchain images
        let old_format = self.swapchain_wrapper.get_surface_format();
        let old_swapchain_images = unsafe {
            self.swapchain_wrapper
                .recreate(self.physical_device, new_extent, self.surface.clone())
                .unwrap()
        };
        let new_format = self.swapchain_wrapper.get_surface_format();
        if new_format != old_format {
            unimplemented!("Swapchain format has changed");
        }

        // Add old swapchain images to recycling queue
        let seq_num = self.instance.shared_state.last_submission_num();
        self.recycled_swapchain_images.entry(seq_num).or_default()
            .extend(old_swapchain_images);
    }

    pub fn wait_idle(&mut self) {
        let g = range_event_start!("[Vulkan] Wait queue idle");
        unsafe {
            self.device.queue_wait_idle(self.queue).unwrap();
        }

        // after wait_idle, all submissions up to last_submission_num are done
        let last_submitted = self.instance.shared_state.last_submission_num();
        if last_submitted > 0 {
            self.instance.shared_state.confirm_all_waited(last_submitted);
        }

        self.semaphore_manager.on_wait_idle();
        self.command_buffer_manager.on_wait_idle();
        self.recycle_old_resources();
    }

    pub fn wait_prev_submission(&mut self, rel_sub_num: usize) -> HostWaitedNum {
        self.instance.shared_state.wait_submission(rel_sub_num)
    }

    pub fn swapchain_image_count(&self) -> usize {
        self.swapchain_wrapper.get_images().len()
    }

    pub fn swapchain_format(&self) -> vk::Format {
        self.swapchain_wrapper.get_surface_format()
    }

    pub fn swapchain_extent(&self) -> vk::Extent2D {
        self.swapchain_wrapper.get_extent()
    }

    fn handle_add_sync_point(&mut self) {
        if let Some(calibrated_timestamps) = &self.calibrated_timestamps {
            let g = range_event_start!("Add gpu time sync point");
            if self.last_time_sync_tm.is_none_or(| t| t.elapsed().as_millis() > 50) && self.timestamp_pool.is_some() {
                self.last_time_sync_tm = Some(Instant::now());

                if let Some((gpu_tm, host_tm, provider)) = calibrated_timestamps.get_timestamps_pair() {
                    self.sparkles_gpu_channel.push_sync_point(host_tm * (1_000_000_000 / get_perf_frequency()), gpu_tm);
                }
            }
        }
    }

    pub fn record_device_commands<'a, F>(&'a mut self, wait_ref: Option<WaitSemaphoreStagesRef>, f: F)
    where
        F: FnOnce(&mut RecordContext) {
        self.record_device_commands_impl(f, wait_ref, None)
    }

    pub fn record_device_commands_signal<'a, F>(&'a mut self, wait_ref: Option<WaitSemaphoreStagesRef>, f: F) -> WaitSemaphoreRef
    where
        F: FnOnce(&mut RecordContext) {
        let (signal_ref, new_wait_ref) = self.semaphore_manager.create_semaphore_pair();

        self.record_device_commands_impl(f, wait_ref, Some(signal_ref));

        new_wait_ref
    }

    fn split_into_barrier_groups<'a>(commands: &'a [DeviceCommand]) -> Vec<&'a [DeviceCommand]> {
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
            // no explicit barriers: each command gets its own group, except render passes stay together
            let mut i = 0;
            while i < commands.len() {
                if matches!(commands[i], DeviceCommand::RenderPassBegin { .. }) {
                    // find matching RenderPassEnd
                    let start = i;
                    i += 1;
                    while i < commands.len() {
                        if matches!(commands[i], DeviceCommand::RenderPassEnd { .. }) {
                            i += 1;
                            break;
                        }
                        i += 1;
                    }
                    groups.push(&commands[start..i]);
                } else {
                    groups.push(&commands[i..i+1]);
                    i += 1;
                }
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

    fn record_device_commands_impl<'a, F>(&'a mut self, f: F, wait_ref: Option<WaitSemaphoreStagesRef>, signal_ref: Option<semaphores::SignalSemaphoreRef>)
    where
        F: FnOnce(&mut RecordContext),
    {
        let g = range_event_start!("Record and submit");
        self.handle_add_sync_point();

        let mut record_context = RecordContext::new(); // lives for 'c
        f(&mut record_context);

        self.recycle_old_resources();
        let submission_num = self.instance.shared_state.increment_and_get_submission_num();

        self.instance.shared_state.poll_completed_fences(); // fast check if any of previous submissions are finished
        let last_waited_submission = self.instance.shared_state.last_host_waited_submission();
        self.semaphore_manager.on_last_waited_submission(last_waited_submission); // recycle old semaphores
        self.command_buffer_manager.on_last_waited_submission(last_waited_submission); // recycle old command buffers

        // handle wait semaphore
        let mut wait_semaphore = None;
        if let Some(wait_sem) = wait_ref {
            let stage_flags = wait_sem.stage_flags;
            let (sem, wait_operation) = self.semaphore_manager.get_wait_semaphore(wait_sem, Some(submission_num));

            wait_semaphore = Some((sem, stage_flags, wait_operation));
        }

        let cmd_buffer = self.command_buffer_manager.take_command_buffer(submission_num);

        // begin recording
        unsafe {
            self.device.begin_command_buffer(cmd_buffer, &CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)
            ).unwrap();
        }

        // write timestamp
        let mut slot = None;
        if let Some(timestamp_pool) = &mut self.timestamp_pool {
            slot = Some(timestamp_pool.write_start_timestamp(cmd_buffer, submission_num));

            // reset old queries
            timestamp_pool.reset_old_slots(cmd_buffer);
        }

        // record commands grouped by barriers
        let commands = record_context.take_commands();
        let groups = Self::split_into_barrier_groups(&commands);

        for (group_num, group) in groups.iter().enumerate() {
            #[cfg(feature = "recording-logs")]
            info!("{{");
            #[cfg(feature = "recording-logs")]
            info!("  Submission number: {:?}", submission_num);
            let mut buffer_barriers: Vec<BufferMemoryBarrier> = vec![];
            let mut image_barriers: Vec<ImageMemoryBarrier> = vec![];
            let mut src_stage_mask = PipelineStageFlags::empty();
            let mut dst_stage_mask = PipelineStageFlags::empty();

            // 1) accumulate barriers for all commands in the group
            for cmd in group.iter() {
                if let DeviceCommand::RenderPassBegin {render_pass, framebuffer_index, ..} = cmd {
                    // create framebuffers if not yet created
                    let _ = self.get_or_create_framebuffers(render_pass);
                }
                for usage in cmd.usages(submission_num, self.swapchain_wrapper.get_images(), &self.framebuffers) {
                    match usage {
                        SpecificResourceUsage::BufferUsage {
                            usage,
                            buffer,
                        } => {
                            // 1) update state if waited on host
                            let buffer_inner = buffer.buffer_inner().get(&mut self.token);
                            buffer_inner.usages.on_host_waited(last_waited_submission);

                            // 2) Add new usage and get required memory synchronization state
                            let required_sync = buffer_inner.usages.add_usage(usage);

                            // 3) add memory barrier if required
                            if let Some(required_sync) = required_sync {
                                if buffer_barriers.iter().any(|b| b.buffer == buffer.buffer()) {
                                    panic!("Missing required pipeline barrier between same buffer usages! Required sync: {:?}", required_sync);
                                }

                                let barrier = BufferMemoryBarrier::default()
                                    .buffer(buffer.buffer())
                                    .size(WHOLE_SIZE)
                                    .src_access_mask(required_sync.src_access)
                                    .dst_access_mask(required_sync.dst_access);

                                src_stage_mask |= required_sync.src_stages;
                                dst_stage_mask |= required_sync.dst_stages;

                                buffer_barriers.push(barrier);
                            }
                        }
                        SpecificResourceUsage::ImageUsage {
                            usage,
                            image,
                            required_layout,
                            image_aspect,
                        } => {
                            let image_inner = image.inner.get(&mut self.token);
                            let prev_layout = image_inner.layout;

                            // 1) update state if waited on host
                            image_inner.usages.on_host_waited(last_waited_submission);

                            // 2) Add new usage and get required memory synchronization state
                            let need_layout_transition = required_layout.is_some_and(|required_layout| prev_layout == ImageLayout::GENERAL || required_layout != prev_layout);
                            let required_sync = image_inner.usages.add_usage(usage);

                            // 3) add memory barrier if usage changed or layout transition required
                            let need_barrier = required_sync.is_some() || need_layout_transition;
                            if need_barrier {
                                if image_barriers.iter().any(|b| b.image == image.image) {
                                    panic!("Missing required pipeline barrier between same image usages! Usage1: {:?}, Usage2: {:?}", required_sync, usage);
                                }

                                let required_sync = required_sync.unwrap_or_else(|| {
                                    // if only layout transition is needed, create minimal sync
                                    let mut res = RequiredSync::default();

                                    res.dst_access = usage.access_flags;
                                    res.dst_stages = usage.stage_flags;

                                    res
                                });

                                let mut barrier = ImageMemoryBarrier::default()
                                    .image(image.image)
                                    .subresource_range(ImageSubresourceRange::default()
                                        .base_mip_level(0)
                                        .base_array_layer(0)
                                        .layer_count(1)
                                        .level_count(1)
                                        .aspect_mask(image_aspect))
                                    .src_access_mask(required_sync.src_access)
                                    .dst_access_mask(required_sync.dst_access);

                                src_stage_mask |= required_sync.src_stages;
                                dst_stage_mask |= required_sync.dst_stages;

                                // 3.2 add layout transition if needed
                                if let Some(required_layout) = required_layout && (prev_layout == ImageLayout::GENERAL || required_layout != prev_layout) {
                                    barrier = barrier
                                        .old_layout(prev_layout)
                                        .new_layout(required_layout);

                                    image_inner.layout = required_layout;
                                }
                                else {
                                    if prev_layout != ImageLayout::UNDEFINED {
                                        barrier = barrier
                                            .old_layout(prev_layout)
                                            .new_layout(prev_layout)
                                    }
                                    else {
                                        barrier = barrier
                                            .old_layout(prev_layout)
                                            .new_layout(ImageLayout::GENERAL)
                                    }
                                }

                                image_barriers.push(barrier);
                            }
                        }
                        SpecificResourceUsage::ValidateTransition {
                            image,
                            dst_layout,
                            sync
                        } => {
                            let image_inner = image.inner.get(&mut self.token);
                            image_inner.usages.on_host_waited(last_waited_submission);

                            // 1) try reset presented usage from semaphore info
                            if let Some((_, stages, operation)) = wait_semaphore
                                && let SemaphoreWaitOperation::ImageAcquire(img) = operation
                                && image.image == img
                                && matches!(image_inner.usages, LastResourceUsage::Presented)
                                && (stages == PipelineStageFlags::ALL_COMMANDS || sync.src_stages.contains(stages)) {

                                image_inner.usages = LastResourceUsage::FenceWaited;
                            }

                            // 2) apply sync + layout transition
                            if image_inner.usages.validate_layout_transition(sync, submission_num) {
                                image_inner.layout = dst_layout;
                            }
                            else {
                                panic!("Render pass transition does not synchronize with previous usage! Required sync: {:?}, actual usage: {:?}",
                                    sync,
                                    image_inner.usages,
                                );
                            }
                        }

                        SpecificResourceUsage::ValidateAttachmentUsage {
                            image,
                            usage,
                            layout,
                        } => {
                            let image_inner = image.inner.get(&mut self.token);
                            image_inner.usages.on_host_waited(last_waited_submission);
                            if !image_inner.usages.validate_usage(usage) {
                                panic!("Render pass attachment usage does not synchronize with previous usage! Required usage: {:?}, actual usage: {:?}",
                                    usage,
                                    image_inner.usages,
                                );
                            }
                        }
                    }
                }
            }

            // 2) insert single barrier for entire group if needed
            if !buffer_barriers.is_empty() || !image_barriers.is_empty() {
                assert!(!dst_stage_mask.is_empty(), "Source stage mask is empty for barrier!");

                let src_stage_mask = if src_stage_mask == PipelineStageFlags::empty() {
                    PipelineStageFlags::TOP_OF_PIPE
                } else {
                    src_stage_mask
                };
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

            // 3) Record all commands from group to the command buffer
            for cmd in group.iter() {
                #[cfg(feature = "recording-logs")]
                info!("  Recording command: {:?}", cmd.discriminant());
                match cmd {
                    DeviceCommand::CopyBuffer { src, dst, regions } => {
                        let src_buffer = src.buffer();
                        let dst_buffer = dst.buffer;
                        unsafe {
                            self.device.cmd_copy_buffer(cmd_buffer, src_buffer, dst_buffer, &regions);
                        }
                    }
                    DeviceCommand::CopyBufferToImage {src, dst, regions} => {
                        unsafe {
                            self.device.cmd_copy_buffer_to_image(cmd_buffer, src.buffer(), dst.image, dst.inner.get(&mut self.token).layout, &regions);
                        }
                    }
                    DeviceCommand::FillBuffer {buffer, offset, size, data} => {
                        unsafe {
                            self.device.cmd_fill_buffer(cmd_buffer, buffer.buffer, *offset, *size, *data);
                        }
                    }
                    DeviceCommand::Barrier => {} // not a command in particular
                    DeviceCommand::ImageLayoutTransition {..} => {} // not a command in particular
                    DeviceCommand::ClearColorImage {image, clear_color, image_aspect} => {
                        let image_inner = image.inner.get(&mut self.token);
                        unsafe {
                            self.device.cmd_clear_color_image(
                                cmd_buffer,
                                image.image,
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
                        let image_inner = image.inner.get(&mut self.token);
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
                                image.image,
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
                    DeviceCommand::RenderPassBegin {
                        render_pass,
                        framebuffer_index,
                        clear_values,
                    } => {
                        let framebuffers = self.get_or_create_framebuffers(render_pass);
                        let info = RenderPassBeginInfo::default()
                            .render_pass(render_pass.render_pass)
                            .framebuffer(framebuffers[*framebuffer_index as usize])
                            .clear_values(clear_values)
                            .render_area(Rect2D::default().extent(self.swapchain_wrapper.swapchain_extent));
                        unsafe {
                            self.device.cmd_begin_render_pass(cmd_buffer, &info, SubpassContents::INLINE);

                            // set dynamic scissors and viewport
                            self.device.cmd_set_viewport(cmd_buffer, 0, &[Viewport::default()
                                .height(self.swapchain_wrapper.get_extent().height as f32)
                                .width(self.swapchain_wrapper.get_extent().width as f32)
                                .min_depth(0.0)
                                .max_depth(1.0)
                            ]);
                            self.device.cmd_set_scissor(cmd_buffer, 0, &[self.swapchain_wrapper.get_extent().into()])
                        }
                    }
                    DeviceCommand::DrawCommand(DrawCommand::Draw {
                                                   vertex_count,
                                                   instance_count,
                                                   first_vertex,
                                                   first_instance,
                                                   new_vertex_buffer,
                                                   pipeline,
                                                   pipeline_changed: pipeline_handle_changed,
                                                   new_descriptor_set_bindings,
                                               } ) => {
                        unsafe {
                            if let Some(vert_binding) = new_vertex_buffer {
                                let offset = vert_binding.custom_range.as_ref().map(|r| r.start).unwrap_or(0) as u64;
                                self.device.cmd_bind_vertex_buffers(cmd_buffer, 0, &[vert_binding.buffer.buffer], &[offset]);
                            }
                            if *pipeline_handle_changed {
                                self.device.cmd_bind_pipeline(cmd_buffer, PipelineBindPoint::GRAPHICS, pipeline.pipeline);
                            }
                            for (binding, descriptor_set) in new_descriptor_set_bindings {
                                // update descriptor set if have new bindings
                                descriptor_set.update_descriptor_set(&self.device);

                                // bind descriptor set
                                self.device.cmd_bind_descriptor_sets(
                                    cmd_buffer,
                                    PipelineBindPoint::GRAPHICS,
                                    pipeline.pipeline_layout,
                                    *binding,
                                    &[descriptor_set.descriptor_set],
                                    &[],
                                );
                            }
                            self.device.cmd_draw(cmd_buffer, *vertex_count, *instance_count, *first_vertex, *first_instance);
                        }
                    }
                    DeviceCommand::RenderPassEnd {
                        render_pass,
                        framebuffer_index,
                    } => {
                        unsafe {
                            self.device.cmd_end_render_pass(cmd_buffer);
                        }
                    }
                }
            }
            #[cfg(feature = "recording-logs")]
            info!("}}");
        }
        record_context.unlock_descriptor_sets();

        // write end timestamp
        if let Some(slot) = slot {
            self.timestamp_pool.as_mut().unwrap().write_end_timestamp(cmd_buffer, slot);
        }

        // end recording
        unsafe {
            self.device.end_command_buffer(cmd_buffer).unwrap();
        }

        // get fence
        let fence = self.instance.shared_state.take_free_fence();
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
            // add information about waiting for this submission to complete
            let signal_semaphore = self.semaphore_manager.allocate_signal_semaphore(&signal, SemaphoreWaitOperation::SubmissionWait(submission_num));
            signal_semaphores = [signal_semaphore];

            submit_info = submit_info.signal_semaphores(&signal_semaphores);
        }

        // submit
        let g = range_event_start!("Submit command buffer");
        unsafe {
            self.device.queue_submit(self.queue, &[submit_info], fence).unwrap();
        }
        drop(g);

        // handle timestamp queries
        if let Some(timestamp_pool) = &mut self.timestamp_pool {
            let ev_name = self.sparkles_gpu_channel.map_event_name(static_name!("Command buffer execution"));
            for (submission_num, begin, end) in timestamp_pool.read_timestamps() {
                self.sparkles_gpu_channel.push_events(&[begin, end], &[(ev_name, 1), (ev_name, 0x81)])
            }
        }

        // register fence
        self.instance.shared_state.submitted_fence(submission_num, fence);
    }

    // Acquire next swapchain image with semaphore signaling
    pub fn acquire_next_image(&mut self) -> anyhow::Result<(u32, WaitSemaphoreRef, bool)> {
        let (signal_ref, wait_ref) = self.semaphore_manager.create_semaphore_pair();
        let signal_semaphore = self.semaphore_manager.allocate_signal_semaphore(&signal_ref, SemaphoreWaitOperation::ImageAcquire(vk::Image::null()));

        let (index, is_suboptimal) = unsafe {
            self.swapchain_wrapper
                .swapchain_loader
                .acquire_next_image(
                    self.swapchain_wrapper.get_swapchain(),
                    u64::MAX,
                    signal_semaphore,
                    vk::Fence::null(),
                )
                .context("acquire_next_image")?
        };
        let image = self.swapchain_wrapper.get_images()[index as usize].image;

        #[cfg(feature = "recording-logs")]
        info!("Recording command AcquireNextImage (image_index = {})", index);

        self.semaphore_manager.modify_wait_operation(&wait_ref, SemaphoreWaitOperation::ImageAcquire(image));

        Ok((index, wait_ref, is_suboptimal))
    }

    // Present with semaphore wait
    pub fn queue_present(&mut self, image_index: u32, wait_ref: WaitSemaphoreRef) -> anyhow::Result<bool> {
        // Convert to WaitSemaphoreStagesRef (present doesn't use stages)
        let wait_stages_ref = wait_ref.with_stages(PipelineStageFlags::ALL_COMMANDS);

        // Present operations don't have fence tracking, use None for untracked semaphore
        let (wait_semaphore, wait_operation) = self.semaphore_manager.get_wait_semaphore(wait_stages_ref, None);
        // ensure swapchain image is prepared and is in PRESENT layout
        let image = self.swapchain_wrapper.get_images()[image_index as usize].clone();
        let image_inner = image.inner.get(&mut self.token);
        if image_inner.layout != ImageLayout::GENERAL && image_inner.layout != ImageLayout::PRESENT_SRC_KHR {
            warn!("Image layout for presentable image must be PRESENT or GENERAL!");
        }
        if let Some(submission_num) = image_inner.usages.last_write_submission_num() {
            if let SemaphoreWaitOperation::SubmissionWait(sub_num) = wait_operation {
                if sub_num >= *submission_num {
                    image_inner.usages = LastResourceUsage::Presented;
                }
                else {
                    warn!("Present is not synchronized with last write to swapchain image! Last write submission: {}, present wait submission: {}",
                        submission_num,
                        sub_num,
                    );
                }
            }
            else {
                panic!("Present wait operation must be SubmissionWait for swapchain images");
            }
        }
        else {
            image_inner.usages = LastResourceUsage::Presented;
        }

        #[cfg(feature = "recording-logs")]
        info!("Recording command QueuePresent (image_index = {})", image_index);

        unsafe {
            self.swapchain_wrapper
                .swapchain_loader
                .queue_present(
                    self.queue,
                    &vk::PresentInfoKHR::default()
                        .wait_semaphores(&[wait_semaphore])
                        .swapchains(&[self.swapchain_wrapper.get_swapchain()])
                        .image_indices(&[image_index]),
                )
                .context("queue_present")
        }
    }
}

impl Drop for GraphicsQueue {
    fn drop(&mut self) {
        self.wait_idle();
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

