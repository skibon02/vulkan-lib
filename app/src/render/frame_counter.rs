use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

#[derive(Clone)]
pub struct FrameCounter {
    submitted_frame: Arc<AtomicUsize>,
}

impl FrameCounter {
    
}

