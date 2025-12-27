pub mod queue_local;
pub mod command_buffers;
pub mod memory_manager;
pub mod recording;
pub mod semaphores;
pub mod shared;

use std::collections::HashMap;
use std::mem;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use anyhow::Context;
use ash::vk;
use ash::vk::{AccessFlags, AttachmentDescription, BufferMemoryBarrier, CommandBufferBeginInfo, DependencyFlags, Extent2D, Extent3D, Framebuffer, ImageAspectFlags, ImageLayout, ImageMemoryBarrier, ImageSubresourceRange, PhysicalDevice, PipelineBindPoint, PipelineStageFlags, Queue, Rect2D, RenderPassBeginInfo, SubpassContents, Viewport, WHOLE_SIZE};
use log::{info, warn};
use smallvec::{smallvec, SmallVec};
use sparkles::monotonic::get_perf_frequency;
use sparkles::{range_event_start, static_name};
use sparkles::external_events::ExternalEventsSource;
use strum::IntoDiscriminant;
use crate::extensions::calibrated_timestamps::CalibratedTimestamps;
use crate::resources::image::ImageResource;
use crate::resources::render_pass::RenderPassResource;
use crate::runtime::{WaitSemaphoreRef, WaitSemaphoreStagesRef};
use command_buffers::CommandBufferManager;
use shared::SharedState;
use crate::queue::recording::{DeviceCommand, DrawCommand, RecordContext, SpecificResourceUsage};
use crate::queue::semaphores::{SemaphoreManager, WaitedOperation};
use crate::resources::{LastResourceUsage, RequiredSync, ResourceUsage};
use crate::swapchain_wrapper::SwapchainWrapper;
use crate::wrappers::device::VkDeviceRef;
use crate::wrappers::surface::VkSurfaceRef;
use crate::wrappers::timestamp_pool::TimestampPool;

pub struct GraphicsQueue {
    physical_device: PhysicalDevice,
    device: VkDeviceRef,
    queue: vk::Queue,
    surface: VkSurfaceRef,
    swapchain_wrapper: SwapchainWrapper,
    framebuffers: HashMap<vk::RenderPass, (Arc<RenderPassResource>, SmallVec<[(Framebuffer, SmallVec<[ImageResource; 5]>); 5]>)>,

    shared_state: shared::SharedState,
    semaphore_manager: SemaphoreManager,
    command_buffer_manager: CommandBufferManager,


    last_time_sync_tm: Option<Instant>,
    sparkles_gpu_channel: ExternalEventsSource,


    // queries
    timestamp_pool: Option<TimestampPool>,

    // extensions
    calibrated_timestamps: Option<CalibratedTimestamps>,
}
impl GraphicsQueue {
    pub fn new(
        device: VkDeviceRef,
        queue_family_index: u32,
        queue: Queue,
        physical_device: PhysicalDevice,
        swapchain_wrapper: SwapchainWrapper,
        surface: VkSurfaceRef,
        calibrated_timestamps: Option<CalibratedTimestamps>,
        timestamp_pool: Option<TimestampPool>,
    ) -> Self {
        let shared_state = SharedState::new(device.clone());

        let mut sparkles_gpu_channel = ExternalEventsSource::new("Vulkan GPU".to_string());
        if let Some(calibrated_timestamps) = &calibrated_timestamps {
            if let Some((gpu_tm, host_tm, provider)) = calibrated_timestamps.get_timestamps_pair() {
                sparkles_gpu_channel.push_sync_point(host_tm * (1_000_000_000 / get_perf_frequency()), gpu_tm);
            }
        }

        GraphicsQueue {
            physical_device,
            device: device.clone(),
            queue,
            surface,
            swapchain_wrapper,
            framebuffers: HashMap::new(),

            shared_state,
            semaphore_manager: SemaphoreManager::new(device.clone()),
            command_buffer_manager: CommandBufferManager::new(device.clone(), queue_family_index),

            last_time_sync_tm: None,
            sparkles_gpu_channel,
            timestamp_pool,
            calibrated_timestamps,
        }
    }

