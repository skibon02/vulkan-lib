use std::sync::atomic::AtomicBool;
use log::{error, info};
use sparkles::config::SparklesConfig;
use sparkles::{range_event_start, FinalizeGuard};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{WindowAttributes, WindowId};
use crate::app::App;

mod app;
pub mod render;

#[cfg(target_os = "android")]
pub mod android;

#[cfg(target_os = "android")]
fn sparkles_init() -> FinalizeGuard{
    sparkles::init(SparklesConfig::default()
        .without_file_sender()
        .with_udp_multicast_default())
}
#[cfg(not(target_os = "android"))]
fn sparkles_init() -> FinalizeGuard{
    sparkles::init(SparklesConfig::default()
        .with_udp_multicast_default())
}

static FIRST_RUN: AtomicBool = AtomicBool::new(true);
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use crate::android::android_main;

    if !FIRST_RUN.swap(false, std::sync::atomic::Ordering::SeqCst) {
        std::process::exit(0);
    }

    let g = sparkles_init();
    let event_loop = android_main(app);
    let mut winit_app: WinitApp = WinitApp::new(g);
    event_loop.run_app(&mut winit_app).unwrap();
    info!("Winit application exited without error!");
    std::process::exit(0);
}
pub fn run() {
    let g = sparkles_init();
    let event_loop = EventLoop::new().unwrap();
    let mut winit_app: WinitApp = WinitApp::new(g);
    event_loop.run_app(&mut winit_app).unwrap();
}

struct WinitApp {
    app: Option<App>,
    g: FinalizeGuard,
}

impl WinitApp {
    fn new(g: FinalizeGuard) -> Self {

        Self { app: None, g }
    }
}

impl ApplicationHandler for WinitApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let g = range_event_start!("[WINIT] resumed");
        info!("\t\t*** APP RESUMED ***");
        let window = event_loop
            .create_window(WindowAttributes::default().with_title("shades of pink"))
            .unwrap();

        window.request_redraw();

        let app_state = App::new_winit(window);
        self.app = Some(app_state);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let g = range_event_start!("[WINIT] window event");
        if self.app.as_mut().unwrap().is_finished() {
            info!("Exit requested!");
            event_loop.exit();
        }
        if let Err(e) = self.app.as_mut().unwrap().handle_event(event_loop, event) {
            error!("Error handling event: {:?}", e);
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        let g = range_event_start!("[WINIT] Exiting");
        info!("\t\t*** APP EXITING ***");
    }
    //
    // fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
    //     info!("\t\t*** APP ABOUT TO WAIT ***");
    // }

    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
        let g = range_event_start!("[WINIT] Memory warning");
        info!("\t\t*** APP MEMORY WARNING ***");
    }
}