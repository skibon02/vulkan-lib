use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use ash::vk;
use ash::vk::{DescriptorBufferInfo, DescriptorImageInfo, DescriptorSetLayout, DescriptorType, ImageLayout, WriteDescriptorSet, WHOLE_SIZE};
use log::{error, warn};
use slotmap::DefaultKey;
use smallvec::{smallvec, SmallVec};
use crate::queue::OptionSeqNumShared;
use crate::resources::buffer::BufferResource;
use crate::resources::image::ImageResource;
use crate::resources::sampler::SamplerResource;
use crate::queue::shared::SharedState;
use crate::shaders::DescriptorSetLayoutBindingDesc;
use crate::wrappers::device::VkDeviceRef;

#[derive(Clone)]
pub enum BoundResource {
    Buffer(Arc<BufferResource>),
    Image(Arc<ImageResource>),
    CombinedImageSampler {
        image: Arc<ImageResource>,
        sampler: Arc<SamplerResource>
    },
}

pub struct DescriptorSetBinding {
    pub binding_index: u32,
    pub descriptor_type: DescriptorType,
    pub descriptor_count: u32,
    pub resource: Option<BoundResource>,
    pub resource_updated: bool,
}

pub struct DescriptorSetResource {
    pub(crate) descriptor_set: vk::DescriptorSet,
    pub(crate) pool_index: usize,
    pub(crate) layout: DescriptorSetLayout,
    pub(crate) bindings: Mutex<SmallVec<[DescriptorSetBinding; 5]>>,
    pub(crate) submission_usage: OptionSeqNumShared,
    pub(crate) updates_locked: AtomicBool,

    pub(crate) dropped: bool,
}

impl DescriptorSetResource {
    pub(crate) fn bindings(&self) -> &Mutex<SmallVec<[DescriptorSetBinding; 5]>> {
        &self.bindings
    }

    pub fn try_bind_buffer(&self, binding_index: u32, buffer: Arc<BufferResource>) {
        let mut bindings = self.bindings.lock().unwrap();
        if self.updates_locked.load(Ordering::Relaxed) {
            warn!("Attempted to bind buffer to descriptor set while updates are locked!");
            return;
        }

        if let Some(binding) = bindings.iter_mut().find(|b| b.binding_index == binding_index) {
            binding.resource = Some(BoundResource::Buffer(buffer));
            binding.resource_updated = true;
        }
        else {
            warn!("Incorrect binding index specified in bind_buffer!");
        }
    }

    pub fn try_bind_image(&self, binding_index: u32, image: Arc<ImageResource>) {
        let mut bindings = self.bindings.lock().unwrap();
        if self.updates_locked.load(Ordering::Relaxed) {
            warn!("Attempted to bind buffer to descriptor set while updates are locked!");
            return;
        }

        if let Some(binding) = bindings.iter_mut().find(|b| b.binding_index == binding_index) {
            binding.resource = Some(BoundResource::Image(image));
            binding.resource_updated = true;
        }
        else {
            warn!("Incorrect binding index specified in bind_image!");
        }
    }

    pub fn try_bind_image_sampler(&self, binding_index: u32, image: Arc<ImageResource>, sampler: Arc<SamplerResource>) {
        let mut bindings = self.bindings.lock().unwrap();
        if self.updates_locked.load(Ordering::Relaxed) {
            warn!("Attempted to bind buffer to descriptor set while updates are locked!");
            return;
        }

        if let Some(binding) = bindings.iter_mut().find(|b| b.binding_index == binding_index) {
            binding.resource = Some(BoundResource::CombinedImageSampler { image, sampler });
            binding.resource_updated = true;
        }
        else {
            warn!("Incorrect binding index specified in bind_image_and_sampler!");
        }
    }

    pub(crate) fn lock_updates(&self) {
        self.updates_locked.store(true, Ordering::Relaxed);
    }

    pub(crate) fn unlock_updates(&self) {
        self.updates_locked.store(false, Ordering::Relaxed);
    }

    /// SAFETY: Must ensure descriptor set is not currently used in any command buffers.
    pub(crate) fn update_descriptor_set(&self, device: &VkDeviceRef) {
        let mut buffer_bindings: SmallVec<[_; 4]> = smallvec![];
        let mut image_bindings: SmallVec<[_; 4]> = smallvec![];
        let mut bindings = self.bindings.lock().unwrap();
        for binding in bindings.iter_mut() {
            if binding.resource.is_none() {
                error!("Descriptor set binding {}:{:?} is not set during draw command!", binding.binding_index, binding.descriptor_type);
            }

            if !binding.resource_updated {
                continue;
            }
            if let Some(resource) = &binding.resource {
                match resource {
                    BoundResource::Buffer(buffer) => {
                        buffer_bindings.push((binding.binding_index, buffer.buffer));
                    }
                    BoundResource::Image(image) => {
                        image_bindings.push((binding.binding_index, image.image_view, None))
                    }
                    BoundResource::CombinedImageSampler {
                        image, sampler
                    } => {
                        image_bindings.push((binding.binding_index, image.image_view, Some(sampler.clone())))
                    }
                }
            }
        }

        let buffer_infos: SmallVec<[_; 4]> = buffer_bindings.into_iter()
            .map(|(i, buf)| {
                (i, DescriptorBufferInfo::default()
                    .buffer(buf)
                    .offset(0)
                    .range(WHOLE_SIZE))
            }).collect();

        let image_infos: SmallVec<[_; 4]> = image_bindings.into_iter()
            .map(|(i, iv, sampler)| {
                let mut info = DescriptorImageInfo::default()
                    .image_view(iv)
                    .image_layout(ImageLayout::SHADER_READ_ONLY_OPTIMAL);

                if let Some(sampler) = sampler {
                    info.sampler = sampler.sampler;
                }
                (i, info)
            }).collect();

        let mut descriptor_writes: SmallVec<[_; 4]> = smallvec![];
        for (binding, buffer_info) in buffer_infos.iter() {
            descriptor_writes.push(WriteDescriptorSet::default()
                .dst_set(self.descriptor_set)
                .dst_binding(*binding)
                .descriptor_type(DescriptorType::UNIFORM_BUFFER)
                .buffer_info(std::slice::from_ref(&buffer_info))
            );
        }

        for (binding, image_info) in image_infos.iter() {
            let t = if image_info.sampler != vk::Sampler::null() {
                DescriptorType::COMBINED_IMAGE_SAMPLER
            } else {
                DescriptorType::SAMPLED_IMAGE
            };
            descriptor_writes.push(WriteDescriptorSet::default()
                .dst_set(self.descriptor_set)
                .dst_binding(*binding)
                .descriptor_type(t)
                .image_info(std::slice::from_ref(&image_info))
            );
        }

        unsafe {
            device.update_descriptor_sets(&descriptor_writes, &[]);
        }
    }
}

impl Drop for DescriptorSetResource {
    fn drop(&mut self) {
        if !self.dropped {
            error!("DescriptorSetResource dropped without proper destruction!");
        }
    }
}