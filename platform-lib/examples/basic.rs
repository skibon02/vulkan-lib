use std::thread;
use std::time::Duration;
use log::LevelFilter;
use rand::random_range;
use sparkles::config::SparklesConfig;
use tokio::task::block_in_place;
use platform_lib::{start_app, ApplicationLogic, WindowManager};
use platform_lib::window::WindowAttributes;

pub struct MyApp;
impl ApplicationLogic for MyApp {
    fn spawn_logic_task(mut manager: WindowManager) {
        thread::spawn(move || {
            loop {
                let w = random_range(100..=300);
                let h = random_range(100..=300);
                let x = random_range(100..=1800);
                let y = random_range(100..=900);
                let mut attrib = WindowAttributes::default();
                attrib.title = ":P".into();
                attrib.initial_pos = Some((x, y));
                attrib.initial_size = Some((w, h));
                attrib.borderless = false;
                let win = manager.create_window(attrib);
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs(4));
                    win.close_window();

                    // thread::sleep(Duration::from_secs(7));
                });
                thread::sleep(Duration::from_millis(100));
            }
        });
    }
}

#[tokio::main]
async fn main() {
    simple_logger::SimpleLogger::new().with_level(LevelFilter::Info).init().unwrap();
    let g = sparkles::init(SparklesConfig::default()
        .without_file_sender()
        .with_udp_multicast_default());

    block_in_place(|| {
        // sparkles::wait_client_connected();
        let g = sparkles::range_event_start!("The whole program");

        start_app::<MyApp>();
    })
}