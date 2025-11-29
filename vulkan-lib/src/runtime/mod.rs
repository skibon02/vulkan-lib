use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use anyhow::Context;
use ash::vk;
use ash::vk::{AccessFlags, BufferCreateFlags, BufferMemoryBarrier, BufferUsageFlags, CommandBufferBeginInfo, DependencyFlags, Extent2D, Format, ImageAspectFlags, ImageLayout, ImageMemoryBarrier, ImageSubresourceRange, ImageUsageFlags, PhysicalDevice, PipelineBindPoint, PipelineStageFlags, Queue, Rect2D, RenderPassBeginInfo, SampleCountFlags, SubpassContents, TimeDomainEXT, WHOLE_SIZE};
use log::{info, warn};
use smallvec::{smallvec, SmallVec};
use sparkles::external_events::ExternalEventsSource;
use sparkles::{range_event_start, static_name};
use sparkles::monotonic::{get_monotonic, get_monotonic_nanos, get_perf_frequency};
use crate::runtime::recording::{DeviceCommand, DrawCommand, RecordContext, SpecificResourceUsage};
use crate::runtime::resources::{AttachmentsDescription, ImageInner, ResourceStorage, ResourceUsage, ResourceUsages};
use crate::runtime::semaphores::{SemaphoreManager, WaitedOperation};
use crate::runtime::command_buffers::CommandBufferManager;
use crate::runtime::shared::SharedState;
use crate::runtime::memory_manager::{MemoryTypeAlgorithm};
use crate::wrappers::device::VkDeviceRef;
use crate::wrappers::surface::VkSurfaceRef;

pub mod resources;
pub mod recording;
pub mod semaphores;
pub mod command_buffers;
pub mod shared;
pub mod memory_manager;

pub use semaphores::{SignalSemaphoreRef, WaitSemaphoreRef, WaitSemaphoreStagesRef};
use resources::buffers::{BufferResource, MappableBufferResource};
use resources::descriptor_sets::DescriptorSet;
use resources::images::{ImageResource, ImageResourceHandle};
use crate::extensions::calibrated_timestamps::CalibratedTimestamps;
use crate::runtime::resources::pipeline::{GraphicsPipeline, GraphicsPipelineDesc};
use crate::runtime::resources::render_pass::{RenderPassHandle, RenderPassResource};
use crate::shaders::DescriptorSetLayoutBindingDesc;
use crate::swapchain_wrapper::SwapchainWrapper;
use crate::wrappers::timestamp_pool::TimestampPool;

pub struct RuntimeState {
    device: VkDeviceRef,
    shared_state: shared::SharedState,

    semaphore_manager: SemaphoreManager,
    command_buffer_manager: CommandBufferManager,
    queue: Queue,
    resource_storage: ResourceStorage,

    physical_device: PhysicalDevice,

    last_time_sync_tm: Option<Instant>,
    sparkles_gpu_channel: ExternalEventsSource,

    // swapchain
    swapchain_wrapper: SwapchainWrapper,
    surface: VkSurfaceRef,

    // queries
    timestamp_pool: Option<TimestampPool>,

    // extensions
    calibrated_timestamps: Option<CalibratedTimestamps>,
}

impl RuntimeState {
    pub fn new(
        device: VkDeviceRef,
        queue_family_index: u32,
        queue: Queue,
        physical_device: PhysicalDevice,
        memory_types: Vec<vk::MemoryType>,
        memory_heaps: Vec<vk::MemoryHeap>,
        swapchain_wrapper: SwapchainWrapper,
        surface: VkSurfaceRef,
        calibrated_timestamps: Option<CalibratedTimestamps>,
        timestamp_pool: Option<TimestampPool>,
    ) -> Self {
        let shared_state = SharedState::new(device.clone());
        let resource_storage = ResourceStorage::new(device.clone(), memory_types, memory_heaps);

        let mut sparkles_gpu_channel = ExternalEventsSource::new("Vulkan GPU".to_string());
        if let Some(calibrated_timestamps) = &calibrated_timestamps {
            if let Some((gpu_tm, host_tm, provider)) = calibrated_timestamps.get_timestamps_pair() {
                sparkles_gpu_channel.push_sync_point(host_tm * 1_000_000_000 / get_perf_frequency(), gpu_tm);
            }
        }

        Self {
            device: device.clone(),

            shared_state,
            semaphore_manager: SemaphoreManager::new(device.clone()),
            command_buffer_manager: CommandBufferManager::new(device, queue_family_index),
            queue,
            resource_storage,
            physical_device,
            swapchain_wrapper,
            surface,
            timestamp_pool,
            calibrated_timestamps,

            sparkles_gpu_channel,
            last_time_sync_tm: None,
        }
    }

    /// Create new buffer in mappable memory for TRANSFER_SRC usage
    pub fn new_host_buffer(&mut self, size: u64) -> MappableBufferResource {
        let flags = BufferCreateFlags::empty();
        let usage = BufferUsageFlags::TRANSFER_SRC;

        let (buffer, memory) = self.resource_storage.create_buffer(usage, flags, size, MemoryTypeAlgorithm::Host, self.shared_state.clone());
        MappableBufferResource::new(buffer, memory)
    }

    /// Create new buffer in device_local memory
    pub fn new_device_buffer(&mut self, usage: BufferUsageFlags, size: u64) -> BufferResource {
        let flags = BufferCreateFlags::empty();

        let (buffer, _) = self.resource_storage.create_buffer(usage, flags, size, MemoryTypeAlgorithm::Device, self.shared_state.clone());
        buffer
    }

