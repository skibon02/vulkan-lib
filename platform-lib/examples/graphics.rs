use std::thread;
use std::time::Duration;
use log::LevelFilter;
use rand::random_range;
use sparkles::config::SparklesConfig;
use platform_lib::{start_app, ApplicationLogic, WindowManager};
use platform_lib::window::WindowAttributes;

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
                let res = manager.read_event();
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