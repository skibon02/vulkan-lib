use std::collections::HashMap;
use ash::vk::{BufferCreateFlags, BufferUsageFlags, Format, ImageCreateFlags, ImageTiling, ImageUsageFlags, MemoryHeap, MemoryPropertyFlags, MemoryType};
use crate::wrappers::device::VkDeviceRef;
use crate::util::image::is_color_format;
use ash::vk;

pub enum MemoryTypeAlgorithm {
    Host,
    Device,
}

pub struct MemoryManager {
    device: VkDeviceRef,
    memory_types: Vec<MemoryType>,
    memory_heaps: Vec<MemoryHeap>,
    buffer_memory_requirements: HashMap<(BufferCreateFlags, BufferUsageFlags), (u64, u32)>,
    image_memory_requirements: HashMap<(Format, ImageTiling, ImageCreateFlags, ImageUsageFlags), u32>,
}

impl MemoryManager {
    pub fn new(
        device: VkDeviceRef,
        memory_types: Vec<MemoryType>,
        memory_heaps: Vec<MemoryHeap>,
    ) -> Self {
        Self {
            device,
            memory_types,
            memory_heaps,
            buffer_memory_requirements: HashMap::new(),
            image_memory_requirements: HashMap::new(),
        }
    }

    pub fn get_buffer_memory_requirements(&mut self, usage: BufferUsageFlags, flags: BufferCreateFlags) -> (u64, u32) {
        let device_memory_type = self.buffer_memory_requirements
            .entry((flags, usage))
            .or_insert_with(|| {
                let buffer_create_info = vk::BufferCreateInfo::default()
                    .size(1)
                    .usage(usage)
                    .flags(flags)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE);

                let buffer = unsafe { self.device.create_buffer(&buffer_create_info, None) }.unwrap();
                let memory_requirements = unsafe { self.device.get_buffer_memory_requirements(buffer) };
                unsafe { self.device.destroy_buffer(buffer, None) };
                let alignment = memory_requirements.alignment;

                (alignment, memory_requirements.memory_type_bits)
            });

        *device_memory_type
    }

    pub fn get_image_memory_requirements(&mut self, format: Format, tiling: ImageTiling, usage: ImageUsageFlags, flags: ImageCreateFlags) -> u32 {
        let format = if is_color_format(format) {
            Format::UNDEFINED
        }
        else {
            format
        };

        let usage = usage & (ImageUsageFlags::TRANSIENT_ATTACHMENT | ImageUsageFlags::COLOR_ATTACHMENT | ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | ImageUsageFlags::INPUT_ATTACHMENT);
        let flags = flags & ImageCreateFlags::SPARSE_BINDING;

        let device_memory_type = self.image_memory_requirements
            .entry((format, tiling, flags, usage))
            .or_insert_with(|| {
                let image_create_info = vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(vk::Extent3D {
                        width: 1,
                        height: 1,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(tiling)
                    .usage(usage)
                    .flags(flags)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .initial_layout(vk::ImageLayout::UNDEFINED);

                let image = unsafe { self.device.create_image(&image_create_info, None) }.unwrap();
                let memory_requirements = unsafe { self.device.get_image_memory_requirements(image) };
                unsafe { self.device.destroy_image(image, None) };

                memory_requirements.memory_type_bits
            });

        *device_memory_type
    }

    pub fn best_host_type(&self, memory_type_bits: u32) -> u32 {
        self.memory_types
            .iter()
            .enumerate()
            .filter(|(i, memory_type)| {
                memory_type.property_flags.contains(MemoryPropertyFlags::HOST_COHERENT) && (1u32 << i) & memory_type_bits != 0
            })
            .next()
            .expect("Guaranteed to support at least 1 host mappable memory type for buffer").0 as u32
    }

    pub fn best_device_type(&self, memory_type_bits: u32) -> u32 {
        self.memory_types
            .iter()
            .enumerate()
            .filter(|(i, memory_type)| {
                memory_type.property_flags.contains(MemoryPropertyFlags::DEVICE_LOCAL) && (1u32 << i) & memory_type_bits != 0
            })
            .max_by_key(|(_, mem)| {
                let only_1_flag = mem.property_flags == MemoryPropertyFlags::DEVICE_LOCAL;
                let heap_size = self.memory_heaps[mem.heap_index as usize].size;

                heap_size + only_1_flag as u64
            })
            .expect("Guaranteed to support at least 1 device_local memory type for buffer").0 as u32
    }

    pub fn select_memory_type(&self, memory_type_bits: u32, algorithm: MemoryTypeAlgorithm) -> u32 {
        match algorithm {
            MemoryTypeAlgorithm::Host => self.best_host_type(memory_type_bits),
            MemoryTypeAlgorithm::Device => self.best_device_type(memory_type_bits),
        }
    }
}