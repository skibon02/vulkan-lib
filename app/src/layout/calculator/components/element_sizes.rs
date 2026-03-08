use std::ops::{Deref, DerefMut};
use crate::layout::calculator::{ParametricStage, SideParametricKind};
use crate::layout::Lu;

pub struct Calculated(pub Vec<ElementSizes>);
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

#[derive(Clone, Debug, Default)]
pub struct ElementSizes {
    pub parametric: ParametricSolveState,
    pub post_parametric: ParametricSolveState,
    pub parent_parametric: ParametricSolveState,
    pub dim_fix: DimFixState,
    pub pos_fix: PosFixState,
    pub has_problems: bool,
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
#[derive(Clone, Debug, Copy)]
pub enum ParametricKind {
    Normal {
        width: SideParametricKind,
        height: SideParametricKind,
    },
    SelfDepBoth {
        stretch: bool,
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

#[derive(Copy, Clone, Debug, Default)]
pub struct ParametricSolveState {
    pub min_width: Lu,
    pub min_height: Lu,
    pub kind: ParametricKind,
}
#[derive(Clone, Debug, Default)]
pub struct DimFixState {
    height: Option<Lu>,
    width: Option<Lu>,
    processed: bool,
}
impl DimFixState {
    pub fn height(&self) -> Option<Lu> {
        self.height
    }
    pub fn width(&self) -> Option<Lu> {
        self.width
    }
    pub fn set_height(&mut self, height: Lu) {
        self.height = Some(height);
        if self.width.is_some() {
            self.processed = true;
        }
    }
    pub fn set_width(&mut self, width: Lu) {
        self.width = Some(width);
        if self.height.is_some() {
            self.processed = true;
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct PosFixState {
    pos_x: Lu,
    pos_y: Lu,
}
