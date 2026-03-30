use std::time::Instant;
use smallvec::smallvec;
use crate::layout::{AttributeValue, BoxValue, Color, ColValue, ElementKind, ElementNodeRepr, Fill, GeneralValue, MainGapMode, MainSizeMode, RowValue, RowChildValue, YAlign};
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
            // 0: Root row
            ElementNodeRepr {
                parent_i: 0,
                element: ElementKind::Row,
                attributes: smallvec![
                    AttributeValue::Row(RowValue::MainSizeMode(MainSizeMode::EqualWidth)),
                    AttributeValue::Row(RowValue::MainGapMode(MainGapMode::Fixed(10))),
                ],
            },
            // 1: Red box
            ElementNodeRepr {
                parent_i: 0,
                element: ElementKind::Box,
                attributes: smallvec![
                    AttributeValue::Box(BoxValue::Fill(Some(Fill::Solid(Color(220, 50, 50, 1.0))))),
                    AttributeValue::General(GeneralValue::MinHeight(100)),
                    AttributeValue::General(GeneralValue::NostretchY(true)),
                ],
            },
            // 2: Inner col
            ElementNodeRepr {
                parent_i: 0,
                element: ElementKind::Col,
                attributes: smallvec![
                    AttributeValue::Col(ColValue::MainSizeMode(MainSizeMode::EqualWidth)),
                    AttributeValue::Col(ColValue::MainGapMode(MainGapMode::Fixed(10))),
                ],
            },
            // 3: Green box (child of col)
            ElementNodeRepr {
                parent_i: 2,
                element: ElementKind::Box,
                attributes: smallvec![
                    AttributeValue::Box(BoxValue::Fill(Some(Fill::Solid(Color(50, 200, 80, 1.0))))),
                ],
            },
            // 4: Blue box (child of col)
            ElementNodeRepr {
                parent_i: 2,
                element: ElementKind::Box,
                attributes: smallvec![
                    AttributeValue::Box(BoxValue::Fill(Some(Fill::Solid(Color(50, 100, 220, 1.0))))),
                ],
            },
            // 5: Yellow box
            ElementNodeRepr {
                parent_i: 0,
                element: ElementKind::Box,
                attributes: smallvec![
                    AttributeValue::Box(BoxValue::Fill(Some(Fill::Solid(Color(230, 200, 50, 1.0))))),
                    AttributeValue::General(GeneralValue::MinHeight(60)),
                    AttributeValue::RowChild(RowChildValue::CrossAlign(YAlign::Top), false),
                    AttributeValue::General(GeneralValue::MinWidth(150)),
                ],
            },
            // 6: Nice box
            ElementNodeRepr {
                parent_i: 0,
                element: ElementKind::Box,
                attributes: smallvec![
                    AttributeValue::Box(BoxValue::Fill(Some(Fill::Solid(Color(180, 40, 210, 1.0))))),
                    AttributeValue::General(GeneralValue::MinHeight(60)),
                    AttributeValue::RowChild(RowChildValue::CrossAlign(YAlign::Bottom), false),
                    AttributeValue::General(GeneralValue::MinWidth(100)),
                    AttributeValue::General(GeneralValue::NostretchX(true)),
                    AttributeValue::General(GeneralValue::NostretchY(true)),
                ],
            },
        ]);
        self.start_tm = Instant::now();
    }

    pub fn poll(&mut self, calculator: &mut LayoutCalculator) {
    }
}
