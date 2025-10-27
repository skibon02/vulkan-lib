
use ash::{vk};
use ash::vk::{AccessFlags, AttachmentLoadOp, Extent2D, Format, Framebuffer, ImageAspectFlags, ImageTiling, ImageUsageFlags, ImageView, PipelineBindPoint, PipelineStageFlags, RenderPass, SampleCountFlags};
use sparkles::range_event_start;
use crate::wrappers::device::VkDeviceRef;

// this one depends on swapchain
pub struct RenderPassResources {
    device: VkDeviceRef,
    pub framebuffers: Vec<Framebuffer>,

    pub swapchain_image_set: Vec<SwapchainImageSet>,
}

pub enum SwapchainImageSet {
    NoMSAA {
        depth_image: ImageResource,
        depth_imageview: ImageView,
    },
    WithMSAA {
        depth_image: ImageResource,
        depth_imageview: ImageView,
        color_image: ImageResource,
        color_imageview: ImageView,
    }
}

impl RenderPassResources {
    pub unsafe fn destroy(&mut self, resource_manager: &mut ResourceManager) {
        // framebuffers
        for framebuffer in self.framebuffers.drain(..) {
            unsafe { self.device.destroy_framebuffer(framebuffer, None); }
        }

        for image_set in self.swapchain_image_set.drain(..) {
            match image_set {
                SwapchainImageSet::NoMSAA { depth_image, depth_imageview} => {
                    unsafe { self.device.destroy_image_view(depth_imageview, None)};
                    resource_manager.destroy_image(depth_image);
                },
                SwapchainImageSet::WithMSAA { color_imageview, color_image,
                    depth_image, depth_imageview} => {
                    unsafe { self.device.destroy_image_view(depth_imageview, None)};
                    resource_manager.destroy_image(depth_image);


                    unsafe { self.device.destroy_image_view(color_imageview, None)};
                    resource_manager.destroy_image(color_image);
                }
            }
        }
    }
}

pub struct RenderPassWrapper {
    device: VkDeviceRef,
    render_pass: RenderPass,

    msaa_samples: Option<SampleCountFlags>,
    surface_format: Format
}

impl RenderPassWrapper {
    pub fn new(device: VkDeviceRef, surface_format: Format, msaa_samples: Option<SampleCountFlags>) -> Self {
        let g = range_event_start!("Create render pass");

        let intermediate_sample_count = msaa_samples.unwrap_or(SampleCountFlags::TYPE_1);
        let render_pass = {

            let load_op = if msaa_samples.is_some() {
                AttachmentLoadOp::DONT_CARE
            } else {
                AttachmentLoadOp::CLEAR
            };
            let attachments = [
                // 0. final color attachment (resolve attachment)
                vk::AttachmentDescription::default()
                    .format(surface_format)
                    .samples(SampleCountFlags::TYPE_1)
                    .load_op(load_op)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .initial_layout(vk::ImageLayout::UNDEFINED)
                    .final_layout(vk::ImageLayout::PRESENT_SRC_KHR),

                // 1. depth attachment
                vk::AttachmentDescription::default()
                    .format(Format::D16_UNORM)
                    .samples(intermediate_sample_count)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .initial_layout(vk::ImageLayout::UNDEFINED)
                    .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),

                // 2. Color attachment
                vk::AttachmentDescription::default()
                    .format(surface_format)
                    .samples(intermediate_sample_count)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .initial_layout(vk::ImageLayout::UNDEFINED)
                    .final_layout(vk::ImageLayout::PRESENT_SRC_KHR),
            ];

            let resolve_attachment_i = 0;
            let color_attachment_i = if msaa_samples.is_some() {
                2
            }
            else {
                0
            };

            let color_attachment_refs = [vk::AttachmentReference::default()
                .attachment(color_attachment_i)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
            let depth_attachment_ref = vk::AttachmentReference::default()
                .attachment(1)
                .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
            let resolve_attachment_ref = [vk::AttachmentReference::default()
                .attachment(resolve_attachment_i)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];

            let mut subpasses = [vk::SubpassDescription::default()
                .pipeline_bind_point(PipelineBindPoint::GRAPHICS)
                .color_attachments(&color_attachment_refs)
                .depth_stencil_attachment(&depth_attachment_ref)];
            if msaa_samples.is_some() {
                subpasses[0] = subpasses[0].resolve_attachments(&resolve_attachment_ref);
            }
            let dependencies = [vk::SubpassDependency::default()
                .src_subpass(vk::SUBPASS_EXTERNAL)
                .dst_subpass(0)
                .src_stage_mask(PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | PipelineStageFlags::EARLY_FRAGMENT_TESTS)
                .src_access_mask(AccessFlags::empty())
                .dst_stage_mask(PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | PipelineStageFlags::EARLY_FRAGMENT_TESTS)
                .dst_access_mask(AccessFlags::COLOR_ATTACHMENT_WRITE | AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)];

            let render_pass_create_info =
                vk::RenderPassCreateInfo::default()
                    .subpasses(&subpasses)
                    .dependencies(&dependencies);
            if msaa_samples.is_some() {
                let render_pass_create_info = render_pass_create_info.attachments(&attachments);
                unsafe { device.create_render_pass(&render_pass_create_info, None).unwrap() }
            }
            else {
                let render_pass_create_info = render_pass_create_info.attachments(&attachments[..2]);
                unsafe { device.create_render_pass(&render_pass_create_info, None).unwrap() }
            }

        };

        Self {
            device,

            render_pass,

            msaa_samples,
            surface_format,
        }
    }

