use ash::vk;
use ash::vk::{CommandPoolCreateFlags, CommandPoolCreateInfo};
use crate::queue::shared::HostWaitedNum;
use crate::wrappers::device::VkDeviceRef;

struct PendingCommandBuffer {
    cmd_buffer: vk::CommandBuffer,
    used_in_submission: usize,
}

pub(crate) struct CommandBufferManager {
    device: VkDeviceRef,
    command_pool: vk::CommandPool,
    pending: Vec<PendingCommandBuffer>,
    last_waited_submission: usize,
}

impl CommandBufferManager {
    pub fn new(device: VkDeviceRef, queue_family_index: u32) -> Self {
        let command_pool = unsafe {
            device.create_command_pool(&CommandPoolCreateInfo::default()
                .queue_family_index(queue_family_index)
                .flags(CommandPoolCreateFlags::TRANSIENT),
            None).unwrap()
        };
        Self {
            device,
            command_pool,
            pending: Vec::new(),
            last_waited_submission: 0,
        }
    }

    /// Allocate transient command buffer for the given submission number
    pub fn take_command_buffer(&mut self, submission_num: usize) -> vk::CommandBuffer {
        let cmd_buffer = unsafe {
            self.device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(self.command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1)
            ).unwrap()[0]
        };

        self.pending.push(PendingCommandBuffer {
            cmd_buffer,
            used_in_submission: submission_num,
        });

        cmd_buffer
    }

    /// Free command buffers that were used in submissions <= last_waited_submission
    pub fn on_last_waited_submission(&mut self, last_waited_submission: HostWaitedNum) {
        let last_waited_submission = last_waited_submission.num();
        if self.last_waited_submission >= last_waited_submission {
            return;
        }
        self.last_waited_submission = last_waited_submission;

        let mut to_free = Vec::new();
        self.pending.retain(|pending| {
            if pending.used_in_submission <= last_waited_submission {
                to_free.push(pending.cmd_buffer);
                false
            } else {
                true
            }
        });

        if !to_free.is_empty() {
            unsafe {
                self.device.free_command_buffers(self.command_pool, &to_free);
            }
        }
    }

    /// Called when queue is idle - free all pending buffers
    pub fn on_wait_idle(&mut self) {
        let to_free: Vec<_> = self.pending.iter().map(|p| p.cmd_buffer).collect();

        if !to_free.is_empty() {
            unsafe {
                self.device.free_command_buffers(self.command_pool, &to_free);
            }
        }

        self.pending.clear();
    }
}

impl Drop for CommandBufferManager {
    fn drop(&mut self) {
        // let cmd_buffers: Vec<_> = self.pending.iter().map(|p| p.cmd_buffer).collect();

        // if !cmd_buffers.is_empty() {
        //     unsafe {
        //         self.device.free_command_buffers(self.command_pool, &cmd_buffers);
        //     }
        // }
        unsafe {
            self.device.destroy_command_pool(self.command_pool, None);
        }
    }
}