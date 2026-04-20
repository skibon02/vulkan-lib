use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use log::{error, info};
use swash::{FontRef, GlyphId};
use swash::scale::image::Image;
use crate::layout::FontFamily;
use crate::resources::get_resource;

pub struct Fonts(pub HashMap<FontFamily, FontInfo>);
impl Fonts {
    pub fn load_font(&mut self, name: FontFamily) -> &mut FontInfo {
        self.entry(name.clone())
            .or_insert_with(|| {

                static BASIC_FONT: &'static [u8] = include_bytes!("../../../../assets/fonts/Basic-Regular.ttf");
                let default_font = FontInfo {
                    default_line_height: 16.0,
                    font_raw: BASIC_FONT.to_vec(),
                    sizes: HashMap::new(),
                };
                let font_data = match name.clone() {
                    FontFamily::Default => {
                        BASIC_FONT.to_vec()
                    }
                    FontFamily::Named(name) => {
                        match get_resource(Path::new("fonts").join(&*name.clone())) {
                            Ok(bytes) => bytes,
                            Err(e) => {
                                error!("Failed to load font resource '{}': {}", name, e);
                                return default_font;
                            }
                        }
                    }
                };
                let font = match FontRef::from_index(&font_data, 0) {
                    Some(font) => font,
                    None => {
                        error!("Failed to parse font '{:?}'", name);
                        return default_font;
                    }
                };

                info!("Loaded font '{:?}' with attributes: {:?}", name, font.attributes());

                FontInfo {
                    default_line_height: 16.0,
                    font_raw: font_data,
                    sizes: HashMap::new(),
                }
            })
    }
}

impl Deref for Fonts {
    type Target = HashMap<FontFamily, FontInfo>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Fonts {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}


pub struct FontInfo {
    font_raw: Vec<u8>,
    default_line_height: f32,
    sizes: HashMap<f32, FontSizeInfo>
}

impl FontInfo {
    /// Ensure all provided glyphs are rendered in `size`, return rendered glyphs
    pub fn render(&mut self, size: f32, glyphs: impl Iterator<Item=GlyphId>) -> &mut FontSizeInfo {
        todo!()
    }
    
    pub fn font_raw(&self) -> &[u8] {
        &self.font_raw
    }
}

pub struct FontSizeInfo {
    // map of rendered glyphs for fixed size
    rendered_glyphs: HashMap<GlyphId, Image>,
}