    pub fn create_render_pass(&mut self, device: VkDeviceRef, shared: SharedState,
                              swapchain_images: SmallVec<[ImageResourceHandle; 3]>, mut attachments_description: AttachmentsDescription) -> RenderPassResource {
        // Create images for framebuffer and framebuffers
        let swapchain_extent = vk::Extent2D {
            width: swapchain_images[0].width,
            height: swapchain_images[0].height,
        };

        // Create images and image views for attachments (except swapchain image)
        let framebuffers = self.create_framebuffers(
            device.clone(),
            &swapchain_images,
            Extent3D {
                width: swapchain_extent.width,
                height: swapchain_extent.height,
                depth: 1,
            },
            shared.clone(),
        );

        let render_pass_inner = RenderPassInner {
            render_pass,
            last_used_in: 0,
        };
        let key = self.render_passes.insert(render_pass_inner);

        RenderPassResource::new(key, shared)
    }
    fn destroy_render_pass_resources(&mut self, render_pass: vk::RenderPass) {
        let framebuffers = self.framebuffers.remove(&render_pass);
        if let Some((rp,framebuffers)) = framebuffers {

        }
    }
    pub fn recreate_render_pass_resources(&mut self, render_pass_handle: RenderPassHandle, device: VkDeviceRef, shared: SharedState,
                                          swapchain_images: &SmallVec<[ImageResourceHandle; 3]>) {
        let render_pass_inner = self.render_passes.get_mut(render_pass_handle.0).unwrap();
        assert!(render_pass_inner.framebuffers.is_empty(), "Render pass resources must be destroyed using `destroy_render_pass_resources` before recreation");

        let render_pass = render_pass_inner.render_pass;
        let attachments_description = &render_pass_inner.attachments_description;
        let mut attachments = SmallVec::<[AttachmentDescription; 5]>::new();
        attachments.push(attachments_description.swapchain_attachment_desc);
        if let Some(depth_attachment) = &attachments_description.depth_attachment_desc {
            attachments.push(*depth_attachment);
        }
        if let Some(resolve_attachment) = &attachments_description.color_attachement_desc {
            attachments.push(*resolve_attachment);
        }
        let swapchain_format = self.image(swapchain_images[0].state_key).format;

        let framebuffers = self.create_framebuffers(
            device.clone(),
            render_pass,
            swapchain_images,
            Extent3D {
                width: swapchain_images[0].width,
                height: swapchain_images[0].height,
                depth: 1,
            },
            &attachments,
            swapchain_format,
            shared.clone(),
        );

        let render_pass_inner = self.render_passes.get_mut(render_pass_handle.0).unwrap();
        render_pass_inner.framebuffers = framebuffers;
    }



    // Swapchain methods
    pub(crate) fn update_swapchain_image_handles(&mut self) {
        let extent = self.swapchain_wrapper.get_extent();
        if let Some(old_handles) = self.swapchain_wrapper.try_get_images() {
            for image in old_handles {
                self.resource_storage.destroy_image(image);
            }
        }
        let format = self.swapchain_wrapper.get_surface_format();
        let swapchain_images = self.swapchain_wrapper.swapchain_images.clone();
        let images = swapchain_images.iter().map(|i| {
            let image = self.resource_storage.add_image(*i, format, extent.width, extent.height);
            image
        }).collect::<Vec<_>>();
        self.swapchain_wrapper.register_image_handles(&images);
    }

