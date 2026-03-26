use crate::layout::calculator::components::element_sizes::{ElementSizes, ElementSizesChildren, ParametricSolveState};
use crate::layout::{ChildAttributes, Lu};
use crate::layout::calculator::components::elements::{ElementsChildrenIter, ElementsChildrenIterMut, ElementsChildrenMut};

pub mod img;
pub mod box_el;
pub mod text;
pub mod row;
pub mod col;
pub mod stack;

pub trait HasChildAttributes: Sized {
    type ChildAttributes;
    fn unwrap(val: &mut ChildAttributes) -> &mut Self::ChildAttributes;
}
pub trait ContainerParametricSolver: Sized + HasChildAttributes {
    type State: Default;
    fn handle_child(&mut self, state: &mut Self::State, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes) -> (Option<Option<Lu>>, Option<Option<Lu>>);
    fn finalize(self, state: Self::State) -> ParametricSolveState;
}

pub trait ContainerFixSolver: Sized + HasChildAttributes {
    type State;
    fn init(&mut self, children_sizes: &ElementSizesChildren, children: ElementsChildrenIter) -> Self::State;
    /// Must provide all free axis information as a result to make full fix
    fn handle_child(&mut self, state: &mut Self::State, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes, sizes: &ElementSizes) -> (Option<Option<Lu>>, Option<Option<Lu>>);
}

pub enum SelfDepResolve {
    Width(Lu),
    Height(Lu),
}
