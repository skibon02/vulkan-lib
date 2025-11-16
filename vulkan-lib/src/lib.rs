use std::collections::HashMap;
use std::ffi::{c_char, CString};
use ash::vk;
use ash::vk::{make_api_version, ApplicationInfo, BufferCreateFlags, BufferCreateInfo, Extent2D, Format, ImageCreateFlags, ImageCreateInfo, ImageTiling, ImageType, ImageUsageFlags, MemoryAllocateInfo, MemoryHeap, MemoryRequirements, MemoryType, PhysicalDevice, Queue, SampleCountFlags};
use log::{debug, info, warn};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use smallvec::SmallVec;
use sparkles::range_event_start;
use runtime::resources::{BufferResource, ResourceStorage};
use crate::swapchain_wrapper::SwapchainWrapper;
use crate::wrappers::capabilities_checker::CapabilitiesChecker;
use crate::wrappers::debug_report::VkDebugReport;
use crate::wrappers::device::VkDeviceRef;
use crate::wrappers::surface::{VkSurface, VkSurfaceRef};
use crate::runtime::{LocalState, WaitSemaphoreRef};
use crate::runtime::resources::{BufferInner, ImageInner, ImageResource, ImageResourceHandle, MappableBufferResource, ResourceUsages};
use crate::util::image::is_color_format;

pub use vk::BufferUsageFlags;
pub use vk::PipelineStageFlags;
pub use vk::BufferCopy;
pub use vk::BufferImageCopy;
pub use vk::Extent3D;
pub use vk::Offset3D;
pub use vk::ImageLayout;
pub use vk::ImageAspectFlags;
pub use vk::ImageSubresourceLayers;
pub use vk::ClearColorValue;

pub mod instance;
mod wrappers;
mod swapchain_wrapper;
mod pipeline;
mod descriptor_sets;
pub mod util;
pub mod shaders;
pub mod runtime;

pub struct VulkanRenderer {
    debug_report: VkDebugReport,
    surface: VkSurfaceRef,
    physical_device: PhysicalDevice,
    device: VkDeviceRef,

    swapchain_wrapper: SwapchainWrapper,
    acq_semaphore: vk::Semaphore,

    // memory resources
    memory_types: Vec<MemoryType>,
    memory_heaps: Vec<MemoryHeap>,
    buffer_memory_requirements: HashMap<(BufferCreateFlags, BufferUsageFlags), (u64, u32)>,
    image_memory_requirements: HashMap<(Format, ImageTiling, ImageCreateFlags, ImageUsageFlags), u32>,

    // runtime state
    runtime_state: LocalState,

    // extensions
}