    pub fn recreate_resize(&mut self, new_extent: (u32, u32)) {
        let g = range_event_start!("[Vulkan] Recreate swapchain");
        let new_extent = Extent2D {
            width: new_extent.0,
            height: new_extent.1,
        };
        // Submit all commands and wait for idle
        // TODO: we can schedule destruction for old swapchain :)
        let g = range_event_start!("Wait idle");
        self.wait_idle();
        drop(g);

        let active_render_passes = self.resource_storage.render_passes();
        // 1. Destroy swapchain dependent resources (framebuffers)
        for render_pass in &active_render_passes {
            self.destroy_render_pass_resources(*render_pass, self.shared_state.clone());
        }

        // 2. Recreate swapchain
        let old_format = self.swapchain_wrapper.get_surface_format();
        let old_image_handles = self.swapchain_wrapper.get_images();
        unsafe {
            self.swapchain_wrapper
                .recreate(self.physical_device, new_extent, self.surface.clone())
                .unwrap()
        };
        for image_handle in old_image_handles {
            self.destroy_image(image_handle);
        }
        let new_format = self.swapchain_wrapper.get_surface_format();
        if new_format != old_format {
            unimplemented!("Swapchain format has changed");
        }

        // 2.1 update image handles
        self.update_swapchain_image_handles();
        let new_images = self.swapchain_wrapper.get_images();

        // 3. Recreate swapchain_dependent resources (framebuffers)
        let g = range_event_start!("Create framebuffers");
        for render_pass in &active_render_passes {
            self.resource_storage.recreate_render_pass_resources(*render_pass, self.device.clone(), self.shared_state.clone(), &new_images);
        }
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

    fn record_device_commands_impl<'a, 'b, F>(&'a mut self, f: F, wait_ref: Option<WaitSemaphoreStagesRef>, signal_ref: Option<semaphores::SignalSemaphoreRef>)
    where
        F: FnOnce(&mut RecordContext<'b>),
    {
        let g = range_event_start!("Record and submit");
        self.handle_add_sync_point();

        let mut record_context = RecordContext::new(); // lives for 'c
        f(&mut record_context);

        self.destroy_old_resources();

        let submission_num = self.shared_state.increment_and_get_submission_num();

        self.shared_state.poll_completed_fences(); // fast check if any of previous submissions are finished
        let last_waited_submission = self.shared_state.last_host_waited_submission();
        self.semaphore_manager.on_last_waited_submission(last_waited_submission); // recycle old semaphores
        self.command_buffer_manager.on_last_waited_submission(last_waited_submission); // recycle old command buffers

        // handle wait semaphore
        let mut wait_semaphore = None;
        if let Some(wait_sem) = wait_ref {
            let stage_flags = wait_sem.stage_flags;
            let (sem, sem_waited_operations) = self.semaphore_manager.get_wait_semaphore(wait_sem, Some(submission_num));

            for waited_op in &sem_waited_operations {
                if let WaitedOperation::SwapchainImageAcquired(image_handle) = waited_op {
                    let image_inner = self.resource_storage.image(image_handle.state_key);
                    // create usage with the same stage flags to create dependency chain with waited semaphore
                    image_inner.usages = LastResourceUsage::HasWrite {
                        last_write: Some(ResourceUsage::new(None, stage_flags, AccessFlags::empty())),
                        visible_for: AccessFlags::empty(),
                    };
                }
            }
            let waited_except_swapchain_image_acq = sem_waited_operations.into_iter().filter(|op| {
                !matches!(op, WaitedOperation::SwapchainImageAcquired(_))
            }).collect::<SmallVec<_>>();
            wait_semaphore = Some((sem, stage_flags, waited_except_swapchain_image_acq));
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
                for usage in cmd.usages(submission_num, &mut self.resource_storage, self.swapchain_wrapper.get_images()) {
                    match usage {
                        SpecificResourceUsage::BufferUsage {
                            usage,
                            handle
                        } => {
                            // Update host state last_used_in for mappable buffers
                            if let Some(host_state) = handle.host_state {
                                host_state.last_used_in.store(Some(submission_num));
                            }

                            let had_host_writes = handle.host_state.is_some_and(|s| s.has_host_writes.swap(false, Ordering::Relaxed));

                            let buffer_inner = self.resource_storage.buffer(handle.state_key);

                            // 1) update state if waited on host
                            buffer_inner.usages.on_host_waited(last_waited_submission, had_host_writes);

                            // 2) Add new usage and get required memory synchronization state
                            let required_sync = buffer_inner.usages.add_usage(usage);

                            let buffer = buffer_inner.buffer;

                            // 3) add memory barrier if required
                            if let Some(required_sync) = required_sync {
                                if buffer_barriers.iter().any(|b| b.buffer == buffer) {
                                    panic!("Missing required pipeline barrier between same buffer usages! Required sync: {:?}", required_sync);
                                }

                                let barrier = BufferMemoryBarrier::default()
                                    .buffer(buffer)
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
                            handle,
                            required_layout,
                            image_aspect,
                        } => {
                            let image_inner = self.resource_storage.image(handle.state_key);
                            let prev_layout = image_inner.layout;

                            // 1) update state if waited on host
                            image_inner.usages.on_host_waited(last_waited_submission, false);

                            // 2) Add new usage and get required memory synchronization state
                            let need_layout_transition = required_layout.is_some_and(|required_layout| prev_layout == ImageLayout::GENERAL || required_layout != prev_layout);
                            let required_sync = image_inner.usages.add_usage(usage);

                            let image = image_inner.image;

                            // 3) add memory barrier if usage changed or layout transition required
                            let need_barrier = required_sync.is_some() || need_layout_transition;
                            if need_barrier {
                                if image_barriers.iter().any(|b| b.image == image) {
                                    panic!("Missing required pipeline barrier between same image usages! Usage1: {:?}, Usage2: {:?}", required_sync, usage);
                                }

                                let required_sync = required_sync.unwrap_or(RequiredSync::default());

                                let mut barrier = ImageMemoryBarrier::default()
                                    .image(image)
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
                    }
                }
                if let DeviceCommand::RenderPassEnd {render_pass, framebuffer_index} = cmd {
                    // mark attachments as having a new layout
                    let attachments_description = self.resource_storage.render_pass(render_pass.0).attachments_description.clone();

                    let swapchain_image_final_layout = attachments_description.get_swapchain_desc().final_layout;
                    let swapchain_images = self.swapchain_wrapper.get_images();
                    let swapchain_image_inner = self.resource_storage.image(swapchain_images[*framebuffer_index as usize].state_key);
                    swapchain_image_inner.layout = swapchain_image_final_layout;

                    let mut attachment_i = 0;
                    if let Some(depth_att_desc) = attachments_description.get_depth_attachment_desc() {
                        let depth_image_final_layout = depth_att_desc.final_layout;

                        let image_handle = self.resource_storage.render_pass(render_pass.0).framebuffers[*framebuffer_index as usize].1[attachment_i].handle();
                        let depth_image = self.resource_storage.image(image_handle.state_key);
                        depth_image.layout = depth_image_final_layout;

                        attachment_i += 1;
                    }

                    if let Some(color_attachment_desc) = attachments_description.get_color_attachment_desc() {
                        let color_attachment_final_layout = color_attachment_desc.final_layout;

                        let image_handle = self.resource_storage.render_pass(render_pass.0).framebuffers[*framebuffer_index as usize].1[attachment_i].handle();
                        let color_attachment_image = self.resource_storage.image(image_handle.state_key);
                        color_attachment_image.layout = color_attachment_final_layout;

                        attachment_i += 1;
                    }

                    // update last_used_in
                    self.resource_storage.render_pass(render_pass.0).last_used_in = submission_num;
                }
            }

            // 2) insert single barrier for entire group if needed
            if !buffer_barriers.is_empty() || !image_barriers.is_empty() {
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
                    DeviceCommand::RenderPassBegin {
                        render_pass,
                        framebuffer_index,
                        clear_values,
                    } => {
                        let render_pass = self.resource_storage.render_pass(render_pass.0);
                        let info = RenderPassBeginInfo::default()
                            .render_pass(render_pass.render_pass)
                            .framebuffer(render_pass.framebuffers[*framebuffer_index as usize].0)
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
                                                   pipeline_handle,
                                                   pipeline_handle_changed,
                                                   new_descriptor_set_bindings,
                                               } ) => {
                        unsafe {
                            if let Some(vert_binding) = new_vertex_buffer {
                                let buffer = self.resource_storage.buffer(vert_binding.state_key).buffer;
                                self.device.cmd_bind_vertex_buffers(cmd_buffer, 0, &[buffer], &[0]);
                            }
                            if *pipeline_handle_changed {
                                let pipeline = self.resource_storage.pipeline(pipeline_handle.key).pipeline;
                                self.device.cmd_bind_pipeline(cmd_buffer, PipelineBindPoint::GRAPHICS, pipeline);
                            }
                            for (binding, desc_set_handle) in new_descriptor_set_bindings {
                                // update descriptor set if have new bindings
                                if desc_set_handle.updates_locked.load(Ordering::Relaxed) {
                                    self.resource_storage.update_descriptor_set(desc_set_handle.clone());
                                }

                                // bind descriptor set
                                let descriptor_set = self.resource_storage.descriptor_set(desc_set_handle.key);
                                let pipeline_layout = self.resource_storage.pipeline(pipeline_handle.key).pipeline_layout;
                                self.device.cmd_bind_descriptor_sets(
                                    cmd_buffer,
                                    PipelineBindPoint::GRAPHICS,
                                    pipeline_layout,
                                    *binding,
                                    &[descriptor_set],
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

        // write end timestamp
        if let Some(slot) = slot {
            self.timestamp_pool.as_mut().unwrap().write_end_timestamp(cmd_buffer, slot);
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
            // add information about waiting for this submission to complete
            waited_operations.push(WaitedOperation::Submission(submission_num, vk::PipelineStageFlags::ALL_COMMANDS));
            let signal_semaphore = self.semaphore_manager.allocate_signal_semaphore(&signal, waited_operations);
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
        self.shared_state.submitted_fence(submission_num, fence);
    }

    // Acquire next swapchain image with semaphore signaling
    pub fn acquire_next_image(&mut self) -> anyhow::Result<(u32, WaitSemaphoreRef, bool)> {
        let (signal_ref, wait_ref) = self.semaphore_manager.create_semaphore_pair();
        let signal_semaphore = self.semaphore_manager.allocate_signal_semaphore(&signal_ref, smallvec![]);

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
        let image_handle = self.swapchain_wrapper.get_images()[index as usize];

        #[cfg(feature = "recording-logs")]
        info!("Recording command AcquireNextImage (image_index = {})", index);

        self.semaphore_manager.modify_waited_operations(&wait_ref, smallvec![WaitedOperation::SwapchainImageAcquired(image_handle)]);

        Ok((index, wait_ref, is_suboptimal))
    }

    // Present with semaphore wait
    pub fn queue_present(&mut self, image_index: u32, wait_ref: WaitSemaphoreRef) -> anyhow::Result<bool> {
        // Convert to WaitSemaphoreStagesRef (present doesn't use stages)
        let wait_stages_ref = wait_ref.with_stages(PipelineStageFlags::ALL_COMMANDS);

        // Present operations don't have fence tracking, use None for untracked semaphore
        let (wait_semaphore, waited_operations) = self.semaphore_manager.get_wait_semaphore(wait_stages_ref, None);
        // ensure swapchain image is prepared and is in PRESENT layout
        let image_handle = self.swapchain_wrapper.get_images()[image_index as usize];
        let image_inner = self.resource_storage.image(image_handle.state_key);
        if image_inner.layout != ImageLayout::GENERAL && image_inner.layout != ImageLayout::PRESENT_SRC_KHR {
            warn!("Image layout for presentable image must be PRESENT or GENERAL!");
        }
        if let LastResourceUsage::HasWrite{last_write: Some(ResourceUsage {submission_num, ..}), ..} = &mut image_inner.usages {
            if let Some(usage_sub_num) = submission_num {
                let mut write_syncronized = false;
                for waited_op in waited_operations {
                    if let WaitedOperation::Submission(sub_num, stages) = waited_op {
                        if sub_num >= *usage_sub_num {
                            write_syncronized = true;
                        }
                    }
                }

                if !write_syncronized {
                    warn!("Found unsynchronized write to swapchain image before present! Last write submission: {}",
                        usage_sub_num,
                    );
                }
            }
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

