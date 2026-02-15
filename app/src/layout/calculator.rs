use std::cmp::max;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::ops::{Deref, DerefMut};
use log::{error, info, warn};
use swash::{FontRef, GlyphId};
use swash::scale::{ScaleContext, Source, StrikeWith};
use swash::scale::image::Image;
use swash::shape::cluster::Glyph;
use swash::shape::Direction;
use swash::text::Script;
use crate::layout::{AttributeValue, Element, ElementKind, ElementNode, ElementNodeRepr, FontFamily, Lu, MainGapMode, MainSizeMode, ParsedAttributes, SelfDepAxis, PX_PER_LU};
use crate::resources::get_resource;
use crate::util::read_image_from_bytes;

const ZERO_LENGTH_GUARD: Lu = 20;

pub enum FixAxis {
    FixWidth,
    FixHeight,
}


#[derive(Copy, Clone, Debug, Default)]
pub enum SideParametricKind {
    Fixed,
    #[default]
    Stretchable,
    Dependent,
}

#[derive(Clone, Debug, Copy)]
pub enum ParametricStage {
    Parametric,
    PostParametric,
    ParentParametric,
}

#[derive(Clone, Debug, Copy)]
pub enum ParametricKind {
    Normal {
        width: SideParametricKind,
        height: SideParametricKind,
    },
    SelfDepBoth {
        stretch: bool
    }
}

impl Default for ParametricKind {
    fn default() -> Self {
        ParametricKind::Normal {
            width: SideParametricKind::default(),
            height: SideParametricKind::default(),
        }
    }
}

impl ParametricKind {
    pub fn width_to_height() -> Self {
        ParametricKind::Normal {
            width: SideParametricKind::Stretchable,
            height: SideParametricKind::Dependent,
        }
    }

    pub fn height_to_width() -> Self {
        ParametricKind::Normal {
            width: SideParametricKind::Dependent,
            height: SideParametricKind::Stretchable,
        }
    }

    pub fn fixed() -> Self {
        ParametricKind::Normal {
            width: SideParametricKind::Fixed,
            height: SideParametricKind::Fixed,
        }
    }

    pub fn is_height_to_width(&self) -> bool {
        match self {
            ParametricKind::Normal { height: SideParametricKind::Stretchable, width: SideParametricKind::Dependent } => false,
            ParametricKind::SelfDepBoth { .. } => true,
            _ => false
        }
    }

    pub fn is_width_to_height(&self) -> bool {
        match self {
            ParametricKind::Normal { width: SideParametricKind::Stretchable, height: SideParametricKind::Dependent } => false,
            ParametricKind::SelfDepBoth { .. } => true,
            _ => false
        }
    }

    pub fn is_fixed(&self) -> bool {
        matches!(self, ParametricKind::Normal { width: SideParametricKind::Fixed | SideParametricKind::Dependent, height: SideParametricKind::Fixed | SideParametricKind::Dependent })
    }

    pub fn is_width_stretch(&self) -> bool {
        match self {
            ParametricKind::Normal { width: SideParametricKind::Stretchable, .. } => true,
            ParametricKind::SelfDepBoth { stretch } => *stretch,
            _ => false
        }
    }

    pub fn is_height_stretch(&self) -> bool {
        match self {
            ParametricKind::Normal { height: SideParametricKind::Stretchable, .. } => true,
            ParametricKind::SelfDepBoth { stretch } => *stretch,
            _ => false
        }
    }

    pub fn is_selfdep_both(&self) -> bool {
        matches!(self, ParametricKind::SelfDepBoth{..})
    }
}

#[derive(Clone, Debug, Default)]
struct ParametricSolveState {
    min_width: Lu,
    min_height: Lu,
    kind: ParametricKind,
}
#[derive(Clone, Debug, Default)]
struct DimFixState {
    height: Option<Lu>,
    width: Option<Lu>,
    processed: bool,
}
#[derive(Clone, Debug, Default)]
struct PosFixState {
    pos_x: Lu,
    pos_y: Lu,
}

