use std::cmp::max;
use std::collections::HashMap;
use std::sync::Arc;
use log::{error, warn};
use crate::layout::{AttributeValue, AttributeValues, Element, ElementKind, ElementNode, ElementNodeRepr, Lu, ParsedAttributes};

#[derive(Default)]
enum SelfDepKind {
    #[default]
    None,
    HeightFromWidth,
    WidthFromHeight,
    Both
}

#[derive(Default)]
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

#[derive(Copy, Clone)]
pub enum Phase {
    ParametricSolve,
    FixPass,
}

pub struct LayoutCalculator {
    elements: Vec<ElementNode>,
    calculated: Vec<ElementSizes>,
    images: HashMap<String, ImageInfo>,
    fonts: HashMap<String, FontInfo>,
    texts: HashMap<u32, TextInfo>
}

pub struct ImageInfo {
    // calculated as height / width
    aspect: f32,
}

pub struct FontInfo {
    default_line_height: f32,
}

#[derive(Clone)]
pub struct TextInfo {
    value: Arc<str>,
}

impl LayoutCalculator {
    pub fn new() -> Self {
        LayoutCalculator {
            elements: Vec::new(),
            calculated: Vec::new(),
            images: HashMap::new(),
            fonts: HashMap::new(),
            texts: HashMap::new()
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
        self.elements[element_id as usize].apply(attr);
    }

    fn calc_text_layout(&self, i: u32) -> (Lu, Lu) {
        let el = &self.elements[i as usize];
        if let Element::Text(txt)  = &el.element {
            let oneline = txt.oneline;
            let max_symbols = txt.symbols_limit;
            let font_size = txt.font_size;

        }

        error!("calc_text_layout called on non-text element");
        (0, 0)
    }

    fn process_child_p1(&mut self, child_i: usize, parents: &[usize]) {
        let el = &self.elements[child_i];
        let calc = &mut self.calculated[child_i];

        let general_attrs = &el.general_attributes;
        match &el.element {
            Element::Box(b) => {
                calc.min_width = general_attrs.min_width;
                if general_attrs.nostretch_x {
                    calc.width = Some(calc.min_width);
                }
                calc.min_height = general_attrs.min_height;
                if general_attrs.nostretch_y {
                    calc.height = Some(calc.min_height);
                }
            }
            Element::Img(img) => {
                if let Some(w) = img.width && let Some(h) = img.height {
                    calc.min_width = w;
                    calc.min_height = h;
                    calc.width = Some(w);
                    calc.height = Some(h);
                }
                else if let Some(w) = img.width {
                    calc.min_width = w;
                    let aspect = self.images.get(&img.resource).unwrap().aspect;
                    let h = (w as f32 * aspect) as Lu;
                    calc.min_height = h;
                    calc.width = Some(w);
                    calc.height = Some(h);
                }
                else if let Some(h) = img.height {
                    calc.min_height = h;
                    let aspect = self.images.get(&img.resource).unwrap().aspect;
                    let w = (h as f32 / aspect) as Lu;
                    calc.min_width = w;
                    calc.width = Some(w);
                    calc.height = Some(h);
                }
                else {
                    calc.self_dep = SelfDepKind::Both;
                }

                calc.min_width = max(calc.min_width, general_attrs.min_width);
                calc.min_height = max(calc.min_height, general_attrs.min_width);
            }
            Element::Text(text) => {
                let oneline = text.oneline;
                let hide_overflow = text.hide_overflow;
                if !oneline && !hide_overflow {
                    calc.self_dep = SelfDepKind::HeightFromWidth;
                }
                else if !oneline && hide_overflow {
                    calc.min_width = general_attrs.min_width;
                    if general_attrs.nostretch_x {
                        calc.width = Some(calc.min_width);
                    }
                    calc.min_height = general_attrs.min_height;
                    if general_attrs.nostretch_y {
                        calc.height = Some(calc.min_height);
                    }
                }
                else { // if oneline
                    if hide_overflow {

                    }
                    else {
                        calc.min_width = general_attrs.min_width;
                    }
                }
            }
            _ => {
                // enter container
            }
        }
    }

    fn finalize_container_p1(&mut self, container_i: usize) {

    }

    fn handle_node(&mut self, child_i: usize, parents: &[usize], phase: Phase) {
        if matches!(phase, Phase::FixPass) {
            self.process_child_p1(child_i, parents);
        }
    }

    fn finalize_node(&mut self, container_i: usize, phase: Phase) {
        if matches!(phase, Phase::ParametricSolve) {
            self.finalize_container_p1(container_i);
        }
    }


    pub fn calculate_layout(&mut self, width: u32, height: u32) {
        // reset on each recalculation for now
        for el in self.calculated.iter_mut() {
            *el = Default::default();
        }
        // pass 1: min + self_dep calculation
        self.dfs(0, Phase::ParametricSolve);
    }
    pub fn dfs(&mut self, first_element: usize, phase: Phase) {
        let mut parents = vec![first_element];
        self.handle_node(first_element, &[], phase);
        for i in first_element+1..self.elements.len() {
            let mut last_parent = *parents.last().unwrap();
            while self.elements[i].parent_i < last_parent as u32 {
                // we just left a container
                self.finalize_node(last_parent, phase);
                parents.pop();
                if parents.is_empty() {
                    return;
                }
                last_parent = *parents.last().unwrap();
            }


            self.handle_node(i, &parents, phase);
            parents.push(i);
        }

        while let Some(parent) = parents.pop() {
            self.finalize_node(parent, phase);
        }
    }

    pub fn get_elements(&self) -> Vec<ElementCalculated> {
        vec![]
    }
}