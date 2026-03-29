use std::cmp::max;
use std::collections::BTreeMap;
use log::warn;
use crate::layout::{BoxAttributes, ChildAttributes, ColAttributes, Element, Lu, MainGapMode, MainSizeMode, RowAttributes, RowChildAttributes};
use crate::layout::calculator::components::element_sizes::{DimFixState, ElementSizes, ElementSizesChildren, ParametricKind, ParametricKindState, ParametricSolveState};
use crate::layout::calculator::components::elements::{ElementsChildrenIter, ElementsChildrenIterMut};
use crate::layout::calculator::elements::{ContainerFixSolver, ContainerParametricSolver, HasChildAttributes, SelfDepResolve};
use crate::layout::calculator::SideParametricKind;

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
}
impl ContainerParametricSolver for RowSolver<'_> {
    type State = RowSolverState;
    fn handle_child(&mut self, state: &mut RowSolverState, child_sizes: &ElementSizes, _: &RowChildAttributes) -> (Option<Option<Lu>>, Option<Option<Lu>>) {
        state.children_width_sum += child_sizes.min_width();
        state.children_max_height = max(state.children_max_height, child_sizes.min_height());

        let grow_en = matches!(self.attrs.main_size_mode, MainSizeMode::EqualWidth);
        let cross_stretch_en = self.attrs.cross_stretch;

        if child_sizes.cur_parametric().state.is_self_dep_both() && !grow_en && !cross_stretch_en{
            // Fix width for self-dep-both child
            (Some(None), None)
        }
        else {
            let cur_parametric = child_sizes.cur_parametric().state;
            let width = (!grow_en && cur_parametric.can_fix_width()).then_some(None);
            let height = (!cross_stretch_en && cur_parametric.can_fix_height()).then_some(None);

            match cur_parametric.kind() {
                ParametricKind::HeightToWidth => {
                    if width.is_none() {
                        state.is_any_selfdepx = true;
                    }
                }
                ParametricKind::WidthToHeight => {
                    if height.is_none() {
                        state.is_any_selfdepy = true;
                    }
                }
                ParametricKind::SelfDepBoth => {
                    if width.is_none() && height.is_none(){
                        state.is_any_selfdepboth = true;
                    }
                }
                _ => {}
            }


            (width, height)
        }
    }

    fn finalize(self, state: RowSolverState) -> ParametricSolveState {
        let mut res = ParametricSolveState::default();

        let grow_en = matches!(self.attrs.main_size_mode, MainSizeMode::EqualWidth);
        let gap_en = matches!(self.attrs.main_gap_mode, MainGapMode::Around | MainGapMode::Between);
        let cross_stretch_en = self.attrs.cross_stretch;

        let has_selfdepx = state.is_any_selfdepx;
        let has_selfdepy = state.is_any_selfdepy;
        let has_selfdepboth = state.is_any_selfdepboth && !has_selfdepx && !has_selfdepy;
        if has_selfdepx && has_selfdepy {
            warn!("row parametric: selfdepx and selfdepy conflict!");
        }

        res.min_width = state.children_width_sum;
        res.min_height = state.children_max_height;
        res.state = ParametricKindState::default();

        if has_selfdepx || has_selfdepboth{
            res.state.height = SideParametricKind::Dependent
        } else if has_selfdepy {
            res.state.width = SideParametricKind::Dependent
        } else {
            if !grow_en && !gap_en {
                res.state.width = SideParametricKind::Fixed;
            }
            if !cross_stretch_en {
                res.state.width = SideParametricKind::Fixed;
            }
        }

        res
    }
}

pub struct RowSolverFixState {
    has_self_dep_x: bool,
    has_self_dep_y: bool,
    fixed_width_sum: Lu,
    sum_width: Lu,
    max_height: Lu,
    breakpoints: BTreeMap<Lu, usize>
}
// STAGE 2: DIM FIX

impl ContainerFixSolver for RowSolver<'_> {
    type State = RowSolverFixState;

    fn init(&self, children_sizes: &ElementSizesChildren, children: ElementsChildrenIter) -> Self::State {
        let mut has_self_dep_x = false;
        let mut has_self_dep_y = false;
        let mut has_self_dep_both = false;
        let mut breakpoints = BTreeMap::new();

        let mut fixed_width_sum = 0;
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

            fixed_width_sum += child_sizes.dim_fix.width().unwrap_or(0);
            if child_sizes.cur_parametric().state.can_fix_width() {
                breakpoints.entry(child_sizes.cur_parametric().min_width).and_modify(|v| *v += 1).or_insert(1);
            }
        }

        if has_self_dep_x || (!has_self_dep_y && has_self_dep_both){
            has_self_dep_y = false;
        }
        else {
            has_self_dep_x = false;
        }

        RowSolverFixState {
            has_self_dep_x,
            has_self_dep_y,
            fixed_width_sum,
            breakpoints,
            max_height: 0,
            sum_width: 0,
        }
    }

    fn early_handle_child(&self, state: &mut Self::State, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes, el_sizes: &ElementSizes) -> (Option<Option<Lu>>, Option<Option<Lu>>) {
        if state.has_self_dep_y {
            let cur_parametric = child_sizes.cur_parametric();
            match cur_parametric.state.kind() {
                ParametricKind::HeightToWidth | ParametricKind::SelfDepBoth => {
                    (None, Some(Some(el_sizes.min_height())))
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
                    (Some(Some(el_sizes.min_width())), None)
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
            // recalculate width sum
            for (i, _) in children {
                let child_sizes = children_sizes.get(i);
                let fix_width = child_sizes.min_width();

                state.sum_width += fix_width;
            }

            Some(SelfDepResolve::Width(state.sum_width))
        }
        else if state.has_self_dep_x {
            // recalculate max min height
            for (i, _) in children {
                let child_sizes = children_sizes.get(i);
                let fix_height= child_sizes.min_height();

                state.max_height = max(state.max_height, fix_height)
            }

            Some(SelfDepResolve::Height(state.max_height))
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
            let width = if cur_parametric.state.can_fix_width() {
                let free_space = el_sizes.dim_fix.width().unwrap() - state.fixed_width_sum;
                let mut min_sum = 0;
                let mut min_cnt = 0;
                'outer: for (&sz, &cnt) in &state.breakpoints {
                    for i in 0..cnt {
                        min_sum += sz;
                        min_cnt += 1;
                        if min_sum > free_space {
                            break 'outer;
                        }
                    }
                }

                let w = 0;
                Some(Some(w))
            }
            else {
                None
            };

            let height = cur_parametric.state.can_fix_height().then_some(cross_stretch_en.then_some(state.max_height));
            (width, height)
        }
        else {

            let width = cur_parametric.state.can_fix_width().then_some(None);
            let height = cur_parametric.state.can_fix_height().then_some(cross_stretch_en.then_some(state.max_height));
            (width, height)
        }
    }
}