use ui_macro::{AttributeEnum, generate_parsed_attributes};

pub mod calculator;

// Generate ParsedAttributes struct and From<Vec<AttributeValue>> implementation
generate_parsed_attributes!();

/// Initial layout structure
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
    ColChild(ColChildValue),
    Row(RowValue),
    RowChild(RowChildValue),
    Stack(StackValue),
    StackChild(StackChildValue),
    Img(ImgValue),
    Text(TextValue),
    Box(BoxValue),
    General(GeneralValue),
}

pub struct ElementNode {
    i: u32,
    parent_i: u32,
    element: Element,
    general_attributes: GeneralAttributes,
    self_attributes: SelfAttributes
}

pub type ElementNodeList = Vec<(u32, ElementNodeRepr)>;
pub struct ElementNodeRepr {
    parent_i: u32,
    element: ElementKind,
    attributes: Vec<AttributeValue>
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
    EqualGrow,
    #[default]
    EqualWidth,
    Min
}

#[derive(Copy, Clone, Debug, Default)]
pub enum MainGapMode {
    #[default]
    Between,
    Around,
    None
}

#[derive(Copy, Clone, Debug, Default)]
pub enum SelfDepAxis {
    #[default]
    XStretch,
    YStretch
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

#[derive(Copy, Clone, Debug, AttributeEnum)]
pub struct GeneralAttributes {
    pub min_width: Option<Lu>,
    pub min_height: Option<Lu>,
    pub nostretch_x: bool,
    pub nostretch_y: bool,
    pub margin_x: Lu,
    pub margin_y: Lu,
    pub opacity: f32,
}

impl Default for GeneralAttributes {
    fn default() -> Self {
        Self {
            min_width: None,
            min_height: None,
            nostretch_x: false,
            nostretch_y: false,
            margin_x: 0,
            margin_y: 0,
            opacity: 1.0,
        }
    }
}

#[derive(Clone, Debug, AttributeEnum)]
pub struct TextAttributes {
    pub oneline: bool,
    pub hide_overflow: bool,
    pub font_size: f32,
    pub font_weight: u16,
    pub text_align_x: XAlign,
    pub text_align_y: YAlign,
    pub symbols_limit: Option<u32>,
    pub text_color: Fill,
}

impl Default for TextAttributes {
    fn default() -> Self {
        Self {
            oneline: false,
            hide_overflow: false,
            font_size: 16.0,
            font_weight: 400,
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
    pub separator_width: Option<Lu>,
    pub separator_fill: Fill,
    pub children_default: RowChildAttributes,
}

#[derive(Clone, Debug, Default, AttributeEnum)]
pub struct ColAttributes {
    pub main_size_mode: MainSizeMode,
    pub main_gap_mode: MainGapMode,
    pub main_align: YAlign,
    pub separator_width: Option<Lu>,
    pub separator_fill: Fill,
    pub children_default: ColChildAttributes,
}

#[derive(Clone, Debug, Default, AttributeEnum)]
pub struct StackAttributes {
    pub self_dep_axis: SelfDepAxis,
    pub children_default: StackChildAttributes,
}

pub enum SelfAttributes {
    Stack(StackChildAttributes),
    Row(RowChildAttributes),
    Col(ColChildAttributes),
}

#[derive(Clone, Debug, AttributeEnum)]
pub struct RowChildAttributes {
    pub cross_stretch: bool,
    pub cross_align: YAlign,
    pub cross_size: Option<Lu>,
}
impl Default for RowChildAttributes {
    fn default() -> Self {
        Self {
            cross_stretch: true,
            cross_align: YAlign::default(),
            cross_size: None,
        }
    }
}

#[derive(Clone, Debug, AttributeEnum)]
pub struct ColChildAttributes {
    pub cross_stretch: bool,
    pub cross_align: XAlign,
    pub cross_size: Option<Lu>,
}
impl Default for ColChildAttributes {
    fn default() -> Self {
        Self {
            cross_stretch: true,
            cross_align: XAlign::default(),
            cross_size: None,
        }
    }
}
#[derive(Clone, Debug, AttributeEnum)]
pub struct StackChildAttributes {
    pub stretch_x: bool,
    pub stretch_y: bool,
    pub align_x: XAlign,
    pub align_y: YAlign,
    pub width: Option<Lu>,
    pub height: Option<Lu>,
    pub self_dep_mode: SelfDepMode,
}

impl Default for StackChildAttributes {
    fn default() -> Self {
        Self {
            stretch_x: true,
            stretch_y: true,
            align_x: XAlign::Center,
            align_y: YAlign::Center,
            width: None,
            height: None,
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
        let _text_val = TextValue::FontSize(16.0);
        let _general_val = GeneralValue::Opacity(0.5);
        let _box_val = BoxValue::Fill(Some(Fill::Solid(Color::WHITE)));

        // Test From<Vec<*Value>> for *Attributes - should apply each value
        let text_attrs: TextAttributes = vec![
            TextValue::FontSize(24.0),
            TextValue::Oneline(true),
            TextValue::FontWeight(700),
        ].into();
        assert_eq!(text_attrs.font_size, 24.0);
        assert_eq!(text_attrs.oneline, true);
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
            TextValue::FontSize(24.0),
            TextValue::FontSize(32.0),  // This should win
            TextValue::Oneline(false),
            TextValue::Oneline(true),   // This should win
        ].into();
        assert_eq!(text_attrs.font_size, 32.0);
        assert_eq!(text_attrs.oneline, true);
    }

    #[test]
    fn test_parsed_attributes() {
        // Test ParsedAttributes generation
        let attr_values = vec![
            AttributeValue::Text(TextValue::FontSize(20.0)),
            AttributeValue::General(GeneralValue::Opacity(0.9)),
            AttributeValue::Box(BoxValue::Fill(Some(Fill::Solid(Color::SKY)))),
        ];

        let parsed: ParsedAttributes = attr_values.into();

        assert!(parsed.text.is_some());
        assert_eq!(parsed.text.unwrap().font_size, 20.0);

        assert!(parsed.general.is_some());
        assert_eq!(parsed.general.unwrap().opacity, 0.9);

        assert!(parsed.box_attr.is_some());
    }

    #[test]
    fn test_parsed_attributes_multiple_same_type() {
        // Test that multiple attributes of the same type accumulate properly
        let attr_values = vec![
            AttributeValue::Text(TextValue::FontSize(20.0)),
            AttributeValue::General(GeneralValue::Opacity(0.9)),
            AttributeValue::Text(TextValue::Oneline(true)),
            AttributeValue::Text(TextValue::FontWeight(600)),
            AttributeValue::General(GeneralValue::MarginX(10)),
            AttributeValue::General(GeneralValue::MarginY(5)),
        ];

        let parsed: ParsedAttributes = attr_values.into();

        // Text attributes should have all three values applied
        assert!(parsed.text.is_some());
        let text = parsed.text.unwrap();
        assert_eq!(text.font_size, 20.0);
        assert_eq!(text.oneline, true);
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
        assert_eq!(general.min_width, None);
        assert_eq!(general.min_height, None);
    }

    #[test]
    fn test_parsed_attributes_duplicate_fields() {
        // Test that last value wins for duplicate fields
        let attr_values = vec![
            AttributeValue::Text(TextValue::FontSize(20.0)),
            AttributeValue::Text(TextValue::Oneline(false)),
            AttributeValue::Text(TextValue::FontSize(30.0)),  // Should overwrite previous
            AttributeValue::General(GeneralValue::Opacity(0.5)),
            AttributeValue::Text(TextValue::Oneline(true)),   // Should overwrite previous
            AttributeValue::General(GeneralValue::Opacity(0.8)), // Should overwrite previous
        ];

        let parsed: ParsedAttributes = attr_values.into();

        let text = parsed.text.unwrap();
        assert_eq!(text.font_size, 30.0);  // Last value
        assert_eq!(text.oneline, true);     // Last value

        let general = parsed.general.unwrap();
        assert_eq!(general.opacity, 0.8);   // Last value
    }

    #[test]
    fn test_parsed_attributes_mixed_types() {
        // Test parsing with all different attribute types
        let attr_values = vec![
            AttributeValue::General(GeneralValue::Opacity(0.7)),
            AttributeValue::Text(TextValue::FontSize(18.0)),
            AttributeValue::Img(ImgValue::Width(Some(100))),
            AttributeValue::Box(BoxValue::RoundCorners(Some(5))),
            AttributeValue::Row(RowValue::MainSizeMode(MainSizeMode::EqualGrow)),
            AttributeValue::Col(ColValue::MainAlign(YAlign::Top)),
            AttributeValue::Stack(StackValue::SelfDepAxis(SelfDepAxis::YStretch)),
            AttributeValue::RowChild(RowChildValue::CrossStretch(false)),
            AttributeValue::ColChild(ColChildValue::CrossAlign(XAlign::Left)),
            AttributeValue::StackChild(StackChildValue::StretchX(false)),
        ];

        let parsed: ParsedAttributes = attr_values.into();

        // Verify each type is present and has correct value
        assert!(parsed.general.is_some());
        assert_eq!(parsed.general.unwrap().opacity, 0.7);

        assert!(parsed.text.is_some());
        assert_eq!(parsed.text.unwrap().font_size, 18.0);

        assert!(parsed.img.is_some());
        assert_eq!(parsed.img.unwrap().width, Some(100));

        assert!(parsed.box_attr.is_some());
        assert_eq!(parsed.box_attr.unwrap().round_corners, Some(5));

        assert!(parsed.row.is_some());
        assert!(matches!(parsed.row.unwrap().main_size_mode, MainSizeMode::EqualGrow));

        assert!(parsed.col.is_some());
        assert!(matches!(parsed.col.unwrap().main_align, YAlign::Top));

        assert!(parsed.stack.is_some());
        assert!(matches!(parsed.stack.unwrap().self_dep_axis, SelfDepAxis::YStretch));

        assert!(parsed.row_child.is_some());
        assert_eq!(parsed.row_child.unwrap().cross_stretch, false);

        assert!(parsed.col_child.is_some());
        assert!(matches!(parsed.col_child.unwrap().cross_align, XAlign::Left));

        assert!(parsed.stack_child.is_some());
        assert_eq!(parsed.stack_child.unwrap().stretch_x, false);
    }
}