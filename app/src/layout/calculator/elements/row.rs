use std::cmp::max;
use std::collections::BTreeMap;
use log::warn;
use crate::layout::{BoxAttributes, ChildAttributes, ColAttributes, Element, Lu, MainGapMode, MainSizeMode, RowAttributes, RowChildAttributes};
use crate::layout::calculator::components::element_sizes::{DimFixState, ElementSizes, ElementSizesChildren, ParametricKind, ParametricKindState, ParametricSolveState};
use crate::layout::calculator::components::elements::{ElementsChildrenIter, ElementsChildrenIterMut};
use crate::layout::calculator::elements::{ContainerFixSolver, ContainerParametricSolver, HasChildAttributes, SelfDepResolve};
use crate::layout::calculator::SideParametricKind;

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
}
impl ContainerParametricSolver for RowSolver<'_> {
    type State = RowSolverState;
    fn handle_child(&mut self, state: &mut RowSolverState, child_sizes: &ElementSizes, _: &RowChildAttributes) -> (Option<Option<Lu>>, Option<Option<Lu>>) {
        state.children_width_sum += child_sizes.min_width();
        state.children_max_height = max(state.children_max_height, child_sizes.min_height());

        match child_sizes.cur_parametric().state.kind() {
            ParametricKind::HeightToWidth => {
                state.is_any_selfdepy = true;
            }
            ParametricKind::WidthToHeight => {
                state.is_any_selfdepx = true;
            }
            _ => {}
        }

        let grow_en = matches!(self.attrs.main_size_mode, MainSizeMode::EqualWidth);
        let cross_stretch_en = self.attrs.cross_stretch;

        let width = (!grow_en && child_sizes.cur_parametric().state.can_fix_width()).then_some(None);
        let height = (!cross_stretch_en && child_sizes.cur_parametric().state.can_fix_height()).then_some(None);

        (width, height)
    }

    fn finalize(self, state: RowSolverState) -> ParametricSolveState {
        let mut res = ParametricSolveState::default();

        let grow_en = matches!(self.attrs.main_size_mode, MainSizeMode::EqualWidth);
        let gap_en = matches!(self.attrs.main_gap_mode, MainGapMode::Around | MainGapMode::Between);
        let cross_stretch_en = self.attrs.cross_stretch;

        let has_selfdepx = grow_en && state.is_any_selfdepx;
        let has_selfdepy = cross_stretch_en && state.is_any_selfdepy;
        if has_selfdepx && has_selfdepy {
            warn!("row parametric: selfdepx and selfdepy conflict!");
        }

        res.min_width = state.children_width_sum;
        res.min_height = state.children_max_height;
        res.state = ParametricKindState::default();

        if has_selfdepx {
            if !grow_en && !gap_en {
                res.state.width = SideParametricKind::Fixed;
            }

            res.state.height = SideParametricKind::Dependent
        } else if has_selfdepy {
            if !cross_stretch_en {
                res.state.height = SideParametricKind::Fixed;
            }

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
    breakpoints: BTreeMap<Lu, usize>
}
// STAGE 2: DIM FIX

impl ContainerFixSolver for RowSolver<'_> {
    type State = RowSolverFixState;

    fn init(&mut self, children_sizes: &ElementSizesChildren, children: ElementsChildrenIter) -> Self::State {
        let mut has_self_dep_x = false;
        let mut has_self_dep_y = false;
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

            fixed_width_sum += child_sizes.dim_fix.width().unwrap_or(0);
            if child_sizes.cur_parametric().state.can_fix_width() {
                breakpoints.entry(child_sizes.cur_parametric().min_width).and_modify(|v| *v += 1).or_insert(1);
            }
        }

        if has_self_dep_y && has_self_dep_x {
            has_self_dep_y = false;
        }

        RowSolverFixState {
            has_self_dep_x,
            has_self_dep_y,
            fixed_width_sum,
            breakpoints
        }
    }

    fn handle_child(&mut self, state: &mut Self::State, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes, el_sizes: ElementSizes) -> (Option<Option<Lu>>, Option<Option<Lu>>) {
        let grow_en = matches!(self.attrs.main_size_mode, MainSizeMode::EqualWidth) && !state.has_self_dep_y;
        let gap_en = matches!(self.attrs.main_gap_mode, MainGapMode::Around | MainGapMode::Between) && !state.has_self_dep_y;
        let cross_stretch_en = self.attrs.cross_stretch;

        let width = if grow_en {
            let free_space = el_sizes.dim_fix.width().unwrap() - state.fixed_width_sum;
            let mut min_sum = 0;
            let mut min_cnt = 0;
            'outer: for (sz, cnt) in state.breakpoints {
                for i in 0..cnt {
                    min_sum += sz;
                    cnt += 1;
                    if min_sum > free_space {
                        break 'outer;
                    }
                }
            }
        };
        (width, None)
    }
}

pub fn resolve_selfdep(attrs: &ColAttributes, sizes: &ElementSizes, children: ElementsChildrenIter, children_sizes: &ElementSizesChildren) -> SelfDepResolve {
    SelfDepResolve::Width(100)
}
