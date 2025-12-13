use std::collections::HashMap;
use crate::layout::{AttributeValue, AttributeValues, Element, ElementKind, ElementNode, ElementNodeRepr, Lu, ParsedAttributes};

enum SelfDepKind {
    None,
    WidthToHeight,
    HeightToWidth,
    Both
}

struct ElementSizes {
    // 1 pass
    pub min_width: Lu,
    pub min_height: Lu,
    pub self_dep: SelfDepKind,

    // 1/2 pass
    pub height: Option<Lu>,
    pub width: Option<Lu>,

    // 2 pass
    pub rel_pos_x: Lu,
    pub rel_pos_y: Lu,
    pub pos_x: Lu,
    pub pos_y: Lu,
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

    pub fn init(&mut self, elements: Vec<ElementNodeRepr>) {
        let mut element_nodes = Vec::with_capacity(elements.len());
        let mut last_sibling_i: HashMap<u32, u32> = HashMap::new();
        for (i, elem) in elements.into_iter().enumerate() {
            let attributes = ParsedAttributes::from(elem.attributes);
            let element = Element::from((elem.element, &attributes));
            element_nodes.push(ElementNode {
                next_sibling_i: None,
                parent_i: elem.parent_i,
                element,
                general_attributes: attributes.general.unwrap_or_default(),
                self_child_attributes: attributes.self_child.unwrap_or_default(),
            });

            if i > 0 && let Some(last_sibling_i) = last_sibling_i.get(&elem.parent_i) {
                element_nodes[*last_sibling_i as usize].next_sibling_i = Some(i as u32);
            }

            last_sibling_i.insert(elem.parent_i, i as u32);
        }
    }

    pub fn hide_element(&mut self, element_id: u32) {

    }

    pub fn show_element(&mut self, element_id: u32) {

    }

    pub fn update_attribute(&mut self, element_id: u32, attr: AttributeValue) {

    }

    pub fn calculate_layout(&mut self, width: u32, height: u32) {

    }

    pub fn get_elements(&self) -> Vec<ElementCalculated> {
        vec![]
    }
}