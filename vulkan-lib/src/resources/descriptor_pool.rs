use ash::vk::{DescriptorPool, DescriptorPoolCreateFlags, DescriptorPoolCreateInfo, DescriptorPoolSize, DescriptorSet, DescriptorSetAllocateInfo, DescriptorSetLayout, DescriptorType};
use smallvec::SmallVec;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use ash::vk;
use log::warn;
use crate::queue::OptionSeqNumShared;
use crate::queue::shared::SharedState;
use crate::resources::descriptor_set::{DescriptorSetBinding, DescriptorSetResource};
use crate::shaders::DescriptorSetLayoutBindingDesc;
use crate::wrappers::device::VkDeviceRef;

const INITIAL_POOL_SIZE: u32 = 8;
const INITIAL_DESCRIPTORS_PER_TYPE: u32 = 8;

struct DescriptorPoolInfo {
    pool: DescriptorPool,
    max_sets: u32,
    descriptor_counts: HashMap<DescriptorType, u32>,
    allocated_sets: u32,
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


pub(crate) struct DescriptorSetAllocator {
    device: VkDeviceRef,
    pools: Vec<DescriptorPoolInfo>,
    sets: Vec<Arc<DescriptorSetResource>>,
    shared_state: SharedState,
}

impl DescriptorSetAllocator {
    pub fn new(device: VkDeviceRef, shared_state: SharedState) -> Self {
        Self {
            device,
            pools: Vec::new(),
            sets: Vec::new(),
            shared_state,
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

    pub fn allocate_descriptor_set(&mut self, layout: DescriptorSetLayout, bindings_desc: &[DescriptorSetLayoutBindingDesc]) -> Arc<DescriptorSetResource> {
        let required_descriptors = Self::calculate_required_descriptors(bindings_desc);
        let pool_index = self.find_or_create_pool(&required_descriptors);
        let descriptor_set = self.pools[pool_index].allocate(&self.device, layout, &required_descriptors);

        let bindings = bindings_desc.iter().map(|b| {
            DescriptorSetBinding {
                binding_index: b.binding,
                descriptor_count: b.descriptor_count,
                descriptor_type: b.descriptor_type,
                resource: None,
                resource_updated: false,
            }
        }).collect();

        let ds = Arc::new(DescriptorSetResource {
            descriptor_set,
            pool_index,
            layout,
            bindings: Mutex::new(bindings),
            submission_usage: OptionSeqNumShared::default(),
            updates_locked: AtomicBool::new(false),
        });

        self.sets.push(ds.clone());

        ds
    }

    /// Call this periodically to recycle descriptor sets that are no longer in use by the GPU.
    pub fn on_submission_waited(&mut self, last_waited_submission: usize) {
        let mut i = 0;
        while i < self.sets.len() {
            if self.sets[i].submission_usage.load().is_none_or(|u| u <= last_waited_submission) && Arc::strong_count(&self.sets[i]) == 1 {
                let ds = Arc::into_inner(self.sets.swap_remove(i)).unwrap();

                let descriptor_set = ds.descriptor_set;
                let pool_idx = ds.pool_index;
                let bindings = ds.bindings.lock().unwrap();
                let req_desc = Self::calculate_required_descriptors(&bindings.iter().map(|b| DescriptorSetLayoutBindingDesc {
                    binding: b.binding_index,
                    descriptor_type: b.descriptor_type,
                    descriptor_count: b.descriptor_count,
                    stage_flags: vk::ShaderStageFlags::empty(),
                }).collect::<Vec<_>>());

                self.pools[pool_idx].free(&self.device, descriptor_set, &req_desc);
            }
            else {
                i += 1;
            }
        }
    }
}

impl Drop for DescriptorSetAllocator {
    fn drop(&mut self) {
        let last_waited = self.shared_state.last_host_waited_submission().num();

        let mut sets_in_use = 0;
        let mut sets_leaked = 0;

        for set in &self.sets {
            let strong_count = Arc::strong_count(set);
            let submission = set.submission_usage.load();

            if strong_count > 1 {
                sets_leaked += 1;
                warn!(
                    "DescriptorSetAllocator dropped with descriptor set still referenced (strong_count: {})",
                    strong_count
                );
            }

            if let Some(submission_num) = submission {
                if submission_num > last_waited {
                    sets_in_use += 1;
                    warn!(
                        "DescriptorSetAllocator dropped with descriptor set still in use by GPU (submission: {}, last_waited: {})",
                        submission_num, last_waited
                    );
                }
            }
        }

        if sets_in_use > 0 || sets_leaked > 0 {
            warn!(
                "DescriptorSetAllocator dropped with {} descriptor sets still in use by GPU and {} leaked references",
                sets_in_use, sets_leaked
            );
        }

        unsafe {
            for pool in &self.pools {
                self.device.destroy_descriptor_pool(pool.pool, None);
            }
        }
    }
}