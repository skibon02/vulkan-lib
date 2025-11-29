use ash::vk::{DescriptorPool, DescriptorPoolCreateFlags, DescriptorPoolCreateInfo, DescriptorPoolSize, DescriptorSet, DescriptorSetAllocateInfo, DescriptorSetLayout, DescriptorType};
use slotmap::{DefaultKey, SlotMap};
use smallvec::SmallVec;
use std::collections::HashMap;
use ash::vk;
use crate::shaders::DescriptorSetLayoutBindingDesc;
use crate::wrappers::device::VkDeviceRef;

const INITIAL_POOL_SIZE: u32 = 8;
const INITIAL_DESCRIPTORS_PER_TYPE: u32 = 8;

struct DescriptorPoolInfo {
    pool: DescriptorPool,
    max_sets: u32,
    allocated_sets: u32,
    descriptor_counts: HashMap<DescriptorType, u32>,
    allocated_descriptor_counts: HashMap<DescriptorType, u32>,
}

impl DescriptorPoolInfo {
    fn new(device: &VkDeviceRef, max_sets: u32, descriptor_type_counts: &HashMap<DescriptorType, u32>) -> Self {
        let pool_sizes: SmallVec<[DescriptorPoolSize; 8]> = descriptor_type_counts
            .iter()
            .map(|(&ty, &count)| {
                DescriptorPoolSize::default()
                    .ty(ty)
                    .descriptor_count(count)
            })
            .collect();

        let pool_create_info = DescriptorPoolCreateInfo::default()
            .flags(DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
            .max_sets(max_sets)
            .pool_sizes(&pool_sizes);

        let pool = unsafe {
            device.create_descriptor_pool(&pool_create_info, None).unwrap()
        };

        Self {
            pool,
            max_sets,
            allocated_sets: 0,
            descriptor_counts: descriptor_type_counts.clone(),
            allocated_descriptor_counts: HashMap::new(),
        }
    }

    fn can_allocate(&self, required_descriptors: &HashMap<DescriptorType, u32>) -> bool {
        if self.allocated_sets >= self.max_sets {
            return false;
        }

        for (&ty, &required_count) in required_descriptors {
            let available = self.descriptor_counts.get(&ty).copied().unwrap_or(0);
            let allocated = self.allocated_descriptor_counts.get(&ty).copied().unwrap_or(0);

            if allocated + required_count > available {
                return false;
            }
        }

        true
    }

    fn allocate(&mut self, device: &VkDeviceRef, layout: DescriptorSetLayout, required_descriptors: &HashMap<DescriptorType, u32>) -> DescriptorSet {
        let layouts = [layout];
        let alloc_info = DescriptorSetAllocateInfo::default()
            .descriptor_pool(self.pool)
            .set_layouts(&layouts);

        let descriptor_set = unsafe {
            device.allocate_descriptor_sets(&alloc_info).unwrap()[0]
        };

        self.allocated_sets += 1;
        for (&ty, &count) in required_descriptors {
            *self.allocated_descriptor_counts.entry(ty).or_insert(0) += count;
        }

        descriptor_set
    }

    fn free(&mut self, device: &VkDeviceRef, descriptor_set: DescriptorSet, required_descriptors: &HashMap<DescriptorType, u32>) {
        unsafe {
            device.free_descriptor_sets(self.pool, &[descriptor_set]).unwrap();
        }

        self.allocated_sets -= 1;
        for (&ty, &count) in required_descriptors {
            if let Some(allocated) = self.allocated_descriptor_counts.get_mut(&ty) {
                *allocated -= count;
            }
        }
    }
}

enum DescriptorSetSlot {
    Unallocated,
    Allocated {
        descriptor_set: DescriptorSet,
        pool_index: usize,
        layout: DescriptorSetLayout,
        required_descriptors: HashMap<DescriptorType, u32>,
        last_used_in: usize,
        pending_recycle: bool,
    }
}

pub(crate) struct DescriptorSetAllocator {
    device: VkDeviceRef,
    pools: Vec<DescriptorPoolInfo>,
    slots: SlotMap<DefaultKey, DescriptorSetSlot>,
}

impl DescriptorSetAllocator {
    pub fn new(device: VkDeviceRef) -> Self {
        Self {
            device,
            pools: Vec::new(),
            slots: SlotMap::new(),
        }
    }

    fn calculate_required_descriptors(bindings: &[DescriptorSetLayoutBindingDesc]) -> HashMap<DescriptorType, u32> {
        let mut counts = HashMap::new();
        for binding in bindings {
            *counts.entry(binding.descriptor_type).or_insert(0) += binding.descriptor_count;
        }
        counts
    }

