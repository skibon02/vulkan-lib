use std::cmp::max;
use std::collections::BTreeMap;
use log::warn;
use crate::layout::{BoxAttributes, ChildAttributes, ColAttributes, Element, Lu, MainGapMode, MainSizeMode, ColChildAttributes};
use crate::layout::calculator::components::element_sizes::{DimFixState, ElementSizes, ElementSizesChildren, ParametricSolveState};
use crate::layout::calculator::components::elements::{ElementsChildrenIter, ElementsChildrenIterMut};
use crate::layout::calculator::elements::{ContainerFixSolver, ContainerParametricSolver, HasChildAttributes, SelfDepResolve};
use crate::layout::calculator::SideParametricState;

#[derive(Copy, Clone)]
pub struct ColSolver<'a> {
    attrs: &'a ColAttributes,
}

pub fn solver(attrs: &ColAttributes) -> ColSolver {
    ColSolver {
        attrs
    }
}
impl HasChildAttributes for ColSolver<'_> {
    type ChildAttributes = ColChildAttributes;

    fn unwrap(val: &mut ChildAttributes) -> &mut Self::ChildAttributes {
        &mut val.col
    }
}

// STAGE 1: PARAMETRIC SOLVE
#[derive(Default)]
pub struct ColSolverState {
    children_height_sum: Lu,
    children_max_width: Lu,
    is_any_selfdepx: bool,
    is_any_selfdepy: bool,
    is_any_selfdepboth: bool,
    children_count: usize,
}
impl ContainerParametricSolver for ColSolver<'_> {
    type State = ColSolverState;
    fn handle_child(&mut self, state: &mut ColSolverState, child_sizes: &ElementSizes, _: &ColChildAttributes) -> (Option<Option<Lu>>, Option<Option<Lu>>) {
        state.children_height_sum += child_sizes.min_height();
        state.children_max_width = max(state.children_max_width, child_sizes.min_width());
        state.children_count += 1;

        let grow_en = matches!(self.attrs.main_size_mode, MainSizeMode::EqualWidth);
        let cross_stretch_en = self.attrs.cross_stretch;

        // Special case for self-dep-both - fix only height
        if child_sizes.cur_parametric().is_self_dep_both() && !grow_en && !cross_stretch_en{
            // Fix height for self-dep-both child
            return (None, Some(None))
        }

        let cur_parametric = child_sizes.cur_parametric();

        // Disabled grow or cross stretch - reason for early fix
        let height = (!grow_en && cur_parametric.can_fix_height()).then_some(None);
        let width = (!cross_stretch_en && cur_parametric.can_fix_width()).then_some(None);

        if height.is_none() && cur_parametric.width.is_dependent() && !cur_parametric.height.is_fixed() {
            state.is_any_selfdepx = true
        }
        else if width.is_none() && cur_parametric.height.is_dependent() && !cur_parametric.width.is_fixed() {
            state.is_any_selfdepy = true
        }
        else if cur_parametric.is_self_dep_both() && width.is_none() && height.is_none() {
            state.is_any_selfdepboth = true
        }

        (width, height)
    }

    fn finalize(self, state: ColSolverState) -> ParametricSolveState {
        let mut res = ParametricSolveState::default();

        let grow_en = matches!(self.attrs.main_size_mode, MainSizeMode::EqualWidth);
        let gap_en = matches!(self.attrs.main_gap_mode, MainGapMode::Around | MainGapMode::Between);
        let cross_stretch_en = self.attrs.cross_stretch;

        let is_selfdepy = state.is_any_selfdepy || state.is_any_selfdepboth && !state.is_any_selfdepx;
        let is_selfdepx = !state.is_any_selfdepy && state.is_any_selfdepx;
        if state.is_any_selfdepx && state.is_any_selfdepy {
            warn!("col parametric: selfdepx and selfdepy conflict! Choosing selfdepy");
        }


        if is_selfdepy {
            res.width = SideParametricState::new_dependent()
        }
        else {
            res.width.min = state.children_max_width;
        }

        if is_selfdepx {
            res.height = SideParametricState::new_dependent();
        }
        else {
            res.height.min = state.children_height_sum;
            if let Some(gap) = self.attrs.main_gap_mode.fixed() {
                res.height.min += gap * state.children_count.saturating_sub(1) as Lu;
            }
        }

        if !cross_stretch_en {
            res.width.set_fixed();
        }
        if !grow_en && !gap_en {
            res.height.set_fixed();
        }

        res
    }
}

pub struct ColSolverFixStateY {
    fixed_height_sum: Lu,
    breakpoints: BTreeMap<Lu, usize>
}
// STAGE 2: DIM FIX

impl ContainerFixSolver for ColSolver<'_> {
    type StateX = ();

    fn init_x(&self, children_sizes: &ElementSizesChildren, children: ElementsChildrenIter) -> Self::StateX {
        ()
    }

    fn handle_child_x(&self, state: &mut Self::StateX, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes, el_sizes: &ElementSizes) -> Option<Option<Lu>> {
        let cross_stretch_en = self.attrs.cross_stretch;
        let child_parametric = child_sizes.cur_parametric();

        child_parametric.can_fix_width().then_some(cross_stretch_en.then_some(el_sizes.dim_fix.width().unwrap()))
    }


    type StateY = ColSolverFixStateY;
    fn init_y(&self, children_sizes: &ElementSizesChildren, children: ElementsChildrenIter) -> Self::StateY {
        let mut breakpoints = BTreeMap::new();

        let mut fixed_height_sum = 0;
        let mut ch_cnt = 0usize;
        for (i, child) in children {
            let child_sizes = children_sizes.get(i);

            fixed_height_sum += child_sizes.dim_fix.height().unwrap_or(0);
            if child_sizes.cur_parametric().can_fix_height() {
                breakpoints.entry(child_sizes.cur_parametric().height.min).and_modify(|v| *v += 1).or_insert(1);
            }
            ch_cnt += 1;
        }

        if let Some(gap) = self.attrs.main_gap_mode.fixed() {
            fixed_height_sum += gap * ch_cnt.saturating_sub(1) as Lu;
        }

        ColSolverFixStateY {
            fixed_height_sum,
            breakpoints,
        }
    }

    fn handle_child_y(&self, state: &mut Self::StateY, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes, el_sizes: &ElementSizes) -> Option<Option<Lu>> {
        let grow_en = matches!(self.attrs.main_size_mode, MainSizeMode::EqualWidth);
        let child_parametric = child_sizes.cur_parametric();

        if grow_en {
            let height = if child_parametric.can_fix_height() {
                let free_space = el_sizes.dim_fix.height().unwrap() - state.fixed_height_sum;

                // Find target height T so that all free children get equal height where possible.
                // Children whose min_height > T keep their min_height; the rest get T.
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

                let target_height = if remaining_count > 0 {
                    remaining_space / remaining_count as Lu
                } else {
                    child_parametric.height.min
                };

                let h = max(child_parametric.height.min, target_height);
                Some(Some(h))
            }
            else {
                None
            };

            height
        }
        else {
            child_parametric.can_fix_height().then_some(None)
        }
    }
}
