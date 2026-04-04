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
    type StateX;
    fn init_x(&self, children_sizes: &ElementSizesChildren, children: ElementsChildrenIter) -> Self::StateX;
    fn handle_child_x(&self, state: &mut Self::StateX, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes, sizes: &ElementSizes) -> Option<Option<Lu>>;

    type StateY;
    fn init_y(&self, children_sizes: &ElementSizesChildren, children: ElementsChildrenIter) -> Self::StateY;
    fn handle_child_y(&self, state: &mut Self::StateY, child_sizes: &ElementSizes, child_attrs: &Self::ChildAttributes, sizes: &ElementSizes) -> Option<Option<Lu>>;
}

pub enum SelfDepResolve {
    Width(Lu),
    Height(Lu),
}
