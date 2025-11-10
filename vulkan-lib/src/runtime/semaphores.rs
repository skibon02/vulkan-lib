use std::collections::VecDeque;
use ash::vk;
use log::{error, warn};
use slotmap::{SlotMap, DefaultKey};
use crate::wrappers::device::VkDeviceRef;

enum SemaphoreSlot {
    Unallocated,
    Signaled(vk::Semaphore),
    WaitScheduled { semaphore: vk::Semaphore, used_in_submission: Option<usize> },
}

impl SemaphoreSlot {
    fn new_unallocated() -> Self {
        Self::Unallocated
    }
}

/// Reference for signaling a semaphore after submission
#[derive(Clone)]
pub struct SignalSemaphoreRef {
    key: DefaultKey,
}

/// Reference for waiting on a semaphore from a previous submission
#[derive(Clone)]
pub struct WaitSemaphoreRef {
    key: DefaultKey,
}

impl WaitSemaphoreRef {
    /// Add stage flags to create a WaitSemaphoreStagesRef
    pub fn with_stages(self, stage_flags: vk::PipelineStageFlags) -> WaitSemaphoreStagesRef {
        WaitSemaphoreStagesRef {
            key: self.key,
            stage_flags,
        }
    }
}

/// Reference for waiting on a semaphore with specific pipeline stage flags
#[derive(Clone)]
pub struct WaitSemaphoreStagesRef {
    key: DefaultKey,
    pub(crate) stage_flags: vk::PipelineStageFlags,
}

pub(crate) struct SemaphoreManager {
    device: VkDeviceRef,
    free_semaphores: Vec<vk::Semaphore>,
    slots: SlotMap<DefaultKey, SemaphoreSlot>,
    last_waited_submission: usize,
    untracked_keys: VecDeque<DefaultKey>,
}

impl SemaphoreManager {
    pub fn new(device: VkDeviceRef) -> Self {
        Self {
            device,
            free_semaphores: Vec::new(),
            slots: SlotMap::new(),
            last_waited_submission: 0,
            untracked_keys: VecDeque::new(),
        }
    }

    /// Create a semaphore pair (signal + wait) for regular operations
    pub(crate) fn create_semaphore_pair(&mut self) -> (SignalSemaphoreRef, WaitSemaphoreRef) {
        let key = self.slots.insert(SemaphoreSlot::new_unallocated());
        (
            SignalSemaphoreRef { key },
            WaitSemaphoreRef { key },
        )
    }

    /// Allocate a semaphore for signaling - must be called before wait
    pub fn allocate_signal_semaphore(&mut self, signal_ref: &SignalSemaphoreRef) -> vk::Semaphore {
        let semaphore = self.take_free_semaphore();

        let slot = self.slots.get_mut(signal_ref.key)
            .expect("Invalid signal semaphore reference");

        match slot {
            SemaphoreSlot::Unallocated => {
                *slot = SemaphoreSlot::Signaled(semaphore);
                semaphore
            }
            SemaphoreSlot::Signaled(_) => {
                panic!("Attempted to signal a semaphore that was already signaled but not waited on");
            }
            SemaphoreSlot::WaitScheduled { .. } => {
                panic!("Attempted to signal a semaphore that is currently scheduled for wait");
            }
        }
    }

    /// Get semaphore to wait on and mark it as used in the given submission
    pub fn get_wait_semaphore(&mut self, wait_ref: WaitSemaphoreStagesRef, used_in_submission: Option<usize>) -> vk::Semaphore {
        let slot = self.slots.get_mut(wait_ref.key)
            .expect("Invalid wait semaphore reference");

        match slot {
            SemaphoreSlot::Signaled(semaphore) => {
                let sem = *semaphore;
                *slot = SemaphoreSlot::WaitScheduled {
                    semaphore: sem,
                    used_in_submission,
                };

                // for untracked semaphores, recycle old ones when we have 5+ newer
                if used_in_submission.is_none() {
                    self.untracked_keys.push_back(wait_ref.key);
                    self.recycle_old_untracked();
                }

                sem
            }
            _ => panic!("Semaphore must be signaled before waiting"),
        }
    }

    pub fn on_last_waited_submission(&mut self, last_waited_submission: usize) {
        if self.last_waited_submission >= last_waited_submission {
            return;
        }
        self.last_waited_submission = last_waited_submission;

        // collect keys to remove (can't modify while iterating)
        let mut to_recycle = Vec::new();

        for (key, slot) in &self.slots {
            if let SemaphoreSlot::WaitScheduled { semaphore, used_in_submission } = slot {
                if let Some(submission) = used_in_submission {
                    if *submission <= last_waited_submission {
                        to_recycle.push((key, *semaphore));
                    }
                }
            }
        }

        // recycle completed semaphores
        for (key, semaphore) in to_recycle {
            self.slots.remove(key);
            self.free_semaphores.push(semaphore);
        }
    }

    /// Called when queue is idle - recycles all semaphores
    pub fn on_wait_idle(&mut self) {
        let mut to_recycle = Vec::new();

        for (key, slot) in &self.slots {
            if let SemaphoreSlot::WaitScheduled { semaphore, .. } = slot {
                to_recycle.push((key, *semaphore));
            }
        }

        for (key, semaphore) in to_recycle {
            self.slots.remove(key);
            self.free_semaphores.push(semaphore);
        }

        self.untracked_keys.clear();
    }

    fn recycle_old_untracked(&mut self) {
        const MAX_UNTRACKED: usize = 5;

        if self.untracked_keys.len() > MAX_UNTRACKED {
            let oldest = self.untracked_keys.pop_front().unwrap();
            let oldest_s = self.slots.get_mut(oldest).unwrap();
            match oldest_s {
                SemaphoreSlot::WaitScheduled {
                    semaphore,
                    ..
                } => {
                    self.free_semaphores.push(*semaphore);
                }
                _ => panic!("Attempted to untrace the old untracked key"),
            }

            self.slots.remove(oldest);
        }
    }

    fn take_free_semaphore(&mut self) -> vk::Semaphore {
        self.free_semaphores.pop().unwrap_or_else(|| {
            unsafe {
                self.device
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                    .unwrap()
            }
        })
    }
}


impl Drop for SemaphoreManager {
    fn drop(&mut self) {
        unsafe {
            for semaphore in self.free_semaphores.drain(..) {
                self.device.destroy_semaphore(semaphore, None);
            }

            if self.slots.iter().any(|s| matches!(s.1, SemaphoreSlot::WaitScheduled {..} | SemaphoreSlot::Signaled(_))) {
                error!("Semaphore manager have some submitted semaphores! Wait for idle before dropping!");
            }
            for (_, semaphore) in &self.slots {
                if let SemaphoreSlot::WaitScheduled{semaphore, ..} | SemaphoreSlot::Signaled(semaphore) = semaphore {
                    self.device.destroy_semaphore(*semaphore, None);
                }
            }
        }
    }
}