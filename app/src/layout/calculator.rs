use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::ops::{Deref, DerefMut};
use crate::layout::{AttributeValue, Element, ElementKind, ElementNode, ElementNodeRepr, Lu, ParsedAttributes};
use crate::resources::get_resource;
use crate::util::read_image_from_bytes;

#[derive(Clone, Debug)]
#[derive(Default)]
pub enum SelfDepKind {
    #[default]
    Free,
    HeightFromWidth,
    WidthFromHeight,
    Both
}

#[derive(Clone, Debug)]
struct ParametricSolveState {
    min_width: Lu,
    min_height: Lu,
    self_dep: SelfDepKind,
    stretch_x: bool,
    stretch_y: bool,
}
impl Default for ParametricSolveState {
    fn default() -> Self {
        ParametricSolveState {
            min_width: 0,
            min_height: 0,
            self_dep: SelfDepKind::Free,
            stretch_x: true,
            stretch_y: true,
        }
    }
}
#[derive(Clone, Debug, Default)]
struct DimFixState {
    dim_fixed: bool, // Set to true during subtree fix or dim fix pass
    height: Lu,
    width: Lu,
}
#[derive(Clone, Debug, Default)]
struct PosFixState {
    // pub rel_pos_x: Lu,
    // pub rel_pos_y: Lu,
    pos_x: Lu,
    pos_y: Lu,
}

