use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use image::DynamicImage;
use sparkles::range_event_start;
use thiserror::Error;
use vulkan_lib::queue::shared::{HostWaitedNum, SharedState};
use vulkan_lib::resources::staging_buffer::{StagingBufferRange, StagingBufferResource};
use vulkan_lib::resources::VulkanAllocator;
use vulkan_lib::vk::Extent2D;

#[derive(Clone)]
pub struct FrameCounter(Rc<AtomicUsize>);
impl FrameCounter {
    pub fn new() -> Self {
        Self(Rc::new(AtomicUsize::new(0)))
    }

    pub fn current_frame(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }

    pub fn increment_frame(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

pub struct DoubleBuffered<T> {
    pub buffers: [T; 2],
    last_frame: usize,
    current: bool,
    frame_counter: FrameCounter,
}

impl<T> DoubleBuffered<T> {
    pub fn new(frame_counter: &FrameCounter, mut f: impl FnMut() -> T) -> Self {
        Self {
            buffers: [f(), f()],
            last_frame: 0,
            current: false,
            frame_counter: frame_counter.clone(),
        }
    }

    pub fn new_with_values(frame_counter: &FrameCounter, a: T, b: T) -> Self {
        Self {
            buffers: [a, b],
            last_frame: 0,
            current: false,
            frame_counter: frame_counter.clone(),
        }
    }

    pub fn current(&mut self) -> &mut T {
        let cur_frame = self.frame_counter.current_frame();
        if cur_frame > self.last_frame {
            self.last_frame = cur_frame;
            self.current = !self.current;
        }
        &mut self.buffers[self.current as usize]
    }
}

pub struct TrippleAutoStaging {
    s1: Vec<StagingBufferResource>,
    s2: Vec<StagingBufferResource>,
    s3: Vec<StagingBufferResource>,
    shared_state: SharedState,

    cur: usize,
    last_frame: usize,
    frame_counter: FrameCounter,
}

impl TrippleAutoStaging {
    pub fn new(frame_counter: &FrameCounter, allocator: &mut VulkanAllocator, initial_size: u64) -> Self {
        let s1 = vec![allocator.new_staging_buffer(initial_size)];
        let s2 = vec![allocator.new_staging_buffer(initial_size)];
        let s3 = vec![allocator.new_staging_buffer(initial_size)];

        Self {
            s1,
            s2,
            s3,

            shared_state: allocator.shared(),
            cur: 0,
            last_frame: 0,

            frame_counter: frame_counter.clone(),
        }
    }

    fn try_switch_buffer(&mut self) {
        let g = range_event_start!("Try switch staging buffer");
        let cur_frame = self.frame_counter.current_frame();
        if cur_frame > self.last_frame {
            self.last_frame = cur_frame;
            self.cur = (self.cur + 1) % 3;

            // unfreeze
            let last_waited = self.shared_state.last_host_waited_cached();
            let buffers = match self.cur {
                0 => &mut self.s1,
                1 => &mut self.s2,
                2 => &mut self.s3,
                _ => unreachable!(),
            };

            for buffer in buffers {
                if buffer.try_unfreeze(last_waited).is_none() {
                    panic!("Failed to unfreeze staging buffer");
                }
            }
        }
    }

    pub fn allocate(&mut self, allocator: &mut VulkanAllocator, size: usize) -> StagingBufferRange {
        self.try_switch_buffer();

        let buffers = match self.cur {
            0 => &mut self.s1,
            1 => &mut self.s2,
            2 => &mut self.s3,
            _ => unreachable!(),
        };

        for buffer in buffers.iter_mut() {
            if let Some(range) = buffer.try_freeze(size) {
                return range;
            }
        }

        // need to allocate a new buffer, keep allocating double the size until it fits
        let mut new_size = buffers.last().as_ref().unwrap().len();
        loop {
            new_size *= 2;

            let g = range_event_start!("Allocate new staging buffer, twice the size");
            let buffer = allocator.new_staging_buffer(new_size as u64);
            buffers.push(buffer);

            if let Some(range) = buffers.last_mut().unwrap().try_freeze(size) {
                return range;
            }
        }
    }
}


#[derive(Error, Debug)]
pub enum ReadImageError {
    #[error("Image error: {0}")]
    ImageError(#[from] image::ImageError),
    #[error("Image has zero size")]
    ZeroSize,
}
pub type ReadImageResult<T> = Result<T, ReadImageError>;
pub fn read_image_from_bytes(image_bytes: Vec<u8>) -> ReadImageResult<(Vec<u8>, Extent2D)> {
    let image_object = image::load_from_memory(&image_bytes)?;

    let (image_width, image_height) = (image_object.width(), image_object.height());

    if image_width == 0 || image_height == 0 {
        return Err(ReadImageError::ZeroSize);
    }

    let image_data = match &image_object {
        DynamicImage::ImageLuma8(_)
        | DynamicImage::ImageRgb8(_) => image_object.to_rgba8().into_raw(),
        DynamicImage::ImageLumaA8(_)
        | DynamicImage::ImageRgba8(_) => image_object.into_bytes(),
        _ => panic!("Unsupported image format"),
    };

    Ok((image_data, Extent2D {
        width: image_width,
        height: image_height,
    }))
}
