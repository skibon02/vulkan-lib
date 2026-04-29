use std::sync::atomic::AtomicBool;
use log::{error, info};
use sparkles::config::SparklesConfig;
use sparkles::{range_event_start, FinalizeGuard};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::event_loop::run_on_demand::EventLoopExtRunOnDemand;
use winit::window::{WindowAttributes, WindowId};
use crate::app::App;

mod app;
pub mod render;
pub mod util;
pub mod logic;

#[cfg(target_os = "android")]
pub mod android;
pub mod layout;
pub mod component;
pub mod resources;

#[cfg(target_os = "android")]
fn sparkles_init() -> FinalizeGuard{
    sparkles::init(SparklesConfig::default()
        .without_file_sender()
        .with_udp_multicast_default())
}
#[cfg(not(target_os = "android"))]
fn sparkles_init() -> FinalizeGuard{
    sparkles::init(SparklesConfig::default()
        .without_file_sender()
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
    let mut event_loop = android_main(app);
    let winit_app: WinitApp = WinitApp::new(g);
    event_loop.run_app_on_demand(winit_app).unwrap();
    info!("Winit application exited without error!");
    std::process::exit(0);
}
pub fn run() {
    let g = sparkles_init();
    let mut event_loop = EventLoop::new().unwrap();
    let winit_app: WinitApp = WinitApp::new(g);
    event_loop.run_app_on_demand(winit_app).unwrap();
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
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        let g = range_event_start!("[WINIT] can_create_surfaces");
        info!("\t\t*** APP CAN CREATE SURFACES ***");
        let window = event_loop
            .create_window(WindowAttributes::default()
                .with_title(":P"))
            .unwrap();

        // preserve instances between surface recreation on android (aka state ser/deser)
        let instances = self.app.take().map(|a| a.instances.clone());
        let wakeup = event_loop.create_proxy();
        let app_state = App::new_winit(window, instances.unwrap_or_default(), wakeup);
        self.app = Some(app_state);
    }

    fn destroy_surfaces(&mut self, _event_loop: &dyn ActiveEventLoop) {
        info!("\t\t*** APP DESTROY SURFACES ***");
        self.app = None;
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        if let Err(e) = self.app.as_mut().unwrap().handle_event(event_loop, event) {
            error!("Error handling event: {:?}", e);
        }
        if self.app.as_ref().unwrap().is_finished() {
            info!("Exit requested!");
            event_loop.exit();
        }
    }

    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        if let Some(app) = self.app.as_mut() {
            app.handle_wakeup();
        }
    }
    //
    //
    // fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
    //     info!("\t\t*** APP ABOUT TO WAIT ***");
    // }

    fn memory_warning(&mut self, event_loop: &dyn ActiveEventLoop) {
        let g = range_event_start!("[WINIT] Memory warning");
        info!("\t\t*** APP MEMORY WARNING ***");
    }
}