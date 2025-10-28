use std::ops::Range;
use crate::pipeline::VertexInputDesc;
use types::GlslTypeVariant;

pub mod types {
    use std::mem::MaybeUninit;
    use ash::vk::Format;

    pub trait GlslType {
        const T: GlslTypeVariant;
        type Inner;
    }

    #[derive(Copy, Clone)]
    #[repr(C, align(8))]
    pub struct vec2<const P: usize>([f32; 2], MaybeUninit<[u32; P]>);
    impl<const P: usize> GlslType for vec2<P> {
        const T: GlslTypeVariant = GlslTypeVariant::Vec2;
        type Inner = [f32; 2];
    }
    impl<const P: usize> From<[f32; 2]> for vec2<P> {
        fn from(data: [f32; 2]) -> Self {
            vec2(data, MaybeUninit::uninit())
        }
    }
    impl<const P: usize> From<vec2<P>> for [f32; 2] {
        fn from(data: vec2<P>) -> [f32; 2] {
            data.0
        }
    }

    #[derive(Copy, Clone)]
    #[repr(C, align(16))]
    pub struct vec3<const P: usize>([f32; 3], MaybeUninit<[u32; P]>);
    impl<const P: usize> GlslType for vec3<P> {
        const T: GlslTypeVariant = GlslTypeVariant::Vec3;
        type Inner = [f32; 3];
    }
    impl<const P: usize> From<[f32; 3]> for vec3<P> {
        fn from(data: [f32; 3]) -> Self {
            vec3(data, MaybeUninit::uninit())
        }
    }
    impl<const P: usize> From<vec3<P>> for [f32; 3] {
        fn from(data: vec3<P>) -> [f32; 3] {
            data.0
        }
    }

    #[derive(Copy, Clone)]
    #[repr(C, align(16))]
    pub struct vec4<const P: usize>([f32; 4], MaybeUninit<[u32; P]>);
    impl<const P: usize> GlslType for vec4<P> {
        const T: GlslTypeVariant = GlslTypeVariant::Vec4;
        type Inner = [f32; 4];
    }
    impl<const P: usize> From<[f32; 4]> for vec4<P> {
        fn from(data: [f32; 4]) -> Self {
            vec4(data, MaybeUninit::uninit())
        }
    }
    impl<const P: usize> From<vec4<P>> for [f32; 4] {
        fn from(data: vec4<P>) -> [f32; 4] {
            data.0
        }
    }


    #[derive(Copy, Clone)]
    #[repr(C)]
    pub struct float<const P: usize>(f32, MaybeUninit<[u32; P]>);
    impl<const P: usize> GlslType for float<P> {
        const T: GlslTypeVariant = GlslTypeVariant::Float;
        type Inner = f32;
    }
    impl<const P: usize> From<f32> for float<P> {
        fn from(data: f32) -> Self {
            float(data, MaybeUninit::uninit())
        }
    }
    impl<const P: usize> From<float<P>> for f32 {
        fn from(data: float<P>) -> f32 {
            data.0
        }
    }


    #[derive(Copy, Clone)]
    #[repr(C)]
    pub struct uint<const P: usize>(u32, MaybeUninit<[u32; P]>);
    impl<const P: usize> GlslType for uint<P> {
        const T: GlslTypeVariant = GlslTypeVariant::Uint;
        type Inner = u32;
    }
    impl<const P: usize> From<u32> for uint<P> {
        fn from(data: u32) -> Self {
            uint(data, MaybeUninit::uninit())
        }
    }
    impl<const P: usize> From<uint<P>> for u32 {
        fn from(data: uint<P>) -> u32 {
            data.0
        }
    }
    
    #[derive(Copy, Clone)]
    #[repr(C)]
    pub struct int<const P: usize>(i32, MaybeUninit<[u32; P]>);
    impl<const P: usize> GlslType for int<P> {
        const T: GlslTypeVariant = GlslTypeVariant::Int;
        type Inner = i32;
    }
    impl<const P: usize> From<i32> for int<P> {
        fn from(data: i32) -> Self {
            int(data, MaybeUninit::uninit())
        }
    }
    impl<const P: usize> From<int<P>> for i32 {
        fn from(data: int<P>) -> i32 {
            data.0
        }
    }

    #[derive(Debug, Copy, Clone)]
    pub enum GlslTypeVariant {
        Vec2,
        Vec3,
        Vec4,
        Float,
        Uint,
        Int,
    }
    impl GlslTypeVariant {
        pub fn format(&self) -> Format {
            match self {
                GlslTypeVariant::Vec2 => Format::R32G32_SFLOAT,
                GlslTypeVariant::Vec3 => Format::R32G32B32_SFLOAT,
                GlslTypeVariant::Vec4 => Format::R32G32B32A32_SFLOAT,
                GlslTypeVariant::Float => Format::R32_SFLOAT,
                GlslTypeVariant::Uint => Format::R32_UINT,
                GlslTypeVariant::Int => Format::R32_SINT,
            }
        }
    }

}

pub trait LayoutInfo : Sized {
    // const LAYOUT: StateLayout;

    const MEMBERS_META: &'static [MemberMeta];

    // Full structure size. Alignment included.
    const SIZE: usize = size_of::<Self>();
    fn get_attributes_configuration() -> VertexInputDesc {
        VertexInputDesc::new(Self::MEMBERS_META, Self::SIZE)
    }

    fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(self as *const Self as *const u8, Self::SIZE)
        }
    }
}

pub struct MemberMeta {
    pub name: &'static str,
    pub range: Range<usize>,
    pub ty: GlslTypeVariant,
    // r#type: TypeId,
}