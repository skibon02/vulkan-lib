use std::sync::{Arc, Mutex, OnceLock};
use jni::JavaVM;
use jni::objects::GlobalRef;
use log::warn;

pub static VM: OnceLock<Arc<Mutex<Option<jni::JavaVM>>>> = OnceLock::new();
pub static ACTIVITY: OnceLock<Arc<Mutex<Option<jni::objects::GlobalRef>>>> = OnceLock::new();

pub fn set_android_context(vm: Arc<Mutex<Option<GlobalRef>>>, activity: Arc<Mutex<Option<JavaVM>>>) {
    let _ = ACTIVITY.set(vm).inspect_err(|e| warn!("Android: Duplicate init ACTIVITY"));
    let _ = VM.set(activity).inspect_err(|e| warn!("Android: Duplicate init VM"));
}