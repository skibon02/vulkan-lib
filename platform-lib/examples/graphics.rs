use std::thread;
use std::time::Duration;
use log::{error, warn, LevelFilter};
use sparkles::config::SparklesConfig;
use wayland_client::backend::smallvec::smallvec;
use platform_lib::{start_app, ApplicationLogic, WindowManager};
use platform_lib::window::WindowAttributes;
use vulkan_lib::resources::render_pass::AttachmentsDescription;
use vulkan_lib::vk::{AttachmentDescription, AttachmentLoadOp, AttachmentStoreOp, ClearColorValue, ClearDepthStencilValue, ClearValue, Filter, Format, ImageLayout, PipelineStageFlags, SampleCountFlags, API_VERSION_1_3};
use vulkan_lib::VulkanInstance;

pub struct MyApp;
impl ApplicationLogic for MyApp {
    fn spawn_logic_task(mut manager: WindowManager) {
        thread::spawn(move || {
            let mut attrib = WindowAttributes::default();
            attrib.title = ":P".into();
            attrib.initial_pos = Some((300, 100));
            attrib.initial_size = Some((800, 500));
            attrib.borderless = false;
            let win = manager.create_window(attrib);
            
            loop {
                let wh = win.rwh();
                let dh = win.rdh();
                // init vulkan
                // let res = manager.read_event();

                let mut vulkan = VulkanInstance::new_for_handle(wh, dh, (500, 800), API_VERSION_1_3).unwrap() ;
                let mut allocator = vulkan.new_allocator();

                // Create render pass
                let msaa_samples = SampleCountFlags::TYPE_1;

                let need_resolve = msaa_samples != SampleCountFlags::TYPE_1;

                let load_op = if need_resolve {
                    AttachmentLoadOp::DONT_CARE
                } else {
                    AttachmentLoadOp::CLEAR
                };
                let swapchain_attachment = AttachmentDescription::default()
                    .samples(SampleCountFlags::TYPE_1)
                    .load_op(load_op)
                    .store_op(AttachmentStoreOp::STORE)
                    .initial_layout(ImageLayout::UNDEFINED)
                    .final_layout(ImageLayout::PRESENT_SRC_KHR);

                let depth_attachment = AttachmentDescription::default()
                    .format(Format::D16_UNORM)
                    .samples(msaa_samples)
                    .load_op(AttachmentLoadOp::CLEAR)
                    .store_op(AttachmentStoreOp::DONT_CARE)
                    .stencil_load_op(AttachmentLoadOp::DONT_CARE)
                    .stencil_store_op(AttachmentStoreOp::DONT_CARE)
                    .initial_layout(ImageLayout::UNDEFINED)
                    .final_layout(ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);

                let mut attachments_desc = AttachmentsDescription::new(swapchain_attachment, ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .with_depth_attachment(depth_attachment, ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);

                let pixel_perfect_sampler = allocator.new_sampler(|i| {
                    i
                        .min_filter(Filter::NEAREST)
                        .mag_filter(Filter::NEAREST)
                });

                if need_resolve {
                    // Add color attachment for MSAA
                    let color_attachment = AttachmentDescription::default()
                        .samples(msaa_samples)
                        .load_op(AttachmentLoadOp::CLEAR)
                        .store_op(AttachmentStoreOp::DONT_CARE)
                        .initial_layout(ImageLayout::UNDEFINED)
                        .final_layout(ImageLayout::COLOR_ATTACHMENT_OPTIMAL);

                    attachments_desc = attachments_desc.with_color_attachment(color_attachment, ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
                }

                let swapchain_format = vulkan.swapchain_format();
                let render_pass = allocator.new_render_pass(
                    attachments_desc.clone(),
                    swapchain_format,
                );

                let bg_color = [0.15, 0.12, 0.11];
                let bg_clear_color = ClearColorValue {
                    float32: [bg_color[2], bg_color[1], bg_color[0], 1.0],
                };
                'render: loop {
                    let (image_index, acquire_wait_ref, is_suboptimal) = match vulkan.acquire_next_image() {
                        Ok(result) => result,
                        Err(e) => {
                            error!("Failed to acquire next image after recreate: {:?}", e);

                            break 'render;
                        }
                    };

                    let mut clear_values = smallvec![
                        ClearValue {
                            color: bg_clear_color,
                        },
                        ClearValue {
                            depth_stencil: ClearDepthStencilValue::default().depth(1.0)
                        },
                    ];
                    if need_resolve {
                        clear_values.push(ClearValue {
                            color: bg_clear_color,
                        });
                    }

                    let (present_wait_ref, _) = vulkan.record_device_commands_signal(Some(acquire_wait_ref.with_stages(PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)), |ctx| {
                        ctx.render_pass(render_pass.clone(), image_index, clear_values, |ctx| {

                        })
                    });

                    match vulkan.queue_present(image_index, present_wait_ref) {
                        Ok(r) => {
                            if r {
                                warn!("Swapchain present: Swapchain is suboptimal!");
                            }
                        }
                        Err(e) => {
                            error!("Present error: {:?}", e);
                        }
                    }

                    allocator.destroy_old_resources();
                    thread::sleep(Duration::from_millis(10));
                }
            }
        });
    }
}

fn main() {
    simple_logger::SimpleLogger::new().with_level(LevelFilter::Info).init().unwrap();
    let g = sparkles::init(SparklesConfig::default()
        .without_file_sender()
        .with_udp_multicast_default());

    // sparkles::wait_client_connected();

    let g = sparkles::range_event_start!("The whole program");
    start_app::<MyApp>();
}