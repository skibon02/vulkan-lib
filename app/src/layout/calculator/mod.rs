use std::cmp::max;
use std::collections::{HashMap};
use log::warn;
use crate::layout::{AttributeValue, Element, ElementKind, ElementNode, ElementNodeRepr, Lu, MainGapMode, ParsedAttributes, XAlign, YAlign};
use crate::layout::calculator::components::element_sizes::{Calculated, ElementSizes, ElementSizesChildren, ParametricSolveState};
use crate::layout::calculator::components::elements::{Elements, ElementsChildrenMut};
use crate::layout::calculator::components::font::Fonts;
use crate::layout::calculator::components::image::Images;
use crate::layout::calculator::components::text::Texts;
use crate::layout::calculator::elements::{ContainerFixSolver, ContainerParametricSolver, SelfDepResolve};

mod elements;
pub mod components;

const ZERO_LENGTH_GUARD: Lu = 200;

pub enum FixAxis {
    FixWidth,
    FixHeight,
}


#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct SideParametricState {
    min: Lu,
    fixed: bool,
    dependent: bool,
}

impl SideParametricState {
    pub fn new_free() -> Self {
        Self {
            min: 0,
            fixed: false,
            dependent: false
        }
    }
    pub fn new_dependent() -> Self {
        Self {
            min: 0,
            fixed: false,
            dependent: true
        }
    }
    pub fn new_dependent_fixed() -> Self {
        Self {
            min: 0,
            fixed: true,
            dependent: true
        }
    }
    pub fn new_fixed() -> Self {
        Self {
            min: 0,
            fixed: true,
            dependent: false
        }
    }
    fn is_fixed(&self) -> bool {
        self.fixed
    }
    pub fn set_fixed(&mut self) {
        self.fixed = true;
    }
    fn is_dependent(&self) -> bool {
        self.dependent
    }
    fn min_len(&self) -> Lu {
        self.min
    }
    fn apply_min_len(&mut self, len: Lu) {
        self.min = max(self.min, len);
    }
}

#[derive(Default, Clone, Debug, Copy)]
pub enum ParametricStage {
    #[default]
    Parametric,
    PostParametric,
    ParentParametric,
}


pub struct RenderRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
    pub depth: f32,
}

#[derive(Copy, Clone)]
pub enum Phase {
    ParametricSolve,
    FixPassX,
    FixPassY,
    PosFixPass,
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

