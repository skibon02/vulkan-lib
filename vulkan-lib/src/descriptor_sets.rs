use ash::vk;
use ash::vk::{BufferUsageFlags, CommandBuffer, DescriptorBufferInfo, DescriptorPool, DescriptorPoolSize, DescriptorSet, DescriptorSetAllocateInfo, DescriptorSetLayout, DescriptorSetLayoutBinding, DescriptorType, Extent2D, ImageTiling, PipelineBindPoint, PipelineLayout, SampleCountFlags, ShaderStageFlags, WriteDescriptorSet, WHOLE_SIZE};
use log::info;
use sparkles::range_event_start;
use crate::wrappers::device::VkDeviceRef;

pub struct DescriptorSetPool {
    device: VkDeviceRef,

    descriptor_pool: DescriptorPool,

    allocated_sets: u32,
    capacity_sets: u32,

    allocated_uniform_buffers: u32,
    capacity_uniform_buffers: u32,

    allocated_image_samplers: u32,
    capacity_image_samplers: u32,
}

impl DescriptorSetPool {
    pub fn new(device: VkDeviceRef) -> Self {
        let capacity_sets = 100;
        let capacity_uniform_buffers = 100;
        let capacity_image_samplers = 100;

        let pool_sizes = [
            DescriptorPoolSize::default()
                .descriptor_count(capacity_uniform_buffers)
                .ty(DescriptorType::UNIFORM_BUFFER),
            DescriptorPoolSize::default()
                .descriptor_count(capacity_image_samplers)
                .ty(DescriptorType::COMBINED_IMAGE_SAMPLER)];
        let desc_pool_info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(capacity_sets)
            .pool_sizes(&pool_sizes)
            .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET);

        let descriptor_pool = unsafe {
            device
                .create_descriptor_pool(&desc_pool_info, None)
                .unwrap()
        };

        DescriptorSetPool {
            device,
            descriptor_pool,

            capacity_image_samplers,
            capacity_sets,
            capacity_uniform_buffers,
            
            allocated_image_samplers: 0,
            allocated_sets: 0,
            allocated_uniform_buffers: 0,
        }
    }


    pub fn allocate_descriptor_sets<'a>(&mut self, descriptor_set_layout: DescriptorSetLayout,
                                        buffer_bindings: impl Iterator<Item=(u32, BufferResource)>,
                                        image_bindings: impl Iterator<Item=(u32, &'a UniformImage)>) -> DescriptorSet {

        let set_layouts = [descriptor_set_layout];
        let alloc_info = DescriptorSetAllocateInfo::default()
            .descriptor_pool(self.descriptor_pool)
            .set_layouts(&set_layouts);
        let descriptor_set = unsafe { self.device.allocate_descriptor_sets(&alloc_info).unwrap()[0] };
        

        let buffer_bindings: Vec<_> = buffer_bindings.collect();
        let image_bindings: Vec<_> = image_bindings.collect();
        
        self.allocated_sets += 1;
        self.allocated_uniform_buffers += buffer_bindings.len() as u32;
        self.allocated_image_samplers += image_bindings.len() as u32;

        // if self.allocated_sets > self.capacity_sets ||
        //     self.allocated_uniform_buffers > self.capacity_uniform_buffers ||
        //     self.allocated_image_samplers > self.capacity_image_samplers {
        //     panic!("Descriptor set pool exceeded capacity");
        // }
        // Update descriptor set
        let buffer_infos: Vec<_> = buffer_bindings.iter().map(|(_, buffer)| {
            [
                DescriptorBufferInfo::default()
                    .offset(0)
                    .buffer(buffer.buffer)
                    .range(WHOLE_SIZE)
            ]
        }).collect();
        let image_infos: Vec<_> = image_bindings.iter().map(|(binding, image_sampler)| {
            let image = image_sampler.image_view;
            let sampler = image_sampler.sampler;

            [vk::DescriptorImageInfo::default()
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .image_view(image)
                .sampler(sampler)
            ]
        }).collect();

        // let mut image_info_i = 0;
        let descriptor_writes: Vec<_> = buffer_bindings.iter().enumerate().map(|(i, (binding, _))| {
            WriteDescriptorSet::default()
                .descriptor_type(DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(1)
                .dst_set(descriptor_set)
                .dst_binding(*binding)
                .dst_array_element(0)
                .buffer_info(&buffer_infos[i])
        }).chain(image_bindings.iter().enumerate().map(|(i, (binding, _))| {
            WriteDescriptorSet::default()
                .descriptor_type(DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(1)
                .dst_set(descriptor_set)
                .dst_binding(*binding)
                .dst_array_element(0)
                .image_info(&image_infos[i])
        })).collect();
        
        // info!("Descriptor writes: {:?}", descriptor_writes);

        unsafe { self.device.update_descriptor_sets(&descriptor_writes, &[]) }

        descriptor_set
    }

}

impl Drop for DescriptorSetPool {
    fn drop(&mut self) {
        unsafe {
            self.device
                .destroy_descriptor_pool(self.descriptor_pool, None);
        }
    }
}


/// Represents an exact resource bindings (uniforms/images) for a single object
pub struct ObjectDescriptorSet {
    device: VkDeviceRef,

    /// borrowed from the pipeline
    descriptor_set_layout: DescriptorSetLayout,
    descriptor_set: DescriptorSet,
}

impl ObjectDescriptorSet {
    pub fn new<'a>(device: VkDeviceRef, descriptor_set_pool: &mut DescriptorSetPool,
                   descriptor_set_layout: DescriptorSetLayout,
                   buffer_bindings: impl Iterator<Item=(u32, BufferResource)>,
                   image_bindings: impl Iterator<Item=(u32, &'a UniformImage)>) -> ObjectDescriptorSet {
        let g = range_event_start!("[Vulkan] Create descriptor sets");

        // Ask pool to allocate descriptor set and perform writes
        let descriptor_set = descriptor_set_pool.allocate_descriptor_sets(descriptor_set_layout, buffer_bindings, image_bindings);
        
        Self {
            device,
            descriptor_set_layout,
            descriptor_set,
        }
    }

    pub fn get_descriptor_set_layout(&self) -> DescriptorSetLayout {
        self.descriptor_set_layout
    }

    pub fn bind_sets(&self, command_buffer: CommandBuffer, pipeline_layout: PipelineLayout) {
        let descriptor_sets = [self.descriptor_set];
        unsafe {
            self.device.cmd_bind_descriptor_sets(
                command_buffer,
                PipelineBindPoint::GRAPHICS,
                pipeline_layout,
                0,
                &descriptor_sets,
                &[],
            );
        }
    }
    
    pub fn destroy(self, descriptor_pool: &mut DescriptorSetPool) {
        unsafe {
            self.device.free_descriptor_sets(descriptor_pool.descriptor_pool, &[self.descriptor_set]).unwrap();
        }
    }
}