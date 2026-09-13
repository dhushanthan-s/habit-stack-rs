//! JNI surface for the Android reminder receiver.
//!
//! Calls only ever go Java -> Rust. The receiver is a thin Java shim that asks
//! these functions what is due and when to wake next; the decisions stay in
//! `crate::reminders`, where they are unit-tested, rather than being
//! reimplemented against the Android APIs.

use crate::reminders;
use crate::storage::Storage;
use chrono::Local;
use jni::objects::{JClass, JString};
use jni::sys::{jlong, jstring};
use jni::JNIEnv;
use std::path::PathBuf;

fn db_path(env: &mut JNIEnv, files_dir: &JString) -> Option<PathBuf> {
    let dir: String = env.get_string(files_dir).ok()?.into();
    Some(PathBuf::from(dir).join("habits.db"))
}

/// JSON array of `{id, name, at}` for everything due and not yet announced.
/// Returns `[]` on any failure: a broken reminder must not crash the receiver.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_habitstack_Reminders_nativeDueNow<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    files_dir: JString<'local>,
) -> jstring {
    let payload = (|| {
        let path = db_path(&mut env, &files_dir)?;
        let storage = Storage::new(&path).ok()?;
        let due = reminders::scan(&storage, Local::now().naive_local()).ok()?;
        let items: Vec<serde_json::Value> = due
            .iter()
            .map(|d| {
                serde_json::json!({
                    "id": d.habit_id.to_string(),
                    "name": d.habit_name,
                    "at": d.at.format("%H:%M").to_string(),
                })
            })
            .collect();
        Some(serde_json::Value::Array(items).to_string())
    })()
    .unwrap_or_else(|| "[]".to_string());

    env.new_string(payload)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Marks a reminder announced so it cannot fire twice in one day.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_habitstack_Reminders_nativeMarkFired<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    files_dir: JString<'local>,
    habit_id: JString<'local>,
) {
    let _ = (|| {
        let path = db_path(&mut env, &files_dir)?;
        let id: String = env.get_string(&habit_id).ok()?.into();
        let id = uuid::Uuid::parse_str(&id).ok()?;
        let storage = Storage::new(&path).ok()?;
        storage
            .mark_reminder_fired(id, Local::now().date_naive())
            .ok()
    })();
}

/// Epoch millis of the next reminder, or 0 when none is scheduled.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_habitstack_Reminders_nativeNextAlarmMillis<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    files_dir: JString<'local>,
) -> jlong {
    (|| {
        let path = db_path(&mut env, &files_dir)?;
        let storage = Storage::new(&path).ok()?;
        let list = storage.list_reminders().ok()?;
        let next = reminders::next_fire_after(&list, Local::now().naive_local())?;
        // Interpret the naive local time in the device's zone.
        let local = next.and_local_timezone(Local).earliest()?;
        Some(local.timestamp_millis())
    })()
    .unwrap_or(0)
}

/// Receives window insets in logical pixels from the activity. Slint 1.15 has
/// no inset API, so without this the header draws under the status bar on the
/// edge-to-edge enforcement that Android 15+ applies.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_habitstack_Reminders_nativeSetInsets<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    top: f32,
    bottom: f32,
) {
    if let Some(weak) = crate::app::WINDOW.get() {
        // Hops to the UI thread; `upgrade()` would fail off it.
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.set_top_inset(top);
            app.set_bottom_inset(bottom);
        });
    }
}
