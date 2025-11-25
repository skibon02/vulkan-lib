use ash::vk::{DescriptorType, ShaderStageFlags};
use smallvec::SmallVec;

pub mod layout;

#[derive(Debug, Clone)]
pub enum UniformBindingType {
    UniformBuffer,
    CombinedImageSampler,
}


#[macro_export]
macro_rules! use_shader {
    ($name:expr) => {
        (
            include_bytes!(concat!("../shaders/compiled/", $name, "_vert.spv")),
            include_bytes!(concat!("../shaders/compiled/", $name, "_frag.spv"))
        )
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DescriptorSetLayoutBindingDesc {
    pub binding: u32,
    pub descriptor_type: DescriptorType,
    pub descriptor_count: u32,
    pub stage_flags: ShaderStageFlags,
}

#[macro_export]
macro_rules! descriptor_set {
    (
        pub struct $name:ident {
            $(
                $(#[$stage:ident])?
                $binding:literal -> $desc_type:ident $([$count:literal])?
            ),* $(,)?
        }
    ) => {
        pub struct $name;

        impl $name {
            pub fn bindings() -> &'static [$crate::shaders::DescriptorSetLayoutBindingDesc] {
                &[
                    $(
                        $crate::shaders::DescriptorSetLayoutBindingDesc {
                            binding: $binding,
                            descriptor_type: descriptor_set!(@desc_type $desc_type),
                            descriptor_count: descriptor_set!(@count $($count)?),
                            stage_flags: descriptor_set!(@stage $($stage)?),
                        },
                    )*
                ]
            }
        }
    };

    (@count) => { 1 };
    (@count $count:literal) => { $count };

    (@stage) => { $crate::ShaderStageFlags::ALL };
    (@stage vert) => { $crate::ShaderStageFlags::VERTEX };
    (@stage frag) => { $crate::ShaderStageFlags::FRAGMENT };
    (@stage comp) => { $crate::ShaderStageFlags::COMPUTE };

    (@desc_type UniformBuffer) => { $crate::DescriptorType::UNIFORM_BUFFER };
    (@desc_type CombinedImageSampler) => { $crate::DescriptorType::COMBINED_IMAGE_SAMPLER };
    (@desc_type StorageBuffer) => { $crate::DescriptorType::STORAGE_BUFFER };
    (@desc_type StorageImage) => { $crate::DescriptorType::STORAGE_IMAGE };
    (@desc_type SampledImage) => { $crate::DescriptorType::SAMPLED_IMAGE };
    (@desc_type Sampler) => { $crate::DescriptorType::SAMPLER };
}

