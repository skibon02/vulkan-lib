use std::cmp::max;
use std::collections::{HashMap};
use crate::layout::{AttributeValue, Element, ElementKind, ElementNode, ElementNodeRepr, Lu, ParsedAttributes, SelfDepAxis};
use crate::layout::calculator::components::element_sizes::{Calculated, ParametricKind, ParametricKindState};
use crate::layout::calculator::components::elements::Elements;
use crate::layout::calculator::components::font::Fonts;
use crate::layout::calculator::components::image::Images;
use crate::layout::calculator::components::text::Texts;

mod elements;
mod components;

const ZERO_LENGTH_GUARD: Lu = 200;

pub enum FixAxis {
    FixWidth,
    FixHeight,
}


#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub enum SideParametricKind {
    Fixed,
    #[default]
    Free,
    Dependent,
}

#[derive(Default, Clone, Debug, Copy)]
pub enum ParametricStage {
    #[default]
    Parametric,
    PostParametric,
    ParentParametric,
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



#[derive(PartialEq, PartialOrd)]
enum ControlFlow {
    Continue,
    SkipChildren,
}

pub struct LayoutCalculator {
    elements: Elements,
    calculated: Calculated,
    images: Images,
    fonts: Fonts,
    texts: Texts
}

impl LayoutCalculator {
    pub fn new() -> Self {
        LayoutCalculator {
            elements: Elements(Vec::new()),
            calculated: Calculated(Vec::new()),
            images: Images(HashMap::new()),
            fonts: Fonts(HashMap::new()),
            texts: Texts(HashMap::new())
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

    /// Phase 1.1: Parametric solve (dfs)
    /// fill in *.parametric, probably make subtree fix
    /// fill in all children.parent_parametric (if not fixed), probably make subtree fix
    /// Call guarantee: children are post-general solved or fixed. Current element is not solved
    fn parametric_solve(&mut self, i: usize) {
        let me = &mut self.elements[i];
        match &me.element {
            Element::Img(attrs) => {
                self.calculated[i].parametric = elements::img::parametric_solve(attrs, &mut self.images);
            },
            Element::Box(attrs) => {
                self.calculated[i].parametric = elements::box_el::parametric_solve(attrs);
            }
            Element::Text(attrs) => {
                self.calculated[i].parametric = elements::text::parametric_solve(attrs, i, &mut self.fonts, &mut self.texts)
            }
            Element::Row(attrs) => {
                self.calculated[i].parametric = elements::row::parametric_solve(attrs);
            }
            Element::Col(attrs) => {
                self.calculated[i].parametric = elements::col::parametric_solve(attrs);
            }
            Element::Stack(attrs) => {
                self.calculated[i].parametric = elements::stack::parametric_solve(attrs);
            }
        }
        if self.calculated[i].parametric.state.is_fixed() {
            // set dim fixed
            let parametric = self.calculated[i].parametric;
            self.calculated[i].dim_fix.set_width(parametric.min_width);
            self.calculated[i].dim_fix.set_height(parametric.min_height);
        }
    }

    /// Phase 1.2: Apply general attributes
    /// fill in *.post_parametric, probably make subtree fix
    /// Call guarantee: parametric solved, not fixed
    fn apply_general_attrs(&mut self, i: usize) {
        self.calculated[i].post_parametric = self.calculated[i].parametric.clone();
        self.calculated[i].set_parametric_stage(ParametricStage::PostParametric);

        self.calculated[i].post_parametric.apply_min_width(self.elements[i].general_attributes.min_width);
        self.calculated[i].post_parametric.apply_min_height(self.elements[i].general_attributes.min_height);
        if self.elements[i].general_attributes.nostretch_x {
            if self.calculated[i].try_fix_width(None) {
                self.dfs(i, Phase::FixPass);

                return
            }
        }

        if self.elements[i].general_attributes.nostretch_x {
            if self.calculated[i].try_fix_width(None) {
                self.dfs(i, Phase::FixPass);

                return;
            }
        }
    }

    /// Phase 2: Normal flow subtree fix.
    /// Call guarantee: fix dimensions are provided for element i, matching parametric solve ranges.
    /// Result: specify fix dimensions for all direct children, maybe update some internal positioning information.
    fn fix_subtree(&mut self, i: usize) {

    }
    fn handle_node(&mut self, i: usize, parents: &[usize], phase: Phase) -> ControlFlow {
        match phase {
            Phase::ParametricSolve => {
                ControlFlow::Continue
            }
            Phase::FixPass => {
                ControlFlow::Continue
            }
        }
    }

    fn finalize_node(&mut self, i: usize, phase: Phase) {
        if matches!(phase, Phase::ParametricSolve) {
            self.parametric_solve(i);
            if self.calculated[i].parametric.state.is_fixed() {
                return;
            }
            self.apply_general_attrs(i);
            if self.calculated[i].post_parametric.state.is_fixed() {
                return;
            }
        }
    }


    pub fn calculate_layout(&mut self, width: u32, height: u32) {
        // reset on each recalculation for now
        for el in self.calculated.iter_mut() {
            *el = Default::default();
        }

        self.dfs(0, Phase::ParametricSolve);
        self.dfs(0, Phase::FixPass);
    }
    pub fn dfs(&mut self, first_element: usize, phase: Phase) {
        let mut parents = vec![first_element];
        if self.handle_node(first_element, &[], phase) == ControlFlow::SkipChildren {
            self.finalize_node(first_element, phase);
            return;
        }
        let mut i = first_element + 1;
        while i < self.elements.len() {
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


            if self.handle_node(i, &parents, phase) == ControlFlow::SkipChildren {
                self.finalize_node(i, phase);
                // Skip all descendants by advancing until we find a node that's not a child
                let skip_below = i;
                i += 1;
                while i < self.elements.len() && self.elements[i].parent_i > skip_below as u32 {
                    i += 1;
                }
            } else {
                parents.push(i);
                i += 1;
            }
        }

        while let Some(parent) = parents.pop() {
            self.finalize_node(parent, phase);
        }
    }

    pub fn get_elements(&self) -> Vec<ElementCalculated> {
        vec![]
    }
}