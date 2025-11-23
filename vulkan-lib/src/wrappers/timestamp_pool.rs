use ash::vk;
use ash::vk::{CommandBuffer, PipelineStageFlags, QueryPool, QueryPoolCreateInfo, QueryResultFlags};
use log::{info, warn};
use crate::wrappers::device::VkDeviceRef;

pub struct TimestampPool {
    device: VkDeviceRef,
    query_pool: QueryPool,
    slots: Vec<Option<usize>>,
    tm_period: f32,
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
            slots: vec![None; max_timestamp_slots as usize],
            tm_period
        })
    }
    pub fn write_start_timestamp(&mut self, cb: CommandBuffer, submission_num: usize) -> u32 {
        let slot = self.slots.iter().position(|s| s.is_none()).unwrap_or(0) as u32;

        self.cmd_reset(cb, slot, 1);
        unsafe { self.device.cmd_write_timestamp(cb, PipelineStageFlags::TOP_OF_PIPE, self.query_pool, slot * 2); }
        if self.slots[slot as usize].is_some() {
            warn!("Overwriting timestamp slot {}", slot);
        }
        self.slots[slot as usize] = Some(submission_num);

        slot
    }
    pub fn write_end_timestamp(&mut self, cb: CommandBuffer, slot: u32) {
        unsafe { self.device.cmd_write_timestamp(cb, PipelineStageFlags::BOTTOM_OF_PIPE, self.query_pool, slot * 2 + 1); }
    }

    fn cmd_reset(&mut self, cb: CommandBuffer, slot: u32, count: u32) {
        unsafe { self.device.cmd_reset_query_pool(cb,  self.query_pool, slot * 2, count * 2) };
    }

    pub fn read_timestamps(&mut self) -> Vec<(usize, u64, u64)> {
        if self.slots.iter().all(|s| s.is_none()) {
            return vec![];
        }

        let mut min = self.slots.len();
        let mut max = 0;
        for (i, slot) in self.slots.iter().enumerate() {
            if let Some(submission_num) = slot {
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
            let mut buffer = vec![(0u64, 0u64); 2];
            if self.slots[i].is_none() {
                continue;
            }

            let query_res = unsafe { self.device.get_query_pool_results(self.query_pool, i as u32 * 2, &mut buffer, QueryResultFlags::TYPE_64 | QueryResultFlags::WITH_AVAILABILITY) };
            if let Err(e) = query_res  && e != vk::Result::NOT_READY {
                warn!("Failed to read timestamps from query pool: {:?}", e);
                return vec![];
            }

            if let Some(submission_num) = self.slots[i] {
                let (start, start_available) = buffer[0];
                let (end, end_available) = buffer[1];
                if start_available != 0 && end_available != 0 {
                    res.push((submission_num, start, end));
                    self.slots[i] = None;
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
