use crate::layout::calculator::{Images, ParametricKind, ParametricSolveState, SideParametricKind};
use crate::layout::{ImgAttributes, Lu};

pub fn parametric_solve(attrs: &ImgAttributes, images: &mut Images) -> ParametricSolveState {
    let mut res = ParametricSolveState::default();
    
    let name = attrs.resource.clone();
    let img_info = images.load_image(name);
    if attrs.height.is_none() && attrs.width.is_none() {
        res.kind = ParametricKind::SelfDepBoth { stretch: true };
    }
    else {
        res.kind = ParametricKind::Normal {
            width: SideParametricKind::Fixed,
            height: SideParametricKind::Fixed,
        };

        if let Some(width) = attrs.width {
            res.min_width = width;
            res.min_height = (width as f32 * img_info.aspect) as Lu;
        }
        else if let Some(height) = attrs.height {
            res.min_height = height;
            res.min_width = (height as f32 / img_info.aspect) as Lu;
        }
        else {
            unreachable!()
        }
    }
    
    res
}