#[derive(Clone, Debug, Default)]
struct ElementSizes {
    parametric: ParametricSolveState,
    post_parametric: ParametricSolveState,
    parent_parametric: ParametricSolveState,
    dim_fix: DimFixState,
    pos_fix: PosFixState,
    has_problems: bool,
}

impl ElementSizes {
    pub fn parametric(&mut self, stage: ParametricStage) -> &mut ParametricSolveState {
        match stage {
            ParametricStage::Parametric => &mut self.parametric,
            ParametricStage::PostParametric => &mut self.post_parametric,
            ParametricStage::ParentParametric => &mut self.parent_parametric,
        }
    }
}

pub struct ElementCalculated {
    id: u32,
    kind: ElementKind,
    pos_x: f32,
    pos_y: f32,
}

#[derive(Copy, Clone)]
pub enum Phase {
    ParametricSolve,
    FixPass,
}

pub struct Elements(Vec<ElementNode>);

impl Elements {
    fn children(&mut self, i: usize) -> (&mut ElementNode, impl Iterator<Item = (usize, &ElementNode)> + Clone + '_) {
        let (before, after) = self.0.split_at_mut(i + 1);
        let parent = &mut before[i];
        let after_ref: &[ElementNode] = after;

        let mut next_i = after_ref.get(0)
            .is_some_and(|e| e.parent_i == i as u32)
            .then(|| i + 1);

        let children_iter = std::iter::from_fn(move || {
            let child_i = next_i?;
            let offset = child_i - i - 1;
            let node = &after_ref[offset];
            next_i = node.next_sibling_i.map(|n| n as usize);
            Some((child_i, node))
        });

        (parent, children_iter)
    }
}

