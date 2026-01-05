use std::fs;
use std::path::PathBuf;
use log::info;

#[cfg(not(target_os = "android"))]
pub fn get_resource(path: PathBuf) -> anyhow::Result<Vec<u8>> {
    Ok(fs::read(path)?)
}

#[cfg(target_os = "android")]
pub fn get_resource(path: PathBuf) -> anyhow::Result<Vec<u8>> {
    use ndk_sys::AAssetManager_fromJava;
    use std::ptr::NonNull;
    use std::ffi::CString;
    use crate::android::{ACTIVITY, VM};

    let mut vm_lock = VM.lock().unwrap();
    let vm = vm_lock.as_mut().unwrap();
    let attach_guard = vm.attach_current_thread().unwrap();
    let mut env = vm.get_env().unwrap();

    let mut activity_lock = ACTIVITY.lock().unwrap();
    let activity = activity_lock.as_mut().unwrap();

    let asset_manager = env
        .call_method(
            &*activity,
            "getAssets",
            "()Landroid/content/res/AssetManager;",
            &[],
        )?
        .l()?;

    let asset_manager_ptr = unsafe { AAssetManager_fromJava(env.get_native_interface(), asset_manager.into_raw()) };
    let asset_manager = unsafe { ndk::asset::AssetManager::from_ptr(NonNull::new(asset_manager_ptr).unwrap()) };
    let filename_cstr = CString::new(path.to_str().unwrap())?;
    info!("Opening asset: {}", path.to_str().unwrap());
    let mut asset = asset_manager.open(&filename_cstr).unwrap();
    let mut buffer = Vec::new();
    use std::io::Read;
    asset.read_to_end(&mut buffer)?;

    Ok(buffer)
}