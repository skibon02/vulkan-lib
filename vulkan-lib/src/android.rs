use std::sync::{Arc, Mutex, OnceLock};
use jni::JavaVM;
use jni::objects::{GlobalRef, JObject};
use log::warn;

pub static VM: OnceLock<Arc<Mutex<Option<jni::JavaVM>>>> = OnceLock::new();
pub static ACTIVITY: OnceLock<Arc<Mutex<Option<jni::objects::GlobalRef<JObject<'static>>>>>> = OnceLock::new();

pub fn set_android_context(activity: Arc<Mutex<Option<GlobalRef<JObject<'static>>>>>, vm: Arc<Mutex<Option<JavaVM>>>) {
    let _ = ACTIVITY.set(activity).inspect_err(|e| warn!("Android: Duplicate init ACTIVITY"));
    let _ = VM.set(vm).inspect_err(|e| warn!("Android: Duplicate init VM"));
}