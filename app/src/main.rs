use log::LevelFilter;
use simple_logger::SimpleLogger;

fn main() {
    SimpleLogger::new().with_utc_timestamps().with_colors(true).with_level(LevelFilter::Info).with_module_level("sparkles_parser", LevelFilter::Warn).init().unwrap();
    app::run();
}
