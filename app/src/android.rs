use std::sync::{Arc, Mutex};
use jni::{jni_sig, jni_str, JavaVM};
use jni::objects::GlobalRef;
use lazy_static::lazy_static;
use log::info;
use sparkles::range_event_start;
use winit::event_loop::{EventLoop, EventLoopBuilder};
use winit::platform::android::activity::*;

pub fn android_main(app: AndroidApp) -> EventLoop<()> {
    use jni::objects::{JObject, JObjectArray, JValue};
    use winit::platform::android::EventLoopBuilderExtAndroid;
    use vulkan_lib::android::set_android_context;

    let g = range_event_start!("android_main init");

    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );

    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as _) };

    let activity_global = vm.attach_current_thread(|env| {
        let activity = unsafe { JObject::from_raw(env, app.activity_as_ptr() as jni::sys::jobject) };

        let windowmanager = env
            .call_method(&activity, jni_str!("getWindowManager"), jni_sig!("()Landroid/view/WindowManager;"), &[])
            .unwrap()
            .l()
            .unwrap();
        let display = env
            .call_method(&windowmanager, jni_str!("getDefaultDisplay"), jni_sig!("()Landroid/view/Display;"), &[])
            .unwrap()
            .l()
            .unwrap();
        let supported_modes = env
            .call_method(&display, jni_str!("getSupportedModes"), jni_sig!("()[Landroid/view/Display$Mode;"), &[])
            .unwrap()
            .l()
            .unwrap();
        let supported_modes = unsafe { JObjectArray::<JObject>::from_raw(env, supported_modes.as_raw()) };
        let length = env.get_array_length(&supported_modes).unwrap() as usize;
        info!("Found {} supported modes", length);
        let mut modes = Vec::new();
        for i in 0..length {
            let mode = env.get_object_array_element(&supported_modes, i).unwrap();
            let height = env.call_method(&mode, jni_str!("getPhysicalHeight"), jni_sig!("()I"), &[]).unwrap().i().unwrap();
            let width = env.call_method(&mode, jni_str!("getPhysicalWidth"), jni_sig!("()I"), &[]).unwrap().i().unwrap();
            let refresh_rate = env.call_method(&mode, jni_str!("getRefreshRate"), jni_sig!("()F"), &[]).unwrap().f().unwrap();
            let index = env.call_method(&mode, jni_str!("getModeId"), jni_sig!("()I"), &[]).unwrap().i().unwrap();
            modes.push((index, refresh_rate));
            info!("Mode {}: {}x{}@{}", index, width, height, refresh_rate);
        }

        let max_framerate_mode = modes.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).unwrap();
        info!("Max framerate: {}", max_framerate_mode.1);

        let preferred_id = 1;

        let window = env
            .call_method(&activity, jni_str!("getWindow"), jni_sig!("()Landroid/view/Window;"), &[])
            .unwrap()
            .l()
            .unwrap();
        let layout_params_class = env.find_class(jni_str!("android/view/WindowManager$LayoutParams")).unwrap();
        let layout_params = env
            .call_method(&window, jni_str!("getAttributes"), jni_sig!("()Landroid/view/WindowManager$LayoutParams;"), &[])
            .unwrap()
            .l()
            .unwrap();
        let preferred_display_mode_id_field_id = env
            .get_field_id(&layout_params_class, jni_str!("preferredDisplayModeId"), jni_sig!("I"))
            .unwrap();
        unsafe { env.set_field_unchecked(&layout_params, preferred_display_mode_id_field_id, JValue::from(preferred_id)) }
            .unwrap();

        let window = env
            .call_method(&activity, jni_str!("getWindow"), jni_sig!("()Landroid/view/Window;"), &[])
            .unwrap()
            .l()
            .unwrap();
        env.call_method(&window, jni_str!("setAttributes"), jni_sig!("(Landroid/view/WindowManager$LayoutParams;)V"), &[(&layout_params).into()])
            .unwrap();

        env.new_global_ref(activity)
    }).unwrap();

    drop(g);

    *VM.lock().unwrap() = Some(vm);
    *ACTIVITY.lock().unwrap() = Some(activity_global);
    set_android_context(ACTIVITY.clone(), VM.clone());
    let event_loop = EventLoopBuilder::default()
        .with_android_app(app)
        .build().unwrap();
    event_loop
}

lazy_static!{
    pub static ref VM: Arc<Mutex<Option<JavaVM>>> = Arc::new(Mutex::new(None));
    pub static ref ACTIVITY: Arc<Mutex<Option<GlobalRef<JObject<'static>>>>> = Arc::new(Mutex::new(None));
}

use jni::objects::JObject;
