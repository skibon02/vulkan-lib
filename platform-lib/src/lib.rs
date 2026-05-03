mod platform;
pub use platform::platform_impl::*;

pub trait AppLogic {
    fn init();
    fn poll();
}