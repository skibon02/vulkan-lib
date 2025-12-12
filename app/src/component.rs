use crate::layout::{ElementNode, ElementNodeList};

pub struct Component {

}

impl Component {
    pub fn new() -> Self {
        Component {

        }
    }

    pub fn init(&mut self, _id: u32) -> ElementNodeList {
        Vec::new()
    }
}
