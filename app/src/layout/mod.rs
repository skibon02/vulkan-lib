use std::sync::Arc;
use smallvec::{SmallVec, smallvec};
use ui_macro::{AttributeEnum, generate_parsed_attributes};

pub mod calculator;

// Generate ParsedAttributes struct and From<Vec<AttributeValue>> implementation
generate_parsed_attributes!();

/// Initial layout structure
#[derive(Clone, Debug)]
pub enum Element {
    Col(ColAttributes),
    Row(RowAttributes),
    Stack(StackAttributes),
    Img(ImgAttributes),
    Text(TextAttributes),
    Box(BoxAttributes)
}

impl Element {
    pub fn kind(&self) -> ElementKind {
        match self {
            Element::Col(_) => ElementKind::Col,
            Element::Row(_) => ElementKind::Row,
            Element::Stack(_) => ElementKind::Stack,
            Element::Img(_) => ElementKind::Img,
            Element::Text(_) => ElementKind::Text,
            Element::Box(_) => ElementKind::Box,
        }
    }
}

impl From<(ElementKind, &ParsedAttributes)> for Element {
    fn from((element_kind, attributes): (ElementKind, &ParsedAttributes)) -> Self {
        match element_kind {
            ElementKind::Dynamic => panic!("Cannot create Element from Dynamic ElementKind"),
            ElementKind::Col => Element::Col(attributes.col.clone().unwrap_or_default()),
            ElementKind::Row => Element::Row(attributes.row.clone().unwrap_or_default()),
            ElementKind::Stack => Element::Stack(attributes.stack.clone().unwrap_or_default()),
            ElementKind::Img => Element::Img(attributes.img.clone().unwrap_or_default()),
            ElementKind::Text => Element::Text(attributes.text.clone().unwrap_or_default()),
            ElementKind::Box => Element::Box(attributes.box_attr.clone().unwrap_or_default()),
        }
    }
}

pub enum ElementKind {
    Col,
    Row,
    Stack,
    Img,
    Text,
    Box,

    Dynamic
}

impl ElementKind {
    pub fn is_container(&self) -> bool {
        matches!(self, ElementKind::Box | ElementKind::Row | ElementKind::Col)
    }
}

pub enum AttributeValue {
    Col(ColValue),
    Row(RowValue),
    Stack(StackValue),
    ColChild(ColChildValue, bool),
    RowChild(RowChildValue, bool),
    StackChild(StackChildValue, bool),
    Img(ImgValue),
    Text(TextValue),
    Box(BoxValue),
    General(GeneralValue),
}

#[derive(Clone, Debug)]
pub struct ElementNode {
    parent_i: u32,
    next_sibling_i: Option<u32>,
    element: Element,
    general_attributes: GeneralAttributes,
    self_child_attributes: ChildAttributes
}

impl ElementNode {
    pub fn apply(&mut self, attr: AttributeValue) {
        if let AttributeValue::General(general) = &attr {
            self.general_attributes.apply(general.clone());
        }
        if let AttributeValue::ColChild(col, false) = &attr {
            self.self_child_attributes.col.apply(col.clone());
        }
        if let AttributeValue::RowChild(row, false) = &attr {
            self.self_child_attributes.row.apply(row.clone());
        }
        if let AttributeValue::StackChild(stack, false) = &attr {
            self.self_child_attributes.stack.apply(stack.clone());
        }
        if let AttributeValue::ColChild(col, true) = &attr && let Element::Col(el) = &mut self.element{
            el.children_default.apply(col.clone());
        }
        if let AttributeValue::RowChild(row, true) = &attr && let Element::Row(el) = &mut self.element{
            el.children_default.apply(row.clone());
        }
        if let AttributeValue::StackChild(row, true) = &attr && let Element::Stack(el) = &mut self.element{
            el.children_default.apply(row.clone());
        }
        if let AttributeValue::Row(row) = &attr && let Element::Row(el) = &mut self.element{
            el.apply(row.clone())
        }
        if let AttributeValue::Col(row) = &attr && let Element::Col(el) = &mut self.element{
            el.apply(row.clone())
        }
        if let AttributeValue::Stack(row) = &attr && let Element::Stack(el) = &mut self.element{
            el.apply(row.clone())
        }
        if let AttributeValue::Img(row) = &attr && let Element::Img(el) = &mut self.element{
            el.apply(row.clone())
        }
        if let AttributeValue::Text(row) = &attr && let Element::Text(el) = &mut self.element{
            el.apply(row.clone())
        }
        if let AttributeValue::Box(row) = &attr && let Element::Box(el) = &mut self.element{
            el.apply(row.clone())
        }
    }
}

