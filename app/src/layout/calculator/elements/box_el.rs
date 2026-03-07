use crate::layout::BoxAttributes;
use crate::layout::calculator::{ParametricKind, ParametricSolveState, SideParametricKind};

pub fn parametric_solve(attrs: &BoxAttributes) -> ParametricSolveState {
    let mut res = ParametricSolveState::default();

    res.kind = ParametricKind::Normal {
        width: SideParametricKind::Stretchable,
        height: SideParametricKind::Stretchable,
    };
    
    res
}