use std::cmp::max;
use std::collections::BTreeMap;
use log::warn;
use crate::layout::calculator::components::element_sizes::{ElementSizes, ElementSizesChildren, ParametricKind, ParametricKindState, ParametricSolveState};
use crate::layout::calculator::SideParametricState;
use crate::layout::{ChildAttributes, ColAttributes, ColChildAttributes, Lu, MainGapMode, MainSizeMode};
use crate::layout::calculator::components::elements::{ElementsChildrenIter, ElementsChildrenIterMut};
use crate::layout::calculator::elements::{ContainerFixSolver, ContainerParametricSolver, HasChildAttributes, SelfDepResolve};

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
// Col is the transposition of Row: main axis = height, cross axis = width.
#[derive(Default)]
pub struct ColSolverState {
    children_height_sum: Lu,
    children_max_width: Lu,
    is_any_selfdepx: bool,
    is_any_selfdepy: bool,
    is_any_selfdepboth: bool,
}
impl ContainerParametricSolver for ColSolver<'_> {
    type State = ColSolverState;
    fn handle_child(&mut self, state: &mut ColSolverState, child_sizes: &ElementSizes, _: &ColChildAttributes) -> (Option<Option<Lu>>, Option<Option<Lu>>) {
        state.children_height_sum += child_sizes.min_height();
        state.children_max_width = max(state.children_max_width, child_sizes.min_width());

        let grow_en = matches!(self.attrs.main_size_mode, MainSizeMode::EqualWidth);
        let cross_stretch_en = self.attrs.cross_stretch;

        if child_sizes.cur_parametric().state.is_self_dep_both() && !grow_en && !cross_stretch_en {
            // Fix height (main axis) for self-dep-both child
            (None, Some(None))
        }
        else {
            let cur_parametric = child_sizes.cur_parametric().state;
            let height = (!grow_en && cur_parametric.can_fix_height()).then_some(None);
            let width = (!cross_stretch_en && cur_parametric.can_fix_width()).then_some(None);

            match cur_parametric.kind() {
                ParametricKind::WidthToHeight => {
                    if height.is_none() {
                        state.is_any_selfdepx = true;
                    }
                }
                ParametricKind::HeightToWidth => {
                    if width.is_none() {
                        state.is_any_selfdepy = true;
                    }
                }
                ParametricKind::SelfDepBoth => {
                    if height.is_none() && width.is_none() {
                        state.is_any_selfdepboth = true;
                    }
                }
                _ => {}
            }

            (width, height)
        }
    }

    fn finalize(self, state: ColSolverState) -> ParametricSolveState {
        let mut res = ParametricSolveState::default();

        let grow_en = matches!(self.attrs.main_size_mode, MainSizeMode::EqualWidth);
        let gap_en = matches!(self.attrs.main_gap_mode, MainGapMode::Around | MainGapMode::Between);
        let cross_stretch_en = self.attrs.cross_stretch;

        let has_selfdepx = state.is_any_selfdepx;
        let has_selfdepy = state.is_any_selfdepy;
        let has_selfdepboth = state.is_any_selfdepboth && !has_selfdepx && !has_selfdepy;
        if has_selfdepx && has_selfdepy {
            warn!("col parametric: selfdepx and selfdepy conflict!");
        }

        res.min_width = state.children_max_width;
        res.min_height = state.children_height_sum;
        res.state = ParametricKindState::default();

        if has_selfdepx || has_selfdepboth {
            res.state.width = SideParametricState::Dependent
        } else if has_selfdepy {
            res.state.height = SideParametricState::Dependent
        } else {
            if !grow_en && !gap_en {
                res.state.height = SideParametricState::Fixed;
            }
            if !cross_stretch_en {
                res.state.width = SideParametricState::Fixed;
            }
        }

        res
    }
}

pub struct ColSolverFixState {
    has_self_dep_x: bool,
    has_self_dep_y: bool,
    fixed_height_sum: Lu,
    sum_height: Lu,
    max_width: Lu,
    breakpoints: BTreeMap<Lu, usize>
}
// STAGE 2: DIM FIX

