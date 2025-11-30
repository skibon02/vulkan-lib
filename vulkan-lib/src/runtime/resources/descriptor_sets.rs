use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use ash::vk::DescriptorType;
use log::warn;
use slotmap::DefaultKey;
use smallvec::SmallVec;
use crate::runtime::resources::buffers::BufferResourceHandle;
use crate::runtime::resources::images::ImageResourceHandle;
use crate::runtime::resources::sampler::SamplerHandle;
use crate::runtime::SharedState;
use crate::shaders::DescriptorSetLayoutBindingDesc;

#[derive(Clone, Copy, PartialEq)]
pub enum BoundResource<'a> {
    Buffer(BufferResourceHandle<'a>),
    Image(ImageResourceHandle),
    CombinedImageSampler { 
        image: ImageResourceHandle,
        sampler: SamplerHandle
    },
}

#[derive(Clone)]
pub struct DescriptorSetBinding<'a> {
    pub binding_index: u32,
    pub descriptor_type: DescriptorType,
    pub descriptor_count: u32,
    pub resource: Option<BoundResource<'a>>,
}

pub struct DescriptorSet<'a> {
    shared: SharedState,
    handle: DescriptorSetHandle<'a>,
}

impl<'a> DescriptorSet<'a> {
    pub(crate) fn new(shared: SharedState, key: DefaultKey, layout_bindings: &'static [DescriptorSetLayoutBindingDesc]) -> Self {
        let bindings = layout_bindings.iter().map(|desc| {
            DescriptorSetBinding {
                binding_index: desc.binding,
                descriptor_type: desc.descriptor_type,
                descriptor_count: desc.descriptor_count,
                resource: None,
            }
        }).collect();

        Self {
            shared,
            handle: DescriptorSetHandle {
                key,
                bindings,
                bindings_updated: Rc::new(AtomicBool::new(false)),
            },
        }
    }

    pub fn handle(&self) -> DescriptorSetHandle<'a> {
        self.handle.clone()
    }

    pub fn bind_buffer(&mut self, binding_index: u32, buffer: BufferResourceHandle<'a>) {
        if let Some(binding) = self.handle.bindings.iter_mut().find(|b| b.binding_index == binding_index) {
            binding.resource = Some(BoundResource::Buffer(buffer));
            self.handle.bindings_updated.store(true, Ordering::Relaxed);
        }
        else {
            warn!("Incorrect binding index specified in bind_buffer!");
        }
    }

    pub fn bind_image(&mut self, binding_index: u32, image: ImageResourceHandle) {
        if let Some(binding) = self.handle.bindings.iter_mut().find(|b| b.binding_index == binding_index) {
            binding.resource = Some(BoundResource::Image(image));
            self.handle.bindings_updated.store(true, Ordering::Relaxed);
        }
        else {
            warn!("Incorrect binding index specified in bind_image!");
        }
    }
    
    pub fn bind_image_and_sampler(&mut self, binding_index: u32, image: ImageResourceHandle, sampler: SamplerHandle) {
        if let Some(binding) = self.handle.bindings.iter_mut().find(|b| b.binding_index == binding_index) {
            binding.resource = Some(BoundResource::CombinedImageSampler { image, sampler });
            self.handle.bindings_updated.store(true, Ordering::Relaxed);
        }
        else {
            warn!("Incorrect binding index specified in bind_image_and_sampler!");
        }
    }
}

impl Drop for DescriptorSet<'_> {
    fn drop(&mut self) {
        self.shared.schedule_recycle_descriptor_set(self.handle().into());
    }
}

#[derive(Clone)]
pub struct DescriptorSetHandle<'a> {
    pub(crate) key: DefaultKey,
    pub(crate) bindings: SmallVec<[DescriptorSetBinding<'a>; 4]>,
    pub(crate) bindings_updated: Rc<AtomicBool>,
}

pub struct DescriptorSetDestroyHandle {
    pub(crate) key: DefaultKey,
}

impl From<DescriptorSetHandle<'_>> for DescriptorSetDestroyHandle {
    fn from(handle: DescriptorSetHandle) -> Self {
        Self {
            key: handle.key,
        }
    }
}
