use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use habit_stack_rs::model::{Habit, Reminder, ALL_DAYS};
use habit_stack_rs::reminders::{due_now, to_announce};
use std::collections::HashSet;
use uuid::Uuid;

/// 2026-09-11 is a Friday, so bit 4 is the matching weekday.
const FRIDAY: u8 = 1 << 4;
const MONDAY: u8 = 1 << 0;

fn at(hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 9, 11)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
}

fn fixture() -> (Habit, Reminder) {
    let habit = Habit::new("Morning run", None);
    let mut reminder = Reminder::new(habit.id);
    reminder.hour = 8;
    reminder.minute = 0;
    reminder.days = ALL_DAYS;
    (habit, reminder)
}

fn due(reminder: &Reminder, habit: &Habit, done: &[Uuid], now: NaiveDateTime) -> usize {
    let done: HashSet<Uuid> = done.iter().copied().collect();
    due_now(std::slice::from_ref(reminder), std::slice::from_ref(habit), &done, now).len()
}

#[test]
fn becomes_due_once_the_time_has_passed() {
    let (habit, reminder) = fixture();
    assert_eq!(due(&reminder, &habit, &[], at(7, 59)), 0, "not yet");
    assert_eq!(due(&reminder, &habit, &[], at(8, 0)), 1, "exactly on time");
    assert_eq!(due(&reminder, &habit, &[], at(23, 0)), 1, "stays due all day");
}

#[test]
fn a_done_habit_is_never_due() {
    let (habit, reminder) = fixture();
    assert_eq!(due(&reminder, &habit, &[habit.id], at(9, 0)), 0);
}

#[test]
fn a_disabled_reminder_is_never_due() {
    let (habit, mut reminder) = fixture();
    reminder.enabled = false;
    assert_eq!(due(&reminder, &habit, &[], at(9, 0)), 0);
}

#[test]
fn weekday_mask_gates_the_reminder() {
    let (habit, mut reminder) = fixture();

    reminder.days = FRIDAY;
    assert_eq!(due(&reminder, &habit, &[], at(9, 0)), 1, "Friday is selected");

    reminder.days = MONDAY;
    assert_eq!(due(&reminder, &habit, &[], at(9, 0)), 0, "Monday-only, but it is Friday");
}

#[test]
fn a_reminder_without_its_habit_is_skipped() {
    let (_habit, reminder) = fixture();
    // Habit archived out of the active list, reminder row still present.
    let done = HashSet::new();
    assert!(due_now(&[reminder], &[], &done, at(9, 0)).is_empty());
}

#[test]
fn due_list_is_ordered_by_time() {
    let early = Habit::new("Early", None);
    let late = Habit::new("Late", None);
    let mut r_early = Reminder::new(early.id);
    r_early.hour = 6;
    let mut r_late = Reminder::new(late.id);
    r_late.hour = 21;

    let done = HashSet::new();
    // Passed in late-first to prove the sort is doing the work.
    let list = due_now(
        &[r_late, r_early],
        &[early.clone(), late.clone()],
        &done,
        at(22, 0),
    );
    assert_eq!(
        list.iter().map(|d| d.habit_name.as_str()).collect::<Vec<_>>(),
        vec!["Early", "Late"]
    );
}

#[test]
fn announcement_fires_once_then_is_suppressed() {
    let (habit, mut reminder) = fixture();
    let habits = [habit.clone()];
    let done = HashSet::new();

    let first = to_announce(&[reminder.clone()], &habits, &done, at(8, 1));
    assert_eq!(first.len(), 1, "first crossing announces");

    // What take_announcements writes after delivering.
    reminder.last_fired = Some(at(8, 1).date());
    let second = to_announce(&[reminder.clone()], &habits, &done, at(8, 2));
    assert!(second.is_empty(), "same day must not re-announce");

    // A late tick, long after the time, still must not double up.
    let late = to_announce(&[reminder.clone()], &habits, &done, at(18, 0));
    assert!(late.is_empty(), "a throttled tick must not re-announce");

    // Next day it is eligible again.
    let tomorrow = NaiveDate::from_ymd_opt(2026, 9, 12)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(8, 1, 0).unwrap());
    assert_eq!(to_announce(&[reminder], &habits, &done, tomorrow).len(), 1);
}

#[test]
fn announcements_still_respect_the_done_check() {
    let (habit, reminder) = fixture();
    let done: HashSet<Uuid> = [habit.id].into_iter().collect();
    assert!(to_announce(&[reminder], &[habit], &done, at(8, 1)).is_empty());
}

#[test]
fn reminder_helpers_round_trip() {
    let (_habit, reminder) = fixture();
    assert_eq!(reminder.label(), "08:00");
    assert_eq!(Reminder::parse_time("08:00"), Some((8, 0)));
    assert_eq!(Reminder::parse_time("23:59"), Some((23, 59)));
    assert_eq!(Reminder::parse_time("24:00"), None, "hour out of range");
    assert_eq!(Reminder::parse_time("08:60"), None, "minute out of range");
    assert_eq!(Reminder::parse_time("nonsense"), None);
}
