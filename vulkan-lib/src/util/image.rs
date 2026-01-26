use ash::vk::{Extent2D, Format};

pub fn is_color_format(format: Format) -> bool {
    !(format >= Format::D16_UNORM && format <= Format::D32_SFLOAT_S8_UINT)
}