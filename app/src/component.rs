use std::time::Instant;
use smallvec::smallvec;
use crate::layout::{AttributeValue, ElementKind, ElementNodeRepr, ImgValue, MainGapMode, RowValue, TextValue, XAlign};
use crate::layout::calculator::LayoutCalculator;

pub struct Component {
    start_tm: Instant
}

impl Component {
    pub fn new() -> Self {
        Component {
            start_tm: Instant::now(),
        }
    }

    pub fn init(&mut self, calculator: &mut LayoutCalculator) {
        calculator.init(vec![
            ElementNodeRepr {
                parent_i: 0,
                element: ElementKind::Row,
                attributes: smallvec![AttributeValue::Row(RowValue::MainGapMode(MainGapMode::Fixed(100)))],
            },
            ElementNodeRepr {
                parent_i: 0,
                element: ElementKind::Img,
                attributes: smallvec![AttributeValue::Img(ImgValue::Resource(String::from("hello.png")))],
            },
            ElementNodeRepr {
                parent_i: 0,
                element: ElementKind::Text,
                attributes: smallvec![AttributeValue::Text(TextValue::Oneline(true)), AttributeValue::Text(TextValue::TextAlignX(XAlign::Center))],
            },
        ]);
        self.start_tm = Instant::now();
    }

    pub fn poll(&mut self, calculator: &mut LayoutCalculator) {
        let hide_img = self.start_tm.elapsed().as_secs() % 2 == 0;
        if hide_img {
            calculator.hide_element(1);
        }
        else {
            calculator.show_element(1);
        }
    }
}
