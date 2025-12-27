use std::mem;
use std::sync::atomic::AtomicBool;
use ash::vk;
use ash::vk::{AccessFlags, AttachmentDescription, AttachmentLoadOp, AttachmentStoreOp, Buffer, BufferCreateFlags, BufferCreateInfo, BufferUsageFlags, DescriptorBufferInfo, DescriptorImageInfo, DescriptorSetLayout, DescriptorType, DeviceMemory, DeviceSize, Extent3D, Format, Framebuffer, Image, ImageCreateFlags, ImageCreateInfo, ImageLayout, ImageTiling, ImageType, ImageUsageFlags, ImageView, MemoryAllocateInfo, MemoryHeap, MemoryType, Pipeline, PipelineBindPoint, PipelineLayout, PipelineStageFlags, RenderPass, SampleCountFlags, WriteDescriptorSet, WHOLE_SIZE};
use slotmap::{DefaultKey, SlotMap};
use smallvec::{smallvec, SmallVec};
use crate::runtime::{SharedState};
use crate::resources::descriptor_pool::DescriptorSetAllocator;
use crate::queue::shared::ScheduledForDestroy;
use crate::queue::memory_manager::{MemoryManager, MemoryTypeAlgorithm};
use crate::wrappers::device::VkDeviceRef;

pub(crate) struct ResourceStorage {
    device: VkDeviceRef,
    memory_manager: MemoryManager,
    buffers: SlotMap<DefaultKey, BufferInner>,
    images: SlotMap<DefaultKey, ImageInner>,
    render_passes: SlotMap<DefaultKey, RenderPassInner>,
    pipelines: SlotMap<DefaultKey, GraphicsPipelineInner>,
}

impl ResourceStorage {
    pub fn new(device: VkDeviceRef, memory_types: Vec<MemoryType>, memory_heaps: Vec<MemoryHeap>) -> Self{
        let memory_manager = MemoryManager::new(device.clone(), memory_types, memory_heaps);
        let descriptor_set_allocator = DescriptorSetAllocator::new(device.clone());
        Self {
            device,
            memory_manager,
            buffers: SlotMap::new(),
            images: SlotMap::new(),
            pipelines: SlotMap::new(),
            render_passes: SlotMap::new(),
        }
    }

    fn create_framebuffers(&mut self, device: VkDeviceRef, render_pass: RenderPass, swapchain_images: &SmallVec<[ImageResourceHandle; 3]>,
                           swapchain_extent: Extent3D, attachments: &SmallVec<[AttachmentDescription; 5]>, swapchain_format: Format, shared: SharedState) -> SmallVec<[(Framebuffer, SmallVec<[ImageResource; 5]>); 5]> {
        let mut framebuffers = smallvec![];
        for swapchain_image in swapchain_images {
            let mut owned_images: SmallVec<[ImageResource; 5]> = smallvec![];
            for attachment_image in attachments.iter().skip(1) {
                let usage = if attachment_image.format == swapchain_format {
                    ImageUsageFlags::COLOR_ATTACHMENT | ImageUsageFlags::TRANSIENT_ATTACHMENT
                } else {
                    ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | ImageUsageFlags::TRANSIENT_ATTACHMENT
                };

                let image_resource = self.create_image(
                    usage,
                    ImageCreateFlags::empty(),
                    MemoryTypeAlgorithm::Device,
                    swapchain_extent.width,
                    swapchain_extent.height,
                    attachment_image.format,
                    attachment_image.samples,
                    shared.clone(),
                );
                self.image_view(image_resource.handle().state_key); // create image view
                owned_images.push(image_resource);
            }

            let mut views: SmallVec<[ImageView; 5]> = smallvec![];
            views.push(self.image_view(swapchain_image.state_key));
            for owned_image in owned_images.iter() {
                views.push(self.image_view(owned_image.handle().state_key));
            }
            let framebuffer_create_info = vk::FramebufferCreateInfo::default()
                .render_pass(render_pass)
                .attachments(&views)
                .width(swapchain_extent.width)
                .height(swapchain_extent.height)
                .layers(1);
            let framebuffer = unsafe {
                device.create_framebuffer(&framebuffer_create_info, None).unwrap()
            };

            framebuffers.push((framebuffer, owned_images));
        }

        framebuffers
    }

}

impl Drop for ResourceStorage {
    fn drop(&mut self) {
        unsafe {
            // render pass may own images, but they will be destroyed below
            for (_, render_pass_inner) in self.render_passes.drain() {
                for (framebuffer, _) in render_pass_inner.framebuffers {
                    self.device.destroy_framebuffer(framebuffer, None);
                }
                self.device.destroy_render_pass(render_pass_inner.render_pass, None);
            }

            for (_, pipeline_inner) in self.pipelines.drain() {
                self.device.destroy_pipeline(pipeline_inner.pipeline, None);
                self.device.destroy_pipeline_layout(pipeline_inner.pipeline_layout, None);
            }

            for (_, descriptor_set_layout) in self.descriptor_set_layouts.drain() {
                self.device.destroy_descriptor_set_layout(descriptor_set_layout, None);
            }

            for (_, buffer_inner) in self.buffers.drain() {
                self.device.destroy_buffer(buffer_inner.buffer, None);
                self.device.free_memory(buffer_inner.memory, None);
            }

            for (_, image_inner) in self.images.drain() {
                if let Some(memory) = image_inner.memory {
                    self.device.destroy_image(image_inner.image, None);
                    self.device.free_memory(memory, None);
                }
                if let Some(image_view) = image_inner.image_view {
                    self.device.destroy_image_view(image_view, None);
                }
            }
        }
    }
}