impl VulkanRenderer {
    pub fn new_for_window(window_handle: RawWindowHandle, display_handle: RawDisplayHandle, window_size: (u32, u32)) -> anyhow::Result<Self> {
        let g = range_event_start!("[Vulkan] INIT");
        info!(
            "Vulkan init started! Initializing for size: {:?}",
            window_size
        );

        let app_name = CString::new("Hello Vulkan")?;

        let app_info = ApplicationInfo::default()
            .application_name(&app_name)
            .application_version(make_api_version(0, 1, 0, 0))
            .engine_name(&app_name)
            .engine_version(make_api_version(0, 1, 0, 0))
            .api_version(vk::API_VERSION_1_0);

        //define desired layers
        // 1. Khronos validation layers (optional)
        let mut instance_layers = vec![];
        if cfg!(feature = "validation") {
            instance_layers.push(CString::new("VK_LAYER_KHRONOS_validation")?);
        }
        let mut instance_layers_refs: Vec<*const c_char> =
            instance_layers.iter().map(|l| l.as_ptr()).collect();

        //define desired extensions
        // 1 Debug report
        // 2,3 Required extensions for surface support (platform_specific surface + general surface)
        // 4 Portability enumeration (for moltenvk)
        let surface_required_extensions =
            ash_window::enumerate_required_extensions(display_handle)?;
        let mut instance_extensions: Vec<*const c_char> = surface_required_extensions.to_vec();
        instance_extensions.push(ash::ext::debug_report::NAME.as_ptr());

        let mut debug_report_callback_info = VkDebugReport::get_messenger_create_info();

        let mut caps_checker = CapabilitiesChecker::new();

        // caps_checker will check requested layers and extensions and enable only the
        // supported ones, which can be requested later
        let instance = caps_checker.create_instance(&app_info, &mut instance_layers_refs,
                                                    &mut instance_extensions, &mut debug_report_callback_info)?;

        let surface = VkSurface::new(instance.clone(), display_handle, window_handle)?;

        let debug_report = VkDebugReport::new(instance.clone())?;
        // instance is created. debug report ready

        let physical_devices = unsafe { instance.enumerate_physical_devices()? };

        let physical_device = *physical_devices
            .iter()
            .find(|&d| {
                let properties = unsafe { instance.get_physical_device_properties(*d) };
                properties.device_type == vk::PhysicalDeviceType::DISCRETE_GPU
            })
            .or_else(|| {
                warn!("Discrete GPU was not found!");
                physical_devices.iter().find(|&d| {
                    let properties = unsafe { instance.get_physical_device_properties(*d) };
                    properties.device_type == vk::PhysicalDeviceType::INTEGRATED_GPU
                })
            })
            .or_else(|| {
                warn!("Integrated GPU was not found!");
                physical_devices.iter().find(|&d| {
                    let properties = unsafe { instance.get_physical_device_properties(*d) };
                    properties.device_type == vk::PhysicalDeviceType::CPU
                })
            })
            .unwrap_or_else(|| {
                panic!("No avaliable physical device found");
            });

        //select chosen physical device
        let dev_name_array = unsafe {
            instance
                .get_physical_device_properties(physical_device)
                .device_name
        };
        let dev_name = unsafe { std::ffi::CStr::from_ptr(dev_name_array.as_ptr()) };
        info!("Chosen device: {}", dev_name.to_str().unwrap());

        let queue_family_properties =
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
        let queue_family_index = queue_family_properties
            .iter()
            .enumerate()
            .find(|(_, p)| {
                let support_graphics = p.queue_flags.contains(vk::QueueFlags::GRAPHICS);
                let support_presentation = surface.query_presentation_support(physical_device);

                support_graphics && support_presentation
            })
            .map(|(i, _)| i as u32)
            .unwrap_or_else(|| {
                panic!("No available queue family found");
            });

        let device_extensions = vec![ash::khr::swapchain::NAME.as_ptr(), ash::ext::calibrated_timestamps::NAME.as_ptr()];

        let queue_create_infos = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&[1.0])];
        let mut device_create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_create_infos)
            .enabled_extension_names(&device_extensions);

        let device = caps_checker.create_device(
            instance.clone(),
            physical_device,
            &mut device_create_info,
        )?;

        let device_properties = unsafe { instance.get_physical_device_properties(physical_device) };
        let device_limits = device_properties.limits;

        let queue = unsafe { device.get_device_queue(queue_family_index, 0) };


        let extent = Extent2D {
            width: window_size.0,
            height: window_size.1,
        };
        let swapchain_wrapper = SwapchainWrapper::new(
            device.clone(),
            physical_device,
            extent,
            surface.clone(),
            None,
        )?;

        let acq_semaphore = unsafe {
            device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None).unwrap()
        };

        let memory_properties = unsafe {
            device
                .instance()
                .get_physical_device_memory_properties(physical_device)
        };

        let memory_heaps = memory_properties.memory_heaps_as_slice().to_vec();

        let memory_types = memory_properties.memory_types_as_slice().to_vec();

        let resource_storage = ResourceStorage::new(device.clone());
        let runtime_state = LocalState::new(device.clone(), queue_family_index, queue, resource_storage);

        let mut res = Self {
            device,
            debug_report,
            surface,
            physical_device,
            swapchain_wrapper,
            acq_semaphore,

            memory_heaps,
            memory_types,
            buffer_memory_requirements: HashMap::new(),
            image_memory_requirements: HashMap::new(),

            runtime_state,
        };

        res.update_swapchain_image_handles();

        Ok(res)
    }

    pub fn test_buffer_sizes(&mut self, usage: vk::BufferUsageFlags) {
        info!("Test buffer sizes for usage {:?}", usage);

        let buffer = unsafe {
            self.device.create_buffer(&BufferCreateInfo::default()
                .usage(usage)
                .size(256), None).unwrap()
        };

        let memory_requirements = unsafe { self.device.get_buffer_memory_requirements(buffer) };

        let alignment = memory_requirements.alignment;
        let memory_types = memory_requirements.memory_type_bits;

        // if memory_requirements.size != i {
        //     info!("{} -> {}", i, memory_requirements.size);
        // }

        unsafe {
            self.device.destroy_buffer(buffer, None);
        }
        info!("Alignment: {}. Memory types: {:b}", alignment, memory_types);
    }

    fn update_swapchain_image_handles(&mut self) {
        let extent = self.swapchain_wrapper.get_extent();
        if let Some(old_handles) = self.swapchain_wrapper.try_get_images() {
            for image in old_handles {
                self.runtime_state.remove_image(image);
            }
        }
        let format = self.swapchain_wrapper.get_surface_format();
        let images = self.swapchain_wrapper.swapchain_images.iter().map(|i| {
            let image = ImageInner {
                image: *i,
                layout: ImageLayout::UNDEFINED,
                memory: None,
                usages: ResourceUsages::None,
                format
            };
            let key = self.runtime_state.add_image(image);
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
        unsafe {
            self.swapchain_wrapper
                .recreate(self.physical_device, new_extent, self.surface.clone())
                .unwrap()
        };
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

    fn wait_idle(&mut self) {
        let start = std::time::Instant::now();
        self.runtime_state.wait_idle();
        let end = std::time::Instant::now();
        debug!("Waited for idle for {:?}", end - start);
    }

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
                    .extent(Extent3D {
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

    /// Create new buffer in mappable memory for TRANSFER_SRC usage
    pub fn new_host_buffer(&mut self, size: u64) -> MappableBufferResource {
        let flags = BufferCreateFlags::empty();
        let usage = BufferUsageFlags::TRANSFER_SRC;
        let (alignment, memory_types) = self.get_buffer_memory_requirements(usage, flags);
        let host_memory_type = self.best_host_type(memory_types);

        // create buffer
        let buffer = unsafe {
            self.device.create_buffer(&BufferCreateInfo::default()
                .usage(usage)
                .flags(flags)
                .size(size), None).unwrap()
        };
        let memory_requirements = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let allocation_size = memory_requirements.size;

        //allocate memory
        let memory = unsafe {
            self.device.allocate_memory(&MemoryAllocateInfo::default()
                .allocation_size(allocation_size)
                .memory_type_index(host_memory_type),
        None).unwrap() };
        
        unsafe {
            self.device.bind_buffer_memory(buffer, memory, 0).unwrap();
        }


        let state_key = self.runtime_state.add_buffer(BufferInner {
            buffer,
            usages: ResourceUsages::new(),
            memory,
        });

        let buffer = BufferResource::new(self.runtime_state.shared(), state_key, memory, size);
        MappableBufferResource::new(buffer, memory)
    }

    /// Create new buffer in device_local memory
    pub fn new_device_buffer(&mut self, usage: BufferUsageFlags, size: u64) -> BufferResource {
        let flags = BufferCreateFlags::empty();
        let (alignment, memory_types) = self.get_buffer_memory_requirements(usage, flags);
        let device_memory_type = self.best_device_type(memory_types);

        // create buffer
        let buffer = unsafe {
            self.device.create_buffer(&BufferCreateInfo::default()
                .usage(usage)
                .flags(flags)
                .size(size), None).unwrap()
        };
        let memory_requirements = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let allocation_size = memory_requirements.size;

        //allocate memory
        let memory = unsafe {
            self.device.allocate_memory(&MemoryAllocateInfo::default()
                .allocation_size(allocation_size)
                .memory_type_index(device_memory_type),
                                        None).unwrap() };
        
        unsafe {
            self.device.bind_buffer_memory(buffer, memory, 0).unwrap();
        }

        let state_key = self.runtime_state.add_buffer(BufferInner {
            buffer,
            usages: ResourceUsages::new(),
            memory,
        });

        let buffer = BufferResource::new(self.runtime_state.shared(), state_key, memory, size);
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

        let state_key = self.runtime_state.add_image(ImageInner {
            image,
            usages: ResourceUsages::new(),
            memory: Some(memory),
            layout: ImageLayout::UNDEFINED,
            format,
        });

        let image = ImageResource::new(self.runtime_state.shared(), state_key, memory, width, height);
        image
    }

    pub fn runtime_state(&mut self) -> &mut LocalState {
        &mut self.runtime_state
    }

    pub fn acquire_next_image(&mut self) -> anyhow::Result<(u32, WaitSemaphoreRef, bool)> {
        self.runtime_state.acquire_next_image(&mut self.swapchain_wrapper)
    }

    pub fn queue_present(&mut self, image_index: u32, semaphore: WaitSemaphoreRef) -> anyhow::Result<bool> {
        self.runtime_state.queue_present(image_index, semaphore, &mut self.swapchain_wrapper)
    }
}

impl Drop for VulkanRenderer {
    fn drop(&mut self) {
        // Called before everything is dropped
        info!("vulkan: drop");
        self.wait_idle();

        unsafe {
            self.device.destroy_semaphore(self.acq_semaphore, None);
        }
    }
}