    /// Create 2D image with optimal tiling, not mappable to host
    pub fn new_image(&mut self, format: Format, usage: ImageUsageFlags, samples: SampleCountFlags, width: u32, height: u32) -> ImageResource{
        let flags = ash::vk::ImageCreateFlags::empty();
        let image = self.resource_storage.create_image(usage, flags, MemoryTypeAlgorithm::Device, width, height, format, samples, self.shared_state.clone());
        image
    }

    pub fn new_render_pass(&mut self, attachments_desc: AttachmentsDescription) -> RenderPassResource {
        let swapchain_images = self.swapchain_wrapper.get_images();
        self.resource_storage.create_render_pass(self.device.clone(), self.shared_state.clone(), swapchain_images, attachments_desc)
    }

    pub fn new_descriptor_set<'a, 'b>(&'a mut self, bindings: &'static [DescriptorSetLayoutBindingDesc]) -> DescriptorSet<'b> {
        self.resource_storage.allocate_descriptor_set(bindings, self.shared_state.clone())
    }

    pub fn new_pipeline(&mut self, render_pass: RenderPassHandle, pipeline_desc: GraphicsPipelineDesc) -> GraphicsPipeline {
        self.resource_storage.create_graphics_pipeline(render_pass, pipeline_desc, self.shared_state.clone())
    }

    pub fn destroy_old_resources(&mut self) {
        let scheduled_for_destruction = self.shared_state.take_ready_for_destroy();
        if scheduled_for_destruction.is_empty() {
            return;
        }

        self.resource_storage.destroy_scheduled_resources(scheduled_for_destruction);
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
            self.resource_storage.destroy_render_pass_resources(*render_pass, self.shared_state.clone());
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

    pub fn swapchain_images(&self) -> SmallVec<[ImageResourceHandle; 3]> {
        self.swapchain_wrapper.get_images()
    }

    pub fn wait_idle(&mut self) {
        let g = range_event_start!("[Vulkan] Wait queue idle");
        unsafe {
            self.device.queue_wait_idle(self.queue).unwrap();
        }

        // after wait_idle, all submissions up to last_submission_num are done
        let last_submitted = self.shared_state.last_submission_num();
        if last_submitted > 0 {
            self.shared_state.confirm_all_waited(last_submitted);
        }

        self.semaphore_manager.on_wait_idle();
        self.command_buffer_manager.on_wait_idle();
    }

    pub fn wait_prev_submission(&mut self, prev_sub: usize) -> Option<()> {
        let last_submission = self.shared_state.last_submission_num();
        let submission_to_wait = last_submission.checked_sub(prev_sub)?;
        self.shared_state.wait_submission(submission_to_wait);

        Some(())
    }
    pub(crate) fn destroy_image(&mut self, image: ImageResourceHandle) {
        self.shared_state.schedule_destroy_image(image);
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
        let g = range_event_start!("Record and submit");
        self.handle_add_sync_point();

        let mut record_context = RecordContext::new(); // lives for 'c
        f(&mut record_context);

        self.destroy_old_resources();

        let submission_num = self.shared_state.increment_and_get_submission_num();

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

        // write timestamp
        let mut slot = None;
        if let Some(timestamp_pool) = &mut self.timestamp_pool {
            slot = Some(timestamp_pool.write_start_timestamp(cmd_buffer, submission_num));
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

            // accumulate barriers for all commands in the group
            for cmd in group.iter() {
                for usage in cmd.usages(submission_num) {
                    match usage {
                        SpecificResourceUsage::BufferUsage {
                            usage,
                            handle
                        } => {
                            // Update host state last_used_in for mappable buffers
                            if let Some(host_state) = handle.host_state {
                                host_state.last_used_in.store(Some(submission_num));
                            }

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
                        }
                    }
                    DeviceCommand::DrawCommand(DrawCommand::Draw {
                        vertex_count,
                        instance_count,
                        first_vertex,
                        first_instance,
                        vert_buffer_binding,
                        pipeline_binding,
                        desc_set_bindings,
                    } ) => {
                        unsafe {
                            if let Some(vert_binding) = vert_buffer_binding {
                                let buffer = self.resource_storage.buffer(vert_binding.state_key).buffer;
                                self.device.cmd_bind_vertex_buffers(cmd_buffer, 0, &[buffer], &[0]);
                            }
                            if let Some(pipeline_binding) = pipeline_binding {
                                let pipeline = self.resource_storage.pipeline(pipeline_binding.key).pipeline;
                                self.device.cmd_bind_pipeline(cmd_buffer, PipelineBindPoint::GRAPHICS, pipeline);
                            }
                            for (binding, desc_set_handle) in desc_set_bindings {
                                // update descriptor set
                                if desc_set_handle.bindings_updated.load(Ordering::Relaxed) {
                                    self.resource_storage.update_descriptor_set(desc_set_hanlde);
                                }

                                // bind descriptor set if changed
                            }
                            self.device.cmd_draw(cmd_buffer, *vertex_count, *instance_count, *first_vertex, *first_instance);
                        }
                    }
                    DeviceCommand::RenderPassEnd => {
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
        let g = range_event_start!("Submit command buffer");
        unsafe {
            self.device.queue_submit(self.queue, &[submit_info], fence).unwrap();
        }
        drop(g);

        // handle timestamp queries
        if let Some(timestamp_pool) = &mut self.timestamp_pool {
            for (submission_num, begin, end) in timestamp_pool.read_timestamps() {
                let ev_name = self.sparkles_gpu_channel.map_event_name(static_name!("Command buffer execution"));
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

    pub fn dump_resource_usage(&self) {
        self.resource_storage.dump_resource_usage();
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

