use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use anyhow::Context;
use ash::vk;
use ash::vk::{AccessFlags, BufferCreateFlags, BufferCreateInfo, BufferMemoryBarrier, BufferUsageFlags, CommandBufferBeginInfo, DependencyFlags, Extent2D, Extent3D, Format, ImageAspectFlags, ImageCreateFlags, ImageCreateInfo, ImageLayout, ImageMemoryBarrier, ImageSubresourceRange, ImageTiling, ImageType, ImageUsageFlags, MemoryAllocateInfo, MemoryHeap, MemoryType, PhysicalDevice, PipelineStageFlags, Queue, SampleCountFlags, WHOLE_SIZE};
use log::warn;
use slotmap::DefaultKey;
use smallvec::{smallvec, SmallVec};
use sparkles::range_event_start;
use crate::runtime::recording::{DeviceCommand, RecordContext, SpecificResourceUsage};
use crate::runtime::resources::{BufferInner, ImageInner, ResourceStorage, ResourceUsage, ResourceUsages};
use crate::runtime::semaphores::{SemaphoreManager, WaitedOperation};
use crate::runtime::command_buffers::CommandBufferManager;
use crate::runtime::shared::SharedState;
use crate::wrappers::device::VkDeviceRef;
use crate::wrappers::surface::VkSurfaceRef;
use crate::util::image::is_color_format;

pub mod resources;
pub mod recording;
pub mod semaphores;
pub mod command_buffers;
pub mod pipeline;
pub mod shared;
pub mod buffers;
pub mod images;

pub use semaphores::{SignalSemaphoreRef, WaitSemaphoreRef, WaitSemaphoreStagesRef};
use crate::runtime::buffers::{BufferResource, MappableBufferResource};
use crate::runtime::images::{ImageResource, ImageResourceHandle};
use crate::runtime::pipeline::GraphicsPipelineInner;
use crate::swapchain_wrapper::SwapchainWrapper;


pub struct RuntimeState {
    device: VkDeviceRef,
    shared_state: shared::SharedState,

    semaphore_manager: SemaphoreManager,
    command_buffer_manager: CommandBufferManager,
    next_submission_num: usize,
    queue: Queue,
    resource_storage: ResourceStorage,

    // memory management
    physical_device: PhysicalDevice,
    memory_types: Vec<MemoryType>,
    memory_heaps: Vec<MemoryHeap>,
    buffer_memory_requirements: HashMap<(BufferCreateFlags, BufferUsageFlags), (u64, u32)>,
    image_memory_requirements: HashMap<(Format, ImageTiling, ImageCreateFlags, ImageUsageFlags), u32>,

    // swapchain
    swapchain_wrapper: SwapchainWrapper,
    surface: VkSurfaceRef,
}

impl RuntimeState {
    pub fn new(
        device: VkDeviceRef,
        queue_family_index: u32,
        queue: Queue,
        physical_device: PhysicalDevice,
        memory_types: Vec<MemoryType>,
        memory_heaps: Vec<MemoryHeap>,
        swapchain_wrapper: SwapchainWrapper,
        surface: VkSurfaceRef,
    ) -> Self {
        let shared_state = SharedState::new(device.clone());
        let resource_storage = ResourceStorage::new(device.clone());
        Self {
            device: device.clone(),

            shared_state,
            semaphore_manager: SemaphoreManager::new(device.clone()),
            command_buffer_manager: CommandBufferManager::new(device, queue_family_index),
            next_submission_num: 1,
            queue,
            resource_storage,
            physical_device,
            memory_types,
            memory_heaps,
            buffer_memory_requirements: HashMap::new(),
            image_memory_requirements: HashMap::new(),
            swapchain_wrapper,
            surface,
        }
    }
     
