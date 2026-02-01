use std::cmp::max;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::ops::{Deref, DerefMut};
use log::{error, info, warn};
use swash::FontRef;
use swash::scale::{ScaleContext, Source, StrikeWith};
use swash::shape::cluster::Glyph;
use swash::shape::Direction;
use swash::text::Script;
use crate::layout::{AttributeValue, Element, ElementKind, ElementNode, ElementNodeRepr, Lu, MainGapMode, MainSizeMode, ParsedAttributes, SelfDepAxis, PX_PER_LU};
use crate::resources::get_resource;
use crate::util::read_image_from_bytes;

const ZERO_LENGTH_GUARD: Lu = 20;

pub enum FixAxis {
    FixWidth,
    FixHeight,
    NormalFix,
}


#[derive(Clone, Debug, Default)]
pub enum SideParametricKind {
    Fixed,
    #[default]
    Stretchable,
    Dependent,
}

#[derive(Clone, Debug)]
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
    pub fn can_fix_width(&self) -> bool {
        match self {
            ParametricKind::Normal { width, .. } => matches!(width, SideParametricKind::Stretchable),
            ParametricKind::SelfDepBoth { .. } => true,
        }
    }

    pub fn can_fix_height(&self) -> bool {
        match self {
            ParametricKind::Normal { height, .. } => matches!(height, SideParametricKind::Stretchable),
            ParametricKind::SelfDepBoth { .. } => true,
        }
    }

    pub fn is_unidir_selfdep(&self) -> bool {
        match self {
            ParametricKind::Normal { width, height } => {
                matches!(width, SideParametricKind::Dependent) | matches!(height, SideParametricKind::Dependent)
            }
            ParametricKind::SelfDepBoth { .. } => false,
        }
    }

    pub fn disable_stretch_x(&mut self) -> Option<FixAxis> {
        match self {
            ParametricKind::Normal { width, height } => {
                if matches!(width, SideParametricKind::Stretchable) {
                    *width = SideParametricKind::Fixed;
                    match height {
                        SideParametricKind::Dependent => {
                            // need fix
                            *height = SideParametricKind::Fixed;
                            Some(FixAxis::FixHeight)
                        }
                        SideParametricKind::Fixed => {
                            Some(FixAxis::NormalFix)
                        }
                        SideParametricKind::Stretchable => {
                            None
                        }
                    }
                } else {
                    None
                }
            }
            ParametricKind::SelfDepBoth { stretch } => {
                *stretch = false;
                None
            }
        }
    }

    pub fn disable_stretch_y(&mut self) -> Option<FixAxis> {
        match self {
            ParametricKind::Normal { width, height } => {
                if matches!(height, SideParametricKind::Stretchable) {
                    *height = SideParametricKind::Fixed;
                    match width {
                        SideParametricKind::Dependent => {
                            // need fix
                            *width = SideParametricKind::Fixed;
                            Some(FixAxis::FixWidth)
                        }
                        SideParametricKind::Fixed => {
                            Some(FixAxis::NormalFix)
                        }
                        SideParametricKind::Stretchable => {
                            None
                        }
                    }
                } else {
                    None
                }
            }
            ParametricKind::SelfDepBoth { stretch } => {
                *stretch = false;
                None
            }
        }
    }

    pub fn is_both(&self) -> bool {
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
    dim_fixed: bool, // Set to true during subtree fix or dim fix pass
    height: Lu,
    width: Lu,
}
#[derive(Clone, Debug, Default)]
struct PosFixState {
    // pub rel_pos_x: Lu,
    // pub rel_pos_y: Lu,
    pos_x: Lu,
    pos_y: Lu,
}

#[derive(Clone, Debug, Default)]
struct ElementSizes {
    parametric: ParametricSolveState,
    post_parametric: ParametricSolveState,
    dim_fix: DimFixState,
    pos_fix: PosFixState,
    has_problems: bool,
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

pub struct Fonts(HashMap<String, FontInfo>);
impl Fonts {
    pub fn load_font(&mut self, name: String) -> &FontInfo {
        self.entry(name.clone())
            .or_insert_with(|| {
                static BASIC_FONT: &'static [u8] = include_bytes!("../../fonts/Basic-Regular.ttf");
                let font_data = match get_resource(Path::new("fonts").join(&name)) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        error!("Failed to load font resource '{}': {}", name, e);
                        return FontInfo {
                            default_line_height: 16.0,
                            font_raw: BASIC_FONT.to_vec(),
                        }
                    }
                };
                let font = match FontRef::from_index(&font_data, 0) {
                    Some(font) => font,
                    None => {
                        error!("Failed to parse font '{}'", name);
                        return FontInfo {
                            default_line_height: 16.0,
                            font_raw: BASIC_FONT.to_vec(),
                        }
                    }
                };

                info!("Loaded font '{}' with attributes: {:?}", name, font.attributes());

                FontInfo {
                    default_line_height: 16.0,
                    font_raw: font_data,
                }
            })
    }
}