impl Deref for Elements {
    type Target = Vec<ElementNode>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Elements {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

struct Calculated(Vec<ElementSizes>);

impl Deref for Calculated {
    type Target = Vec<ElementSizes>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Calculated {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub struct Images(HashMap<String, ImageInfo>);
impl Images {
    fn load_image(&mut self, src: String) -> &ImageInfo {
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

pub struct Fonts(HashMap<FontFamily, FontInfo>);
impl Fonts {
    pub fn load_font(&mut self, name: FontFamily) -> &mut FontInfo {
        self.entry(name.clone())
            .or_insert_with(|| {

                static BASIC_FONT: &'static [u8] = include_bytes!("../../fonts/Basic-Regular.ttf");
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

pub struct Texts(HashMap<u32, TextInfo>);

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
        let font = FontRef::from_index(&font.font_raw, 0).unwrap();

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

pub struct LayoutCalculator {
    elements: Elements,
    calculated: Calculated,
    images: Images,
    fonts: Fonts,
    texts: Texts
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
}

pub struct FontSizeInfo {
    // map of rendered glyphs for fixed size
    rendered_glyphs: HashMap<GlyphId, Image>,
}

#[derive(PartialEq, PartialOrd)]
enum ControlFlow {
    Continue,
    SkipChildren,
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

impl LayoutCalculator {
    pub fn new() -> Self {
        LayoutCalculator {
            elements: Elements(Vec::new()),
            calculated: Calculated(Vec::new()),
            images: Images(HashMap::new()),
            fonts: Fonts(HashMap::new()),
            texts: Texts(HashMap::new())
        }
    }

    pub fn init(&mut self, elements: Vec<ElementNodeRepr>) {
        let mut element_nodes = Vec::with_capacity(elements.len());
        let mut last_sibling_i: HashMap<u32, u32> = HashMap::new();
        for (i, elem) in elements.into_iter().enumerate() {
            let attributes = ParsedAttributes::from(elem.attributes);
            let element = Element::from((elem.element, &attributes));
            element_nodes.push(ElementNode {
                next_sibling_i: None,
                parent_i: elem.parent_i,
                element,
                general_attributes: attributes.general.unwrap_or_default(),
                self_child_attributes: attributes.self_child.unwrap_or_default(),
            });

            if i > 0 && let Some(last_sibling_i) = last_sibling_i.get(&elem.parent_i) {
                element_nodes[*last_sibling_i as usize].next_sibling_i = Some(i as u32);
            }

            last_sibling_i.insert(elem.parent_i, i as u32);
        }
    }

    pub fn hide_element(&mut self, element_id: u32) {

    }

    pub fn show_element(&mut self, element_id: u32) {

    }

    pub fn update_attribute(&mut self, element_id: u32, attr: AttributeValue) {
        self.elements[element_id as usize].apply(attr);
    }

    /// Phase 1.1: Parametric solve (dfs)
    /// fill in *.parametric, probably make subtree fix
    fn parametric_solve(&mut self, i: usize) {
        // let (me, ref children) = self.elements.children(i);
        let me = &mut self.elements[i];
        match &me.element {
            Element::Img(attrs) => {
                let me_calc = &mut self.calculated[i];
                let name = attrs.resource.clone();
                let img_info = self.images.load_image(name);
                if attrs.height.is_none() && attrs.width.is_none() {
                    me_calc.parametric.kind = ParametricKind::SelfDepBoth { stretch: true };
                }
                else {
                    me_calc.parametric.kind = ParametricKind::Normal {
                        width: SideParametricKind::Fixed,
                        height: SideParametricKind::Fixed,
                    };

                    if let Some(width) = attrs.width {
                        me_calc.parametric.min_width = width;
                        me_calc.parametric.min_height = (width as f32 * img_info.aspect) as Lu;
                    }
                    else if let Some(height) = attrs.height {
                        me_calc.parametric.min_height = height;
                        me_calc.parametric.min_width = (height as f32 / img_info.aspect) as Lu;
                    }
                    else {
                        unreachable!()
                    }

                    // set dim fixed
                    me_calc.post_parametric = me_calc.parametric.clone();
                    me_calc.parent_parametric = me_calc.parametric.clone();
                    me_calc.dim_fix.width = Some(me_calc.parametric.min_width);
                    me_calc.dim_fix.height = Some(me_calc.parametric.min_height);
                    me_calc.dim_fix.processed = true;
                }
            },
            Element::Box(attrs) => {
                let me_calc = &mut self.calculated[i];
                me_calc.parametric.kind = ParametricKind::Normal {
                    width: SideParametricKind::Stretchable,
                    height: SideParametricKind::Stretchable,
                };
            }
            Element::Text(attrs) => {
                let me_calc = &mut self.calculated[i];
                if attrs.preformat {
                    // Solve layout for text without width constraints
                    let font = self.fonts.load_font(attrs.font.clone());
                    let size = attrs.font_size.with_scale(1.0);

                    let text = self.texts.calculate_layout(i as u32, font, attrs.font.clone(), size, None);
                    me_calc.parametric.min_height = text.text_height;
                    if !attrs.hide_overflow {
                        me_calc.parametric.min_width = text.text_width;
                    }

                    me_calc.parametric.kind = ParametricKind::Normal {
                        width: if attrs.hide_overflow { SideParametricKind::Stretchable } else { SideParametricKind::Fixed },
                        height: SideParametricKind::Fixed,
                    };
                }
                else {
                    // Deferred layout calculation until width is known
                    if attrs.hide_overflow {
                        me_calc.parametric.kind = ParametricKind::Normal {
                            width: SideParametricKind::Stretchable,
                            height: SideParametricKind::Stretchable,
                        };
                    }
                    else {
                        me_calc.parametric.kind = ParametricKind::width_to_height();
                    }
                }

                if me_calc.parametric.kind.is_fixed() {
                    // set dim fixed
                    me_calc.post_parametric = me_calc.parametric.clone();
                    me_calc.parent_parametric = me_calc.parametric.clone();
                    me_calc.dim_fix.width = Some(me_calc.parametric.min_width);
                    me_calc.dim_fix.height = Some(me_calc.parametric.min_height);
                    me_calc.dim_fix.processed = true;
                }
            }
            Element::Row(attrs) => {
                // let grow_en = matches!(attrs.main_size_mode, MainSizeMode::EqualWidth);
                // let gap_en = matches!(attrs.main_gap_mode, MainGapMode::Around | MainGapMode::Between);
                // let cross_stretch_en = attrs.cross_stretch;
                // let (me, children) = self.elements.children(i);
                // self.calculated[i].has_problems = false;
                //
                // let has_selfdepx = grow_en && children.clone().any(|(j, _)| self.calculated[j].post_parametric.kind.is_height_to_width());
                // let has_selfdepy = children.clone().any(|(j, _)| !self.calculated[j].post_parametric.kind.is_width_to_height());
                // if has_selfdepx && has_selfdepy {
                //     self.calculated[i].has_problems = true;
                //     // Error case: stretchable selfdepX and selfdepY cannot exist in the same container!
                //
                // } else if has_selfdepx {
                //     // get rid of selfdepboth
                //     for (j, el) in children.clone() {
                //         if self.calculated[j].parametric.kind.is_both() {
                //             self.calculated[j].parametric.kind = ParametricKind::width_to_height();
                //         }
                //     }
                //     if grow_en {
                //         // fast path: fix selfdepx elements by min width
                //         let mut total_min_width = 0;
                //         let mut max_height = 0;
                //         for (j, el) in children.clone() {
                //             let (width, height) = if self.calculated[j].dim_fix.dim_fixed {
                //                 (self.calculated[j].dim_fix.width, self.calculated[j].dim_fix.height)
                //             }
                //             else {
                //             };
                //             total_min_width += width;
                //             if height > max_height {
                //                 max_height = height;
                //             }
                //         }
                //
                //         self.calculated[i].parametric.min_width = total_min_width;
                //         self.calculated[i].parametric.min_height = max_height;
                //     }
                //     // first handle x axis: handle stretch case
                //     let mut total_min_width = 0;
                //     let mut max_height = 0;
                //     for (j, el) in children.clone() {
                //         let min_width = self.calculated[j].min_width();
                //         total_min_width += min_width;
                //
                //         let min_height = self.calculated[j].min_height();
                //         if min_height > max_height {
                //             max_height = min_height;
                //         }
                //     }
                //
                //     self.calculated[i].parametric.min_width = total_min_width;
                //     self.calculated[i].parametric.min_height = max_height;
                // } else if has_selfdepy {
                // } else {
                // }
            }
            _ => {}
        }
    }

    /// Phase 1.2: Apply general attributes
    /// fill in *.post_parametric, probably make subtree fix
    /// Call guarantee: parametric is not fixed
    fn apply_general_attrs(&mut self, i: usize) {
        self.calculated[i].post_parametric = self.calculated[i].parametric.clone();
        self.calculated[i].post_parametric.min_width = max(self.elements[i].general_attributes.min_width, self.calculated[i].post_parametric.min_width);
        self.calculated[i].post_parametric.min_height = max(self.elements[i].general_attributes.min_height, self.calculated[i].post_parametric.min_height);
        if self.elements[i].general_attributes.nostretch_x {
            if let ParametricKind::SelfDepBoth {stretch} = &mut self.calculated[i].post_parametric.kind {
                *stretch = true;
            }
            else if self.try_fix_axis_subtree(i, None, FixAxis::FixWidth, ParametricStage::PostParametric) {
                self.calculated[i].parent_parametric = self.calculated[i].post_parametric.clone();
                return;
            }
        }
        if self.elements[i].general_attributes.nostretch_y {
            if let ParametricKind::SelfDepBoth {stretch} = &mut self.calculated[i].post_parametric.kind {
                *stretch = true;
            }
            else if self.try_fix_axis_subtree(i, None, FixAxis::FixHeight, ParametricStage::PostParametric) {
                self.calculated[i].parent_parametric = self.calculated[i].post_parametric.clone();
                return;
            }
        }

        if let ParametricKind::SelfDepBoth {stretch} = self.calculated[i].post_parametric.kind {
            match self.elements[i].general_attributes.self_dep_axis {
                SelfDepAxis::HeightFromWidth => {
                    if stretch {
                        // <- nostretch_x is not enabled, transform to selfdepx
                        self.calculated[i].post_parametric.kind = ParametricKind::width_to_height();
                    }
                    else {
                        // cannot stay selfdepx with stretch disabled - subtree fix
                        if self.try_fix_axis_subtree(i, None, FixAxis::FixWidth, ParametricStage::PostParametric) {
                            self.calculated[i].parent_parametric = self.calculated[i].post_parametric.clone();
                        }
                    }
                }
                SelfDepAxis::WidthFromHeight => {
                    if stretch {
                        // <- nostretch_y is not enabled, transform to selfdepy
                        self.calculated[i].post_parametric.kind = ParametricKind::height_to_width();
                    }
                    else {
                        // cannot stay selfdepy with stretch disabled - subtree fix
                        if self.try_fix_axis_subtree(i, None, FixAxis::FixHeight, ParametricStage::PostParametric) {
                            self.calculated[i].parent_parametric = self.calculated[i].post_parametric.clone();
                        }
                    }
                }
                SelfDepAxis::Both => {}
            }
        }
    }

    /// Phase 1.3: Apply parent constraints
    /// fill in *.parent_parametric, probably make subtree fix
    /// Call guarantee: post parametric is not fixed
    fn apply_parent_constraints(&mut self, i: usize) {
        self.calculated[i].parent_parametric = self.calculated[i].post_parametric.clone();
        let (me, children) = self.elements.children(i);
        match &mut me.element {
            Element::Row(attrs) => {
                let grow_en = matches!(attrs.main_size_mode, MainSizeMode::EqualWidth);
                if !grow_en {
                    // fix width for all children
                    for (j, children) in children.clone() {
                        // if self.calculated[j].
                    }
                }

                let cross_stretch_en = attrs.cross_stretch;
                if !cross_stretch_en {
                    // fix height for all children
                }
            }
            _ => {}
        }
    }

    /// Fix element axis subtree with length guard. Recursively spawn DFS if fully fix.
    /// If another axis is still stretchable, new fix dfs is not spawned, and false is returned
    fn try_fix_axis_subtree(&mut self, i: usize, length: Option<Lu>, fix_axis: FixAxis, parametric_stage: ParametricStage) -> bool {
        let calculated = &mut self.calculated[i];

        let mut length = length.unwrap_or_else(|| {
            match fix_axis {
                FixAxis::FixWidth => calculated.parametric(parametric_stage).min_width,
                FixAxis::FixHeight => calculated.parametric(parametric_stage).min_height,
            }
        });

        if length == 0 {
            length = ZERO_LENGTH_GUARD;
            calculated.has_problems = true;
        }
        match fix_axis {
            FixAxis::FixWidth => {
                if matches!(calculated.parametric(parametric_stage).kind, ParametricKind::Normal { width: SideParametricKind::Dependent | SideParametricKind::Fixed, .. }) {
                    return false
                }

                if matches!(calculated.parametric(parametric_stage).kind, ParametricKind::SelfDepBoth { stretch: true }) {
                    panic!("Assertion failed! width subtree fix called on selfdepboth with stretch enabled!")
                }

                calculated.dim_fix.width = Some(length);
                match &mut calculated.parametric(parametric_stage).kind {
                    ParametricKind::Normal {
                        width,
                        height
                    } => {
                        *width = SideParametricKind::Fixed;
                        if matches!(height,  SideParametricKind::Stretchable) {
                            false
                        }
                        else {
                            // Launch new DFS to fix selfdep subtree with new length constraint
                            self.dfs(i, Phase::FixPass);
                            true
                        }
                    }
                    ParametricKind::SelfDepBoth {
                        ..
                    } => {
                        calculated.parametric(parametric_stage).kind = ParametricKind::Normal {
                            width: SideParametricKind::Fixed,
                            height: SideParametricKind::Dependent,
                        };
                        // Launch new DFS to fix selfdep subtree with new length constraint
                        self.dfs(i, Phase::FixPass);
                        true
                    }
                }
            }
            FixAxis::FixHeight => {
                if matches!(calculated.parametric(parametric_stage).kind, ParametricKind::Normal { height: SideParametricKind::Dependent | SideParametricKind::Fixed, .. }) {
                    return false
                }
                if matches!(calculated.parametric(parametric_stage).kind, ParametricKind::SelfDepBoth { stretch: true }) {
                    panic!("Assertion failed! height subtree fix called on selfdepboth with stretch enabled!")
                }

                calculated.dim_fix.height = Some(length);
                match &mut calculated.parametric(parametric_stage).kind {
                    ParametricKind::Normal {
                        width,
                        height
                    } => {
                        *height = SideParametricKind::Fixed;
                        if matches!(width,  SideParametricKind::Stretchable) {
                            false
                        } else {
                            // Launch new DFS to fix selfdep subtree with new length constraint
                            self.dfs(i, Phase::FixPass);
                            true
                        }
                    }
                    ParametricKind::SelfDepBoth {
                        ..
                    } => {
                        calculated.parametric(parametric_stage).kind = ParametricKind::Normal {
                            width: SideParametricKind::Dependent,
                            height: SideParametricKind::Fixed,
                        };
                        // Launch new DFS to fix selfdep subtree with new length constraint
                        self.dfs(i, Phase::FixPass);
                        true
                    }
                }
            }
        }
    }

    fn handle_node(&mut self, i: usize, parents: &[usize], phase: Phase) -> ControlFlow {
        match phase {
            Phase::ParametricSolve => {
                ControlFlow::Continue
            }
            Phase::FixPass => {
                ControlFlow::Continue
            }
        }
    }

    fn finalize_node(&mut self, i: usize, phase: Phase) {
        if matches!(phase, Phase::ParametricSolve) {
            self.parametric_solve(i);
            if self.calculated[i].parametric.kind.is_fixed() {
                return;
            }
            self.apply_general_attrs(i);
            if self.calculated[i].post_parametric.kind.is_fixed() {
                return;
            }
            self.apply_parent_constraints(i);
        }
    }


    pub fn calculate_layout(&mut self, width: u32, height: u32) {
        // reset on each recalculation for now
        for el in self.calculated.iter_mut() {
            *el = Default::default();
        }

        self.dfs(0, Phase::ParametricSolve);
        self.dfs(0, Phase::FixPass);
    }
    pub fn dfs(&mut self, first_element: usize, phase: Phase) {
        let mut parents = vec![first_element];
        if self.handle_node(first_element, &[], phase) == ControlFlow::SkipChildren {
            self.finalize_node(first_element, phase);
            return;
        }
        let mut i = first_element + 1;
        while i < self.elements.len() {
            let mut last_parent = *parents.last().unwrap();
            while self.elements[i].parent_i < last_parent as u32 {
                // we just left a container
                self.finalize_node(last_parent, phase);
                parents.pop();
                if parents.is_empty() {
                    return;
                }
                last_parent = *parents.last().unwrap();
            }


            if self.handle_node(i, &parents, phase) == ControlFlow::SkipChildren {
                self.finalize_node(i, phase);
                // Skip all descendants by advancing until we find a node that's not a child
                let skip_below = i;
                i += 1;
                while i < self.elements.len() && self.elements[i].parent_i > skip_below as u32 {
                    i += 1;
                }
            } else {
                parents.push(i);
                i += 1;
            }
        }

        while let Some(parent) = parents.pop() {
            self.finalize_node(parent, phase);
        }
    }

    pub fn get_elements(&self) -> Vec<ElementCalculated> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Element, GeneralAttributes, ChildAttributes};

    fn make_test_node(parent_i: u32, next_sibling_i: Option<u32>) -> ElementNode {
        ElementNode {
            parent_i,
            next_sibling_i,
            element: Element::Box(Default::default()),
            general_attributes: GeneralAttributes::default(),
            self_child_attributes: ChildAttributes::default(),
        }
    }

    #[test]
    fn test_children_no_children() {
        let mut elements = Elements(vec![
            make_test_node(0, None),
        ]);

        let (parent, mut iter) = elements.children(0);
        assert_eq!(parent.parent_i, 0);
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_children_single_child() {
        let mut elements = Elements(vec![
            make_test_node(0, None),      // parent at index 0
            make_test_node(0, None),      // child at index 1
        ]);

        let (parent, mut iter) = elements.children(0);
        assert_eq!(parent.parent_i, 0);

        let (idx, child) = iter.next().unwrap();
        assert_eq!(idx, 1);
        assert_eq!(child.parent_i, 0);
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_children_multiple_children() {
        let mut elements = Elements(vec![
            make_test_node(0, None),      // parent at index 0
            make_test_node(0, Some(2)),   // child 1 at index 1
            make_test_node(0, Some(3)),   // child 2 at index 2
            make_test_node(0, None),      // child 3 at index 3
        ]);

        let (parent, mut iter) = elements.children(0);
        assert_eq!(parent.parent_i, 0);

        let (idx, child) = iter.next().unwrap();
        assert_eq!(idx, 1);
        assert_eq!(child.parent_i, 0);

        let (idx, child) = iter.next().unwrap();
        assert_eq!(idx, 2);
        assert_eq!(child.parent_i, 0);

        let (idx, child) = iter.next().unwrap();
        assert_eq!(idx, 3);
        assert_eq!(child.parent_i, 0);

        assert!(iter.next().is_none());
    }

    #[test]
    fn test_children_with_different_parent() {
        let mut elements = Elements(vec![
            make_test_node(0, None),      // parent at index 0
            make_test_node(1, None),      // not a child of 0 (different parent)
        ]);

        let (parent, mut iter) = elements.children(0);
        assert_eq!(parent.parent_i, 0);

        assert!(iter.next().is_none());
    }

    #[test]
    fn test_children_iterator_is_cloneable() {
        let mut elements = Elements(vec![
            make_test_node(0, None),      // parent at index 0
            make_test_node(0, Some(2)),   // child 1 at index 1
            make_test_node(0, None),      // child 2 at index 2
        ]);

        let (_parent, iter) = elements.children(0);
        let mut iter1 = iter.clone();
        let mut iter2 = iter.clone();

        assert_eq!(iter1.next().unwrap().0, 1);
        assert_eq!(iter1.next().unwrap().0, 2);
        assert!(iter1.next().is_none());

        assert_eq!(iter2.next().unwrap().0, 1);
        assert_eq!(iter2.next().unwrap().0, 2);
        assert!(iter2.next().is_none());
    }

    #[test]
    fn test_children_parent_mutation() {
        let mut elements = Elements(vec![
            make_test_node(0, None),
            make_test_node(0, None),
        ]);

        let (parent, mut iter) = elements.children(0);
        parent.parent_i = 99;

        let (_idx, child) = iter.next().unwrap();
        assert_eq!(child.parent_i, 0);
        assert_eq!(parent.parent_i, 99);
    }
}