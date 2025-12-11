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
    pub fn is_container(&self) -> bool {
        matches!(self, Element::Box(_) | Element::Row(_) | Element::Col(_))
    }
}

pub struct ElementNode {
    i: u32,
    parent_i: u32,
    element: Element,
    general_attributes: GeneralAttributes,
    self_attributes: SelfAttributes
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

pub type Lu = u32;

#[derive(Copy, Clone, Debug)]
pub struct GeneralAttributes {
    pub min_width: Option<Lu>,
    pub max_width: Option<Lu>,
    pub margin_x: Lu,
    pub margin_y: Lu,
    pub opacity: f32,
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
pub struct ImgAttributes {
    pub resource: String,
    pub width: Option<Lu>,
    pub height: Option<Lu>,
}

#[derive(Clone, Debug, Default)]
pub struct BoxAttributes {
    pub fill: Option<Fill>,
    pub round_corners: Option<Lu>,
}

#[derive(Clone, Debug, Default)]
pub struct RowAttributes {
    pub main_size_mode: MainSizeMode,
    pub main_gap_mode: MainGapMode,
    pub main_align: XAlign,
    pub separator_width: Option<Lu>,
    pub separator_fill: Fill,
    pub children_default: RowChildAttributes,
}

#[derive(Clone, Debug, Default)]
pub struct ColAttributes {
    pub main_size_mode: MainSizeMode,
    pub main_gap_mode: MainGapMode,
    pub main_align: YAlign,
    pub separator_width: Option<Lu>,
    pub separator_fill: Fill,
    pub children_default: ColChildAttributes,
}

#[derive(Clone, Debug, Default)]
pub struct StackAttributes {
    pub self_dep_axis: SelfDepAxis,
    pub children_default: StackChildAttributes,
}

pub enum SelfAttributes {
    Stack(StackChildAttributes),
    Row(RowChildAttributes),
    Col(ColChildAttributes),
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
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
#[derive(Clone, Debug)]
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