impl ContainerFixSolver for ColSolver<'_> {
    type State = ColSolverFixState;

    fn init(&self, children_sizes: &ElementSizesChildren, children: ElementsChildrenIter) -> Self::State {
        let mut has_self_dep_x = false;
        let mut has_self_dep_y = false;
        let mut has_self_dep_both = false;
        let mut breakpoints = BTreeMap::new();

        let mut fixed_height_sum = 0;
        for (i, child) in children {
            let child_sizes = children_sizes.get(i);
            let kind = child_sizes.cur_parametric().state.kind();
            if kind == ParametricKind::WidthToHeight {
                has_self_dep_x = true;
            }
            else if kind == ParametricKind::HeightToWidth {
                has_self_dep_y = true;
            }
            else if kind == ParametricKind::SelfDepBoth {
                has_self_dep_both = true;
            }

            fixed_height_sum += child_sizes.dim_fix.height().unwrap_or(0);
            if child_sizes.cur_parametric().state.can_fix_height() {
                breakpoints.entry(child_sizes.cur_parametric().min_height).and_modify(|v| *v += 1).or_insert(1);
            }
        }

        if has_self_dep_x || (!has_self_dep_y && has_self_dep_both) {
            has_self_dep_y = false;
        }
        else {
            has_self_dep_x = false;
        }

        ColSolverFixState {
            has_self_dep_x,
            has_self_dep_y,
            fixed_height_sum,
            breakpoints,
            max_width: 0,
            sum_height: 0,
        }
    }

    fn early_handle_child(&self, state: &mut Self::State, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes, el_sizes: &ElementSizes) -> (Option<Option<Lu>>, Option<Option<Lu>>) {
        if state.has_self_dep_y {
            let cur_parametric = child_sizes.cur_parametric();
            match cur_parametric.state.kind() {
                ParametricKind::HeightToWidth | ParametricKind::SelfDepBoth => {
                    (Some(Some(el_sizes.min_width())), None)
                }
                _ => {
                    (None, None)
                }
            }
        }
        else if state.has_self_dep_x {
            let cur_parametric = child_sizes.cur_parametric();
            match cur_parametric.state.kind() {
                ParametricKind::WidthToHeight | ParametricKind::SelfDepBoth => {
                    (None, Some(Some(el_sizes.min_height())))
                }
                _ => {
                    (None, None)
                }
            }
        }
        else {
            (None, None)
        }
    }
    fn early_finalize(&self, state: &mut Self::State, children_sizes: &ElementSizesChildren, children: ElementsChildrenIter) -> Option<SelfDepResolve> {
        if state.has_self_dep_y {
            // recalculate height sum (main axis)
            for (i, _) in children {
                let child_sizes = children_sizes.get(i);
                state.sum_height += child_sizes.min_height();
            }

            Some(SelfDepResolve::Height(state.sum_height))
        }
        else if state.has_self_dep_x {
            // recalculate max min width (cross axis)
            for (i, _) in children {
                let child_sizes = children_sizes.get(i);
                state.max_width = max(state.max_width, child_sizes.min_width());
            }

            Some(SelfDepResolve::Width(state.max_width))
        }
        else {
            None
        }
    }
    fn handle_child(&self, state: &mut Self::State, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes, el_sizes: &ElementSizes) -> (Option<Option<Lu>>, Option<Option<Lu>>) {
        let cross_stretch_en = self.attrs.cross_stretch;

        let grow_en = matches!(self.attrs.main_size_mode, MainSizeMode::EqualWidth);

        let cur_parametric = child_sizes.cur_parametric();
        if grow_en && !state.has_self_dep_y {
            let height = if cur_parametric.state.can_fix_height() {
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
                    cur_parametric.min_height
                };

                let h = max(cur_parametric.min_height, target_height);
                Some(Some(h))
            }
            else {
                None
            };

            let width = cur_parametric.state.can_fix_width().then_some(cross_stretch_en.then_some(el_sizes.dim_fix.width().unwrap()));
            (width, height)
        }
        else {

            let height = cur_parametric.state.can_fix_height().then_some(None);
            let width = cur_parametric.state.can_fix_width().then_some(cross_stretch_en.then_some(el_sizes.dim_fix.width().unwrap()));
            (width, height)
        }
    }
}