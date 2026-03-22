use std::collections::{HashMap};
use log::warn;
use crate::layout::{AttributeValue, Element, ElementKind, ElementNode, ElementNodeRepr, Lu, ParsedAttributes, SelfDepAxis};
use crate::layout::calculator::components::element_sizes::{Calculated, ElementSizes, ElementSizesChildren, ParametricKind, ParametricKindState, ParametricSolveState};
use crate::layout::calculator::components::elements::{Elements, ElementsChildrenMut};
use crate::layout::calculator::components::font::Fonts;
use crate::layout::calculator::components::image::Images;
use crate::layout::calculator::components::text::Texts;
use crate::layout::calculator::elements::{ContainerFixSolver, ContainerParametricSolver};

mod elements;
pub mod components;

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
    elements_sizes: Calculated,
    images: Images,
    fonts: Fonts,
    texts: Texts
}

impl LayoutCalculator {
    pub fn new() -> Self {
        LayoutCalculator {
            elements: Elements(Vec::new()),
            elements_sizes: Calculated(Vec::new()),
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
        let (element, children) = self.elements.children(i);
        let (el_sizes, children_sizes) = self.elements_sizes.children(i);
        let parametric = if element.element.is_container() {

            fn parametric_solve_container<T: ContainerParametricSolver>(mut solver: T,
                                                                        container_sizes: &mut ElementSizes,
                                                                        children: ElementsChildrenMut,
                                                                        mut children_sizes: ElementSizesChildren) -> (Vec<u32>, ParametricSolveState) {
                let mut solver_state = T::State::default();
                let mut child_fixes = vec![];
                for (i, child) in children.into_iter() {
                    let child_sizes = children_sizes.get_mut(i);
                    if !child_sizes.cur_parametric_mut().state.is_fixed() {
                        child_sizes.parent_parametric = child_sizes.post_parametric.clone();
                        child_sizes.set_parametric_stage(ParametricStage::ParentParametric);
                    }
                    
                    let (fix_width, fix_height) = solver.handle_child(&mut solver_state, child_sizes, T::unwrap(&mut child.self_child_attributes));
                    if let Some(fix_width) = fix_width {
                        if !child_sizes.cur_parametric_mut().state.can_fix_width() {
                            warn!("Tried to fix width on element {i}, but it is already fixed!");
                        }
                        if child_sizes.try_fix_width(fix_width) {
                            child_fixes.push(i);
                            continue;
                        }
                    }

                    if let Some(fix_height) = fix_height {
                        if !child_sizes.cur_parametric_mut().state.can_fix_height() {
                            warn!("Tried to fix height on element {i}, but it is already fixed!");
                        }
                        if child_sizes.try_fix_height(fix_height) {
                            child_fixes.push(i);
                            continue;
                        }
                    }
                }

                (child_fixes, solver.finalize(solver_state))
            }

            let (child_fixes, parametric) = match &element.element {
                Element::Row(attrs) => {
                    parametric_solve_container(elements::row::solver(attrs), el_sizes, children, children_sizes)
                }
                Element::Col(attrs) => {
                    parametric_solve_container(elements::col::solver(attrs), el_sizes, children, children_sizes)
                }
                Element::Stack(attrs) => {
                    parametric_solve_container(elements::stack::solver(attrs), el_sizes, children, children_sizes)
                }
                _ => unreachable!(),
            };
            // fix children
            for child_fix in child_fixes {
                self.dfs(child_fix as usize, Phase::FixPass);
            }
            
            parametric
        }
        else {
            match &element.element {
                Element::Img(attrs) => {
                     elements::img::parametric_solve(attrs, &mut self.images)
                },
                Element::Box(attrs) => {
                     elements::box_el::parametric_solve(attrs)
                }
                Element::Text(attrs) => {
                     elements::text::parametric_solve(attrs, i, &mut self.fonts, &mut self.texts)
                }
                _ => unreachable!(),
            }
        };

        // set self parametric params
        self.elements_sizes[i].parametric = parametric;
        if self.elements_sizes[i].parametric.state.is_fixed() {
            // set dim fixed
            self.elements_sizes[i].dim_fix.set_width(parametric.min_width);
            self.elements_sizes[i].dim_fix.set_height(parametric.min_height);

            // run fix subtree
            self.dfs(i, Phase::FixPass);
        }
    }


    /// Phase 1.2: Apply general attributes
    /// fill in *.post_parametric, probably make subtree fix
    /// Call guarantee: parametric solved, not fixed
    fn apply_general_attrs(&mut self, i: usize) {
        self.elements_sizes[i].post_parametric = self.elements_sizes[i].parametric.clone();
        self.elements_sizes[i].set_parametric_stage(ParametricStage::PostParametric);

        self.elements_sizes[i].post_parametric.apply_min_width(self.elements[i].general_attributes.min_width);
        self.elements_sizes[i].post_parametric.apply_min_height(self.elements[i].general_attributes.min_height);
        if self.elements[i].general_attributes.nostretch_x {
            if self.elements_sizes[i].try_fix_width(None) {
                self.dfs(i, Phase::FixPass);

                return
            }
        }

        if self.elements[i].general_attributes.nostretch_y {
            if self.elements_sizes[i].try_fix_height(None) {
                self.dfs(i, Phase::FixPass);

                return;
            }
        }
    }

    /// Phase 2: Normal flow subtree fix.
    /// Call guarantee: fix dimensions are provided for element i, matching parametric solve ranges.
    /// Result: specify fix dimensions for all direct children, maybe update some internal positioning information.
    fn dim_fix_children(&mut self, i: usize) {
        let (el_sizes, children_sizes) = self.elements_sizes.children(i);
        let (element, children) = self.elements.children(i);
        if element.element.is_container() {
            fn dim_fix_container<T: ContainerFixSolver>(mut solver: T,
                                                        sizes: &mut ElementSizes,
                                                        children: ElementsChildrenMut,
                                                        mut children_sizes: ElementSizesChildren) -> Vec<(usize, Option<Option<Lu>>, Option<Option<Lu>>)> {
                let mut res = vec![];
                let mut state = solver.init(&children_sizes, children.iter());

                for (i, child) in children.into_iter() {
                    let (width, height) = solver.handle_child(&mut state, children_sizes.get_mut(i), T::unwrap(&mut child.self_child_attributes));
                    res.push((i as usize, width, height));
                }
                res
            }

            let (children_fixes) = match &element.element {
                Element::Row(attrs) => {
                    dim_fix_container(elements::row::solver(attrs), el_sizes, children, children_sizes)
                }
                Element::Col(attrs) => {
                    dim_fix_container(elements::col::solver(attrs), el_sizes, children, children_sizes)
                }
                Element::Stack(attrs) => {
                    dim_fix_container(elements::stack::solver(attrs), el_sizes, children, children_sizes)
                }
                _ => unreachable!(),
            };

            for (i, width, height) in children_fixes {
                if let Some(width) = width {
                    if !self.elements_sizes[i].cur_parametric().state.can_fix_width() {
                        panic!("Attempted to fix width for element {i}, but it is not free!");
                    }
                    self.elements_sizes[i].try_fix_width(width);
                }

                if let Some(height) = height {
                    if !self.elements_sizes[i].cur_parametric().state.can_fix_height() {
                        panic!("Attempted to fix height for element {i}, but it is not free!");
                    }
                    self.elements_sizes[i].try_fix_height(height);
                }

                assert!(self.elements_sizes[i].cur_parametric_mut().state.is_fixed(), "All children must be fixed in the end of dim_fix_children!");
            }
        }
    }

    fn dim_fix_finalize(&mut self, i: usize) {
        let (el_sizes, children_sizes) = self.elements_sizes.children(i);
        let (element, children) = self.elements.children(i);
        if element.element.is_container() {
            match &element.element {
                Element::Col(col) => {
                    
                }
                Element::Row(row) => {

                }
                Element::Stack(stack) => {

                }
                _ => unreachable!()
            }
        }
    }
    fn handle_node(&mut self, i: usize, parents: &[usize], phase: Phase) -> ControlFlow {
        match phase {
            Phase::ParametricSolve => {
                ControlFlow::Continue
            }
            Phase::FixPass => {
                if self.elements_sizes[i].dim_fix.is_subtree_fixed() {
                    ControlFlow::SkipChildren
                }
                else {
                    self.dim_fix_children(i);
                    self.elements_sizes[i].dim_fix.set_subtree_fixed();
                    ControlFlow::Continue
                }
            }
        }
    }

    fn finalize_node(&mut self, i: usize, phase: Phase) {
        match phase {
            Phase::ParametricSolve => {
                self.parametric_solve(i);
                if self.elements_sizes[i].parametric.state.is_fixed() {
                    return;
                }
                self.apply_general_attrs(i);
                if self.elements_sizes[i].post_parametric.state.is_fixed() {
                    return;
                }
            }
            Phase::FixPass => {
                if self.elements_sizes[i].cur_parametric_mut().state.is_self_dep() {
                    self.dim_fix_finalize(i);
                }
                self.elements_sizes[i].dim_fix.set_subtree_fixed();
            }
        }
    }


    pub fn calculate_layout(&mut self, width: u32, height: u32) {
        // full reset on each recalculation for now
        for el in self.elements_sizes.iter_mut() {
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