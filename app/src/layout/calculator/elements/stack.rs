use crate::layout::calculator::components::ContainerParametricSolver;
use crate::layout::calculator::components::element_sizes::{ElementSizes, ParametricKindState, ParametricSolveState};
use crate::layout::calculator::SideParametricKind;
use crate::layout::{ChildAttributes, StackAttributes, StackChildAttributes, Lu, RowChildAttributes};

pub struct StackParametricSolver
<'a> {
    attrs: &'a StackAttributes,
}

impl ContainerParametricSolver for StackParametricSolver
<'_> {
    type ChildAttributes = StackChildAttributes;
    fn handle_child(&mut self, child_sizes: &ElementSizes, child_attrs: &StackChildAttributes) -> (Option<Option<Lu>>, Option<Option<Lu>>) {
        (None, None)
    }

    fn finalize(self) -> ParametricSolveState {
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
pub fn parametric_solver(attrs: &StackAttributes) -> StackParametricSolver
{
    StackParametricSolver
    {
        attrs
    }

}