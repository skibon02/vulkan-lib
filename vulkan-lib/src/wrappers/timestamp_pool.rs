use ash::vk;
use ash::vk::{CommandBuffer, PipelineStageFlags, QueryPool, QueryPoolCreateInfo, QueryResultFlags};
use log::{info, warn};
use crate::wrappers::device::VkDeviceRef;

#[derive(Copy, Clone)]
pub enum QuerySlot {
    Submitted(usize),
    Free,
    NeedReset,
}

impl QuerySlot {
    pub fn is_free(&self) -> bool {
        matches!(self, QuerySlot::Free)
    }
    pub fn is_submitted(&self) -> bool {
        matches!(self, QuerySlot::Submitted(_))
    }
}

pub struct TimestampPool {
    device: VkDeviceRef,
    query_pool: QueryPool,
    slots: Vec<QuerySlot>,
    cur_i: usize,
    tm_period: f32,
    need_reset: bool,
}

impl TimestampPool {
    pub fn new(device: VkDeviceRef, max_timestamp_slots: u32, tm_period: f32) -> Option<TimestampPool> {
        let info = QueryPoolCreateInfo::default()
            .query_type(vk::QueryType::TIMESTAMP)
            .query_count(max_timestamp_slots * 2);

        let query_pool = unsafe { device.create_query_pool(&info, None) }.ok()?;
        Some(Self {
            device: device.clone(),
            query_pool,
            slots: vec![QuerySlot::Free; max_timestamp_slots as usize],
            cur_i: 0,
            tm_period,
            need_reset: true,
        })
    }
    pub fn write_start_timestamp(&mut self, cb: CommandBuffer, submission_num: usize) -> u32 {
        let mut slot = 0;
        for i in self.cur_i..self.cur_i + self.slots.len() {
            let real_i = i % self.slots.len();
            if self.slots[real_i].is_free() {
                slot = real_i as u32;
                break;
            }
        }
        self.cur_i = (slot as usize + 1) % self.slots.len();

        if self.need_reset {
            self.need_reset = false;
            unsafe {
                self.device.cmd_reset_query_pool(cb, self.query_pool, 0, self.slots.len() as u32 * 2);
            }
        }
        unsafe { self.device.cmd_write_timestamp(cb, PipelineStageFlags::TOP_OF_PIPE, self.query_pool, slot * 2); }
        if !self.slots[slot as usize].is_free() {
            warn!("Overwriting timestamp slot {}", slot);
        }
        self.slots[slot as usize] = QuerySlot::Submitted(submission_num);

        slot
    }
    pub fn write_end_timestamp(&mut self, cb: CommandBuffer, slot: u32) {
        unsafe { self.device.cmd_write_timestamp(cb, PipelineStageFlags::BOTTOM_OF_PIPE, self.query_pool, slot * 2 + 1); }
    }

    pub fn reset_old_slots(&mut self, cb: CommandBuffer) {
        for (i, slot) in self.slots.iter().copied().enumerate() {
            if matches!(slot, QuerySlot::NeedReset) {
                self.cmd_reset(cb, i as u32, 1);
            }
        }
        for slot in self.slots.iter_mut() {
            if matches!(slot, QuerySlot::NeedReset) {
                *slot = QuerySlot::Free
            }
        }
    }

    fn cmd_reset(&self, cb: CommandBuffer, slot: u32, count: u32) {
        unsafe { self.device.cmd_reset_query_pool(cb,  self.query_pool, slot * 2, count * 2) };
    }

    pub fn read_timestamps(&mut self) -> Vec<(usize, u64, u64)> {
        if !self.slots.iter().any(|s| s.is_submitted()) {
            return vec![];
        }

        let mut min = self.slots.len();
        let mut max = 0;
        for (i, slot) in self.slots.iter().enumerate() {
            if let QuerySlot::Submitted(submission_num) = slot {
                if i < min {
                    min = i;
                }
                if i > max {
                    max = i;
                }
            }
        }

        let min_slot = min;
        let max_slot = max;

        let mut res = vec![];
        let mut i = min_slot;
        while i <= max_slot {
            let mut buffer = [(0u64, 0u64); 2];
            if let QuerySlot::Submitted(submission_num) = self.slots[i] {

                let query_res = unsafe { self.device.get_query_pool_results(self.query_pool, i as u32 * 2, &mut buffer, QueryResultFlags::TYPE_64 | QueryResultFlags::WITH_AVAILABILITY) };
                if let Err(e) = query_res  && e != vk::Result::NOT_READY {
                    warn!("Failed to read timestamps from query pool: {:?}", e);
                    return vec![];
                }

                let (start, start_available) = buffer[0];
                let (end, end_available) = buffer[1];
                if start_available != 0 && end_available != 0 {
                    res.push((submission_num, start, end));
                    self.slots[i] = QuerySlot::NeedReset;
                }

            }
            i += 1;
        }
        res
    }
}

impl Drop for TimestampPool {
    fn drop(&mut self) {
        unsafe { self.device.destroy_query_pool(self.query_pool, None); }
    }
}
