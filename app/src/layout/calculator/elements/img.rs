use crate::layout::calculator::{Images, SideParametricState};
use crate::layout::{ImgAttributes, Lu};
use crate::layout::calculator::components::element_sizes::ParametricSolveState;

pub fn parametric_solve(attrs: &ImgAttributes, images: &mut Images) -> ParametricSolveState {
    let mut res = ParametricSolveState::default();
    
    let name = attrs.resource.clone();
    let img_info = images.load_image(name);
    if attrs.height.is_none() && attrs.width.is_none() {
        res.width = SideParametricState::new_dependent();
        res.height = SideParametricState::new_dependent();
    }
    else {
        res.width = SideParametricState::new_fixed();
        res.height = SideParametricState::new_fixed();

        if let Some(width) = attrs.width {
            res.width.min = width;
            res.height.min = (width as f32 * img_info.aspect()) as Lu;
        }
        else if let Some(height) = attrs.height {
            res.height.min = height;
            res.width.min = (height as f32 / img_info.aspect()) as Lu;
        }
        else {
            unreachable!()
        }
    }
    
    res
}