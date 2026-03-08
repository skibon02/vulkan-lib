use crate::layout::BoxAttributes;
use crate::layout::calculator::{ParametricKind, SideParametricKind};
use crate::layout::calculator::components::element_sizes::ParametricSolveState;

pub fn parametric_solve(attrs: &BoxAttributes) -> ParametricSolveState {
    let mut res = ParametricSolveState::default();

    res.kind = ParametricKind::Normal {
        width: SideParametricKind::Stretchable,
        height: SideParametricKind::Stretchable,
    };
    
    res
}