    fn find_or_create_pool(&mut self, required_descriptors: &HashMap<DescriptorType, u32>) -> usize {
        // Try to find an existing pool with capacity
        for (index, pool) in self.pools.iter().enumerate() {
            if pool.can_allocate(required_descriptors) {
                return index;
            }
        }

        // No suitable pool found, create a new one with exponential growth
        let new_max_sets = if self.pools.is_empty() {
            INITIAL_POOL_SIZE
        } else {
            self.pools.last().unwrap().max_sets * 2
        };

        let mut new_descriptor_counts = HashMap::new();
        for (&ty, &required_count) in required_descriptors {
            let base_count = if self.pools.is_empty() {
                INITIAL_DESCRIPTORS_PER_TYPE
            } else {
                self.pools.last().unwrap().descriptor_counts.get(&ty).copied().unwrap_or(INITIAL_DESCRIPTORS_PER_TYPE) * 2
            };
            new_descriptor_counts.insert(ty, base_count.max(required_count));
        }

        let new_pool = DescriptorPoolInfo::new(&self.device, new_max_sets, &new_descriptor_counts);
        self.pools.push(new_pool);
        self.pools.len() - 1
    }

    pub fn allocate_descriptor_set(&mut self, layout: DescriptorSetLayout, bindings: &[DescriptorSetLayoutBindingDesc]) -> DefaultKey {
        let required_descriptors = Self::calculate_required_descriptors(bindings);

        // Try to reuse an unallocated slot first
        let reuse_key = self.slots.iter().find_map(|(key, slot)| {
            if matches!(slot, DescriptorSetSlot::Unallocated) {
                Some(key)
            } else {
                None
            }
        });

        if let Some(key) = reuse_key {
            let pool_index = self.find_or_create_pool(&required_descriptors);
            let descriptor_set = self.pools[pool_index].allocate(&self.device, layout, &required_descriptors);

            self.slots[key] = DescriptorSetSlot::Allocated {
                descriptor_set,
                pool_index,
                layout,
                required_descriptors,
                last_used_in: 0,
                pending_recycle: false,
            };

            return key;
        }

        // No unallocated slot found, create a new one
        let pool_index = self.find_or_create_pool(&required_descriptors);
        let descriptor_set = self.pools[pool_index].allocate(&self.device, layout, &required_descriptors);

        self.slots.insert(DescriptorSetSlot::Allocated {
            descriptor_set,
            pool_index,
            layout,
            required_descriptors,
            last_used_in: 0,
            pending_recycle: false,
        })
    }
    
    pub fn get_descriptor_set(&mut self, key: DefaultKey) -> vk::DescriptorSet {
        if let Some(slot) = self.slots.get_mut(key) {
            if let DescriptorSetSlot::Allocated { descriptor_set, .. } = slot {
                return *descriptor_set
            }
        }
        panic!("Called get_descriptor_set on invalid or unallocated descriptor set slot")
    }

    pub fn update_last_used(&mut self, key: DefaultKey, submission_num: usize) {
        if let Some(slot) = self.slots.get_mut(key) {
            if let DescriptorSetSlot::Allocated { last_used_in, .. } = slot {
                *last_used_in = submission_num;
            }
        }
    }

    pub fn reset_descriptor_set(&mut self, key: DefaultKey) {
        if let Some(slot) = self.slots.get_mut(key) {
            if let DescriptorSetSlot::Allocated { pending_recycle, .. } = slot {
                *pending_recycle = true;
            }
        }
    }

    pub fn on_submission_waited(&mut self, last_waited_submission: usize) {
        for (_key, slot) in &mut self.slots {
            if let DescriptorSetSlot::Allocated { pending_recycle, last_used_in, descriptor_set, pool_index, required_descriptors, .. } = slot {
                // Recycle descriptor sets that are pending and the GPU has finished using them
                if *pending_recycle && *last_used_in <= last_waited_submission {
                    let ds = *descriptor_set;
                    let pool_idx = *pool_index;
                    let req_desc = required_descriptors.clone();

                    self.pools[pool_idx].free(&self.device, ds, &req_desc);
                    *slot = DescriptorSetSlot::Unallocated;
                }
            }
        }
    }
}

impl Drop for DescriptorSetAllocator {
    fn drop(&mut self) {
        unsafe {
            for pool in &self.pools {
                self.device.destroy_descriptor_pool(pool.pool, None);
            }
        }
    }
}