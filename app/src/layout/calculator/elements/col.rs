use crate::layout::calculator::components::element_sizes::{ElementSizes, ElementSizesChildren, ParametricKindState, ParametricSolveState};
use crate::layout::calculator::SideParametricKind;
use crate::layout::{ChildAttributes, ColAttributes, ColChildAttributes, Lu, RowChildAttributes};
use crate::layout::calculator::components::elements::{ElementsChildrenIter, ElementsChildrenIterMut};
use crate::layout::calculator::elements::{ContainerFixSolver, ContainerParametricSolver, HasChildAttributes, SelfDepResolve};

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
impl ContainerParametricSolver for ColSolver<'_> {
    type State = ();
    fn handle_child(&mut self, state: &mut (), child_sizes: &ElementSizes, child_attrs: &ColChildAttributes) -> (Option<Option<Lu>>, Option<Option<Lu>>) {
        (None, None)
    }

    fn finalize(self, state: ()) -> ParametricSolveState {
        ParametricSolveState {
            min_width: 0,
            min_height: 0,
            state: ParametricKindState {
                width: SideParametricKind::Free,
                height: SideParametricKind::Free,
            }
        }
    }
}

// STAGE 2: DIM FIX

impl ContainerFixSolver for ColSolver<'_> {
    type State = ();
    fn init(&mut self, children_sizes: &ElementSizesChildren, children: ElementsChildrenIter) -> Self::State {
    }

    fn handle_child(&mut self, state: &mut Self::State, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes) -> (Option<Option<Lu>>, Option<Option<Lu>>) {
        (None, None)
    }
}

// STAGE 2.1: RESOLVE SELF-DEP
pub fn resolve_selfdep(attrs: &ColAttributes, sizes: &ElementSizes, children: ElementsChildrenIter, children_sizes: &ElementSizesChildren) -> SelfDepResolve {
    SelfDepResolve::Width(100)
}