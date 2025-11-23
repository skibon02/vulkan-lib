use smallvec::SmallVec;

pub mod layout;

#[derive(Debug, Clone)]
pub enum UniformBindingType {
    UniformBuffer,
    CombinedImageSampler,
}


pub type DescriptorBindingsDesc = SmallVec<[(u32, UniformBindingType); 5]>;

#[macro_export]
macro_rules! use_shader {
    ($name:expr) => {
        (
            include_bytes!(concat!("../shaders/compiled/", $name, "_vert.spv")),
            include_bytes!(concat!("../shaders/compiled/", $name, "_frag.spv"))
        )
    };
}