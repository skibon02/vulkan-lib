use std::cmp::max;
use std::collections::BTreeMap;
use log::warn;
use crate::layout::{BoxAttributes, ChildAttributes, ColAttributes, Element, Lu, MainGapMode, MainSizeMode, RowAttributes, RowChildAttributes};
use crate::layout::calculator::components::element_sizes::{DimFixState, ElementSizes, ElementSizesChildren, ParametricSolveState};
use crate::layout::calculator::components::elements::{ElementsChildrenIter, ElementsChildrenIterMut};
use crate::layout::calculator::elements::{ContainerFixSolver, ContainerParametricSolver, HasChildAttributes, SelfDepResolve};
use crate::layout::calculator::SideParametricState;

#[derive(Copy, Clone)]
pub struct RowSolver<'a> {
    attrs: &'a RowAttributes,
}

pub fn solver(attrs: &RowAttributes) -> RowSolver {
    RowSolver {
        attrs
    }
}
impl HasChildAttributes for RowSolver<'_> {
    type ChildAttributes = RowChildAttributes;

    fn unwrap(val: &mut ChildAttributes) -> &mut Self::ChildAttributes {
        &mut val.row
    }
}

// STAGE 1: PARAMETRIC SOLVE
#[derive(Default)]
pub struct RowSolverState {
    children_width_sum: Lu,
    children_max_height: Lu,
    is_any_selfdepx: bool,
    is_any_selfdepy: bool,
    is_any_selfdepboth: bool,
    children_count: usize,
}
impl ContainerParametricSolver for RowSolver<'_> {
    type State = RowSolverState;
    fn handle_child(&mut self, state: &mut RowSolverState, child_sizes: &ElementSizes, _: &RowChildAttributes) -> (Option<Option<Lu>>, Option<Option<Lu>>) {
        state.children_width_sum += child_sizes.min_width();
        state.children_max_height = max(state.children_max_height, child_sizes.min_height());
        state.children_count += 1;

        let grow_en = matches!(self.attrs.main_size_mode, MainSizeMode::EqualWidth);
        let cross_stretch_en = self.attrs.cross_stretch;

        // Special case for self-dep-both - fix only width
        if child_sizes.cur_parametric().is_self_dep_both() && !grow_en && !cross_stretch_en{
            // Fix width for self-dep-both child
            return (Some(None), None)
        }

        let cur_parametric = child_sizes.cur_parametric();

        // Disabled grow or cross stretch - reason for early fix
        let width = (!grow_en && cur_parametric.can_fix_width()).then_some(None);
        let height = (!cross_stretch_en && cur_parametric.can_fix_height()).then_some(None);

        if width.is_none() && cur_parametric.height.is_dependent() && !cur_parametric.width.is_fixed() {
            state.is_any_selfdepx = true
        }
        else if height.is_none() && cur_parametric.width.is_dependent() && !cur_parametric.height.is_fixed() {
            state.is_any_selfdepy = true
        }
        else if cur_parametric.is_self_dep_both() && width.is_none() && height.is_none() {
            state.is_any_selfdepboth = true
        }

        (width, height)
    }

    fn finalize(self, state: RowSolverState) -> ParametricSolveState {
        let mut res = ParametricSolveState::default();

        let grow_en = matches!(self.attrs.main_size_mode, MainSizeMode::EqualWidth);
        let gap_en = matches!(self.attrs.main_gap_mode, MainGapMode::Around | MainGapMode::Between);
        let cross_stretch_en = self.attrs.cross_stretch;

        let is_selfdepx = state.is_any_selfdepx || state.is_any_selfdepboth && !state.is_any_selfdepy;
        let is_selfdepy = !state.is_any_selfdepx && state.is_any_selfdepy;
        if state.is_any_selfdepx && state.is_any_selfdepy {
            warn!("row parametric: selfdepx and selfdepy conflict! Choosing selfdepx");
        }


        if is_selfdepx {
            res.height = SideParametricState::new_dependent()
        }
        else {
            res.height.min = state.children_max_height;
        }

        if is_selfdepy {
            res.width = SideParametricState::new_dependent();
        }
        else {
            res.width.min = state.children_width_sum;
            if let Some(gap) = self.attrs.main_gap_mode.fixed() {
                res.width.min += gap * state.children_count.saturating_sub(1) as Lu;
            }
        }

        if !cross_stretch_en {
            res.height.set_fixed();
        }
        if !grow_en && !gap_en {
            res.width.set_fixed();
        }

        res
    }
}

