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

// ---------------------------------------------------------- next_fire_after

use habit_stack_rs::reminders::next_fire_after;

#[test]
fn next_fire_finds_later_today_before_tomorrow() {
    let (_h, mut morning) = fixture(); // 08:00, every day
    morning.hour = 8;
    let mut evening = Reminder::new(Uuid::new_v4());
    evening.hour = 21;

    // At 07:00 the next fire is this morning.
    let next = next_fire_after(&[morning.clone(), evening.clone()], at(7, 0)).unwrap();
    assert_eq!(next, at(8, 0));

    // At 09:00 the morning one has passed, so the evening one is next.
    let next = next_fire_after(&[morning.clone(), evening.clone()], at(9, 0)).unwrap();
    assert_eq!(next, at(21, 0));

    // After the last one, it rolls to tomorrow's earliest.
    let next = next_fire_after(&[morning, evening], at(22, 0)).unwrap();
    assert_eq!(next.date(), at(8, 0).date().succ_opt().unwrap());
    assert_eq!(next.time(), NaiveTime::from_hms_opt(8, 0, 0).unwrap());
}

#[test]
fn next_fire_skips_to_the_next_selected_weekday() {
    let (_h, mut reminder) = fixture();
    // Mondays only; 2026-09-11 is a Friday, so the next is Monday the 14th.
    reminder.days = MONDAY;
    reminder.hour = 6;

    let next = next_fire_after(&[reminder], at(12, 0)).unwrap();
    assert_eq!(next.date(), NaiveDate::from_ymd_opt(2026, 9, 14).unwrap());
    assert_eq!(next.time(), NaiveTime::from_hms_opt(6, 0, 0).unwrap());
}

#[test]
fn next_fire_is_none_without_an_enabled_reminder() {
    let (_h, mut reminder) = fixture();
    assert!(next_fire_after(&[], at(9, 0)).is_none(), "no reminders");

    reminder.enabled = false;
    assert!(
        next_fire_after(&[reminder], at(9, 0)).is_none(),
        "disabled reminders must not schedule an alarm"
    );
}

#[test]
fn next_fire_is_strictly_after_now() {
    let (_h, reminder) = fixture(); // 08:00
    // Exactly on the boundary must roll forward, or the alarm would re-fire
    // immediately in a loop.
    let next = next_fire_after(&[reminder], at(8, 0)).unwrap();
    assert_eq!(next.date(), at(8, 0).date().succ_opt().unwrap());
}

// ------------------------------------------------------- recurrence kinds

use habit_stack_rs::model::ScheduleKind;

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

/// 2026-09-11, the Friday the other tests are anchored to.
fn friday() -> NaiveDate {
    day(2026, 9, 11)
}

#[test]
fn every_n_days_counts_from_its_anchor() {
    let (_h, mut reminder) = fixture();
    reminder.kind = ScheduleKind::EveryNDays;
    reminder.interval_days = 3;
    reminder.start_date = friday();

    assert!(reminder.occurs_on(friday()), "the anchor day itself fires");
    assert!(!reminder.occurs_on(day(2026, 9, 12)));
    assert!(!reminder.occurs_on(day(2026, 9, 13)));
    assert!(reminder.occurs_on(day(2026, 9, 14)), "three days on");
    assert!(reminder.occurs_on(day(2026, 9, 17)), "and three more");

    // Nothing before the anchor, even on a day the interval would land on.
    assert!(!reminder.occurs_on(day(2026, 9, 8)));
    assert!(!reminder.occurs_on(day(2026, 9, 5)));
}

#[test]
fn an_interval_of_one_is_every_day() {
    let (_h, mut reminder) = fixture();
    reminder.kind = ScheduleKind::EveryNDays;
    reminder.interval_days = 1;
    reminder.start_date = friday();

    for offset in 0..5 {
        assert!(reminder.occurs_on(friday() + chrono::Duration::days(offset)));
    }
}

#[test]
fn monthly_clamps_to_the_last_day_of_short_months() {
    let (_h, mut reminder) = fixture();
    reminder.kind = ScheduleKind::Monthly;
    reminder.day_of_month = 31;

    // 2027 is not a leap year, 2028 is.
    assert!(reminder.occurs_on(day(2027, 2, 28)), "clamps to 28 Feb");
    assert!(!reminder.occurs_on(day(2027, 2, 27)));
    assert!(reminder.occurs_on(day(2028, 2, 29)), "clamps to 29 Feb");
    assert!(!reminder.occurs_on(day(2028, 2, 28)), "not the 28th in a leap year");
    assert!(reminder.occurs_on(day(2027, 4, 30)), "clamps to 30 April");
    assert!(reminder.occurs_on(day(2027, 5, 31)), "31 exists in May");
    assert!(!reminder.occurs_on(day(2027, 5, 30)));
}

