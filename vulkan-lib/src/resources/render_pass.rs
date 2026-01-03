use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use ash::vk;
use ash::vk::{AccessFlags, AttachmentDescription, AttachmentLoadOp, AttachmentStoreOp, Format, ImageLayout, PipelineBindPoint, PipelineStageFlags, ImageUsageFlags, ImageCreateFlags, SampleCountFlags};
use log::{error, warn};
use smallvec::{smallvec, SmallVec};
use sparkles::range_event_start;
use crate::try_get_instance;
use crate::queue::OptionSeqNumShared;
use crate::queue::memory_manager::MemoryManager;
use crate::resources::image::ImageResource;
use crate::swapchain_wrapper::SwapchainImages;
use crate::wrappers::device::VkDeviceRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentSlot {
    Swapchain,
    Depth,
    ColorMSAA,
}

#[derive(Clone)]
pub enum FrameBufferAttachment {
    SwapchainImage(usize),
    Image(Arc<ImageResource>)
}
pub struct RenderPassResource {
    pub(crate) render_pass: vk::RenderPass,
    attachments_description: AttachmentsDescription,
    pub(crate) submission_usage: OptionSeqNumShared,
    framebuffer_registered: AtomicBool,

    dropped: AtomicBool,
}

impl RenderPassResource {
    pub(crate) fn new(
        device: &VkDeviceRef,
        mut attachments_description: AttachmentsDescription,
        swapchain_format: vk::Format,
    ) -> Self {
        let g = range_event_start!("Create render pass");

        attachments_description.fill_defaults(swapchain_format);

        // Build Vulkan attachment descriptions for render pass
        let mut vk_attachments: SmallVec<[AttachmentDescription; 5]> = smallvec![attachments_description.swapchain_attachment_desc];
        let mut vk_attachment_i = 1;
        let mut subpass = vk::SubpassDescription::default()
            .pipeline_bind_point(PipelineBindPoint::GRAPHICS);

        let depth_attachment_ref;
        if let Some(attachment) = attachments_description.depth_attachment_desc {
            vk_attachments.push(attachment);
            depth_attachment_ref = vk::AttachmentReference::default()
                .attachment(vk_attachment_i)
                .layout(ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
            subpass = subpass.depth_stencil_attachment(&depth_attachment_ref);
            vk_attachment_i += 1;
        }

        let color_attachment_refs;
        let resolve_attachment_refs;
        if let Some(attachment) = attachments_description.color_attachement_desc {
            vk_attachments.push(attachment);
            color_attachment_refs = [vk::AttachmentReference::default()
                .attachment(vk_attachment_i)
                .layout(ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];

            // attachment 0 is treated as resolve attachment
            resolve_attachment_refs = [vk::AttachmentReference::default()
                .attachment(0)
                .layout(ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];

            subpass = subpass.resolve_attachments(&resolve_attachment_refs);
            subpass = subpass.color_attachments(&color_attachment_refs);
        }
        else {
            // attachment 0 is treated as color attachment
            color_attachment_refs = [vk::AttachmentReference::default()
                .attachment(0)
                .layout(ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];

            subpass = subpass.color_attachments(&color_attachment_refs);
        }

        let dependencies = [vk::SubpassDependency::default()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | PipelineStageFlags::EARLY_FRAGMENT_TESTS)
            .src_access_mask(AccessFlags::empty())
            .dst_stage_mask(PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | PipelineStageFlags::EARLY_FRAGMENT_TESTS)
            .dst_access_mask(AccessFlags::COLOR_ATTACHMENT_WRITE | AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)];

        let subpasses = [subpass];
        let render_pass_create_info =
            vk::RenderPassCreateInfo::default()
                .subpasses(&subpasses)
                .dependencies(&dependencies)
                .attachments(&vk_attachments);
        let render_pass = unsafe { device.create_render_pass(&render_pass_create_info, None).unwrap() };

        Self {
            render_pass,
            attachments_description,
            submission_usage: OptionSeqNumShared::default(),
            framebuffer_registered: AtomicBool::new(false),
            dropped: AtomicBool::new(false),
        }
    }

    pub(crate) fn should_register_framebuffers(&self) -> bool {
        !self.framebuffer_registered.swap(true, Ordering::Relaxed)
    }

    pub fn attachments_desc(&self) -> AttachmentsDescription {
        self.attachments_description.clone()
    }
}
#[derive(Clone)]
pub struct AttachmentsDescription {
    swapchain_attachment_desc: AttachmentDescription,
    depth_attachment_desc: Option<AttachmentDescription>,
    /// If present, swapchain_attachment_desc is used as resolve attachment
    color_attachement_desc: Option<AttachmentDescription>,
}

impl AttachmentsDescription {
    pub fn new(swapchain_attachment_desc: AttachmentDescription) -> Self {
        Self {
            swapchain_attachment_desc,
            depth_attachment_desc: None,
            color_attachement_desc: None,
        }
    }

    pub fn with_depth_attachment(mut self, depth_attachment_desc: AttachmentDescription) -> Self {
        self.depth_attachment_desc = Some(depth_attachment_desc);
        self
    }

    pub fn with_color_attachment(mut self, color_attachment_desc: AttachmentDescription) -> Self {
        self.color_attachement_desc = Some(color_attachment_desc);
        self
    }

    pub fn get_swapchain_desc(&self) -> AttachmentDescription {
        self.swapchain_attachment_desc
    }

    pub fn get_depth_attachment_desc(&self) -> Option<AttachmentDescription> {
        self.depth_attachment_desc
    }

    pub fn get_color_attachment_desc(&self) -> Option<AttachmentDescription> {
        self.color_attachement_desc
    }

    pub fn fill_defaults(&mut self, swapchain_format: Format) {
        self.swapchain_attachment_desc.format = swapchain_format;
        // self.color_attachment_desc.load_op = AttachmentLoadOp::CLEAR;
        // self.color_attachment_desc.store_op = AttachmentStoreOp::STORE;
        if let Some(depth_attachment) = &mut self.depth_attachment_desc {
            depth_attachment.stencil_load_op = AttachmentLoadOp::DONT_CARE;
            depth_attachment.stencil_store_op = AttachmentStoreOp::DONT_CARE;
            // depth_attachment.load_op = AttachmentLoadOp::CLEAR;
            // depth_attachment.store_op = AttachmentStoreOp::DONT_CARE;
        }
        if let Some(color_attachment_desc) = &mut self.color_attachement_desc {
            color_attachment_desc.format = swapchain_format;
            // resolve_attachment.load_op = AttachmentLoadOp::DONT_CARE;
            // resolve_attachment.store_op = AttachmentStoreOp::STORE;
        }
    }
    
    pub fn len(&self) -> usize {
        let mut res = 1;
        if self.depth_attachment_desc.is_some() {
            res += 1;
        }
        if self.color_attachement_desc.is_some() {
            res += 1;
        }
        res
    }

    /// Iterator over non-swapchain attachments in order, yielding (attachment_index, slot, description)
    /// attachment_index starts at 0 for the first non-swapchain attachment
    pub fn iter_non_swapchain_attachments(&self) -> impl Iterator<Item = (usize, AttachmentSlot, AttachmentDescription)> {
        let mut attachments = SmallVec::<[(usize, AttachmentSlot, AttachmentDescription); 2]>::new();
        let mut index = 0;

        if let Some(depth_desc) = self.depth_attachment_desc {
            attachments.push((index, AttachmentSlot::Depth, depth_desc));
            index += 1;
        }

        if let Some(color_desc) = self.color_attachement_desc {
            attachments.push((index, AttachmentSlot::ColorMSAA, color_desc));
        }

        attachments.into_iter()
    }
}

impl Drop for RenderPassResource {
    fn drop(&mut self) {
        if !self.dropped.load(Ordering::Relaxed) {
            destroy_render_pass(self, false);
        }
    }
}
pub(crate) fn destroy_render_pass(render_pass: &RenderPassResource, no_usages: bool) {
    if !render_pass.dropped.swap(true, Ordering::Relaxed) {
        if let Some(instance) = try_get_instance() {
            if !no_usages {
                let last_host_waited = instance.shared_state.last_host_waited_cached().num();
                if render_pass.submission_usage.load().is_some_and(|u| u > last_host_waited) {
                    warn!("Trying to destroy render pass resource, but VulkanAllocator was destroyed earlier! Calling device_wait_idle...");
                    unsafe {
                        instance.device.device_wait_idle().unwrap();
                    }
                }
            }
            let device = instance.device.clone();
            unsafe {
                device.destroy_render_pass(render_pass.render_pass, None)
            }
        }
        else {
            error!("VulkanInstance was destroyed! Cannot destroy render pass resource");
        }
    }
}