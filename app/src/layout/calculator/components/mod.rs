use crate::layout::{ChildAttributes, Element, Lu};
use crate::layout::calculator::components::element_sizes::{ElementSizes, ParametricSolveState};

pub mod text;
pub mod font;
pub mod image;
pub mod elements;
pub mod element_sizes;

pub trait ChildAttributesUnwrap: Sized {
    fn unwrap(val: &mut ChildAttributes) -> &mut Self;
}
pub trait ContainerParametricSolver: Sized {
    type ChildAttributes: ChildAttributesUnwrap;
    fn handle_child(&mut self, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes) -> (Option<Option<Lu>>, Option<Option<Lu>>);
    fn finalize(self) -> ParametricSolveState;
}