use crate::layout::BoxAttributes;
use crate::layout::calculator::{ParametricKindState, SideParametricKind};
use crate::layout::calculator::components::element_sizes::ParametricSolveState;

pub fn parametric_solve(attrs: &BoxAttributes) -> ParametricSolveState {
    let mut res = ParametricSolveState::default();

    res.state = ParametricKindState{
        width: SideParametricKind::Free,
        height: SideParametricKind::Free,
    };
    
    res
}