pub type AttributeValues = SmallVec<[AttributeValue; 5]>;

pub struct ElementNodeRepr {
    pub parent_i: u32,
    pub element: ElementKind,
    pub attributes: AttributeValues
}

#[derive(Copy, Clone, Debug, Default)]
pub enum XAlign {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Copy, Clone, Debug, Default)]
pub enum YAlign {
    Top,
    #[default]
    Center,
    Bottom,
}

#[derive(Copy, Clone, Debug)]
pub enum Align {
    Begin,
    Center,
    End
}

impl From<XAlign> for Align {
    fn from(x: XAlign) -> Self {
        match x {
            XAlign::Left => Align::Begin,
            XAlign::Center => Align::Center,
            XAlign::Right => Align::End,
        }
    }
}

impl From<YAlign> for Align {
    fn from(y: YAlign) -> Self {
        match y {
            YAlign::Top => Align::Begin,
            YAlign::Center => Align::Center,
            YAlign::Bottom => Align::End,
        }
    }
}

impl From<Align> for XAlign {
    fn from(align: Align) -> Self {
        match align {
            Align::Begin => XAlign::Left,
            Align::Center => XAlign::Center,
            Align::End => XAlign::Right,
        }
    }
}

impl From<Align> for YAlign {
    fn from(align: Align) -> Self {
        match align {
            Align::Begin => YAlign::Top,
            Align::Center => YAlign::Center,
            Align::End => YAlign::Bottom,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Color(pub u8, pub u8, pub u8, pub f32);

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

impl Color {
    pub const BLACK: Color = Color(0, 0, 0, 1.0);
    pub const WHITE: Color = Color(255, 255, 255, 1.0);
    pub const PURPLE: Color = Color(150, 50, 220, 1.0);
    pub const SKY: Color = Color(140, 210, 250, 1.0);
}

#[derive(Clone, Debug)]
pub enum Fill {
    Solid(Color),
    Custom(String)
}

impl Default for Fill {
    fn default() -> Self {
        Self::Solid(Color::default())
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub enum MainSizeMode {
    #[default]
    EqualWidth,
    Min
}

#[derive(Copy, Clone, Debug, Default)]
pub enum MainGapMode {
    #[default]
    Between,
    Around,
    Fixed(Lu),
    None
}

#[derive(Copy, Clone, Debug, Default)]
pub enum SelfDepAxis {
    HeightFromWidth,
    WidthFromHeight,
    #[default]
    Both
}

#[derive(Copy, Clone, Debug, Default)]
pub enum SelfDepMode {
    #[default]
    FixAxis,
    Cover,
    Fit,
}

/// LU = Layout Unit (pixel for now)
pub type Lu = u32;
const PX_PER_LU: u32 = 10;

#[derive(Copy, Clone, Debug, AttributeEnum)]
pub struct GeneralAttributes {
    pub min_width: Lu,
    pub min_height: Lu,
    pub nostretch_x: bool,
    pub nostretch_y: bool,
    pub margin_x: Lu,
    pub margin_y: Lu,
    pub self_dep_axis: SelfDepAxis,
    pub opacity: f32,
}

impl Default for GeneralAttributes {
    fn default() -> Self {
        Self {
            min_width: 0,
            min_height: 0,
            nostretch_x: false,
            nostretch_y: false,
            margin_x: 0,
            margin_y: 0,
            opacity: 1.0,
            self_dep_axis: SelfDepAxis::default(),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum FontSize {
    Em(f32),
    Lu(Lu)
}

impl FontSize {
    pub fn with_scale(self, scale: f32) -> f32 {
        match self {
            FontSize::Em(em) => em,
            FontSize::Lu(lu) => lu as f32 * scale
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FontFamily {
    Named(Arc<str>),
    Default,
}

#[derive(Clone, Debug, AttributeEnum)]
pub struct TextAttributes {
    /// Disable automatic line breaks, make height fixed
    pub preformat: bool,
    /// preformat=true:
    ///     hide right overflow without additional line breaks
    /// preformat=false:
    ///     free stretching,
    ///     insert "..." if text is not fully fit to the parent-dependent container size
    pub hide_overflow: bool,
    pub font_size: FontSize,
    pub font_weight: u16,
    pub font: FontFamily,
    pub text_align_x: XAlign,
    pub text_align_y: YAlign,
    pub line_height: Option<Lu>,
    pub symbols_limit: Option<u32>,
    pub text_color: Fill,
}

impl Default for TextAttributes {
    fn default() -> Self {
        Self {
            preformat: false,
            hide_overflow: false,
            font_size: FontSize::Em(1.0),
            font_weight: 400,
            font: FontFamily::Default,
            line_height: None,
            text_align_x: XAlign::default(),
            text_align_y: YAlign::default(),
            symbols_limit: None,
            text_color: Fill::default(),
        }
    }
}

#[derive(Clone, Debug, AttributeEnum)]
pub struct ImgAttributes {
    pub resource: String,
    pub width: Option<Lu>,
    pub height: Option<Lu>,
}

impl Default for ImgAttributes {
    fn default() -> Self {
        Self {
            resource: String::new(),
            width: None,
            height: None,
        }
    }
}

#[derive(Clone, Debug, Default, AttributeEnum)]
pub struct BoxAttributes {
    pub fill: Option<Fill>,
    pub round_corners: Option<Lu>,
}

#[derive(Clone, Debug, Default, AttributeEnum)]
pub struct RowAttributes {
    pub main_size_mode: MainSizeMode,
    pub main_gap_mode: MainGapMode,
    pub main_align: XAlign,
    /// Sets desire to stretch in cross axis to parent
    pub cross_stretch: bool,
    pub separator_width: Option<Lu>,
    pub separator_fill: Fill,
    pub children_default: RowChildAttributes,
}

#[derive(Clone, Debug, AttributeEnum)]
pub struct ColAttributes {
    pub main_size_mode: MainSizeMode,
    pub main_gap_mode: MainGapMode,
    pub main_align: YAlign,
    /// Sets desire to stretch in cross axis to parent
    pub cross_stretch: bool,
    pub separator_width: Option<Lu>,
    pub separator_fill: Fill,
    pub children_default: ColChildAttributes,
}

impl Default for ColAttributes {
    fn default() -> Self {
        Self {
            main_size_mode: MainSizeMode::default(),
            main_gap_mode: MainGapMode::default(),
            main_align: YAlign::default(),
            cross_stretch: true,
            separator_width: None,
            separator_fill: Fill::default(),
            children_default: ColChildAttributes::default(),
        }
    }
}

#[derive(Clone, Debug, Default, AttributeEnum)]
pub struct StackAttributes {
    pub children_default: StackChildAttributes,
}


#[derive(Clone, Debug, Default)]
struct ChildAttributes {
    stack: StackChildAttributes,
    row: RowChildAttributes,
    col: ColChildAttributes,
}

#[derive(Clone, Debug, AttributeEnum)]
pub struct RowChildAttributes {
    pub cross_align: YAlign,
}
impl Default for RowChildAttributes {
    fn default() -> Self {
        Self {
            cross_align: YAlign::default(),
        }
    }
}

#[derive(Clone, Debug, AttributeEnum)]
pub struct ColChildAttributes {
    pub cross_align: XAlign,
}
impl Default for ColChildAttributes {
    fn default() -> Self {
        Self {
            cross_align: XAlign::default(),
        }
    }
}
#[derive(Clone, Debug, AttributeEnum)]
pub struct StackChildAttributes {
    pub stretch_x: bool,
    pub stretch_y: bool,
    pub align_x: XAlign,
    pub align_y: YAlign,
    pub self_dep_mode: SelfDepMode,
}

impl Default for StackChildAttributes {
    fn default() -> Self {
        Self {
            stretch_x: true,
            stretch_y: true,
            align_x: XAlign::Center,
            align_y: YAlign::Center,
            self_dep_mode: SelfDepMode::FixAxis,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attribute_value_generation() {
        // Test that the generated enums exist
        let _text_val = TextValue::FontSize(FontSize::Em(1.0));
        let _general_val = GeneralValue::Opacity(0.5);
        let _box_val = BoxValue::Fill(Some(Fill::Solid(Color::WHITE)));

        // Test From<Vec<*Value>> for *Attributes - should apply each value
        let text_attrs: TextAttributes = vec![
            TextValue::FontSize(FontSize::Em(1.0)),
            TextValue::Preformat(true),
            TextValue::FontWeight(700),
        ].into();
        assert_eq!(text_attrs.font_size, FontSize::Em(1.0));
        assert_eq!(text_attrs.preformat, true);
        assert_eq!(text_attrs.font_weight, 700);
        // Other fields should be default
        assert_eq!(text_attrs.hide_overflow, false);

        // Test From<*Value> for *Attributes
        let general_attrs: GeneralAttributes = GeneralValue::Opacity(0.8).into();
        assert_eq!(general_attrs.opacity, 0.8);
        // Other fields should be default
        assert_eq!(general_attrs.margin_x, 0);
    }

    #[test]
    fn test_attribute_value_last_wins() {
        // Test that last value wins when duplicates exist
        let text_attrs: TextAttributes = vec![
            TextValue::FontSize(FontSize::Em(1.5)),
            TextValue::FontSize(FontSize::Em(2.0)),  // This should win
            TextValue::Preformat(false),
            TextValue::Preformat(true),   // This should win
        ].into();
        assert_eq!(text_attrs.font_size, FontSize::Em(2.0));
        assert_eq!(text_attrs.preformat, true);
    }

    #[test]
    fn test_parsed_attributes() {
        // Test ParsedAttributes generation
        let attr_values: AttributeValues = smallvec![
            AttributeValue::Text(TextValue::FontSize(FontSize::Em(1.0))),
            AttributeValue::General(GeneralValue::Opacity(0.9)),
            AttributeValue::Box(BoxValue::Fill(Some(Fill::Solid(Color::SKY)))),
        ];

        let parsed: ParsedAttributes = attr_values.into();

        assert!(parsed.text.is_some());
        assert_eq!(parsed.text.unwrap().font_size, FontSize::Em(1.0));

        assert!(parsed.general.is_some());
        assert_eq!(parsed.general.unwrap().opacity, 0.9);

        assert!(parsed.box_attr.is_some());
    }

    #[test]
    fn test_parsed_attributes_multiple_same_type() {
        // Test that multiple attributes of the same type accumulate properly
        let attr_values: AttributeValues = smallvec![
            AttributeValue::Text(TextValue::FontSize(FontSize::Em(1.0))),
            AttributeValue::General(GeneralValue::Opacity(0.9)),
            AttributeValue::Text(TextValue::Preformat(true)),
            AttributeValue::Text(TextValue::FontWeight(600)),
            AttributeValue::General(GeneralValue::MarginX(10)),
            AttributeValue::General(GeneralValue::MarginY(5)),
        ];

        let parsed: ParsedAttributes = attr_values.into();

        // Text attributes should have all three values applied
        assert!(parsed.text.is_some());
        let text = parsed.text.unwrap();
        assert_eq!(text.font_size, FontSize::Em(1.0));
        assert_eq!(text.preformat, true);
        assert_eq!(text.font_weight, 600);
        // Unset field should be default
        assert_eq!(text.hide_overflow, false);

        // General attributes should have all three values applied
        assert!(parsed.general.is_some());
        let general = parsed.general.unwrap();
        assert_eq!(general.opacity, 0.9);
        assert_eq!(general.margin_x, 10);
        assert_eq!(general.margin_y, 5);
        // Unset fields should be default
        assert_eq!(general.min_width, 0);
        assert_eq!(general.min_height, 0);
    }

    #[test]
    fn test_parsed_attributes_duplicate_fields() {
        // Test that last value wins for duplicate fields
        let attr_values: AttributeValues = smallvec![
            AttributeValue::Text(TextValue::FontSize(FontSize::Em(1.0))),
            AttributeValue::Text(TextValue::Preformat(false)),
            AttributeValue::Text(TextValue::FontSize(FontSize::Em(1.5))),  // Should overwrite previous
            AttributeValue::General(GeneralValue::Opacity(0.5)),
            AttributeValue::Text(TextValue::Preformat(true)),   // Should overwrite previous
            AttributeValue::General(GeneralValue::Opacity(0.8)), // Should overwrite previous
        ];

        let parsed: ParsedAttributes = attr_values.into();

        let text = parsed.text.unwrap();
        assert_eq!(text.font_size, FontSize::Em(1.5));  // Last value
        assert_eq!(text.preformat, true);     // Last value

        let general = parsed.general.unwrap();
        assert_eq!(general.opacity, 0.8);   // Last value
    }

    #[test]
    fn test_parsed_attributes_mixed_types() {
        // Test parsing with all different attribute types
        let attr_values: AttributeValues = smallvec![
            AttributeValue::General(GeneralValue::Opacity(0.7)),
            AttributeValue::Text(TextValue::FontSize(FontSize::Em(1.0))),
            AttributeValue::Img(ImgValue::Width(Some(100))),
            AttributeValue::Box(BoxValue::RoundCorners(Some(5))),
            AttributeValue::Row(RowValue::MainSizeMode(MainSizeMode::EqualWidth)),
            AttributeValue::Col(ColValue::MainAlign(YAlign::Top)),
            // Self attributes (is_parent = false)
            AttributeValue::RowChild(RowChildValue::CrossAlign(YAlign::Top), false),
            AttributeValue::ColChild(ColChildValue::CrossAlign(XAlign::Left), false),
            AttributeValue::StackChild(StackChildValue::StretchX(false), false),
        ];

        let parsed: ParsedAttributes = attr_values.into();

        // Verify each type is present and has correct value
        assert!(parsed.general.is_some());
        assert_eq!(parsed.general.unwrap().opacity, 0.7);

        assert!(parsed.text.is_some());
        assert_eq!(parsed.text.unwrap().font_size, FontSize::Em(1.0));

        assert!(parsed.img.is_some());
        assert_eq!(parsed.img.unwrap().width, Some(100));

        assert!(parsed.box_attr.is_some());
        assert_eq!(parsed.box_attr.unwrap().round_corners, Some(5));

        assert!(parsed.row.is_some());
        assert!(matches!(parsed.row.unwrap().main_size_mode, MainSizeMode::EqualWidth));

        assert!(parsed.col.is_some());
        assert!(matches!(parsed.col.unwrap().main_align, YAlign::Top));

        // Check self_child attributes
        assert!(parsed.self_child.is_some());
        let self_child = parsed.self_child.unwrap();
        assert!(matches!(self_child.row.cross_align, YAlign::Top));
        assert!(matches!(self_child.col.cross_align, XAlign::Left));
        assert_eq!(self_child.stack.stretch_x, false);
    }

    #[test]
    fn test_parsed_attributes_parent_vs_self() {
        // Test that parent flag properly distinguishes parent and self attributes
        let attr_values: AttributeValues = smallvec![
            // Parent attributes (is_parent = true) - go to container.children_default
            AttributeValue::RowChild(RowChildValue::CrossAlign(YAlign::Bottom), true),
            AttributeValue::ColChild(ColChildValue::CrossAlign(XAlign::Right), true),
            AttributeValue::StackChild(StackChildValue::AlignX(XAlign::Left), true),
            // Self attributes (is_parent = false) - go to self_child field
            AttributeValue::RowChild(RowChildValue::CrossAlign(YAlign::Top), false),
            AttributeValue::ColChild(ColChildValue::CrossAlign(XAlign::Left), false),
            AttributeValue::StackChild(StackChildValue::StretchY(false), false),
        ];

        let parsed: ParsedAttributes = attr_values.into();

        // Check parent attributes - should be in container.children_default
        assert!(parsed.row.is_some());
        let row = parsed.row.unwrap();
        assert!(matches!(row.children_default.cross_align, YAlign::Bottom));

        assert!(parsed.col.is_some());
        let col = parsed.col.unwrap();
        assert!(matches!(col.children_default.cross_align, XAlign::Right));

        assert!(parsed.stack.is_some());
        let stack = parsed.stack.unwrap();
        assert!(matches!(stack.children_default.align_x, XAlign::Left));

        // Check self attributes
        assert!(parsed.self_child.is_some());
        let self_child = parsed.self_child.unwrap();
        assert!(matches!(self_child.row.cross_align, YAlign::Top));
        assert!(matches!(self_child.col.cross_align, XAlign::Left));
        assert_eq!(self_child.stack.stretch_y, false);
    }
}