use std::mem;
use std::sync::Arc;
use ash::vk;
use ash::khr::swapchain;
use ash::vk::{Extent2D, Format, Image, ImageAspectFlags, ImageTiling, ImageUsageFlags, ImageView, PhysicalDevice, PresentModeKHR, SampleCountFlags, SwapchainKHR};
use log::info;
use smallvec::SmallVec;
use sparkles::range_event_start;
use crate::resources::image::{destroy_image_resource, ImageResource};
use crate::wrappers::device::VkDeviceRef;
use crate::wrappers::image::{image_2d_info, imageview_info_for_image, swapchain_info};
use crate::wrappers::surface::VkSurfaceRef;

pub struct SwapchainWrapper {
    device: VkDeviceRef,
    
    swapchain: SwapchainKHR,
    pub swapchain_loader: swapchain::Device,
    swapchain_images: SmallVec<[Arc<ImageResource>; 4]>,
    swapchain_format: Format,
    pub swapchain_extent: Extent2D,

    surface: VkSurfaceRef
}
pub type SwapchainImages = SmallVec<[Arc<ImageResource>; 4]>;

impl SwapchainWrapper {
    pub(crate) fn surface(&self) -> VkSurfaceRef {
        self.surface.clone()
    }

    pub fn new(device: VkDeviceRef, physical_device: PhysicalDevice,
               extent: Extent2D, surface_ref: VkSurfaceRef, old_swapchain: Option<SwapchainKHR>) -> anyhow::Result<SwapchainWrapper> {
        let g = range_event_start!("[Vulkan] Init swapchain");

        let surface_loader = surface_ref.loader();
        let surface = surface_ref.surface();

        // TODO: replace with wrapper methods
        let surface_capabilities = unsafe { surface_loader.get_physical_device_surface_capabilities(physical_device, *surface)? };
        let surface_formats = unsafe { surface_loader.get_physical_device_surface_formats(physical_device, *surface)? };
        let surface_present_modes = unsafe { surface_loader.get_physical_device_surface_present_modes(physical_device, *surface)? };

        //prefer B8G8R8A8_UNORM and SRGB_NONLINEAR
        let surface_format = surface_formats.iter().find(|f| {
            f.format == vk::Format::B8G8R8A8_UNORM && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        }).unwrap_or_else(|| {
            surface_formats.first().unwrap()
        });
        //prefer MAILBOX then IMMEDIATE or default FIFO
        let present_mode = surface_present_modes.iter().find(|m| {
            **m == vk::PresentModeKHR::MAILBOX
        }).unwrap_or_else(|| {
            surface_present_modes.iter().find(|m| {
                **m == vk::PresentModeKHR::IMMEDIATE
            }).unwrap_or_else(|| {
                surface_present_modes.first().unwrap()
            })
        });
        // let present_mode = &PresentModeKHR::FIFO;

        // 1 additional image, so we can acquire 2 images at a time.
        let image_count = surface_capabilities.min_image_count + 1;
        info!("\n\tCreating swapchain...\n\tPresent mode: {:?}\n\tSwapchain image count: {:?}, Color space: {:?}, Image format: {:?}", present_mode, image_count, surface_format.color_space, surface_format.format);

        let swapchain_extent = if surface_capabilities.current_extent.width != u32::MAX {
            surface_capabilities.current_extent
        } else {
            let mut actual_extent = extent;
            actual_extent.width = actual_extent.width.max(surface_capabilities.min_image_extent.width).min(surface_capabilities.max_image_extent.width);
            actual_extent.height = actual_extent.height.max(surface_capabilities.min_image_extent.height).min(surface_capabilities.max_image_extent.height);
            actual_extent
        };


        let swapchain_loader = swapchain::Device::new(device.instance(), &device);
        let swapchain_image_info = image_2d_info(surface_format.format, ImageUsageFlags::COLOR_ATTACHMENT | ImageUsageFlags::TRANSFER_DST,
                                             swapchain_extent, SampleCountFlags::TYPE_1, ImageTiling::OPTIMAL);
        let swapchain_create_info = swapchain_info(swapchain_image_info, surface_format.color_space)
            .surface(*surface)
            .min_image_count(image_count)
            .pre_transform(surface_capabilities.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(*present_mode)
            .clipped(true);

        // add old swapchain
        let swapchain_create_info = if let Some(old_swapchain) = old_swapchain {
            swapchain_create_info.old_swapchain(old_swapchain)
        }
        else {
            swapchain_create_info
        };

        let swapchain = unsafe { swapchain_loader.create_swapchain(&swapchain_create_info, None)? };
        let swapchain_images = unsafe { swapchain_loader.get_swapchain_images(swapchain)? };

        let swapchain_images = swapchain_images.into_iter()
            .map(|i| {
                Arc::new(ImageResource::from_image(&device, i, surface_format.format, swapchain_extent.width, swapchain_extent.height))
            })
            .collect();

        Ok(SwapchainWrapper {
            swapchain,
            swapchain_loader,
            swapchain_images,
            swapchain_format: surface_format.format,
            swapchain_extent,

            device,
            surface: surface_ref
        })
    }

    pub fn get_swapchain(&self) -> SwapchainKHR {
        self.swapchain
    }
    pub fn get_surface_format(&self) -> Format {
        self.swapchain_format
    }

    pub fn get_extent(&self) -> Extent2D {
        self.swapchain_extent
    }
    pub fn get_images(&self) -> &SwapchainImages {
        &self.swapchain_images
    }

    /// # Safety
    /// Image views should not be used. Swapchain should not be used.
    /// Returns the old swapchain images that need to be destroyed later
    pub unsafe fn recreate(&mut self, physical_device: PhysicalDevice,
                           extent: Extent2D, surface: VkSurfaceRef) -> anyhow::Result<SwapchainImages> {

        let swapchain = self.swapchain;
        let old_images = mem::take(&mut self.swapchain_images);
        *self = Self::new(self.device.clone(), physical_device, extent, surface, Some(swapchain))?;
        Ok(old_images)
    }
}

impl Drop for SwapchainWrapper {
    fn drop(&mut self) {
        let g = range_event_start!("[Vulkan] Destroy swapchain");
        unsafe { self.swapchain_loader.destroy_swapchain(self.swapchain, None); }
    }
}