use anyhow::Context;
use ash::vk;
use ash::vk::{BufferCreateFlags, BufferUsageFlags, Extent2D, PhysicalDevice, Queue};
use smallvec::SmallVec;
use sparkles::range_event_start;
use strum::IntoDiscriminant;
use crate::queue::semaphores::SemaphoreManager;
use crate::queue::command_buffers::CommandBufferManager;
use crate::queue::shared::SharedState;
use crate::queue::memory_manager::MemoryTypeAlgorithm;
use crate::wrappers::device::VkDeviceRef;
use crate::wrappers::surface::VkSurfaceRef;

pub mod resources;

pub use crate::queue::semaphores::{SignalSemaphoreRef, WaitSemaphoreRef, WaitSemaphoreStagesRef};
use crate::extensions::calibrated_timestamps::CalibratedTimestamps;
use crate::queue::shared;
use crate::shaders::DescriptorSetLayoutBindingDesc;
use crate::swapchain_wrapper::SwapchainWrapper;
use crate::wrappers::timestamp_pool::TimestampPool;

pub struct RuntimeState {
    shared_state: shared::SharedState,

    queue: Queue,

    // swapchain
    swapchain_wrapper: SwapchainWrapper,
    surface: VkSurfaceRef,
}

impl RuntimeState {
    /// Create new buffer in mappable memory for TRANSFER_SRC usage
    pub fn new_host_buffer(&mut self, size: u64) -> MappableBufferResource {
        let flags = BufferCreateFlags::empty();
        let usage = BufferUsageFlags::TRANSFER_SRC;

        let (buffer, memory) = self.resource_storage.create_buffer(usage, flags, size, MemoryTypeAlgorithm::Host, self.shared_state.clone());
        MappableBufferResource::new(buffer, memory)
    }
    pub fn swapchain_images(&self) -> SmallVec<[ImageResourceHandle; 3]> {
        self.swapchain_wrapper.get_images()
    }

    pub fn wait_idle(&mut self) {
        let g = range_event_start!("[Vulkan] Wait queue idle");
        unsafe {
            self.device.queue_wait_idle(self.queue).unwrap();
        }

        // after wait_idle, all submissions up to last_submission_num are done
        let last_submitted = self.shared_state.last_submission_num();
        if last_submitted > 0 {
            self.shared_state.confirm_all_waited(last_submitted);
        }

        self.semaphore_manager.on_wait_idle();
        self.command_buffer_manager.on_wait_idle();
    }

    pub fn wait_prev_submission(&mut self, prev_sub: usize) -> Option<()> {
        let last_submission = self.shared_state.last_submission_num();
        let submission_to_wait = last_submission.checked_sub(prev_sub)?;
        self.shared_state.wait_submission(submission_to_wait);

        Some(())
    }
    pub(crate) fn destroy_image(&mut self, image: ImageResourceHandle) {
        self.shared_state.schedule_destroy_image(image);
    }
}