#[test]
fn monthly_fires_exactly_once_per_month() {
    let (_h, mut reminder) = fixture();
    reminder.kind = ScheduleKind::Monthly;
    reminder.day_of_month = 31;

    // February 2027 has 28 days and must contain exactly one occurrence.
    let hits = (1..=28)
        .filter(|d| reminder.occurs_on(day(2027, 2, *d)))
        .count();
    assert_eq!(hits, 1);
}

#[test]
fn a_one_off_fires_on_its_date_only() {
    let (_h, mut reminder) = fixture();
    reminder.kind = ScheduleKind::Once;
    reminder.start_date = friday();

    assert!(reminder.occurs_on(friday()));
    assert!(!reminder.occurs_on(day(2026, 9, 10)));
    assert!(!reminder.occurs_on(day(2026, 9, 12)));
    // Same weekday a week later must not re-fire.
    assert!(!reminder.occurs_on(day(2026, 9, 18)));
}

#[test]
fn a_one_off_stops_arming_once_it_has_passed() {
    let (_h, mut reminder) = fixture();
    reminder.kind = ScheduleKind::Once;
    reminder.start_date = friday();
    reminder.hour = 8;

    assert_eq!(
        next_fire_after(&[reminder.clone()], at(7, 0)),
        Some(friday().and_time(NaiveTime::from_hms_opt(8, 0, 0).unwrap())),
    );
    assert!(
        next_fire_after(&[reminder], at(9, 0)).is_none(),
        "a spent one-off must not arm another alarm"
    );
}

#[test]
fn a_one_off_years_out_still_arms() {
    let (_h, mut reminder) = fixture();
    reminder.kind = ScheduleKind::Once;
    // Well past the forward-scan horizon, so this only passes because `Once`
    // is answered directly rather than by walking the calendar.
    reminder.start_date = day(2031, 1, 20);
    reminder.hour = 8;

    let next = next_fire_after(&[reminder], at(9, 0)).unwrap();
    assert_eq!(next.date(), day(2031, 1, 20));
}

#[test]
fn next_fire_picks_the_earliest_across_mixed_kinds() {
    let weekly = {
        let mut r = Reminder::new(Uuid::new_v4());
        r.hour = 23; // today, late
        r
    };
    let interval = {
        let mut r = Reminder::new(Uuid::new_v4());
        r.kind = ScheduleKind::EveryNDays;
        r.interval_days = 2;
        r.start_date = friday();
        r.hour = 20;
        r
    };
    let monthly = {
        let mut r = Reminder::new(Uuid::new_v4());
        r.kind = ScheduleKind::Monthly;
        r.day_of_month = 30;
        r.hour = 6;
        r
    };
    let once = {
        let mut r = Reminder::new(Uuid::new_v4());
        r.kind = ScheduleKind::Once;
        r.start_date = day(2026, 9, 13);
        r.hour = 5;
        r
    };

    // At 12:00 on Friday the 11th: the interval one fires at 20:00 today,
    // before the weekly 23:00, the one-off on the 13th and the 30th monthly.
    let next = next_fire_after(&[weekly, interval, monthly, once], at(12, 0)).unwrap();
    assert_eq!(next, friday().and_time(NaiveTime::from_hms_opt(20, 0, 0).unwrap()));
}

#[test]
fn a_disabled_reminder_of_any_kind_is_skipped() {
    let (_h, mut reminder) = fixture();
    reminder.kind = ScheduleKind::Monthly;
    reminder.day_of_month = 11; // would fire on the fixture's Friday
    reminder.enabled = false;

    assert!(next_fire_after(&[reminder], at(1, 0)).is_none());
}

#[test]
fn due_now_respects_a_non_weekly_schedule() {
    let (habit, mut reminder) = fixture();
    reminder.kind = ScheduleKind::EveryNDays;
    reminder.interval_days = 2;
    reminder.start_date = friday();
    reminder.hour = 8;

    assert_eq!(due(&reminder, &habit, &[], at(9, 0)), 1, "anchor day");

    // The next day is off-schedule, so nothing is due however late it gets.
    let tomorrow = day(2026, 9, 12).and_time(NaiveTime::from_hms_opt(23, 0, 0).unwrap());
    let done = HashSet::new();
    assert!(due_now(&[reminder], &[habit], &done, tomorrow).is_empty());
}
