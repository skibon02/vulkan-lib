use log::LevelFilter;
use sparkles::config::SparklesConfig;
use winwin32::run_platform_loop;
use winwin32::window::Window;

fn main() {
    simple_logger::SimpleLogger::new().with_level(LevelFilter::Info).init().unwrap();
    let g = sparkles::init(SparklesConfig::default()
        .without_file_sender()
        .with_udp_multicast_default());

    // sparkles::wait_client_connected();

    let g = sparkles::range_event_start!("The whole program");
    let win = Window::new();
    run_platform_loop();
}