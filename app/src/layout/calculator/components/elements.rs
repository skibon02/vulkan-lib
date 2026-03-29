use std::ops::{Deref, DerefMut};
use crate::layout::ElementNode;

pub struct Elements(pub Vec<ElementNode>);

impl Elements {
    pub fn children(&mut self, i: usize) -> (&mut ElementNode, ElementsChildrenMut) {
        let (element, children) = self.0[i..].split_first_mut().unwrap();

        (element, ElementsChildrenMut {
            parent_i: i as u32,
            elements: children
        })
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

pub struct ElementsChildrenMut<'a> {
    parent_i: u32,
    elements: &'a mut [ElementNode],
}
impl<'a> ElementsChildrenMut<'a> {
    pub fn get(&'a mut self, i: u32) -> &'a mut ElementNode {
        if (self.parent_i..self.parent_i + self.elements.len() as u32).contains(&i) {
            &mut self.elements[i as usize - self.parent_i as usize]
        }
        else {
            panic!("Incorrect element index specified provided to ElementsChildren::get")
        }
    }
    pub fn iter_mut(&mut self) -> ElementsChildrenIterMut<'a, '_> {
        ElementsChildrenIterMut {
            inner: self,
            i: Some(0),
        }
    }
    
    pub fn iter(&'a self) -> ElementsChildrenIter<'a> {
        ElementsChildrenIter {
            inner: self,
            i: Some(0),
        } 
    }
}
pub struct ElementsChildrenIter<'a> {
    inner: &'a ElementsChildrenMut<'a>,
    i: Option<u32>,
}
impl<'a> Iterator for ElementsChildrenIter<'a> {
    type Item = (u32, &'a ElementNode);

    fn next(&mut self) -> Option<Self::Item> {
        if self.inner.elements.is_empty() {
            return None;
        }

        if let Some(i) = self.i {
            let ptr = self.inner.elements.as_ptr();

            // SAFETY: We guarantee that `next_sibling_i` never creates a cycle,
            // so we will never yield a mutable reference to the same index twice.
            // We also assume `i` is always within the bounds of the slice.
            let el: &'a ElementNode = unsafe {
                &*ptr.add(i as usize)
            };

            self.i = el.next_sibling_i.map(|next_i| next_i - self.inner.parent_i - 1);

            Some((i + self.inner.parent_i, el))
        } else {
            None
        }
    }
}
pub struct ElementsChildrenIterMut<'a, 'b> {
    inner: &'b mut ElementsChildrenMut<'a>,
    i: Option<u32>,
}

impl<'a, 'b> Iterator for ElementsChildrenIterMut<'a, 'b> {
    type Item = (u32, &'a mut ElementNode);

    fn next(&mut self) -> Option<Self::Item> {
        if self.inner.elements.is_empty() {
            return None;
        }

        if let Some(i) = self.i {
            let ptr = self.inner.elements.as_mut_ptr();

            // SAFETY: We guarantee that `next_sibling_i` never creates a cycle,
            // so we will never yield a mutable reference to the same index twice.
            // We also assume `i` is always within the bounds of the slice.
            let el: &'a mut ElementNode = unsafe {
                &mut *ptr.add(i as usize)
            };

            self.i = el.next_sibling_i.map(|next_i| next_i - self.inner.parent_i - 1);

            Some((i + self.inner.parent_i, el))
        }
        else {
            None
        }
    }
}