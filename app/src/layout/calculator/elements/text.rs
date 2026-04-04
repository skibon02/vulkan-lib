use crate::layout::calculator::{Fonts, SideParametricState};
use crate::layout::calculator::components::element_sizes::ParametricSolveState;
use crate::layout::calculator::components::text::Texts;
use crate::layout::TextAttributes;

pub fn parametric_solve(attrs: &TextAttributes, i: usize, fonts: &mut Fonts, texts: &mut Texts) -> ParametricSolveState {
    let mut res = ParametricSolveState::default();


    if attrs.preformat {
        // Solve layout for text without width constraints
        let font = fonts.load_font(attrs.font.clone());
        let size = attrs.font_size.with_scale(1.0);

        let text = texts.calculate_layout(i as u32, font, attrs.font.clone(), size, None);
        res.height.min = text.height();
        if !attrs.hide_overflow {
            res.width.min = text.width();
        }

        if !attrs.hide_overflow { 
            res.width.set_fixed();
        }
        res.height.set_fixed();
    }
    else {
        // Deferred layout calculation until width is known
        if attrs.hide_overflow {
            res.width = SideParametricState::new_dependent_fixed();
        }
    }

    res
}
