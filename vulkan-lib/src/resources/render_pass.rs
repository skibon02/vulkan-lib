use std::sync::Arc;
use ash::vk;
use ash::vk::{AccessFlags, AttachmentDescription, AttachmentLoadOp, AttachmentStoreOp, Format, ImageLayout, PipelineBindPoint, PipelineStageFlags};
use log::error;
use smallvec::{smallvec, SmallVec};
use sparkles::range_event_start;
use crate::queue::OptionSeqNumShared;
use crate::resources::image::ImageResource;
use crate::runtime::OptionSeqNumShared;
use crate::wrappers::device::VkDeviceRef;

pub enum FrameBufferAttachment {
    SwapchainImage(usize),
    Image(Arc<ImageResource>)
}
pub struct RenderPassResource {
    pub(crate) render_pass: vk::RenderPass,
    attachments_description: AttachmentsDescription,
    attachments: SmallVec<[SmallVec<[FrameBufferAttachment; 5]>; 5]>,
    submission_usage: OptionSeqNumShared,

    dropped: bool,
}

impl RenderPassResource {
    pub(crate) fn new(device: &VkDeviceRef, swapchain_images: SmallVec<[Arc<FrameBufferAttachment>; 3]>, mut attachments_description: AttachmentsDescription, swapchain_format: vk::Format) -> Self {
        let g = range_event_start!("Create render pass");

        let swapchain_format = swapchain_format;

        attachments_description.fill_defaults(swapchain_format);
        let mut attachments: SmallVec<[AttachmentDescription; 5]> = smallvec![attachments_description.swapchain_attachment_desc];
        let mut attachment_i = 1;
        let mut subpass = vk::SubpassDescription::default()
            .pipeline_bind_point(PipelineBindPoint::GRAPHICS);

        let depth_attachment_ref;
        if let Some(attachment) = attachments_description.depth_attachment_desc {
            attachments.push(attachment);
            depth_attachment_ref = vk::AttachmentReference::default()
                .attachment(attachment_i)
                .layout(ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
            subpass = subpass.depth_stencil_attachment(&depth_attachment_ref);
            attachment_i += 1;
        }
        let color_attachment_refs;
        let resolve_attachment_refs;
        if let Some(attachment) = attachments_description.color_attachement_desc {
            attachments.push(attachment);
            color_attachment_refs = [vk::AttachmentReference::default()
                .attachment(attachment_i)
                .layout(ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];

            // attachment 0 is treated as resolve attachment
            resolve_attachment_refs = [vk::AttachmentReference::default()
                .attachment(0)
                .layout(ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];

            subpass = subpass.resolve_attachments(&resolve_attachment_refs);
            subpass = subpass.color_attachments(&color_attachment_refs);
            attachment_i += 1;
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
                .dependencies(&dependencies);
        let render_pass_create_info = render_pass_create_info.attachments(&attachments);
        let render_pass = unsafe { device.create_render_pass(&render_pass_create_info, None).unwrap() };

        Self {
            render_pass,
            attachments_description,
            attachments: smallvec![swapchain_images.into_iter().map(|img| {
                match &*img {
                    FrameBufferAttachment::SwapchainImage(i) => FrameBufferAttachment::SwapchainImage(*i),
                    FrameBufferAttachment::Image(image) => FrameBufferAttachment::Image(image.clone()),
                }
            }).collect()],
            submission_usage: OptionSeqNumShared::default(),

            dropped: false,
        }
    }
    
    pub(crate) fn attachments_description(&self) -> &AttachmentsDescription {
        &self.attachments_description
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
}

impl Drop for RenderPassResource {
    fn drop(&mut self) {
        if !self.dropped {
            error!("RenderPassResource was not destroyed before dropping!");
        }
    }
}
pub(crate) fn destroy_render_pass(device: &VkDeviceRef, mut render_pass: RenderPassResource) {
    if !render_pass.dropped {
        unsafe {
            device.destroy_render_pass(render_pass.render_pass, None)
        }
        render_pass.dropped = true;
    }
}