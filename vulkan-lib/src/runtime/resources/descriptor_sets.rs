use ash::vk::DescriptorType;
use slotmap::DefaultKey;
use smallvec::SmallVec;
use crate::runtime::resources::buffers::BufferResourceHandle;
use crate::runtime::resources::images::ImageResourceHandle;
use crate::runtime::SharedState;
use crate::shaders::DescriptorSetLayoutBindingDesc;

#[derive(Clone, Copy, PartialEq)]
pub enum BoundResource<'a> {
    Buffer(BufferResourceHandle<'a>),
    Image(ImageResourceHandle),
    // For combined image sampler, we store image handle
    // Sampler will be added later when we implement samplers
    CombinedImageSampler { image: ImageResourceHandle },
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
            },
        }
    }

    pub fn handle(&self) -> DescriptorSetHandle<'a> {
        self.handle.clone()
    }

    pub fn bind_buffer(&mut self, binding_index: u32, buffer: BufferResourceHandle<'a>) {
        if let Some(binding) = self.handle.bindings.iter_mut().find(|b| b.binding_index == binding_index) {
            binding.resource = Some(BoundResource::Buffer(buffer));
        }
    }

    pub fn bind_image(&mut self, binding_index: u32, image: ImageResourceHandle) {
        if let Some(binding) = self.handle.bindings.iter_mut().find(|b| b.binding_index == binding_index) {
            binding.resource = Some(BoundResource::CombinedImageSampler { image });
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
    bindings: SmallVec<[DescriptorSetBinding<'a>; 4]>,
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
