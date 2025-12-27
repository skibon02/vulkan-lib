use std::sync::Arc;
use ash::vk;
use ash::vk::{Extent3D, Format, ImageCreateFlags, ImageCreateInfo, ImageLayout, ImageTiling, ImageType, ImageUsageFlags, ImageView, MemoryAllocateInfo, SampleCountFlags};
use slotmap::DefaultKey;
use crate::queue::queue_local::QueueLocal;
use crate::queue::memory_manager::{MemoryManager, MemoryTypeAlgorithm};
use crate::queue::OptionSeqNumShared;
use crate::resources::{LastResourceUsage, ResourceUsage};
use crate::wrappers::device::VkDeviceRef;

pub struct ImageResource {
    pub(crate) image: vk::Image,
    memory: Option<vk::DeviceMemory>,
    pub(crate) image_view: vk::ImageView,
    format: vk::Format,
    extent: vk::Extent2D,
    pub(crate) submission_usage: OptionSeqNumShared,
    pub(crate)inner: QueueLocal<ImageResourceInner>,

    dropped: bool,
}

pub(crate) struct ImageResourceInner {
    pub usages: LastResourceUsage,
    pub layout: vk::ImageLayout,
}

impl ImageResource {
    pub(crate) fn new(device: &VkDeviceRef, memory_manager: &mut MemoryManager, usage: ImageUsageFlags, flags: ImageCreateFlags,
                      width: u32, height: u32, format: Format, samples: SampleCountFlags) -> Self {
        let memory_type_bits = memory_manager.get_image_memory_requirements(format, ImageTiling::OPTIMAL, usage, flags);
        let memory_type = memory_manager.select_memory_type(memory_type_bits, MemoryTypeAlgorithm::Device);

        // create image
        let image = unsafe {
            device.create_image(&ImageCreateInfo::default()
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
                .samples(samples),
                                     None).unwrap()
        };
        let memory_requirements = unsafe { device.get_image_memory_requirements(image) };
        let allocation_size = memory_requirements.size;

        //allocate memory
        let memory = unsafe {
            device.allocate_memory(&MemoryAllocateInfo::default()
                .allocation_size(allocation_size)
                .memory_type_index(memory_type),
                                        None).unwrap() };

        unsafe {
            device.bind_image_memory(image, memory, 0).unwrap();
        }
        
        let image_view_create_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format)
            .subresource_range(vk::ImageSubresourceRange::default()
                .aspect_mask(format_aspect_flags(format))
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1));
        
        let image_view = unsafe {
            device.create_image_view(&image_view_create_info, None).unwrap()
        };


        Self {
            image,
            memory: Some(memory),
            image_view,
            format,
            extent: vk::Extent2D { width, height },
            submission_usage: OptionSeqNumShared::default(),
            inner: QueueLocal::new(ImageResourceInner {
                usages: LastResourceUsage::None,
                layout: ImageLayout::UNDEFINED,
            }),

            dropped: false,
        }
    }

    pub(crate) fn from_image(device: &VkDeviceRef, image: vk::Image, format: vk::Format, width: u32, height: u32) -> ImageResource {
        let image_view_create_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format)
            .subresource_range(vk::ImageSubresourceRange::default()
                .aspect_mask(format_aspect_flags(format))
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1));

        let image_view = unsafe {
            device.create_image_view(&image_view_create_info, None).unwrap()
        };


        Self {
            image,
            memory: None,
            image_view,
            format,
            extent: vk::Extent2D { width, height },
            submission_usage: OptionSeqNumShared::default(),
            inner: QueueLocal::new(ImageResourceInner {
                usages: LastResourceUsage::None,
                layout: ImageLayout::UNDEFINED,
            }),

            dropped: false,
        }
    }

    pub fn extent(&self) -> vk::Extent2D {
        self.extent
    }

    pub fn get_aspect_flags(&self) -> vk::ImageAspectFlags {
        format_aspect_flags(self.format)
    }
}

pub fn format_aspect_flags(format: Format) -> vk::ImageAspectFlags {
    match format {
        Format::D16_UNORM | Format::D32_SFLOAT => vk::ImageAspectFlags::DEPTH,
        Format::S8_UINT => vk::ImageAspectFlags::STENCIL,
        Format::D24_UNORM_S8_UINT | Format::D32_SFLOAT_S8_UINT => vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL,
        _ => vk::ImageAspectFlags::COLOR,
    }
}

impl Drop for ImageResource {
    fn drop(&mut self) {
        if !self.dropped {
            log::error!("ImageResource was not destroyed before dropping!");
        }
    }
}

pub(crate) fn destroy_image_resource(device: &VkDeviceRef, mut image_resource: ImageResource) {
    if !image_resource.dropped {
        unsafe {
            device.destroy_image_view(image_resource.image_view, None);
            if let Some(mem) = image_resource.memory {
                device.destroy_image(image_resource.image, None);
                device.free_memory(mem, None);
            }
            image_resource.dropped = true;
        }
    }
}