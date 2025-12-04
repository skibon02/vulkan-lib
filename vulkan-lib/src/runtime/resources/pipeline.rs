use std::ffi::CStr;
use ash::vk;
use ash::vk::{ColorComponentFlags, CompareOp, CullModeFlags, DescriptorSetLayout, DescriptorSetLayoutBinding, DescriptorType, DynamicState, Format, GraphicsPipelineCreateInfo, Pipeline, PipelineCache, PipelineCacheCreateInfo, PipelineColorBlendAttachmentState, PipelineColorBlendStateCreateInfo, PipelineDepthStencilStateCreateInfo, PipelineDynamicStateCreateInfo, PipelineInputAssemblyStateCreateInfo, PipelineLayout, PipelineLayoutCreateInfo, PipelineMultisampleStateCreateInfo, PipelineRasterizationStateCreateInfo, PipelineShaderStageCreateInfo, PipelineVertexInputStateCreateInfo, PipelineViewportStateCreateInfo, PrimitiveTopology, RenderPass, SampleCountFlags, ShaderModuleCreateInfo, ShaderStageFlags, VertexInputAttributeDescription, VertexInputBindingDescription, FALSE};
use log::info;
use slotmap::DefaultKey;
use smallvec::SmallVec;
use sparkles::range_event_start;
use crate::runtime::{SharedState};
use crate::runtime::resources::GraphicsPipelineInner;
use crate::shaders::layout::MemberMeta;
use crate::shaders::DescriptorSetLayoutBindingDesc;
use crate::wrappers::device::VkDeviceRef;


pub struct GraphicsPipeline {
    pub(crate) shared: SharedState,

    pub(crate) handle: GraphicsPipelineHandle,
}

impl GraphicsPipeline {
    pub fn handle(&self) -> GraphicsPipelineHandle {
        self.handle
    }
}

impl Drop for GraphicsPipeline {
    fn drop(&mut self) {
        self.shared.schedule_destroy_pipeline(self.handle)
    }
}

#[derive(Copy, Clone)]
pub struct GraphicsPipelineHandle {
    pub(crate) key: DefaultKey,
    pipeline_layout: PipelineLayout, // vkCmdBindDescriptorSets must not be recorded to any command buffer during destruction (lazy destroy)
    pub(crate) pipeline_cache: PipelineCache, // can be destroyed
}
pub struct GraphicsPipelineDestroyHandle {
    pub(crate) key: DefaultKey,
}

impl From<GraphicsPipelineHandle> for GraphicsPipelineDestroyHandle {
    fn from(handle: GraphicsPipelineHandle) -> Self {
        Self {
            key: handle.key,
        }
    }
}


#[derive(Debug, Clone)]
pub struct VertexInputDesc {
    attrib_desc: Vec<VertexInputAttributeDescription>,
    binding_desc: Vec<VertexInputBindingDescription>,
}
impl VertexInputDesc {
    pub fn new(members_meta: &'static [MemberMeta], size: usize) -> Self {
        let binding_desc = vec![VertexInputBindingDescription::default()
                .binding(0)
                .input_rate(vk::VertexInputRate::INSTANCE)
                .stride(size as u32)];

        let attrib_desc = members_meta.iter().enumerate().map(|(i, member)| {
            VertexInputAttributeDescription::default()
                .binding(0)
                .format(member.ty.format())
                .offset(member.range.start as u32)
                .location(i as u32)
        }).collect::<Vec<_>>();
        Self {
            attrib_desc,
            binding_desc,
        }
    }

    pub fn get_input_state_create_info<'a>(&'a self) -> PipelineVertexInputStateCreateInfo<'a> {
        PipelineVertexInputStateCreateInfo::default()
            .vertex_attribute_descriptions(&self.attrib_desc)
            .vertex_binding_descriptions(&self.binding_desc)
    }
}

#[derive(Debug, Copy, Clone)]
pub enum VertexAssembly {
    TriangleStrip,
    TriangleList,
}

