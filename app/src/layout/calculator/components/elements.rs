use std::ops::{Deref, DerefMut};
use crate::layout::ElementNode;

pub struct Elements(pub Vec<ElementNode>);

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
