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
use crate::resources::{RequiredSync, ResourceUsage};
use crate::swapchain_wrapper::SwapchainImages;
use crate::wrappers::device::VkDeviceRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentUsage {
    Color,
    Depth,
    Resolve,
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
    pub(crate) rp_begin_sync: RequiredSync,
    pub(crate) rp_end_sync: RequiredSync,
    pub(crate) subpass_usages: SmallVec<[(usize, ResourceUsage); 3]>,

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
        let mut vk_attachments: SmallVec<[AttachmentDescription; 5]> = smallvec![];
        let mut subpass = vk::SubpassDescription::default()
            .pipeline_bind_point(PipelineBindPoint::GRAPHICS);

        let mut attachment_refs = attachments_description.iter_attachments()
            .map(|(idx, slot, desc, layout)| {
                vk::AttachmentReference::default()
                    .attachment(idx as u32)
                    .layout(layout)
            })
            .collect::<Vec<_>>();

        let mut subpass_usages = smallvec![];
        for ((idx, slot, desc, layout), attachment_ref) in attachments_description.iter_attachments().zip(attachment_refs.iter()) {
            match slot {
                AttachmentUsage::Color => {
                    subpass = subpass.color_attachments(std::slice::from_ref(attachment_ref));
                    subpass_usages.push((idx, ResourceUsage::new(0, PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, AccessFlags::COLOR_ATTACHMENT_WRITE | AccessFlags::COLOR_ATTACHMENT_READ)));
                }
                AttachmentUsage::Depth => {
                    subpass = subpass.depth_stencil_attachment(attachment_ref);
                    subpass_usages.push((idx, ResourceUsage::new(0, PipelineStageFlags::EARLY_FRAGMENT_TESTS | PipelineStageFlags::LATE_FRAGMENT_TESTS, AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE | AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ)));
                }
                AttachmentUsage::Resolve => {
                    subpass = subpass.resolve_attachments(std::slice::from_ref(attachment_ref));
                    subpass_usages.push((idx, ResourceUsage::new(0, PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, AccessFlags::COLOR_ATTACHMENT_WRITE)));
                }
            }
            vk_attachments.push(desc);
        }
        // Perform layout transition in COLOR_ATTACHMENT_OUTPUT stage, make swapchain image available for COLOR_ATTACHMENT_WRITE
        let dependencies = [vk::SubpassDependency::default()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .src_access_mask(AccessFlags::empty())
            .dst_stage_mask(PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | PipelineStageFlags::EARLY_FRAGMENT_TESTS | PipelineStageFlags::LATE_FRAGMENT_TESTS)
            .dst_access_mask(AccessFlags::COLOR_ATTACHMENT_WRITE | AccessFlags::COLOR_ATTACHMENT_READ | AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE | AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ)];

        let rp_begin_sync = RequiredSync {
            src_stages: dependencies[0].src_stage_mask,
            src_access: dependencies[0].src_access_mask,
            dst_stages: dependencies[0].dst_stage_mask,
            dst_access: dependencies[0].dst_access_mask,
        };

        // keep default for automatic layout transitions
        let rp_end_sync = RequiredSync {
            src_stages: PipelineStageFlags::ALL_COMMANDS,
            src_access: AccessFlags::COLOR_ATTACHMENT_WRITE | AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE | AccessFlags::COLOR_ATTACHMENT_READ | AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ,
            dst_stages: PipelineStageFlags::BOTTOM_OF_PIPE,
            dst_access: AccessFlags::empty(),
        };

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
            rp_begin_sync,
            rp_end_sync,
            subpass_usages
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
    swapchain_layout: ImageLayout,
    depth_attachment_desc: Option<(AttachmentDescription, ImageLayout)>,
    /// If present, swapchain_attachment_desc is used as resolve attachment
    color_attachement_desc: Option<(AttachmentDescription, ImageLayout)>,
}

impl AttachmentsDescription {
    pub fn new(swapchain_attachment_desc: AttachmentDescription, swapchain_layout: ImageLayout) -> Self {
        Self {
            swapchain_attachment_desc,
            swapchain_layout,
            depth_attachment_desc: None,
            color_attachement_desc: None,
        }
    }

    pub fn with_depth_attachment(mut self, depth_attachment_desc: AttachmentDescription, depth_image_layout: ImageLayout) -> Self {
        self.depth_attachment_desc = Some((depth_attachment_desc, depth_image_layout));
        self
    }

    pub fn with_color_attachment(mut self, color_attachment_desc: AttachmentDescription, color_image_layout: ImageLayout) -> Self {
        self.color_attachement_desc = Some((color_attachment_desc, color_image_layout));
        self
    }

    pub fn get_swapchain_desc(&self) -> AttachmentDescription {
        self.swapchain_attachment_desc
    }

    pub fn get_depth_attachment_desc(&self) -> Option<(AttachmentDescription, ImageLayout)> {
        self.depth_attachment_desc
    }

    pub fn get_color_attachment_desc(&self) -> Option<(AttachmentDescription, ImageLayout)> {
        self.color_attachement_desc
    }

    pub fn fill_defaults(&mut self, swapchain_format: Format) {
        self.swapchain_attachment_desc.format = swapchain_format;
        // self.color_attachment_desc.load_op = AttachmentLoadOp::CLEAR;
        // self.color_attachment_desc.store_op = AttachmentStoreOp::STORE;
        if let Some((depth_attachment, _)) = &mut self.depth_attachment_desc {
            depth_attachment.stencil_load_op = AttachmentLoadOp::DONT_CARE;
            depth_attachment.stencil_store_op = AttachmentStoreOp::DONT_CARE;
            // depth_attachment.load_op = AttachmentLoadOp::CLEAR;
            // depth_attachment.store_op = AttachmentStoreOp::DONT_CARE;
        }
        if let Some((color_attachment_desc, _)) = &mut self.color_attachement_desc {
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
    pub fn iter_attachments(&self) -> impl Iterator<Item = (usize, AttachmentUsage, AttachmentDescription, ImageLayout)> {
        let mut index = 0;
        let swapchain_attachment_usage = if self.color_attachement_desc.is_some() {
            AttachmentUsage::Resolve
        }
        else {
            AttachmentUsage::Color
        };
        let mut attachments: SmallVec<[_; 3]> = smallvec![(index, swapchain_attachment_usage, self.swapchain_attachment_desc, self.swapchain_layout)];
        index += 1;

        if let Some(depth_desc) = self.depth_attachment_desc {
            attachments.push((index, AttachmentUsage::Depth, depth_desc.0, depth_desc.1));
            index += 1;
        }

        if let Some(color_desc) = self.color_attachement_desc {
            attachments.push((index, AttachmentUsage::Color, color_desc.0, color_desc.1));
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