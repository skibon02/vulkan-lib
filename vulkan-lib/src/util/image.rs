use ash::vk::Extent2D;
use image::{DynamicImage, ImageResult};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ReadImageError {
    #[error("Image error: {0}")]
    ImageError(#[from] image::ImageError),
    #[error("Image has zero size")]
    ZeroSize,
}
pub type ReadImageResult<T> = Result<T, ReadImageError>;
pub fn read_image_from_bytes(image_bytes: Vec<u8>) -> ReadImageResult<(Vec<u8>, Extent2D)> {
    let image_object = image::load_from_memory(&image_bytes)?;

    let (image_width, image_height) = (image_object.width(), image_object.height());

    if image_width == 0 || image_height == 0 {
        return Err(ReadImageError::ZeroSize);
    }

    let image_data = match &image_object {
        DynamicImage::ImageLuma8(_)
        | DynamicImage::ImageRgb8(_) => image_object.to_rgba8().into_raw(),
        DynamicImage::ImageLumaA8(_)
        | DynamicImage::ImageRgba8(_) => image_object.into_bytes(),
        _ => panic!("Unsupported image format"),
    };

    Ok((image_data, Extent2D {
        width: image_width,
        height: image_height,
    }))
}