        let len = element_nodes.len();
        self.elements = Elements(element_nodes);
        self.elements_sizes = Calculated(vec![Default::default(); len]);
    }
    
    pub fn set_text(&mut self, i: u32, text: &str) {
        self.texts.set_text(i, text.into())
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
        let (element, mut children) = self.elements.children(i);
        let (el_sizes, children_sizes) = self.elements_sizes.children(i);
        let parametric = if element.element.is_container() {

            fn parametric_solve_container<'a, T: ContainerParametricSolver>(mut solver: T,
                                                                        container_sizes: &mut ElementSizes,
                                                                        children: &mut ElementsChildrenMut<'a>,
                                                                        mut children_sizes: ElementSizesChildren) -> (Vec<u32>, Vec<u32>, ParametricSolveState) {
                let mut solver_state = T::State::default();
                let mut child_fixes_x = vec![];
                let mut child_fixes_y = vec![];
                for (i, child) in children.iter_mut() {
                    let child_sizes = children_sizes.get_mut(i);
                    if !child_sizes.cur_parametric_mut().is_fixed() {
                        child_sizes.parent_parametric = child_sizes.post_parametric.clone();
                        child_sizes.set_parametric_stage(ParametricStage::ParentParametric);
                    }
                    
                    let (fix_width, fix_height) = solver.handle_child(&mut solver_state, child_sizes, T::unwrap(&mut child.self_child_attributes));
                    if let Some(fix_width) = fix_width {
                        if !child_sizes.cur_parametric().can_fix_width() {
                            warn!("Tried to fix width on element {i}, but it is already fixed!");
                        }
                        if child_sizes.try_provide_width(fix_width) {
                            child_fixes_x.push(i);
                            if fix_height.is_some() {
                                warn!("Tried to fix width and height on element {i}, but fixing width was already enough!");
                            }
                        }
                    }

                    if let Some(fix_height) = fix_height {
                        if !child_sizes.cur_parametric().can_fix_height() {
                            warn!("Tried to fix height on element {i}, but it is already fixed!");
                        }
                        if child_sizes.try_provide_height(fix_height) {
                            child_fixes_y.push(i);
                            continue;
                        }
                    }
                }

                (child_fixes_x, child_fixes_y, solver.finalize(solver_state))
            }

            let (child_fixes_x, child_fixes_y, parametric) = match &element.element {
                Element::Row(attrs) => {
                    parametric_solve_container(elements::row::solver(attrs), el_sizes, &mut children, children_sizes)
                }
                Element::Col(attrs) => {
                    parametric_solve_container(elements::col::solver(attrs), el_sizes, &mut children, children_sizes)
                }
                Element::Stack(attrs) => {
                    parametric_solve_container(elements::stack::solver(attrs), el_sizes, &mut children, children_sizes)
                }
                _ => unreachable!(),
            };
            // fix children
            for child_fix in child_fixes_x {
                self.dfs(child_fix as usize, Phase::FixPassX);
            }
            for child_fix in child_fixes_y {
                self.dfs(child_fix as usize, Phase::FixPassY);
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
        if parametric.is_fixed() {
            // set dim fixed
            if parametric.width.is_fixed() {
                self.elements_sizes[i].dim_fix.set_width(parametric.width.min);
            }
            if parametric.height.is_fixed() {
                self.elements_sizes[i].dim_fix.set_height(parametric.height.min);
            }
        }
    }


    /// Phase 1.2: Apply general attributes
    /// fill in *.post_parametric, probably make subtree fix
    /// Call guarantee: parametric solved, not fixed
    fn apply_general_attrs(&mut self, i: usize) {
        self.elements_sizes[i].post_parametric = self.elements_sizes[i].parametric.clone();
        self.elements_sizes[i].set_parametric_stage(ParametricStage::PostParametric);

        self.elements_sizes[i].post_parametric.width.apply_min_len(self.elements[i].general_attributes.min_width);
        self.elements_sizes[i].post_parametric.height.apply_min_len(self.elements[i].general_attributes.min_height);
        if self.elements[i].general_attributes.nostretch_x {
            if self.elements_sizes[i].try_provide_width(None) {
                self.dfs(i, Phase::FixPassX);
            }
        }

        if self.elements[i].general_attributes.nostretch_y {
            if self.elements_sizes[i].try_provide_height(None) {
                self.dfs(i, Phase::FixPassY);
            }
        }
    }

    fn handle_self_dep_resolve(el_sizes: &mut ElementSizes, res: Option<SelfDepResolve>) {
        match res {
            Some(SelfDepResolve::Height(h)) => {
                el_sizes.dim_fix.set_height(h);
            }
            Some(SelfDepResolve::Width(w)) => {
                el_sizes.dim_fix.set_width(w);
            }
            None => {}
        }
    }

    /// Phase 2: Normal flow subtree fix.
    /// Call guarantee: fix dimensions are provided for element i, matching parametric solve ranges.
    /// Result: specify fix dimensions for all direct children, maybe update some internal positioning information.
    fn dim_fix_children(&mut self, i: usize, fix_x: bool) {
        let (el_sizes, mut children_sizes) = self.elements_sizes.children(i);
        let (element, mut children) = self.elements.children(i);
        if element.element.is_container() {
            fn dim_fix_pass_x<T: ContainerFixSolver>(mut solver: T,
                                                    mut state: T::StateX,
                                                    sizes: &mut ElementSizes,
                                                    children: &mut ElementsChildrenMut,
                                                    children_sizes: &mut ElementSizesChildren) -> Vec<(usize, Option<Option<Lu>>)> {
                let mut res = vec![];

                for (i, child) in children.iter_mut() {
                    let width = solver.handle_child_x(&mut state, children_sizes.get_mut(i), T::unwrap(&mut child.self_child_attributes), sizes);
                    if width.is_some() {
                        res.push((i as usize, width));
                    }
                }
                res
            }

            fn dim_fix_pass_y<T: ContainerFixSolver>(mut solver: T,
                                                     mut state: T::StateY,
                                                     sizes: &mut ElementSizes,
                                                     children: &mut ElementsChildrenMut,
                                                     children_sizes: &mut ElementSizesChildren) -> Vec<(usize, Option<Option<Lu>>)> {
                let mut res = vec![];

                for (i, child) in children.iter_mut() {
                    let height = solver.handle_child_y(&mut state, children_sizes.get_mut(i), T::unwrap(&mut child.self_child_attributes), sizes);
                    if height.is_some() {
                        res.push((i as usize, height));
                    }
                }
                res
            }

            fn fix_children_x(children_fixes: &Vec<(usize, Option<Option<Lu>>)>, element_sizes: &mut ElementSizesChildren) {
                for (i, width) in children_fixes {
                    let element_sizes = element_sizes.get_mut(*i as u32);
                    if let Some(width) = width {
                        if !element_sizes.try_provide_width (*width) {
                            panic!("Attempted to fix width for element {i}, but it is not free!");
                        }
                    }
                }
            }

            fn fix_children_y(children_fixes: &Vec<(usize, Option<Option<Lu>>)>, element_sizes: &mut ElementSizesChildren) {
                for (i, height) in children_fixes {
                    let element_sizes = element_sizes.get_mut(*i as u32);
                    if let Some(height) = height {
                        if !element_sizes.try_provide_height(*height) {
                            panic!("Attempted to fix height for element {i}, but it is not free!");
                        }
                    }
                }
            }

            match element.element.clone() {
                Element::Row(attrs) => {
                    let solver = elements::row::solver(&attrs);
                    if fix_x && el_sizes.try_fix_width() {
                        let state = solver.init_x(&children_sizes, children.iter());
                        let early_fixes = dim_fix_pass_x(solver, state, el_sizes, &mut children, &mut children_sizes);
                        fix_children_x(&early_fixes, &mut children_sizes);
                    }
                    else if !fix_x && el_sizes.try_fix_height() {
                        let state = solver.init_y(&children_sizes, children.iter());
                        let early_fixes = dim_fix_pass_y(solver, state, el_sizes, &mut children, &mut children_sizes);
                        fix_children_y(&early_fixes, &mut children_sizes);
                    }
                }
                Element::Col(attrs) => {
                    let solver = elements::col::solver(&attrs);
                    if fix_x && el_sizes.try_fix_width() {
                        let state = solver.init_x(&children_sizes, children.iter());
                        let early_fixes = dim_fix_pass_x(solver, state, el_sizes, &mut children, &mut children_sizes);
                        fix_children_x(&early_fixes, &mut children_sizes);
                    }
                    else if !fix_x && el_sizes.try_fix_height() {
                        let state = solver.init_y(&children_sizes, children.iter());
                        let early_fixes = dim_fix_pass_y(solver, state, el_sizes, &mut children, &mut children_sizes);
                        fix_children_y(&early_fixes, &mut children_sizes);
                    }
                }
                Element::Stack(attrs) => {
                    let solver = elements::stack::solver(&attrs);
                    if fix_x && el_sizes.try_fix_width() {
                        let state = solver.init_x(&children_sizes, children.iter());
                        let early_fixes = dim_fix_pass_x(solver, state, el_sizes, &mut children, &mut children_sizes);
                        fix_children_x(&early_fixes, &mut children_sizes);
                    }
                    else if !fix_x && el_sizes.try_fix_height() {
                        let state = solver.init_y(&children_sizes, children.iter());
                        let early_fixes = dim_fix_pass_y(solver, state, el_sizes, &mut children, &mut children_sizes);
                        fix_children_y(&early_fixes, &mut children_sizes);
                    }

                }
                _ => unreachable!(),
            };
        }
        else {
            if fix_x {
                assert!(el_sizes.try_fix_width());
            }
            else {
                assert!(el_sizes.try_fix_height());
            }
            match &element.element {
                Element::Img(attrs) => {
                    let img_info = self.images.load_image(attrs.resource.clone());
                    let aspect = img_info.aspect();
                    if let Some(w) = el_sizes.dim_fix.width() && el_sizes.dim_fix.height().is_none() {
                        el_sizes.dim_fix.set_height((w as f32 * aspect) as Lu);
                    }
                    else if let Some(h) = el_sizes.dim_fix.height() && el_sizes.dim_fix.height().is_none() {
                        el_sizes.dim_fix.set_width((h as f32 / aspect) as Lu);
                    }
                }
                Element::Box(_) => {
                    // No dependent dimensions to resolve
                }
                Element::Text(attrs) => {
                    if let Some(w) = el_sizes.dim_fix.width() {
                        let font = self.fonts.load_font(attrs.font.clone());
                        let size = attrs.font_size.with_scale(1.0);
                        let text = self.texts.calculate_layout(i as u32, font, attrs.font.clone(), size, Some(w));

                        if el_sizes.dim_fix.height().is_none() {
                            el_sizes.dim_fix.set_height(text.height());
                        }
                    }
                }
                _ => unreachable!()
            }
        }
    }

    fn dim_fix_self_dep_resolve(&mut self, i: usize, fix_x: bool) {
        let (el_sizes, mut children_sizes) = self.elements_sizes.children(i);
        let (element, mut children) = self.elements.children(i);
        if element.element.is_container() {

            if !el_sizes.cur_parametric().is_self_dep() {
                return;
            }

            fn parametric_solve_container<'a, T: ContainerParametricSolver>(mut solver: T,
                                                                            container_sizes: &mut ElementSizes,
                                                                            children: &mut ElementsChildrenMut<'a>,
                                                                            mut children_sizes: ElementSizesChildren) -> ParametricSolveState {
                let mut solver_state = T::State::default();
                for (i, child) in children.iter_mut() {
                    let child_sizes = children_sizes.get_mut(i);
                    let (fix_width, fix_height) = solver.handle_child(&mut solver_state, child_sizes, T::unwrap(&mut child.self_child_attributes));
                    if fix_width.is_some() {
                        panic!("Parametric {i} attempted to fix width for child, which must already be fixed!");
                    }
                    if fix_height.is_some() {
                        panic!("Parametric {i} attempted to fix height for child, which must already be fixed!");
                    }
                }

                solver.finalize(solver_state)
            }

            let parametric = match &element.element {
                Element::Row(attrs) => {
                    parametric_solve_container(elements::row::solver(attrs), el_sizes, &mut children, children_sizes)
                }
                Element::Col(attrs) => {
                    parametric_solve_container(elements::col::solver(attrs), el_sizes, &mut children, children_sizes)
                }
                Element::Stack(attrs) => {
                    parametric_solve_container(elements::stack::solver(attrs), el_sizes, &mut children, children_sizes)
                }
                _ => unreachable!(),
            };

            if fix_x {
                // only update min_y
                el_sizes.cur_parametric_mut().height.min = parametric.height.min;
            }
            else {
                // only update min_x
                el_sizes.cur_parametric_mut().width.min = parametric.width.min;
            }
        }
    }

    /// Phase 3: Position fix.
    /// Call guarantee: dim_fix is complete for element i and all children.
    /// Sets pos_fix (relative position within parent) for each direct child.
    fn pos_fix_children(&mut self, i: usize) {
        let el_sizes = &self.elements_sizes[i];
        let parent_w = el_sizes.dim_fix.width().unwrap_or(0);
        let parent_h = el_sizes.dim_fix.height().unwrap_or(0);
        let element = &self.elements[i];

        match element.element.clone() {
            Element::Row(attrs) => {
                // Calculate total children width for gap/alignment
                let mut children_width_sum: Lu = 0;
                let mut child_count: u32 = 0;

                for (i, _) in self.elements.children(i).1.iter_mut() {
                    children_width_sum += self.elements_sizes[i as usize].dim_fix.width().unwrap_or(0);
                    child_count += 1;
                }

                let gap = Self::compute_gap(&attrs.main_gap_mode, parent_w, children_width_sum, child_count);
                let main_offset = Self::compute_main_offset_x(&attrs.main_align, parent_w, children_width_sum, gap, child_count);

                let mut x = main_offset;
                for (i, element) in self.elements.children(i).1.iter_mut() {
                    let el_sizes = &mut self.elements_sizes[i as usize];
                    let child_w = el_sizes.dim_fix.width().unwrap_or(0);
                    let child_h = el_sizes.dim_fix.height().unwrap_or(0);

                    let cross_align = element.self_child_attributes.row.cross_align;
                    let y = Self::align_y(cross_align, parent_h, child_h);

                    el_sizes.pos_fix.pos_x = x;
                    el_sizes.pos_fix.pos_y = y;

                    x += child_w + gap;
                }
            }
            Element::Col(attrs) => {
                // Calculate total children height for gap/alignment
                let mut children_height_sum: Lu = 0;
                let mut child_count: u32 = 0;
                let mut ci = i + 1;
                for (i, _) in self.elements.children(i).1.iter_mut() {
                    children_height_sum += el_sizes.dim_fix.height().unwrap_or(0);
                    child_count += 1;
                }

                let gap = Self::compute_gap(&attrs.main_gap_mode, parent_h, children_height_sum, child_count);
                let main_offset = Self::compute_main_offset_y(&attrs.main_align, parent_h, children_height_sum, gap, child_count);

                let mut y = main_offset;
                for (i, element) in self.elements.children(i).1.iter_mut() {
                    let el_sizes = &mut self.elements_sizes[i as usize];
                    let child_w = el_sizes.dim_fix.width().unwrap_or(0);
                    let child_h = el_sizes.dim_fix.height().unwrap_or(0);

                    let cross_align = element.self_child_attributes.col.cross_align;
                    let x = Self::align_x(cross_align, parent_w, child_w);

                    el_sizes.pos_fix.pos_x = x;
                    el_sizes.pos_fix.pos_y = y;

                    y += child_h + gap;
                }
            }
            _ => {
                // Leaf elements and Stack: no positioning needed (Stack is not yet implemented)
            }
        }
    }

    fn compute_gap(mode: &MainGapMode, parent_main: Lu, children_main_sum: Lu, child_count: u32) -> Lu {
        if child_count <= 1 {
            return 0;
        }
        let gaps = child_count - 1;
        match mode {
            MainGapMode::Between => {
                let free = parent_main.saturating_sub(children_main_sum);
                free / gaps as Lu
            }
            MainGapMode::Around => {
                let free = parent_main.saturating_sub(children_main_sum);
                free / (child_count + 1) as Lu
            }
            MainGapMode::Fixed(g) => *g,
            MainGapMode::None => 0,
        }
    }

    fn compute_main_offset_x(align: &XAlign, parent_main: Lu, children_main_sum: Lu, gap: Lu, child_count: u32) -> Lu {
        let gaps = if child_count > 1 { child_count - 1 } else { 0 };
        let total = children_main_sum + gap * gaps as Lu;
        match align {
            XAlign::Left => 0,
            XAlign::Center => parent_main.saturating_sub(total) / 2,
            XAlign::Right => parent_main.saturating_sub(total),
        }
    }

    fn compute_main_offset_y(align: &YAlign, parent_main: Lu, children_main_sum: Lu, gap: Lu, child_count: u32) -> Lu {
        let gaps = if child_count > 1 { child_count - 1 } else { 0 };
        let total = children_main_sum + gap * gaps as Lu;
        match align {
            YAlign::Top => 0,
            YAlign::Center => parent_main.saturating_sub(total) / 2,
            YAlign::Bottom => parent_main.saturating_sub(total),
        }
    }

    fn align_x(align: XAlign, parent: Lu, child: Lu) -> Lu {
        match align {
            XAlign::Left => 0,
            XAlign::Center => parent.saturating_sub(child) / 2,
            XAlign::Right => parent.saturating_sub(child),
        }
    }

    fn align_y(align: YAlign, parent: Lu, child: Lu) -> Lu {
        match align {
            YAlign::Top => 0,
            YAlign::Center => parent.saturating_sub(child) / 2,
            YAlign::Bottom => parent.saturating_sub(child),
        }
    }

    fn handle_node(&mut self, i: usize, parents: &[usize], phase: Phase) -> ControlFlow {
        match phase {
            Phase::ParametricSolve => {
                ControlFlow::Continue
            }
            Phase::FixPassX => {
                if self.elements_sizes[i].dim_fix.width().is_some() && !self.elements_sizes[i].cur_parametric().width.is_fixed() {
                    self.dim_fix_children(i, true);
                    ControlFlow::Continue
                }
                else {
                    ControlFlow::SkipChildren
                }
            }
            Phase::FixPassY => {
                if self.elements_sizes[i].dim_fix.height().is_some() && !self.elements_sizes[i].cur_parametric().height.is_fixed() {
                    self.dim_fix_children(i, false);
                    ControlFlow::Continue
                }
                else {
                    ControlFlow::SkipChildren
                }
            }
            Phase::PosFixPass => {
                self.pos_fix_children(i);
                ControlFlow::Continue
            }
        }
    }

    fn finalize_node(&mut self, i: usize, phase: Phase) {
        match phase {
            Phase::ParametricSolve => {
                self.parametric_solve(i);
                if self.elements_sizes[i].parametric.is_fixed() {
                    return;
                }
                self.apply_general_attrs(i);
                if self.elements_sizes[i].post_parametric.is_fixed() {
                    return;
                }
            }
            Phase::FixPassX => {
                self.dim_fix_self_dep_resolve(i, true);
                let sizes = &mut self.elements_sizes[i];
                if sizes.dim_fix.width().is_some() {
                    sizes.dim_fix.set_subtree_fixed_x()
                }
            }
            Phase::FixPassY => {
                self.dim_fix_self_dep_resolve(i, false);
                let sizes = &mut self.elements_sizes[i];
                if sizes.dim_fix.width().is_some() {
                    sizes.dim_fix.set_subtree_fixed_y()
                }
            }
            Phase::PosFixPass => {
            }
        }
    }


    pub fn calculate_layout(&mut self, width: u32, height: u32) {
        // full reset on each recalculation for now
        for el in self.elements_sizes.iter_mut() {
            *el = Default::default();
        }

        self.dfs(0, Phase::ParametricSolve);

        let min_width = self.elements_sizes[0].cur_parametric().width.min;
        if self.elements_sizes[0].try_provide_width(Some(max(min_width, width))) {
            self.dfs(0, Phase::FixPassX);
        }
        let min_height = self.elements_sizes[0].cur_parametric().height.min;
        if self.elements_sizes[0].try_provide_height(Some(max(min_height, height))) {
            self.dfs(0, Phase::FixPassY);
        }
        self.dfs(0, Phase::PosFixPass);

        // diagnostics print
        for i in 0..self.elements.len() {
            println!("{}: {:?} ({}x{}) [x={}, y={}]", i,
                 self.elements[i].element.kind(), self.elements_sizes[i].min_width(), self.elements_sizes[i].min_height(),
                 self.elements_sizes[i].pos_fix.pos_x, self.elements_sizes[i].pos_fix.pos_y);
            println!("  > Parametric stage: {:?}", self.elements_sizes[i].parametric_stage);
            let parametric = self.elements_sizes[i].cur_parametric();
            println!("  > Parametric min: {}x{}", parametric.width.min, parametric.height.min);
            println!("  > Parametric kind: {:?}", parametric);
        }
    }


    /// Result for FixPass dfs is Fixed parametric kind and exact width and height for element and all other subtree elements
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

    pub fn get_min_root_size(&self) -> (u32, u32) {
        let el_sizes = self.elements_sizes.get(0).unwrap();
        (el_sizes.cur_parametric().width.min, el_sizes.cur_parametric().height.min)
    }

    /// Produce render rects for visualization.
    /// Containers get a 3px border outline, leaf elements get a solid fill.
    pub fn get_render_rects(&self) -> Vec<RenderRect> {
        let mut rects = Vec::new();
        let mut abs_x = vec![0i32; self.elements.len()];
        let mut abs_y = vec![0i32; self.elements.len()];

        // Compute absolute positions
        for i in 0..self.elements.len() {
            let rel_x = self.elements_sizes[i].pos_fix.pos_x as i32;
            let rel_y = self.elements_sizes[i].pos_fix.pos_y as i32;
            let parent = self.elements[i].parent_i as usize;
            if i == 0 {
                abs_x[i] = rel_x;
                abs_y[i] = rel_y;
            } else {
                abs_x[i] = abs_x[parent] + rel_x;
                abs_y[i] = abs_y[parent] + rel_y;
            }

            let w = self.elements_sizes[i].dim_fix.width().unwrap_or(0) as i32;
            let h = self.elements_sizes[i].dim_fix.height().unwrap_or(0) as i32;
            let x = abs_x[i];
            let y = abs_y[i];

            let depth = (i as f32 * 0.001);

            if self.elements[i].element.is_container() {
                // Border: 4 rects, 3px wide
                let bw = 3;
                let (r, g, b) = match &self.elements[i].element {
                    Element::Row(_) => (0.2, 0.8, 0.2),
                    Element::Col(_) => (0.2, 0.4, 0.9),
                    _ => (0.5, 0.5, 0.5),
                };
                // Top
                rects.push(RenderRect { x, y, w, h: bw, r, g, b, a: 1.0, depth });
                // Bottom
                rects.push(RenderRect { x, y: y + h - bw, w, h: bw, r, g, b, a: 1.0, depth });
                // Left
                rects.push(RenderRect { x, y, w: bw, h, r, g, b, a: 1.0, depth });
                // Right
                rects.push(RenderRect { x: x + w - bw, y, w: bw, h, r, g, b, a: 1.0, depth });
            } else {
                let (r, g, b) = match &self.elements[i].element {
                    Element::Box(attrs) => {
                        match &attrs.fill {
                            Some(crate::layout::Fill::Solid(c)) => (c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0),
                            _ => (0.3, 0.3, 0.3),
                        }
                    }
                    Element::Img(_) => (0.8, 0.6, 0.2),
                    Element::Text(_) => (0.9, 0.9, 0.9),
                    _ => (0.5, 0.5, 0.5),
                };
                rects.push(RenderRect { x, y, w, h, r, g, b, a: 1.0, depth });
            }
        }

        rects
    }
}