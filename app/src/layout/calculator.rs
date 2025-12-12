use crate::layout::{AttributeValue, Element, ElementKind, ElementNode, ElementNodeList, Lu};

struct ElementSizes {
    pub min_width: Lu,
    pub min_height: Lu,
    pub fix_height: bool,
    pub fix_width: bool,

    pub pos_x: Lu,
    pub pos_y: Lu,
    pub par_pos_x: Lu,
    pub par_pos_y: Lu,
}

pub struct ElementCalculated {
    id: u32,
    kind: ElementKind,
    pos_x: f32,
    pos_y: f32,
}

pub struct LayoutCalculator {
    elements: Vec<ElementNode>,
    calculated: Vec<ElementSizes>,
}

impl LayoutCalculator {
    pub fn new() -> Self {
        LayoutCalculator {
            elements: Vec::new(),
            calculated: Vec::new(),
        }
    }

    pub fn init(&mut self, elements: ElementNodeList) {

    }

    pub fn update_attribute(&mut self, element_id: u32, attr: AttributeValue) {

    }

    pub fn calculate_layout(&mut self, width: u32, height: u32) {

    }

    pub fn get_elements(&self) -> Vec<ElementCalculated> {
        vec![]
    }
}