pub struct GraphicsPipelineDesc {
    pub vertex_assembly: VertexAssembly,
    pub attributes: VertexInputDesc,
    pub bindings: SmallVec<[&'static [DescriptorSetLayoutBindingDesc]; 4]>,
    pub vert_shader: Vec<u8>,
    pub frag_shader: Vec<u8>,
}

impl GraphicsPipelineDesc {
    pub fn new(shaders: (&'static [u8], &'static [u8]), attributes: VertexInputDesc, bindings: SmallVec<[&'static [DescriptorSetLayoutBindingDesc]; 4]>) -> Self {
        Self {
            vertex_assembly: VertexAssembly::TriangleStrip,
            attributes,
            bindings,
            vert_shader: shaders.0.to_vec(),
            frag_shader: shaders.1.to_vec(),
        }
    }
}

pub fn create_graphics_pipeline(device: VkDeviceRef, render_pass: RenderPass, pipeline_desc: GraphicsPipelineDesc, descriptor_set_layouts: SmallVec<[DescriptorSetLayout; 4]>) -> (GraphicsPipelineInner, GraphicsPipelineHandle) {
    let g = range_event_start!("Create pipeline");

    // 1. Create layout
    let pipeline_layout_info = PipelineLayoutCreateInfo::default()
        .set_layouts(&descriptor_set_layouts);
    let pipeline_layout = unsafe { device.create_pipeline_layout(&pipeline_layout_info, None).unwrap() };

    // shaders
    let vert_code = pipeline_desc.vert_shader;
    let vert_code: Vec<u32> = vert_code.chunks(4).map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap())).collect();
    let vertex_module = unsafe { device.create_shader_module(
        &ShaderModuleCreateInfo::default().code(&vert_code), None)
    }.unwrap();

    let frag_code = pipeline_desc.frag_shader;
    let frag_code: Vec<u32> = frag_code.chunks(4).map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap())).collect();
    let frag_module = unsafe { device.create_shader_module(
        &ShaderModuleCreateInfo::default().code(&frag_code), None)
    }.unwrap();

    let main_name = unsafe { CStr::from_bytes_with_nul_unchecked(b"main\0") };
    let vert_stage = PipelineShaderStageCreateInfo::default()
        .stage(ShaderStageFlags::VERTEX)
        .module(vertex_module)
        .name(main_name);
    let frag_stage = PipelineShaderStageCreateInfo::default()
        .stage(ShaderStageFlags::FRAGMENT)
        .module(frag_module)
        .name(main_name);

    // pipeline parts
    let msaa_samples = SampleCountFlags::TYPE_1; // no MSAA by default
    let multisample_state = PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(msaa_samples);
    let dynamic_state = PipelineDynamicStateCreateInfo::default()
        .dynamic_states(&[DynamicState::VIEWPORT, DynamicState::SCISSOR]);

    let input_assembly = get_assembly_create_info(&pipeline_desc.vertex_assembly);
    let vertex_input = pipeline_desc.attributes.get_input_state_create_info();

    let rast_info = PipelineRasterizationStateCreateInfo::default()
        .cull_mode(CullModeFlags::NONE)
        .line_width(1.0);

    let viewport_state = PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);

    // enable blending
    let color_blend_attachment =
        [PipelineColorBlendAttachmentState::default()
            .color_write_mask(ColorComponentFlags::RGBA)
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ZERO)
            .alpha_blend_op(vk::BlendOp::ADD)
        ];
    let color_blend = PipelineColorBlendStateCreateInfo::default()
        .attachments(&color_blend_attachment);

    let depth_state = PipelineDepthStencilStateCreateInfo::default()
        .depth_test_enable(true)
        .depth_write_enable(true)
        .depth_compare_op(CompareOp::LESS);



    let stages = [vert_stage, frag_stage];
    let pipeline_create_info = GraphicsPipelineCreateInfo::default()
        .layout(pipeline_layout)
        .render_pass(render_pass)
        .dynamic_state(&dynamic_state)
        .multisample_state(&multisample_state)

        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .stages(&stages)
        .rasterization_state(&rast_info)
        .color_blend_state(&color_blend)
        .viewport_state(&viewport_state)
        .depth_stencil_state(&depth_state);

    let pipeline_cache = unsafe {
        device.create_pipeline_cache(&PipelineCacheCreateInfo::default(), None).unwrap()
    };

    let pipeline = unsafe { device.create_graphics_pipelines(pipeline_cache, &[pipeline_create_info], None).unwrap()[0] };

    //destroy shader modules
    unsafe { device.destroy_shader_module(vertex_module, None); }
    unsafe { device.destroy_shader_module(frag_module, None); }

    (
        GraphicsPipelineInner {
            pipeline,
            pipeline_layout,
        },
        GraphicsPipelineHandle {
            key: DefaultKey::default(),
            pipeline_layout,
            pipeline_cache,
        }
    )
}

fn get_assembly_create_info(assembly: &VertexAssembly) -> PipelineInputAssemblyStateCreateInfo<'_> {
    match assembly {
        VertexAssembly::TriangleStrip => PipelineInputAssemblyStateCreateInfo {
            topology: PrimitiveTopology::TRIANGLE_STRIP,
            primitive_restart_enable: FALSE,
            ..Default::default()
        },
        VertexAssembly::TriangleList => PipelineInputAssemblyStateCreateInfo {
            topology: PrimitiveTopology::TRIANGLE_LIST,
            primitive_restart_enable: FALSE,
            ..Default::default()
        },
    }
}