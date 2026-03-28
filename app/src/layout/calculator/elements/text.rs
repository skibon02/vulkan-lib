use crate::layout::{BoxAttributes, ColAttributes, TextAttributes};
use crate::layout::calculator::{Fonts, ParametricKindState, SideParametricKind};
use crate::layout::calculator::components::element_sizes::{ElementSizes, ElementSizesChildren, ParametricSolveState};
use crate::layout::calculator::components::elements::ElementsChildrenIter;
use crate::layout::calculator::components::text::Texts;
use crate::layout::calculator::elements::SelfDepResolve;

pub fn parametric_solve(attrs: &TextAttributes, i: usize, fonts: &mut Fonts, texts: &mut Texts) -> ParametricSolveState {
    let mut res = ParametricSolveState::default();


    if attrs.preformat {
        // Solve layout for text without width constraints
        let font = fonts.load_font(attrs.font.clone());
        let size = attrs.font_size.with_scale(1.0);

        let text = texts.calculate_layout(i as u32, font, attrs.font.clone(), size, None);
        res.min_height = text.height();
        if !attrs.hide_overflow {
            res.min_width = text.width();
        }

        res.state = ParametricKindState {
            width: if attrs.hide_overflow { SideParametricKind::Free } else { SideParametricKind::Fixed },
            height: SideParametricKind::Fixed,
        };
    }
    else {
        // Deferred layout calculation until width is known
        res.state = ParametricKindState {
            width: SideParametricKind::Free,
            height: if attrs.hide_overflow { SideParametricKind::Free } else { SideParametricKind::Dependent },
        };
    }

    res
}
