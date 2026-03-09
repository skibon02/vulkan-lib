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

const ZERO_LENGTH_GUARD: Lu = 20;

pub enum FixAxis {
    FixWidth,
    FixHeight,
}


#[derive(Copy, Clone, Debug, Default)]
pub enum SideParametricKind {
    Fixed,
    #[default]
    Stretchable,
    Dependent,
}

#[derive(Clone, Debug, Copy)]
pub enum ParametricStage {
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
        self.calculated[i].post_parametric.min_width = max(self.elements[i].general_attributes.min_width, self.calculated[i].post_parametric.min_width);
        self.calculated[i].post_parametric.min_height = max(self.elements[i].general_attributes.min_height, self.calculated[i].post_parametric.min_height);
        if self.elements[i].general_attributes.nostretch_x {
            if self.calculated[i].post_parametric.state.kind() == ParametricKind::SelfDepBoth {
                *stretch = true;
            }
            else if self.try_fix_axis_subtree(i, None, FixAxis::FixWidth, ParametricStage::PostParametric) {
                self.calculated[i].parent_parametric = self.calculated[i].post_parametric.clone();
                return;
            }
        }
        if self.elements[i].general_attributes.nostretch_y {
            if self.calculated[i].post_parametric.state.kind() == ParametricKind::SelfDepBoth {
                *stretch = true;
            }
            else if self.try_fix_axis_subtree(i, None, FixAxis::FixHeight, ParametricStage::PostParametric) {
                self.calculated[i].parent_parametric = self.calculated[i].post_parametric.clone();
                return;
            }
        }

        if self.calculated[i].post_parametric.state.kind() == ParametricKind::SelfDepBoth {
            match self.elements[i].general_attributes.self_dep_axis {
                SelfDepAxis::HeightFromWidth => {
                    if stretch {
                        // <- nostretch_x is not enabled, transform to selfdepx
                        self.calculated[i].post_parametric.state = ParametricKindState::width_to_height();
                    }
                    else {
                        // cannot stay selfdepx with stretch disabled - subtree fix
                        if self.try_fix_axis_subtree(i, None, FixAxis::FixWidth, ParametricStage::PostParametric) {
                            self.calculated[i].parent_parametric = self.calculated[i].post_parametric.clone();
                        }
                    }
                }
                SelfDepAxis::WidthFromHeight => {
                    if stretch {
                        // <- nostretch_y is not enabled, transform to selfdepy
                        self.calculated[i].post_parametric.state = ParametricKindState::height_to_width();
                    }
                    else {
                        // cannot stay selfdepy with stretch disabled - subtree fix
                        if self.try_fix_axis_subtree(i, None, FixAxis::FixHeight, ParametricStage::PostParametric) {
                            self.calculated[i].parent_parametric = self.calculated[i].post_parametric.clone();
                        }
                    }
                }
                SelfDepAxis::Auto => {}
            }
        }
    }

    /// Phase 2: Normal flow subtree fix.
    /// Call guarantee: fix dimensions are provided for element i, matching parametric solve ranges.
    /// Result: specify fix dimensions for all direct children, maybe update some internal positioning information.
    fn fix_subtree(&mut self, i: usize) {

    }

    /// Fix element axis subtree with length guard. Recursively spawn DFS if fully fix.
    /// If another axis is still stretchable, new fix dfs is not spawned, and false is returned
    fn try_fix_axis_subtree(&mut self, i: usize, length: Option<Lu>, fix_axis: FixAxis, parametric_stage: ParametricStage) -> bool {
        let calculated = &mut self.calculated[i];

        let mut length = length.unwrap_or_else(|| {
            match fix_axis {
                FixAxis::FixWidth => calculated.parametric(parametric_stage).min_width,
                FixAxis::FixHeight => calculated.parametric(parametric_stage).min_height,
            }
        });

        if length == 0 {
            length = ZERO_LENGTH_GUARD;
            calculated.has_problems = true;
        }
        match fix_axis {
            FixAxis::FixWidth => {
                if matches!(calculated.parametric(parametric_stage).state, ParametricKindState::Normal { width: SideParametricKind::Dependent | SideParametricKind::Fixed, .. }) {
                    return false
                }

                if calculated.parametric(parametric_stage).state.kind() == ParametricKind::SelfDepBoth {
                    panic!("Assertion failed! width subtree fix called on selfdepboth!")
                }

                calculated.dim_fix.set_width(length);
                match &mut calculated.parametric(parametric_stage).state {
                    ParametricKindState::Normal {
                        width,
                        height
                    } => {
                        *width = SideParametricKind::Fixed;
                        if matches!(height,  SideParametricKind::Stretchable) {
                            false
                        }
                        else {
                            // Launch new DFS to fix selfdep subtree with new length constraint
                            self.dfs(i, Phase::FixPass);
                            true
                        }
                    }
                    ParametricKindState::SelfDepBoth {
                        ..
                    } => {
                        calculated.parametric(parametric_stage).state = ParametricKindState::Normal {
                            width: SideParametricKind::Fixed,
                            height: SideParametricKind::Dependent,
                        };
                        // Launch new DFS to fix selfdep subtree with new length constraint
                        self.dfs(i, Phase::FixPass);
                        true
                    }
                }
            }
            FixAxis::FixHeight => {
                if matches!(calculated.parametric(parametric_stage).state, ParametricKindState::Normal { height: SideParametricKind::Dependent | SideParametricKind::Fixed, .. }) {
                    return false
                }
                if calculated.parametric(parametric_stage).state.kind() == ParametricKind::SelfDepBoth {
                    panic!("Assertion failed! height subtree fix called on selfdepboth with stretch enabled!")
                }

                calculated.dim_fix.set_height(length);
                match &mut calculated.parametric(parametric_stage).state {
                    ParametricKindState::Normal {
                        width,
                        height
                    } => {
                        *height = SideParametricKind::Fixed;
                        if matches!(width,  SideParametricKind::Stretchable) {
                            false
                        } else {
                            // Launch new DFS to fix selfdep subtree with new length constraint
                            self.dfs(i, Phase::FixPass);
                            true
                        }
                    }
                    ParametricKindState::SelfDepBoth {
                        ..
                    } => {
                        calculated.parametric(parametric_stage).state = ParametricKindState::Normal {
                            width: SideParametricKind::Dependent,
                            height: SideParametricKind::Fixed,
                        };
                        // Launch new DFS to fix selfdep subtree with new length constraint
                        self.dfs(i, Phase::FixPass);
                        true
                    }
                }
            }
        }
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