#[derive(Clone, Debug, Default)]
struct ElementSizes {
    parametric: ParametricSolveState,
    dim_fix: DimFixState,
    pos_fix: PosFixState,
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

pub struct Elements(Vec<ElementNode>);

impl Elements {
    fn children(&mut self, i: usize) -> (&mut ElementNode, impl Iterator<Item = (usize, &ElementNode)> + Clone + '_) {
        let (before, after) = self.0.split_at_mut(i + 1);
        let parent = &mut before[i];
        let after_ref: &[ElementNode] = after;

        let mut next_i = after_ref.get(0)
            .is_some_and(|e| e.parent_i == i as u32)
            .then(|| i + 1);

        let children_iter = std::iter::from_fn(move || {
            let child_i = next_i?;
            let offset = child_i - i - 1;
            let node = &after_ref[offset];
            next_i = node.next_sibling_i.map(|n| n as usize);
            Some((child_i, node))
        });

        (parent, children_iter)
    }
}

impl Deref for Elements {
    type Target = Vec<ElementNode>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Elements {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

struct Calculated(Vec<ElementSizes>);

impl Deref for Calculated {
    type Target = Vec<ElementSizes>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Calculated {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub struct Images(HashMap<String, ImageInfo>);
impl Images {
    fn load_image(&mut self, src: String) -> &ImageInfo {
        self.entry(src.clone())
            .or_insert_with(|| {
                let Ok(img_bytes) = get_resource(Path::new("images").join(&src)) else {
                    return ImageInfo {
                        aspect: 1.0,
                        src: ImageSource::OpenError,
                    };
                };
                let Ok((img, extent)) = read_image_from_bytes(img_bytes) else {
                    return ImageInfo {
                        aspect: 1.0,
                        src: ImageSource::OpenError,
                    }
                };
                // load image and calculate aspect ratio
                ImageInfo {
                    aspect: extent.height as f32 / extent.width as f32,
                    src: ImageSource::Bytes(img),
                }
            })
    }
}

impl Deref for Images {
    type Target = HashMap<String, ImageInfo>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Images {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub struct Fonts(HashMap<String, FontInfo>);

impl Deref for Fonts {
    type Target = HashMap<String, FontInfo>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Fonts {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub struct Texts(HashMap<u32, TextInfo>);

impl Deref for Texts {
    type Target = HashMap<u32, TextInfo>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Texts {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub struct LayoutCalculator {
    elements: Elements,
    calculated: Calculated,
    images: Images,
    fonts: Fonts,
    texts: Texts
}

pub enum ImageSource {
    Bytes(Vec<u8>),
    OpenError,
}

pub struct ImageInfo {
    // calculated as height / width
    aspect: f32,
    src: ImageSource,
}

pub struct FontInfo {
    default_line_height: f32,
}

#[derive(PartialEq, PartialOrd)]
enum ControlFlow {
    Continue,
    SkipChildren,
}

#[derive(Clone)]
pub struct TextInfo {
    value: Arc<str>,
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

    /// resources


    fn parametric_solve(&mut self, i: usize) {
        // let (me, ref children) = self.elements.children(i);
        let me = &mut self.elements[i];
        let me_calc = &mut self.calculated[i];
        match &me.element {
            Element::Img(attrs) => {
                let name = attrs.resource.clone();
                let img_info = self.images.load_image(name);
                if attrs.height.is_none() && attrs.width.is_none() {
                    me_calc.parametric.self_dep = SelfDepKind::Both;
                }
                else {
                    me_calc.parametric.self_dep = SelfDepKind::Free;
                    me_calc.parametric.stretch_x = false;
                    me_calc.parametric.stretch_y = false;
                    if let Some(width) = attrs.width {
                        me_calc.parametric.min_width = width;
                        me_calc.parametric.min_height = (width as f32 * img_info.aspect) as Lu;
                    }
                    else if let Some(height) = attrs.height {
                        me_calc.parametric.min_height = height;
                        me_calc.parametric.min_width = (height as f32 / img_info.aspect) as Lu;
                    }
                    else {
                        unreachable!()
                    }
                }
            },
            Element::Box(attrs) => {
                me_calc.parametric.self_dep = SelfDepKind::Free;
                me_calc.parametric.stretch_x = true;
                me_calc.parametric.stretch_y = true;
            }
            Element::Text(attrs) => {
                
            }
            _ => {}
        }
    }
    fn apply_general_attrs(&mut self, i: usize) {
    }

    fn handle_node(&mut self, i: usize, parents: &[usize], phase: Phase) -> ControlFlow {
        if matches!(phase, Phase::FixPass) {
            ControlFlow::Continue
        }
        else {
            ControlFlow::Continue
        }
    }

    fn finalize_node(&mut self, i: usize, phase: Phase) {
        if matches!(phase, Phase::ParametricSolve) {
            self.parametric_solve(i);
            self.apply_general_attrs(i);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Element, GeneralAttributes, ChildAttributes};

    fn make_test_node(parent_i: u32, next_sibling_i: Option<u32>) -> ElementNode {
        ElementNode {
            parent_i,
            next_sibling_i,
            element: Element::Box(Default::default()),
            general_attributes: GeneralAttributes::default(),
            self_child_attributes: ChildAttributes::default(),
        }
    }

    #[test]
    fn test_children_no_children() {
        let mut elements = Elements(vec![
            make_test_node(0, None),
        ]);

        let (parent, mut iter) = elements.children(0);
        assert_eq!(parent.parent_i, 0);
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_children_single_child() {
        let mut elements = Elements(vec![
            make_test_node(0, None),      // parent at index 0
            make_test_node(0, None),      // child at index 1
        ]);

        let (parent, mut iter) = elements.children(0);
        assert_eq!(parent.parent_i, 0);

        let (idx, child) = iter.next().unwrap();
        assert_eq!(idx, 1);
        assert_eq!(child.parent_i, 0);
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_children_multiple_children() {
        let mut elements = Elements(vec![
            make_test_node(0, None),      // parent at index 0
            make_test_node(0, Some(2)),   // child 1 at index 1
            make_test_node(0, Some(3)),   // child 2 at index 2
            make_test_node(0, None),      // child 3 at index 3
        ]);

        let (parent, mut iter) = elements.children(0);
        assert_eq!(parent.parent_i, 0);

        let (idx, child) = iter.next().unwrap();
        assert_eq!(idx, 1);
        assert_eq!(child.parent_i, 0);

        let (idx, child) = iter.next().unwrap();
        assert_eq!(idx, 2);
        assert_eq!(child.parent_i, 0);

        let (idx, child) = iter.next().unwrap();
        assert_eq!(idx, 3);
        assert_eq!(child.parent_i, 0);

        assert!(iter.next().is_none());
    }

    #[test]
    fn test_children_with_different_parent() {
        let mut elements = Elements(vec![
            make_test_node(0, None),      // parent at index 0
            make_test_node(1, None),      // not a child of 0 (different parent)
        ]);

        let (parent, mut iter) = elements.children(0);
        assert_eq!(parent.parent_i, 0);

        assert!(iter.next().is_none());
    }

    #[test]
    fn test_children_iterator_is_cloneable() {
        let mut elements = Elements(vec![
            make_test_node(0, None),      // parent at index 0
            make_test_node(0, Some(2)),   // child 1 at index 1
            make_test_node(0, None),      // child 2 at index 2
        ]);

        let (_parent, iter) = elements.children(0);
        let mut iter1 = iter.clone();
        let mut iter2 = iter.clone();

        assert_eq!(iter1.next().unwrap().0, 1);
        assert_eq!(iter1.next().unwrap().0, 2);
        assert!(iter1.next().is_none());

        assert_eq!(iter2.next().unwrap().0, 1);
        assert_eq!(iter2.next().unwrap().0, 2);
        assert!(iter2.next().is_none());
    }

    #[test]
    fn test_children_parent_mutation() {
        let mut elements = Elements(vec![
            make_test_node(0, None),
            make_test_node(0, None),
        ]);

        let (parent, mut iter) = elements.children(0);
        parent.parent_i = 99;

        let (_idx, child) = iter.next().unwrap();
        assert_eq!(child.parent_i, 0);
        assert_eq!(parent.parent_i, 99);
    }
}