    pub fn get_render_pass(&self) -> &RenderPass {
        &self.render_pass
    }
    pub fn get_msaa_samples(&self) -> Option<SampleCountFlags> {
        self.msaa_samples
    }

    pub fn create_render_pass_resources(&self, image_views: Vec<ImageView>, extent: Extent2D,
                    resource_manager: &mut ResourceManager) -> RenderPassResources {
        let g = range_event_start!("[Vulkan] Create renderpass resources");
        let swapchain_image_cnt = image_views.len();


        let mut swapchain_image_set = Vec::with_capacity(swapchain_image_cnt);
        for _ in 0..swapchain_image_cnt {
            let msaa_samples = self.msaa_samples.unwrap_or(SampleCountFlags::TYPE_1);
            let depth_image =
                resource_manager.create_image(extent, Format::D16_UNORM, ImageTiling::OPTIMAL,
                                              ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT, msaa_samples);
            let img_info = depth_image.info;
            let info = imageview_info_for_image(depth_image.image, img_info, ImageAspectFlags::DEPTH);
            let depth_imageview = unsafe { self.device.create_image_view(&info, None).unwrap() };

            if self.msaa_samples.is_some() {
                let color_image =
                    resource_manager.create_image(extent, self.surface_format, ImageTiling::OPTIMAL,
                                                  ImageUsageFlags::COLOR_ATTACHMENT, msaa_samples);
                let img_info = color_image.info;
                let info = imageview_info_for_image(color_image.image, img_info, ImageAspectFlags::COLOR);
                let color_imageview = unsafe { self.device.create_image_view(&info, None).unwrap() };

                swapchain_image_set.push(SwapchainImageSet::WithMSAA {depth_image, depth_imageview, color_image, color_imageview});
            }
            else {
                swapchain_image_set.push(SwapchainImageSet::NoMSAA {depth_image, depth_imageview});
            }
        }

        let framebuffers = swapchain_image_set.iter()
            .zip(image_views.iter())
            .map(|(image_set, resolve_imageview)| {

                let framebuffer_create_info = vk::FramebufferCreateInfo::default()
                    .render_pass(self.render_pass)
                    .width(extent.width)
                    .height(extent.height)
                    .layers(1);

                match image_set {
                    SwapchainImageSet::NoMSAA { depth_image, depth_imageview } => {
                        let attachments = [*resolve_imageview, *depth_imageview];
                        let framebuffer_create_info = framebuffer_create_info.attachments(&attachments);
                        unsafe { self.device.create_framebuffer(&framebuffer_create_info, None).unwrap() }
                    },
                    SwapchainImageSet::WithMSAA { depth_image, depth_imageview, color_image, color_imageview } => {
                        let attachments = [*resolve_imageview, *depth_imageview, *color_imageview];
                        let framebuffer_create_info = framebuffer_create_info.attachments(&attachments);
                        unsafe { self.device.create_framebuffer(&framebuffer_create_info, None).unwrap() }
                    }
                }
        }).collect::<Vec<_>>();

        RenderPassResources {
            device: self.device.clone(),
            swapchain_image_set,
            framebuffers,
        }
    }
}

impl Drop for RenderPassWrapper {
    fn drop(&mut self) {
        let g = range_event_start!("[Vulkan] Destroy render pass");
        //render pass
        unsafe { self.device.destroy_render_pass(self.render_pass, None); }
    }
}