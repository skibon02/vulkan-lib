use log::LevelFilter;
use winwin32::run_platform_loop;
use winwin32::window::Window;

fn main() {
    simple_logger::SimpleLogger::new().with_level(LevelFilter::Info).init().unwrap();

    let win = Window::new();
    run_platform_loop();
}