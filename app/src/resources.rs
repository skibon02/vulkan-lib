use std::path::PathBuf;
use log::info;

#[cfg(not(target_os = "android"))]
pub fn get_resource(path: PathBuf) -> anyhow::Result<Vec<u8>> {
    use std::fs;
    Ok(fs::read(path)?)
}

#[cfg(target_os = "android")]
pub fn get_resource(path: PathBuf) -> anyhow::Result<Vec<u8>> {
    use jni::{jni_sig, jni_str};
    use ndk_sys::AAssetManager_fromJava;
    use std::ptr::NonNull;
    use std::ffi::CString;
    use crate::android::{ACTIVITY, VM};

    let vm_lock = VM.lock().unwrap();
    let vm = vm_lock.as_ref().unwrap();

    let activity_lock = ACTIVITY.lock().unwrap();
    let activity = activity_lock.as_ref().unwrap();

    let buffer = vm.attach_current_thread(|env| -> anyhow::Result<Vec<u8>> {
        let asset_manager = env
            .call_method(
                activity.as_obj(),
                jni_str!("getAssets"),
                jni_sig!("()Landroid/content/res/AssetManager;"),
                &[],
            )?
            .l()?;

        let asset_manager_ptr = unsafe {
            AAssetManager_fromJava(env.get_raw() as _, asset_manager.as_raw())
        };
        let asset_manager = unsafe {
            ndk::asset::AssetManager::from_ptr(NonNull::new(asset_manager_ptr).unwrap())
        };
        let filename_cstr = CString::new(path.to_str().unwrap())?;
        info!("Opening asset: {}", path.to_str().unwrap());
        let mut asset = asset_manager.open(&filename_cstr).unwrap();
        let mut buffer = Vec::new();
        use std::io::Read;
        asset.read_to_end(&mut buffer)?;
        Ok(buffer)
    })?;

    Ok(buffer)
}
