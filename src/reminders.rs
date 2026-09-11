//! Which reminders are due, as pure functions.
//!
//! Nothing here reads a clock or touches storage: callers pass the current
//! local time in. That keeps it directly testable, and lets the in-app badges
//! and the Android notification receiver share one implementation of "is this
//! actually due?" rather than each growing their own.

use crate::model::{Habit, Reminder};
use chrono::{Datelike, NaiveDateTime, NaiveTime};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueReminder {
    pub habit_id: Uuid,
    pub habit_name: String,
    pub at: NaiveTime,
}

/// Reminders scheduled for `now`'s weekday whose time has passed and whose
/// habit is not done yet. Recomputed on every tick to drive the UI, so it
/// deliberately ignores the `last_fired` watermark.
pub fn due_now(
    reminders: &[Reminder],
    habits: &[Habit],
    done_today: &HashSet<Uuid>,
    now: NaiveDateTime,
) -> Vec<DueReminder> {
    let weekday = now.date().weekday();
    let time = now.time();

    let mut due: Vec<DueReminder> = reminders
        .iter()
        .filter(|r| r.enabled)
        .filter(|r| r.fires_on(weekday))
        .filter(|r| r.time_of_day() <= time)
        .filter(|r| !done_today.contains(&r.habit_id))
        .filter_map(|r| {
            // Archived habits keep their row via ON DELETE CASCADE only on
            // delete, so skip anything not in the active list.
            let habit = habits.iter().find(|h| h.id == r.habit_id)?;
            Some(DueReminder {
                habit_id: r.habit_id,
                habit_name: habit.name.clone(),
                at: r.time_of_day(),
            })
        })
        .collect();

    due.sort_by_key(|d| (d.at, d.habit_id));
    due
}

/// `due_now` minus anything already announced today. Only notifications use
/// this; badges should keep showing a habit that was announced earlier.
pub fn to_announce(
    reminders: &[Reminder],
    habits: &[Habit],
    done_today: &HashSet<Uuid>,
    now: NaiveDateTime,
) -> Vec<DueReminder> {
    let today = now.date();
    let fired: HashSet<Uuid> = reminders
        .iter()
        .filter(|r| r.last_fired == Some(today))
        .map(|r| r.habit_id)
        .collect();

    due_now(reminders, habits, done_today, now)
        .into_iter()
        .filter(|d| !fired.contains(&d.habit_id))
        .collect()
}
