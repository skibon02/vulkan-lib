use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use log::{error, warn};
use crate::resources::get_resource;
use crate::util::read_image_from_bytes;

pub struct Images(pub HashMap<String, ImageInfo>);
impl Images {
    pub fn load_image(&mut self, src: String) -> &ImageInfo {
        self.entry(src.clone())
            .or_insert_with(|| {
                let img_bytes = match get_resource(Path::new("images").join(&src)) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        warn!("Failed to load image resource '{}': {}", src, e);
                        return ImageInfo {
                            aspect: 1.0,
                            src: ImageSource::OpenError,
                        }
                    }
                };
                let (img, extent) = match read_image_from_bytes(img_bytes)  {
                    Ok((img, extent)) => (img, extent),
                    Err(e) => {
                        error!("Failed to read image '{}': {}", src, e);
                        return ImageInfo {
                            aspect: 1.0,
                            src: ImageSource::OpenError,
                        }
                    }
                };
                // load image and calculate aspect ratio
                ImageInfo {
                    aspect: extent.height as f32 / extent.width as f32,
                    src: ImageSource::Bytes(img),
                }
            })
    }
}

impl Deref for Images {
    type Target = HashMap<String, ImageInfo>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Images {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub enum ImageSource {
    Bytes(Vec<u8>),
    OpenError,
}

pub struct ImageInfo {
    // calculated as height / width
    aspect: f32,
    src: ImageSource,
}

impl ImageInfo {
    pub fn aspect(&self) -> f32 {
        self.aspect
    }
}
