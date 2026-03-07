use crate::layout::{BoxAttributes, TextAttributes};
use crate::layout::calculator::{Fonts, ParametricKind, ParametricSolveState, SideParametricKind, Texts};

pub fn parametric_solve(attrs: &TextAttributes, i: usize, fonts: &mut Fonts, texts: &mut Texts) -> ParametricSolveState {
    let mut res = ParametricSolveState::default();


    if attrs.preformat {
        // Solve layout for text without width constraints
        let font = fonts.load_font(attrs.font.clone());
        let size = attrs.font_size.with_scale(1.0);

        let text = texts.calculate_layout(i as u32, font, attrs.font.clone(), size, None);
        res.min_height = text.text_height;
        if !attrs.hide_overflow {
            res.min_width = text.text_width;
        }

        res.kind = ParametricKind::Normal {
            width: if attrs.hide_overflow { SideParametricKind::Stretchable } else { SideParametricKind::Fixed },
            height: SideParametricKind::Fixed,
        };
    }
    else {
        // Deferred layout calculation until width is known
        if attrs.hide_overflow {
            res.kind = ParametricKind::Normal {
                width: SideParametricKind::Stretchable,
                height: SideParametricKind::Stretchable,
            };
        }
        else {
            res.kind = ParametricKind::width_to_height();
        }
    }

    res
}