    // Memory types and requirements methods
    fn get_buffer_memory_requirements(&mut self, usage: BufferUsageFlags, flags: BufferCreateFlags) -> (u64, u32) {
        let device_memory_type = self.buffer_memory_requirements
            .entry((flags, usage))
            .or_insert_with(|| {
                // create a dummy buffer to get memory requirements
                let buffer_create_info = vk::BufferCreateInfo::default()
                    .size(1)
                    .usage(usage)
                    .flags(flags)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE);

                let buffer = unsafe { self.device.create_buffer(&buffer_create_info, None) }.unwrap();
                let memory_requirements = unsafe { self.device.get_buffer_memory_requirements(buffer) };
                unsafe { self.device.destroy_buffer(buffer, None) };
                let alignment = memory_requirements.alignment;

                (alignment, memory_requirements.memory_type_bits)
            });

        *device_memory_type
    }

    fn get_image_memory_requirements(&mut self, format: Format, tiling: ImageTiling, usage: ImageUsageFlags, flags: ImageCreateFlags) -> u32 {
        let format = if is_color_format(format) {
            Format::UNDEFINED
        }
        else {
            format
        };

        let usage = usage & ImageUsageFlags::TRANSIENT_ATTACHMENT;
        let flags = flags & ImageCreateFlags::SPARSE_BINDING;

        let device_memory_type = self.image_memory_requirements
            .entry((format, tiling, flags, usage))
            .or_insert_with(|| {
                // create a dummy image to get memory requirements
                let image_create_info = vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(vk::Extent3D {
                        width: 1,
                        height: 1,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(tiling)
                    .usage(usage)
                    .flags(flags)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .initial_layout(vk::ImageLayout::UNDEFINED);

                let image = unsafe { self.device.create_image(&image_create_info, None) }.unwrap();
                let memory_requirements = unsafe { self.device.get_image_memory_requirements(image) };
                unsafe { self.device.destroy_image(image, None) };

                memory_requirements.memory_type_bits
            });

        *device_memory_type
    }

    fn best_host_type(&self, memory_type_bits: u32) -> u32 {
        self.memory_types
            .iter()
            .enumerate()
            .filter(|(i, memory_type)| {
                memory_type.property_flags.contains(vk::MemoryPropertyFlags::HOST_COHERENT) && (1u32 << i) & memory_type_bits != 0
            })
            .next()
            .expect("Guaranteed to support at least 1 host mappable memory type for buffer").0 as u32
    }

    fn best_device_type(&self, memory_type_bits: u32) -> u32 {
        self.memory_types
            .iter()
            .enumerate()
            .filter(|(i, memory_type)| {
                memory_type.property_flags.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL) && (1u32 << i) & memory_type_bits != 0
            })
            .max_by_key(|(_, mem)| {
                let only_1_flag = mem.property_flags == vk::MemoryPropertyFlags::DEVICE_LOCAL;
                let heap_size = self.memory_heaps[mem.heap_index as usize].size;

                heap_size + only_1_flag as u64
            })
            .expect("Guaranteed to support at least 1 device_local memory type for buffer").0 as u32
    }

    // Resource creation methods

    /// Create new buffer in mappable memory for TRANSFER_SRC usage
    pub fn new_host_buffer(&mut self, size: u64) -> MappableBufferResource {
        let flags = BufferCreateFlags::empty();
        let usage = BufferUsageFlags::TRANSFER_SRC;
        let (alignment, memory_types) = self.get_buffer_memory_requirements(usage, flags);
        let host_memory_type = self.best_host_type(memory_types);

        let (buffer, memory) = self.resource_storage.create_buffer(usage, flags, size, host_memory_type, self.shared_state.clone());
        MappableBufferResource::new(buffer, memory)
    }

    /// Create new buffer in device_local memory
    pub fn new_device_buffer(&mut self, usage: BufferUsageFlags, size: u64) -> BufferResource {
        let flags = BufferCreateFlags::empty();
        let (alignment, memory_types) = self.get_buffer_memory_requirements(usage, flags);
        let device_memory_type = self.best_device_type(memory_types);

        let (buffer, _) = self.resource_storage.create_buffer(usage, flags, size, device_memory_type, self.shared_state.clone());
        buffer
    }

    /// Create 2D image with optimal tiling, not mappable to host
    pub fn new_image(&mut self, format: Format, usage: ImageUsageFlags, samples: SampleCountFlags, width: u32, height: u32) -> ImageResource{
        let flags = ImageCreateFlags::empty();
        let memory_types = self.get_image_memory_requirements(format, ImageTiling::OPTIMAL, usage, flags);
        let device_memory_type = self.best_device_type(memory_types);

        // create image
        let image = unsafe {
            self.device.create_image(&ImageCreateInfo::default()
                .usage(usage)
                .flags(flags)
                .extent(Extent3D {
                    width,
                    height,
                    depth: 1
                })
                .tiling(ImageTiling::OPTIMAL)
                .array_layers(1)
                .mip_levels(1)
                .image_type(ImageType::TYPE_2D)
                .initial_layout(ImageLayout::UNDEFINED)
                .format(format)
                .samples(samples)
             , None).unwrap()
        };
        let memory_requirements = unsafe { self.device.get_image_memory_requirements(image) };
        let allocation_size = memory_requirements.size;

        //allocate memory
        let memory = unsafe {
            self.device.allocate_memory(&MemoryAllocateInfo::default()
                .allocation_size(allocation_size)
                .memory_type_index(device_memory_type),
                                        None).unwrap() };

        unsafe {
            self.device.bind_image_memory(image, memory, 0).unwrap();
        }

        let state_key = self.add_image(ImageInner {
            image,
            usages: ResourceUsages::new(),
            memory: Some(memory),
            layout: ImageLayout::UNDEFINED,
            format,
        });

        let image = ImageResource::new(self.shared_state.clone(), state_key, memory, width, height);
        image
    }

    // Swapchain methods

    pub(crate) fn update_swapchain_image_handles(&mut self) {
        let extent = self.swapchain_wrapper.get_extent();
        if let Some(old_handles) = self.swapchain_wrapper.try_get_images() {
            for image in old_handles {
                self.remove_image(image);
            }
        }
        let format = self.swapchain_wrapper.get_surface_format();
        let swapchain_images = self.swapchain_wrapper.swapchain_images.clone();
        let images = swapchain_images.iter().map(|i| {
            let image = ImageInner {
                image: *i,
                layout: ImageLayout::UNDEFINED,
                memory: None,
                usages: ResourceUsages::None,
                format
            };
            let key = self.add_image(image);
            ImageResourceHandle {
                state_key: key,
                width: extent.width,
                height: extent.height,
            }
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
        self.wait_idle();

        // 1. Destroy swapchain dependent resources
        // unsafe {
        //     self.render_pass_resources
        //         .destroy(&mut self.resource_manager);
        // }

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

        // // 3. Recreate swapchain_dependent resources
        // self.render_pass_resources = self.render_pass.create_render_pass_resources(
        //     self.swapchain_wrapper.get_image_views(),
        //     self.swapchain_wrapper.get_extent(),
        //     &mut self.resource_manager,
        // );
    }

    pub fn swapchain_images(&self) -> SmallVec<[ImageResourceHandle; 3]> {
        self.swapchain_wrapper.get_images()
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
    fn add_image(&mut self, image: ImageInner) -> DefaultKey {
        self.resource_storage.add_image(image)
    }

    fn add_pipeline(&mut self, pipeline: GraphicsPipelineInner) -> DefaultKey {
        self.resource_storage.add_pipeline(pipeline)
    }

    fn remove_image(&mut self, image: ImageResourceHandle) {
        self.resource_storage.destroy_image(image.state_key);
    }

    pub(crate) fn destroy_image(&mut self, image: ImageResourceHandle) {
        self.shared_state.schedule_destroy_image(image);
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

