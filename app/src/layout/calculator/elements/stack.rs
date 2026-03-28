use crate::layout::calculator::components::element_sizes::{ElementSizes, ElementSizesChildren, ParametricKindState, ParametricSolveState};
use crate::layout::calculator::SideParametricKind;
use crate::layout::{ChildAttributes, StackAttributes, StackChildAttributes, Lu, RowChildAttributes, ColAttributes};
use crate::layout::calculator::components::elements::{ElementsChildrenIter, ElementsChildrenIterMut};
use crate::layout::calculator::elements::{ContainerFixSolver, ContainerParametricSolver, HasChildAttributes, SelfDepResolve};

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

impl ContainerFixSolver for StackParametricSolver<'_> {
    type State = ();

    fn init(&mut self, children_sizes: &ElementSizesChildren, children: ElementsChildrenIter) -> Self::State {
        ()
    }

    fn handle_child(&mut self, state: &mut Self::State, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes, el_sizes: &ElementSizes) -> (Option<Option<Lu>>, Option<Option<Lu>>) {
        (None, None)
    }
}

pub fn resolve_selfdep(attrs: &ColAttributes, sizes: &ElementSizes, children: ElementsChildrenIter, children_sizes: &ElementSizesChildren) -> SelfDepResolve {
    SelfDepResolve::Width(100)
}
