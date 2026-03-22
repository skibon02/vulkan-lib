use crate::layout::{BoxAttributes, ChildAttributes, Element, Lu, RowAttributes, RowChildAttributes};
use crate::layout::calculator::components::ContainerParametricSolver;
use crate::layout::calculator::components::element_sizes::{ElementSizes, ParametricKindState, ParametricSolveState};
use crate::layout::calculator::SideParametricKind;

pub struct RowParametricSolver<'a> {
    attrs: &'a RowAttributes,
}

impl ContainerParametricSolver for RowParametricSolver<'_> {
    type ChildAttributes = RowChildAttributes;
    fn handle_child(&mut self, child_sizes: &ElementSizes, child_attrs: &RowChildAttributes) -> (Option<Option<Lu>>, Option<Option<Lu>>) {
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
pub fn parametric_solver(attrs: &RowAttributes) -> RowParametricSolver {
    RowParametricSolver {
        attrs
    }

    // let grow_en = matches!(attrs.main_size_mode, MainSizeMode::EqualWidth);
    // let gap_en = matches!(attrs.main_gap_mode, MainGapMode::Around | MainGapMode::Between);
    // let cross_stretch_en = attrs.cross_stretch;
    // let (me, children) = self.elements.children(i);
    // self.calculated[i].has_problems = false;
    //
    // let has_selfdepx = grow_en && children.clone().any(|(j, _)| self.calculated[j].post_parametric.kind.is_height_to_width());
    // let has_selfdepy = children.clone().any(|(j, _)| !self.calculated[j].post_parametric.kind.is_width_to_height());
    // if has_selfdepx && has_selfdepy {
    //     self.calculated[i].has_problems = true;
    //     // Error case: stretchable selfdepX and selfdepY cannot exist in the same container!
    //
    // } else if has_selfdepx {
    //     // get rid of selfdepboth
    //     for (j, el) in children.clone() {
    //         if self.calculated[j].parametric.kind.is_both() {
    //             self.calculated[j].parametric.kind = ParametricKind::width_to_height();
    //         }
    //     }
    //     if grow_en {
    //         // fast path: fix selfdepx elements by min width
    //         let mut total_min_width = 0;
    //         let mut max_height = 0;
    //         for (j, el) in children.clone() {
    //             let (width, height) = if self.calculated[j].dim_fix.dim_fixed {
    //                 (self.calculated[j].dim_fix.width, self.calculated[j].dim_fix.height)
    //             }
    //             else {
    //             };
    //             total_min_width += width;
    //             if height > max_height {
    //                 max_height = height;
    //             }
    //         }
    //
    //         self.calculated[i].parametric.min_width = total_min_width;
    //         self.calculated[i].parametric.min_height = max_height;
    //     }
    //     // first handle x axis: handle stretch case
    //     let mut total_min_width = 0;
    //     let mut max_height = 0;
    //     for (j, el) in children.clone() {
    //         let min_width = self.calculated[j].min_width();
    //         total_min_width += min_width;
    //
    //         let min_height = self.calculated[j].min_height();
    //         if min_height > max_height {
    //             max_height = min_height;
    //         }
    //     }
    //
    //     self.calculated[i].parametric.min_width = total_min_width;
    //     self.calculated[i].parametric.min_height = max_height;
    // } else if has_selfdepy {
    // } else {
    // }
}