use crate::layout::BoxAttributes;
use crate::layout::calculator::{SideParametricState};
use crate::layout::calculator::components::element_sizes::ParametricSolveState;

pub fn parametric_solve(attrs: &BoxAttributes) -> ParametricSolveState {
    let mut res = ParametricSolveState::default();

    res.width = SideParametricState::new_free();
    res.height = SideParametricState::new_free();

    res
}