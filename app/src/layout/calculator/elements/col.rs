use crate::layout::calculator::components::ContainerParametricSolver;
use crate::layout::calculator::components::element_sizes::{ElementSizes, ParametricKindState, ParametricSolveState};
use crate::layout::calculator::SideParametricKind;
use crate::layout::{ChildAttributes, ColAttributes, ColChildAttributes, Lu, RowChildAttributes};

pub struct ColParametricSolver<'a> {
    attrs: &'a ColAttributes,
}

impl ContainerParametricSolver for ColParametricSolver<'_> {
    type ChildAttributes = ColChildAttributes;
    fn handle_child(&mut self, child_sizes: &ElementSizes, child_attrs: &ColChildAttributes) -> (Option<Option<Lu>>, Option<Option<Lu>>) {
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
pub fn parametric_solver(attrs: &ColAttributes) -> ColParametricSolver {
    ColParametricSolver {
        attrs
    }

}