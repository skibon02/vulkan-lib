use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use swash::{FontRef, GlyphId};
use swash::shape::Direction;
use swash::text::Script;
use crate::layout::{FontFamily, Lu, PX_PER_LU};
use crate::layout::calculator::components::font::FontInfo;

pub struct Texts(pub HashMap<u32, TextInfo>);

impl Deref for Texts {
    type Target = HashMap<u32, TextInfo>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Texts {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Texts {
    /// calculate glyphs placement for text and font.
    /// After this call, width and height are guaranteed to be valid
    pub fn calculate_layout(&mut self, text_id: u32,
                            font: &mut FontInfo, font_name: FontFamily, size: f32, width_constraint: Option<Lu>) -> &mut TextInfo {
        let text = self.entry(text_id)
            .or_default();

        let new_cache = TextInfoCache::new(font_name.clone(), size, width_constraint);
        if text.calculated_cache.as_ref() == Some(&new_cache) {
            return text;
        }
        text.calculated_cache = Some(new_cache);

        // <- need to recalculate glyphs layout
        let font = FontRef::from_index(&font.font_raw(), 0).unwrap();

        let mut context = swash::shape::ShapeContext::new();
        let mut y_pos = 0.0;
        let mut glyphs = Vec::new();
        let mut font_glyphs = HashSet::new();
        let mut max_width = 0.0;
        for line in text.value.lines() {
            let mut shaper = context.builder(font)
                .script(Script::Common)
                .direction(Direction::LeftToRight)
                .size(size)
                .build();

            shaper.add_str(line);

            let metrics = shaper.metrics();
            let line_height = metrics.ascent + metrics.descent + metrics.leading;
            let mut x_pos = 0.0;
            shaper.shape_with(|cluster| {
                for g in cluster.glyphs {
                    glyphs.push(TextGlyph {
                        font: font_name.clone(),
                        glyph: g.id,
                        x: x_pos + g.x,
                        y: y_pos + g.y,
                    });

                    font_glyphs.insert(g.id);

                    x_pos += g.advance;
                    if let Some(max_w) = width_constraint && x_pos * PX_PER_LU as f32 > max_w as f32 {
                        y_pos += line_height;
                    }
                }
            });


            if x_pos > max_width {
                max_width = x_pos;
            }
            y_pos += line_height;
        }



        text.text_width = (max_width * PX_PER_LU as f32) as Lu;
        text.text_height = (y_pos * PX_PER_LU as f32) as Lu;
        text.glyphs = glyphs;
        text.fonts.insert(font_name.clone(), font_glyphs);


        // let mut context = ScaleContext::new();
        // let mut scaler = context.builder(font)
        //     .size(36.0)
        //     .build();
        // let mut font_rnd = Render::new(&[
        //     // Color outline with the first palette
        //     Source::ColorOutline(0),
        //     // Color bitmap with best fit selection mode
        //     Source::ColorBitmap(StrikeWith::BestFit),
        //     // Standard scalable outline
        //     Source::Outline,
        // ]);

        // let glyph = font.charmap().map('ы');
        // let img = font_rnd.format(swash::zeno::Format::Alpha)
        //     .render(&mut scaler, glyph).unwrap();
        text
    }

    pub fn set_text(&mut self, text_id: u32, value: Arc<str>) {
        let mut entry = self.entry(text_id).or_default();
        if entry.value != value {
            entry.value = value;
            entry.calculated_cache = None;
        }
    }

    pub fn remove_text(&mut self, text_id: u32) {
        self.remove(&text_id);
    }
}
#[derive(Clone)]
pub struct TextGlyph {
    font: FontFamily,
    glyph: GlyphId,
    x: f32,
    y: f32,
}

#[derive(Clone, PartialEq)]
pub struct TextInfoCache {
    font: FontFamily,
    font_size: f32,
    width_constraint: Option<Lu>,
}

impl TextInfoCache {
    pub fn new(font: FontFamily, font_size: f32, width_constraint: Option<Lu>) -> Self {
        Self {
            font,
            font_size,
            width_constraint
        }
    }
}


#[derive(Clone, Default)]
pub struct TextInfo {
    value: Arc<str>,
    // true -> width,height,glyphs are valid for value
    calculated_cache: Option<TextInfoCache>,
    text_height: Lu,
    text_width: Lu,
    glyphs: Vec<TextGlyph>,
    fonts: HashMap<FontFamily, HashSet<GlyphId>>, // additional information which fonts and glyphs are used
}

impl TextInfo {
    pub fn width(&self) -> Lu {
        self.text_width
    }
    pub fn height(&self) -> Lu {
        self.text_height
    }
}