impl Deref for Fonts {
    type Target = HashMap<String, FontInfo>;

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
    pub fn calculate_layout(&mut self, text_id: u32, font: &FontInfo, size: f32, width_constraint: Option<Lu>) -> TextInfo {
        let mut text = self.get(&text_id)
            .cloned()
            .unwrap_or(TextInfo {
                value: Arc::from(""),
                layout_calculated: false,
                text_height: 0,
                text_width: 0,
                glyphs: Vec::new(),
            });

        if text.layout_calculated {
            return text;
        }

        let font = FontRef::from_index(&font.font_raw, 0).unwrap();

        let mut context = swash::shape::ShapeContext::new();
        let mut shaper = context.builder(font)
            .script(Script::Common)
            .direction(Direction::LeftToRight)
            .size(size)
            .build();


        shaper.add_str(&text.value);

        let metrics = shaper.metrics();
        let width = metrics.max_width;
        let line_height = metrics.ascent + metrics.descent + metrics.leading;
        shaper.shape_with(|cluster| {
            text.glyphs.extend_from_slice(cluster.glyphs);
        });
        text.text_width = (width * PX_PER_LU as f32) as Lu;
        text.text_height = (line_height * PX_PER_LU as f32) as Lu;


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
        self.insert(text_id, TextInfo {
            value,
            layout_calculated: false,
            text_height: 0,
            text_width: 0,
            glyphs: Vec::new(),
        });
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
}

#[derive(PartialEq, PartialOrd)]
enum ControlFlow {
    Continue,
    SkipChildren,
}

#[derive(Clone)]
pub struct TextInfo {
    value: Arc<str>,
    layout_calculated: bool,
    text_height: Lu,
    text_width: Lu,
    glyphs: Vec<Glyph>,
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

    /// Phase 1: Parametric solve (dfs)
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
                    let text = self.texts.calculate_layout(i as u32, font, size, None);

                    me_calc.parametric.min_height = text.text_height;

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
            }
            Element::Row(attrs) => {
                let grow_en = matches!(attrs.main_size_mode, MainSizeMode::EqualWidth);
                let gap_en = matches!(attrs.main_gap_mode, MainGapMode::Around | MainGapMode::Between);
                let cross_stretch_en = attrs.cross_stretch;
                let (me, children) = self.elements.children(i);
                self.calculated[i].has_problems = false;

                let has_selfdepx = grow_en && children.clone().any(|(j, _)| !self.calculated[j].post_parametric.kind.can_fix_height() && self.calculated[j].post_parametric.kind.is_unidir_selfdep());
                let has_selfdepy = children.clone().any(|(j, _)| !self.calculated[j].post_parametric.kind.can_fix_width() && self.calculated[j].post_parametric.kind.is_unidir_selfdep());
                let main_axis_x = if has_selfdepx && has_selfdepy {
                    self.calculated[i].has_problems = true;
                    // Error case: stretchable selfdepX and selfdepY cannot exist in the same container!
                    true
                } else if has_selfdepx {

                    true
                } else if has_selfdepy {
                    false
                } else {
                    true
                };
            }
            _ => {}
        }
    }
    fn fix_axis_subtree(&mut self, i: usize, need_fix: FixAxis) {
        let mut length = match need_fix {
            FixAxis::FixWidth => self.calculated[i].post_parametric.min_width,
            FixAxis::FixHeight => self.calculated[i].post_parametric.min_height,
            FixAxis::NormalFix => self.calculated[i].post_parametric.min_width,
        };
        if length == 0 {
            length = ZERO_LENGTH_GUARD;
            self.calculated[i].has_problems = true;
        }
        match need_fix {
            FixAxis::FixWidth => {
                // fix width for selfdepx/selfdepboth subtree rooted at i
            }
            FixAxis::FixHeight => {
                // fix height for selfdepy/selfdepboth subtree rooted at i
            }
            FixAxis::NormalFix => {
                let width = length;
                let height = self.calculated[i].post_parametric.min_height;
                if height == 0 {
                    length = ZERO_LENGTH_GUARD;
                    self.calculated[i].has_problems = true;
                }


            }
        }
    }
    fn apply_general_attrs(&mut self, i: usize) {
        self.calculated[i].post_parametric = self.calculated[i].parametric.clone();
        self.calculated[i].post_parametric.min_width = max(self.elements[i].general_attributes.min_width, self.calculated[i].post_parametric.min_width);
        self.calculated[i].post_parametric.min_height = max(self.elements[i].general_attributes.min_height, self.calculated[i].post_parametric.min_height);
        if self.elements[i].general_attributes.nostretch_x {
            if let Some(need_fix) = self.calculated[i].post_parametric.kind.disable_stretch_x() {
                self.fix_axis_subtree(i, need_fix);
            }
        }
        if self.elements[i].general_attributes.nostretch_y {
            if let Some(need_fix) = self.calculated[i].post_parametric.kind.disable_stretch_y() {
                self.fix_axis_subtree(i, need_fix);
            }
        }

        if let ParametricKind::SelfDepBoth {stretch} = self.calculated[i].post_parametric.kind {
            match self.elements[i].general_attributes.self_dep_axis {
                SelfDepAxis::HeightFromWidth => {
                    // transform to selfdepx
                    if stretch {
                        self.calculated[i].post_parametric.kind = ParametricKind::width_to_height();
                    }
                    else {
                        self.calculated[i].post_parametric.kind = ParametricKind::fixed();
                        self.fix_axis_subtree(i, FixAxis::FixWidth);
                    }
                }
                SelfDepAxis::WidthFromHeight => {
                    // transform to selfdepy
                    if stretch {
                        self.calculated[i].post_parametric.kind = ParametricKind::height_to_width();
                    }
                    else {
                        self.calculated[i].post_parametric.kind = ParametricKind::fixed();
                        self.fix_axis_subtree(i, FixAxis::FixHeight);
                    }
                }
                SelfDepAxis::Both => {}
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
            self.apply_general_attrs(i);
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