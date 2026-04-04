use std::cmp::max;
use std::ops::{Deref, DerefMut};
use crate::layout::calculator::{ParametricStage, SideParametricState, ZERO_LENGTH_GUARD};
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

    /// Returns true -> width was fixed
    pub fn try_provide_width(&mut self, w: Option<Lu>) -> bool {
        let cur = self.cur_parametric_mut();
        if !cur.can_fix_width() {
            return false;
        }

        let width = if let Some(w) = w {
            w
        } else {
            if cur.width.min == 0 {
                ZERO_LENGTH_GUARD
            }
            else {
                cur.width.min
            }
        };
        assert!(width >= cur.width.min);
        self.dim_fix.set_width(width);
        true
    }

    pub fn try_fix_width(&mut self) -> bool {
        if !self.cur_parametric().can_fix_width() || self.dim_fix.width.is_none() {
            return false;
        }

        self.cur_parametric_mut().width.set_fixed();
        true
    }

    /// Returns true -> width was fixed
    pub fn try_provide_height(&mut self, h: Option<Lu>) -> bool {
        let cur = self.cur_parametric_mut();
        if !cur.can_fix_height() {
            return false;
        }

        let height = if let Some(h) = h {
            h
        } else {
            if cur.height.min == 0 {
                ZERO_LENGTH_GUARD
            }
            else {
                cur.height.min
            }
        };
        assert!(height >= cur.height.min);
        self.dim_fix.set_height(height);
        true
    }


    /// Returns true -> height was fixed
    pub fn try_fix_height(&mut self) -> bool {
        if !self.cur_parametric().can_fix_height() || self.dim_fix.height.is_none() {
            return false;
        }

        self.cur_parametric_mut().height.set_fixed();
        true
    }
    
    // min width for current parametric or dim fix width if fixed
    pub fn min_width(&self) -> Lu {
        self.dim_fix.width.unwrap_or(self.cur_parametric().width.min)
    }
    pub fn min_height(&self) -> Lu {
        self.dim_fix.height.unwrap_or(self.cur_parametric().height.min)
    }
}

pub struct ElementSizesChildren<'a> {
    parent_i: usize,
    elements: &'a mut [ElementSizes]
}
impl<'a> ElementSizesChildren<'a> {
    pub fn get_mut(&mut self, i: u32) -> &mut ElementSizes {
        if (self.parent_i+1..=self.parent_i + self.elements.len()).contains(&(i as usize)) {
            &mut self.elements[i as usize - self.parent_i - 1]
        }
        else {
            panic!("Incorrect element index specified provided to ElementsChildren::get")
        }
    }
    pub fn get(&self, i: u32) -> &ElementSizes {
        if (self.parent_i+1..=self.parent_i + self.elements.len()).contains(&(i as usize)) {
            &self.elements[i as usize - self.parent_i - 1]
        }
        else {
            panic!("Incorrect element index specified provided to ElementsChildren::get")
        }
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct ParametricSolveState {
    pub width: SideParametricState,
    pub height: SideParametricState,
}
impl ParametricSolveState {
    pub fn new_width_to_height() -> Self {
        Self {
            width: SideParametricState::new_free(),
            height: SideParametricState::new_dependent_fixed(),
        }
    }

    pub fn new_height_to_width() -> Self {
        Self {
            width: SideParametricState::new_dependent_fixed(),
            height: SideParametricState::new_free(),
        }
    }

    pub fn new_fixed() -> Self {
        Self {
            width: SideParametricState::new_fixed(),
            height: SideParametricState::new_fixed(),
        }
    }

    /// Is full fixed? (no input params)
    pub fn is_fixed(&self) -> bool {
        self.width.is_fixed() && self.height.is_fixed()
    }

    /// Is self dep x/y/both? (1/2 input param)
    pub fn is_self_dep(&self) -> bool {
        (self.width.is_dependent() || self.height.is_dependent()) && !self.is_fixed()
    }
    
    /// Is unresolved self dep both? (1 input param)
    pub fn is_self_dep_both(&self) -> bool {
        self.width.is_dependent() && self.height.is_dependent() && !self.width.is_fixed() && !self.height.is_fixed()
    }

    /// Can fix width right now?
    pub fn can_fix_width(&self) -> bool {
        !self.width.is_fixed() && !(self.width.is_dependent() && !self.height.is_fixed()) || self.is_self_dep_both()
    }

    /// Can fix height right now?
    pub fn can_fix_height(&self) -> bool {
        !self.height.is_fixed() && !(self.height.is_dependent() && !self.width.is_fixed()) || self.is_self_dep_both()
    }
}


#[derive(Clone, Debug, Default)]
pub struct DimFixState {
    height: Option<Lu>,
    width: Option<Lu>,
    subtree_fixed_x: bool,
    subtree_fixed_y: bool,
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
    pub fn is_subtree_fixed_x(&self) -> bool {
        self.subtree_fixed_x
    }
    pub fn set_subtree_fixed_x(&mut self) {
        self.subtree_fixed_x = true;
    }
    pub fn is_subtree_fixed_y(&self) -> bool {
        self.subtree_fixed_y
    }
    pub fn set_subtree_fixed_y(&mut self) {
        self.subtree_fixed_y = true;
    }
}
#[derive(Clone, Debug, Default)]
pub struct PosFixState {
    pub pos_x: Lu,
    pub pos_y: Lu,
}
