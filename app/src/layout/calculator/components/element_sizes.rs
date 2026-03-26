use std::cmp::max;
use std::ops::{Deref, DerefMut};
use crate::layout::calculator::{ParametricStage, SideParametricKind, ZERO_LENGTH_GUARD};
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

impl Calculated {
    /// This method will panic if index i is not present!
    pub fn children(&mut self, i: usize) -> (&mut ElementSizes, ElementSizesChildren) {
        let (element, rest) = (self.0[i..]).split_first_mut().unwrap();
        (element, ElementSizesChildren {
            parent_i: i,
            elements: rest,
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct ElementSizes {
    pub parametric: ParametricSolveState,
    pub post_parametric: ParametricSolveState,
    pub parent_parametric: ParametricSolveState,
    pub parametric_stage: ParametricStage,
    pub dim_fix: DimFixState,
    pub pos_fix: PosFixState,
    pub has_problems: bool,
}

impl ElementSizes {
    pub fn parametric_mut(&mut self, stage: ParametricStage) -> &mut ParametricSolveState {
        match stage {
            ParametricStage::Parametric => &mut self.parametric,
            ParametricStage::PostParametric => &mut self.post_parametric,
            ParametricStage::ParentParametric => &mut self.parent_parametric,
        }
    }
    pub fn cur_parametric_mut(&mut self) -> &mut ParametricSolveState {
        self.parametric_mut(self.parametric_stage)
    }
    pub fn parametric(&self, stage: ParametricStage) -> &ParametricSolveState {
        match stage {
            ParametricStage::Parametric => &self.parametric,
            ParametricStage::PostParametric => &self.post_parametric,
            ParametricStage::ParentParametric => &self.parent_parametric,
        }
    }
    pub fn cur_parametric(&self) -> &ParametricSolveState {
        self.parametric(self.parametric_stage)
    }
    pub fn set_parametric_stage(&mut self, stage: ParametricStage) {
        self.parametric_stage = stage;
    }
    
    /// Return true -> need to run subtree fix for this element
    pub fn try_fix_width(&mut self, width: Option<Lu>) -> bool {
        let cur = self.cur_parametric_mut();
        if cur.state.can_fix_width() {
            cur.state.width = SideParametricKind::Fixed;
            let is_fixed = cur.state.is_fixed();
            
            let width = if let Some(w) = width {
                w
            } else {
                ZERO_LENGTH_GUARD
            };
            self.dim_fix.set_width(width);
            is_fixed
        }
        else {
            false
        }
    }
    
    /// Returns true -> need to run subtree fix for this element
    pub fn try_fix_height(&mut self, height: Option<Lu>) -> bool {
        let cur = self.cur_parametric_mut();
        if cur.state.can_fix_width() {
            cur.state.width = SideParametricKind::Fixed;
            let is_fixed = cur.state.is_fixed();

            let height = if let Some(h) = height {
                h
            } else {
                ZERO_LENGTH_GUARD
            };
            self.dim_fix.set_height(height);
            is_fixed
        }
        else {
            false
        }
    }
    
    // min width for current parametric or dim fix width if fixed
    pub fn min_width(&self) -> Lu {
        self.dim_fix.width.unwrap_or(self.cur_parametric().min_width)
    }
    pub fn min_height(&self) -> Lu {
        self.dim_fix.height.unwrap_or(self.cur_parametric().min_height)
    }
    
    // for use from dim fix stage
    pub fn is_width_fixed(&self) -> bool {
        self.dim_fix.width.is_some()
    }
    pub fn is_height_fixed(&self) -> bool {
        self.dim_fix.height.is_some()
    }

}

pub struct ElementSizesChildren<'a> {
    parent_i: usize,
    elements: &'a mut [ElementSizes]
}
impl<'a> ElementSizesChildren<'a> {
    pub fn get_mut(&mut self, i: u32) -> &mut ElementSizes {
        if (self.parent_i..self.parent_i + self.elements.len()).contains(&(i as usize)) {
            &mut self.elements[i as usize - self.parent_i]
        }
        else {
            panic!("Incorrect element index specified provided to ElementsChildren::get")
        }
    }
    pub fn get(&self, i: u32) -> &ElementSizes {
        if (self.parent_i..self.parent_i + self.elements.len()).contains(&(i as usize)) {
            &self.elements[i as usize - self.parent_i]
        }
        else {
            panic!("Incorrect element index specified provided to ElementsChildren::get")
        }
    }
}
#[derive(Clone, Debug, Copy)]
pub struct ParametricKindState {
    pub width: SideParametricKind,
    pub height: SideParametricKind,
}

#[derive(Clone, Debug, Copy, PartialEq)]
pub enum ParametricKind {
    /// At least one side is free
    NotFixed,
    /// Both sides are fixed
    Fixed,
    /// selfdepx
    WidthToHeight,
    /// selfdepy
    HeightToWidth,
    /// selfdepboth
    SelfDepBoth,
}

impl Default for ParametricKindState {
    fn default() -> Self {
        Self {
            width: SideParametricKind::default(),
            height: SideParametricKind::default(),
        }
    }
}

impl ParametricKindState {
    pub fn new_width_to_height() -> Self {
        Self {
            width: SideParametricKind::Free,
            height: SideParametricKind::Dependent,
        }
    }

    pub fn new_height_to_width() -> Self {
        Self {
            width: SideParametricKind::Dependent,
            height: SideParametricKind::Free,
        }
    }

    pub fn new_fixed() -> Self {
        Self {
            width: SideParametricKind::Fixed,
            height: SideParametricKind::Fixed,
        }
    }

    pub fn is_fixed(&self) -> bool {
        match (self.width, self.height) {
            (SideParametricKind::Fixed, SideParametricKind::Dependent | SideParametricKind::Fixed) => true,
            (SideParametricKind::Dependent, SideParametricKind::Fixed) => true,
            _ => false
        }
    }
    
    pub fn is_self_dep(&self) -> bool {
        self.width == SideParametricKind::Dependent || self.height == SideParametricKind::Dependent
    }
    pub fn is_self_dep_both(&self) -> bool {
        self.width == SideParametricKind::Dependent && self.height == SideParametricKind::Dependent
    }

    pub fn can_fix_width(&self) -> bool {
        self.width == SideParametricKind::Free || self.is_self_dep_both()
    }

    pub fn can_fix_height(&self) -> bool {
        self.height == SideParametricKind::Free || self.is_self_dep_both()
    }

    pub fn kind(&self) -> ParametricKind {
        match (self.width, self.height) {
            (SideParametricKind::Free, SideParametricKind::Dependent) => ParametricKind::WidthToHeight,
            (SideParametricKind::Dependent, SideParametricKind::Free) => ParametricKind::HeightToWidth,
            (_, SideParametricKind::Free) => ParametricKind::NotFixed,
            (SideParametricKind::Free, _) => ParametricKind::NotFixed,
            _ => ParametricKind::Fixed,
        }
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct ParametricSolveState {
    pub min_width: Lu,
    pub min_height: Lu,
    pub state: ParametricKindState,
}

impl ParametricSolveState {
    pub fn apply_min_width(&mut self, width: Lu) {
        self.min_width = max(self.min_width, width);
    }
    pub fn apply_min_height(&mut self, height: Lu) {
        self.min_height = max(self.min_height, height);
    }
}
#[derive(Clone, Debug, Default)]
pub struct DimFixState {
    height: Option<Lu>,
    width: Option<Lu>,
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
    }
    pub fn set_width(&mut self, width: Lu) {
        self.width = Some(width);
    }
}
#[derive(Clone, Debug, Default)]
pub struct PosFixState {
    pos_x: Lu,
    pos_y: Lu,
}
