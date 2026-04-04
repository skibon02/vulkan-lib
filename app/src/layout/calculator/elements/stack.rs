use crate::layout::calculator::components::element_sizes::{ElementSizes, ElementSizesChildren, ParametricKindState, ParametricSolveState};
use crate::layout::calculator::SideParametricState;
use crate::layout::{ChildAttributes, StackAttributes, StackChildAttributes, Lu, RowChildAttributes, ColAttributes};
use crate::layout::calculator::components::elements::{ElementsChildrenIter, ElementsChildrenIterMut};
use crate::layout::calculator::elements::{ContainerFixSolver, ContainerParametricSolver, HasChildAttributes, SelfDepResolve};

#[derive(Copy, Clone)]
pub struct StackParametricSolver<'a> {
    attrs: &'a StackAttributes,
}

pub fn solver(attrs: &StackAttributes) -> StackParametricSolver
{
    StackParametricSolver
    {
        attrs
    }
}
impl HasChildAttributes for StackParametricSolver<'_> {
    type ChildAttributes = StackChildAttributes;

    fn unwrap(val: &mut ChildAttributes) -> &mut Self::ChildAttributes {
        &mut val.stack
    }
}


// STAGE 1: PARAMETRIC SOLVE
impl ContainerParametricSolver for StackParametricSolver<'_> {
    type State = ();
    fn handle_child(&mut self, state: &mut (), child_sizes: &ElementSizes, child_attrs: &StackChildAttributes) -> (Option<Option<Lu>>, Option<Option<Lu>>) {
        (None, None)
    }

    fn finalize(self, state: ()) -> ParametricSolveState {
        ParametricSolveState::default()
    }
}
// STAGE 2: DIM FIX

impl ContainerFixSolver for StackParametricSolver<'_> {
    type StateX = ();

    fn init_x(&self, children_sizes: &ElementSizesChildren, children: ElementsChildrenIter) -> Self::StateX {
        todo!()
    }

    fn handle_child_x(&self, state: &mut Self::StateX, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes, sizes: &ElementSizes) -> Option<Option<Lu>> {
        todo!()
    }

    type StateY = ();

    fn init_y(&self, children_sizes: &ElementSizesChildren, children: ElementsChildrenIter) -> Self::StateY {
        todo!()
    }

    fn handle_child_y(&self, state: &mut Self::StateY, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes, sizes: &ElementSizes) -> Option<Option<Lu>> {
        todo!()
    }
}

pub fn resolve_selfdep(attrs: &ColAttributes, sizes: &ElementSizes, children: ElementsChildrenIter, children_sizes: &ElementSizesChildren) -> SelfDepResolve {
    SelfDepResolve::Width(100)
}
