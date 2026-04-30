use log::LevelFilter;
use sparkles::config::SparklesConfig;
use platform_lib::run_platform_loop;
use platform_lib::window::Window;

fn main() {
    simple_logger::SimpleLogger::new().with_level(LevelFilter::Info).init().unwrap();
    let g = sparkles::init(SparklesConfig::default()
        .without_file_sender()
        .with_udp_multicast_default());

    // sparkles::wait_client_connected();

    let g = sparkles::range_event_start!("The whole program");
    let win = Window::new();
    let win2 = Window::new();
    run_platform_loop();
}