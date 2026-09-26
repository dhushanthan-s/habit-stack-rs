#[cfg(target_os = "android")]
pub mod android_jni;
pub mod app;
pub mod model;
pub mod reminders;
pub mod storage;
pub mod view_model;

/// Android entry point. `android-activity` calls this instead of `main`, and it
/// must live in the cdylib rather than the binary.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    // The DB path has to be resolved from the activity before any storage call,
    // since `directories` cannot find a writable location on Android.
    if let Some(dir) = app.internal_data_path() {
        crate::storage::set_android_data_dir(dir);
    }
    slint::android::init(app).expect("Failed to init Slint Android backend");
    crate::app::run();
}