pub struct RowSolverFixStateX {
    fixed_width_sum: Lu,
    breakpoints: BTreeMap<Lu, usize>
}
// STAGE 2: DIM FIX

impl ContainerFixSolver for RowSolver<'_> {
    type StateX = RowSolverFixStateX;

    fn init_x(&self, children_sizes: &ElementSizesChildren, children: ElementsChildrenIter) -> Self::StateX {
        let mut breakpoints = BTreeMap::new();

        let mut fixed_width_sum = 0;
        let mut ch_cnt = 0 as usize;
        for (i, child) in children {
            let child_sizes = children_sizes.get(i);

            fixed_width_sum += child_sizes.dim_fix.width().unwrap_or(0);
            if child_sizes.cur_parametric().can_fix_width() {
                breakpoints.entry(child_sizes.cur_parametric().width.min).and_modify(|v| *v += 1).or_insert(1);
            }
            ch_cnt += 1;
        }
        
        if let Some(gap) = self.attrs.main_gap_mode.fixed() {
            fixed_width_sum += gap * ch_cnt.saturating_sub(1) as Lu;
        }

        RowSolverFixStateX {
            fixed_width_sum,
            breakpoints,
        }
    }

    fn handle_child_x(&self, state: &mut Self::StateX, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes, el_sizes: &ElementSizes) -> Option<Option<Lu>> {
        let grow_en = matches!(self.attrs.main_size_mode, MainSizeMode::EqualWidth);
        let child_parametric = child_sizes.cur_parametric();

        if grow_en {
            let width = if child_parametric.can_fix_width() {
                let free_space = el_sizes.dim_fix.width().unwrap() - state.fixed_width_sum;

                // Find target width T so that all free children get equal width where possible.
                // Children whose min_width > T keep their min_width; the rest get T.
                // Iterate from largest breakpoint down, locking oversized children.
                let total_count: usize = state.breakpoints.values().sum();
                let mut remaining_space = free_space;
                let mut remaining_count = total_count;

                for (&bp, &cnt) in state.breakpoints.iter().rev() {
                    if remaining_count == 0 {
                        break;
                    }
                    let target = remaining_space / remaining_count as Lu;
                    if bp > target {
                        remaining_space -= bp * cnt as Lu;
                        remaining_count -= cnt;
                    } else {
                        break;
                    }
                }

                let target_width = if remaining_count > 0 {
                    remaining_space / remaining_count as Lu
                } else {
                    child_parametric.width.min
                };

                let w = max(child_parametric.width.min, target_width);
                Some(Some(w))
            }
            else {
                None
            };

            width
        }
        else {
            child_parametric.can_fix_width().then_some(None)
        }
    }


    type StateY = ();
    fn init_y(&self, children_sizes: &ElementSizesChildren, children: ElementsChildrenIter) -> Self::StateY {
        ()
    }
    fn handle_child_y(&self, state: &mut Self::StateY, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes, el_sizes: &ElementSizes) -> Option<Option<Lu>> {
        let cross_stretch_en = self.attrs.cross_stretch;
        let child_parametric = child_sizes.cur_parametric();

        child_parametric.can_fix_height().then_some(cross_stretch_en.then_some(el_sizes.dim_fix.height().unwrap()))
    }
}