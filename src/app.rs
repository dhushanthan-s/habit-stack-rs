//! Slint wiring: builds the UI models from `AppState` and owns the callbacks.

use crate::model::{Reminder, ScheduleKind};
use crate::view_model::{
    habit_color, hex_of, AppState, ReminderTarget, ThemeMode, WeekStart, EMOJI_PALETTE,
    HABIT_PALETTE,
};
use chrono::Local;
use slint::{Color, ModelRc, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// How often the reminder tick runs. Deliberately short: Slint timers run on a
/// monotonic clock that pauses while the machine sleeps, so one long timer to
/// "08:00" would drift by the sleep duration. A frequent tick comparing local
/// wall-clock time is immune to that, and to App Nap throttling.
const TICK: Duration = Duration::from_secs(30);

slint::include_modules!();

fn color_of(rgb: (u8, u8, u8)) -> Color {
    Color::from_rgb_u8(rgb.0, rgb.1, rgb.2)
}

fn plural(count: u32, unit: &str) -> String {
    if count == 1 {
        format!("{count} {unit}")
    } else {
        format!("{count} {unit}s")
    }
}

/// Builds a habit's stats line, e.g. "12 day streak · 86% · 24 done". Falls
/// back to the best-ever streak when the current one is zero, so a lapsed
/// streak is still acknowledged.
fn summary_line(stats: &crate::view_model::Stats) -> String {
    let head = if stats.streak > 0 {
        format!("{} streak", plural(stats.streak, "day"))
    } else if stats.best_streak > 0 {
        format!("No streak · best {}", stats.best_streak)
    } else {
        "No streak yet".to_string()
    };
    format!("{head} · {}% · {} done", stats.rate, stats.total_done)
}

/// Monday-first weekday names matching the stored bitmask.
const DAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// "Mon, Wed, Fri", "Every day", or "Weekends".
fn weekday_summary(days: u8) -> String {
    if days == crate::model::ALL_DAYS {
        return "Every day".to_string();
    }
    let picked: Vec<&str> = (0..7)
        .filter(|d| days & (1 << d) != 0)
        .map(|d| DAY_NAMES[d as usize])
        .collect();
    if picked.is_empty() {
        "No days selected".to_string()
    } else {
        picked.join(", ")
    }
}

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// One human line describing the whole schedule, e.g.
/// "Mon, Wed, Fri at 07:00". Built here so the markup does no formatting.
fn schedule_summary(reminder: &Reminder, today: chrono::NaiveDate) -> String {
    // `when` stays a bare phrase so the time always lands right after it; any
    // caveat is appended afterwards rather than swallowed mid-sentence.
    let when = match reminder.kind {
        ScheduleKind::Weekly => weekday_summary(reminder.days),
        ScheduleKind::EveryNDays => {
            if reminder.interval_days == 1 {
                "Every day".to_string()
            } else {
                format!("Every {} days", reminder.interval_days)
            }
        }
        ScheduleKind::Monthly => {
            format!("The {} of each month", ordinal(reminder.day_of_month))
        }
        ScheduleKind::Once => {
            format!("Once on {}", reminder.start_date.format("%-d %b %Y"))
        }
    };

    let caveat = match reminder.kind {
        ScheduleKind::Monthly if reminder.day_of_month > 28 => {
            " \u{2014} shorter months use their last day"
        }
        ScheduleKind::Once if reminder.start_date < today => " \u{2014} already passed",
        _ => "",
    };

    format!("{when} at {}{caveat}", reminder.label())
}

/// What the reminder editor shows. `None` is a habit with no reminder yet:
/// switched off, with empty labels.
fn reminder_view(reminder: Option<&Reminder>, today: chrono::NaiveDate) -> ReminderView {
    let Some(r) = reminder else {
        return ReminderView {
            days: ModelRc::from(Rc::new(VecModel::from(vec![false; 7]))),
            ..ReminderView::default()
        };
    };
    ReminderView {
        on: r.enabled,
        time_label: r.label().into(),
        days: ModelRc::from(Rc::new(VecModel::from(
            (0..7).map(|d| r.days & (1 << d) != 0).collect::<Vec<bool>>(),
        ))),
        kind: r.kind.as_index(),
        summary: schedule_summary(r, today).into(),
        interval_label: if r.interval_days == 1 {
            "1 day".to_string()
        } else {
            format!("{} days", r.interval_days)
        }
        .into(),
        month_day_label: ordinal(r.day_of_month).into(),
        date_label: r.start_date.format("%-d %b %Y").to_string().into(),
    }
}

/// The add form edits its draft reminder under an empty id; anything else
/// names a saved habit.
fn reminder_target(id: &str) -> Option<ReminderTarget> {
    if id.is_empty() {
        Some(ReminderTarget::Draft)
    } else {
        uuid::Uuid::parse_str(id).ok().map(ReminderTarget::Habit)
    }
}

fn weeks_model(weeks: &[crate::view_model::WeekColumn]) -> ModelRc<GraphWeek> {
    ModelRc::from(Rc::new(VecModel::from(
        weeks
            .iter()
            .map(|week| GraphWeek {
                month_label: week.month_label.clone().into(),
                days: ModelRc::from(Rc::new(VecModel::from(
                    week.days
                        .iter()
                        .map(|d| GraphDay {
                            date: d.date.to_string().into(),
                            done: d.done,
                            skipped: d.skipped,
                            is_today: d.is_today,
                            is_future: d.is_future,
                            has_note: d.has_note,
                        })
                        .collect::<Vec<_>>(),
                ))),
            })
            .collect::<Vec<_>>(),
    )))
}

/// Pushes the whole of `state` into the window. Every callback funnels through
/// here so the UI can never show a partially refreshed view.
fn push_state(app: &AppWindow, state: &AppState) {
    // One block per habit on the Graph page.
    let graphs: Vec<HabitGraph> = state
        .graphs
        .iter()
        .filter_map(|graph| {
            let habit = state.habits.iter().find(|h| h.id == graph.habit_id)?;
            Some(HabitGraph {
                id: habit.id.to_string().into(),
                name: habit.name.clone().into(),
                emoji: habit.emoji.clone().unwrap_or_default().into(),
                color: color_of(habit_color(habit)),
                weeks: weeks_model(&graph.weeks),
                summary: summary_line(&graph.stats).into(),
            })
        })
        .collect();
    app.set_habit_graphs(ModelRc::from(Rc::new(VecModel::from(graphs))));

    let habits: Vec<HabitItem> = state
        .habits
        .iter()
        .map(|h| {
            let streak = state.current_streak(crate::view_model::Scope::Habit(h.id));
            let reminder = state.reminder_for(h.id);
            let rgb = habit_color(h);
            // The picker highlights by index, so resolve the stored hex back
            // to one; a colour derived from the UUID matches nothing and
            // reports -1.
            let color_index = h
                .color
                .as_deref()
                .and_then(|hex| {
                    HABIT_PALETTE
                        .iter()
                        .position(|tone| hex_of(*tone).eq_ignore_ascii_case(hex))
                })
                .map_or(-1, |i| i as i32);

            HabitItem {
                id: h.id.to_string().into(),
                name: h.name.clone().into(),
                emoji: h.emoji.clone().unwrap_or_default().into(),
                color: color_of(rgb),
                color_index,
                done_today: state.done_today(h.id),
                streak_label: {
                    let streak_text = if streak > 0 {
                        format!("{} streak", plural(streak, "day"))
                    } else {
                        "No streak yet".to_string()
                    };
                    match reminder.filter(|r| r.enabled) {
                        Some(r) => format!("{} · {}", r.label(), streak_text).into(),
                        None => streak_text.into(),
                    }
                },
                created_label: format!("Since {}", h.created_at.format("%-d %b %Y")).into(),
                due: state.due.iter().any(|d| d.habit_id == h.id),
                reminder: reminder_view(reminder, state.today),
            }
        })
        .collect();
    app.set_habits(ModelRc::from(Rc::new(VecModel::from(habits))));
    app.set_new_reminder(reminder_view(Some(&state.draft_reminder), state.today));

    app.set_totals(HabitStats {
        streak: state.totals.streak.to_string().into(),
        rate: format!("{}%", state.totals.rate).into(),
        done: state.totals.total_done.to_string().into(),
        range_label: format!(
            "{} – {}",
            state.window.0.format("%-d %b"),
            state.window.1.format("%-d %b")
        )
        .into(),
    });

    app.set_summary_label(
        format!(
            "{} · {} – {}",
            plural(state.habits.len() as u32, "habit"),
            state.window.0.format("%-d %b"),
            state.window.1.format("%-d %b")
        )
        .into(),
    );

    app.set_accent(color_of(state.accent()));
    app.set_today_label(state.today.format("%A, %-d %B").to_string().into());

    // Reminder surfacing.
    app.set_due_count(state.due.len() as i32);
    app.set_due_label(match state.due.len() {
        0 => String::new(),
        1 => {
            let due = &state.due[0];
            if due.emoji.is_empty() {
                format!("{} is due now", due.habit_name)
            } else {
                format!("{} {} is due now", due.emoji, due.habit_name)
            }
        }
        n => format!("{} due now", plural(n as u32, "habit")),
    }
    .into());

    // Settings.
    app.set_theme_mode(state.theme_mode.as_index());
    app.set_accent_index(state.accent_index.map_or(-1, |i| i as i32));
    app.set_graph_weeks(state.graph_weeks as i32);
    app.set_week_start_sunday(state.week_start == WeekStart::Sunday);
    app.global::<Theme>().set_mode(state.theme_mode.as_index());

    app.set_weekday_labels(ModelRc::from(Rc::new(VecModel::from(
        state
            .week_start
            .labels()
            .iter()
            .map(|l| (*l).into())
            .collect::<Vec<slint::SharedString>>(),
    ))));
}

/// Resets the whole add form, not just the name: the next habit should not
/// silently inherit the last one's colour, emoji, or reminder.
fn reset_add_form(app: &AppWindow, state: &mut AppState) {
    state.reset_draft_reminder();
    push_state(app, state);
    app.set_new_habit_name("".into());
    app.set_new_color_index(0);
    app.set_new_emoji_index(-1);
}

/// Boots the UI and runs the event loop. Shared by the desktop binary and the
/// Android `android_main` entry point.
/// Set once the window exists, so the Android inset callback can reach it from
/// the JNI thread. `slint::Weak` is Send + Sync by design.
#[cfg(target_os = "android")]
pub(crate) static WINDOW: std::sync::OnceLock<slint::Weak<AppWindow>> = std::sync::OnceLock::new();

pub fn run() {
    let storage = crate::storage::Storage::new_with_default_path().expect("Failed to open storage");
    let state = AppState::new(storage).expect("Failed to init app state");

    let app = AppWindow::new().expect("Failed to create app window");

    app.set_palette(ModelRc::from(Rc::new(VecModel::from(
        HABIT_PALETTE.iter().copied().map(color_of).collect::<Vec<_>>(),
    ))));
    app.set_emoji_palette(ModelRc::from(Rc::new(VecModel::from(
        EMOJI_PALETTE
            .iter()
            .map(|e| (*e).into())
            .collect::<Vec<slint::SharedString>>(),
    ))));
    // No emoji until one is picked.
    app.set_new_emoji_index(-1);
    // Reminder day labels are always Monday-indexed, matching the stored
    // bitmask, regardless of which day the graph starts its weeks on.
    app.set_day_labels(ModelRc::from(Rc::new(VecModel::from(
        ["M", "T", "W", "T", "F", "S", "S"]
            .iter()
            .map(|l| (*l).into())
            .collect::<Vec<slint::SharedString>>(),
    ))));

    push_state(&app, &state);

    #[cfg(target_os = "android")]
    let _ = WINDOW.set(app.as_weak());

    let shared = Rc::new(RefCell::new(state));
    let weak = app.as_weak();

    // Each callback mutates state, then re-pushes everything.
    macro_rules! on {
        ($setter:ident, |$state:ident $(, $arg:ident)*| $body:block) => {
            app.$setter({
                let weak = weak.clone();
                let shared = shared.clone();
                move |$($arg),*| {
                    let Some(app) = weak.upgrade() else { return };
                    let mut $state = shared.borrow_mut();
                    $body
                    push_state(&app, &$state);
                }
            });
        };
    }

    on!(on_toggle_today, |state, id| {
        if let Ok(id) = uuid::Uuid::parse_str(&id) {
            let _ = state.toggle_today_for_habit(id);
        }
    });

    on!(on_set_habit_color, |state, id, color_index| {
        if let Ok(id) = uuid::Uuid::parse_str(&id) {
            let _ = state.set_habit_color(id, color_index.max(0) as usize);
        }
    });

    on!(on_set_habit_emoji, |state, id, emoji_index| {
        if let Ok(id) = uuid::Uuid::parse_str(&id) {
            let picked = (emoji_index >= 0).then(|| emoji_index as usize);
            let _ = state.set_habit_emoji(id, picked);
        }
    });

    on!(on_delete_habit, |state, id| {
        if let Ok(id) = uuid::Uuid::parse_str(&id) {
            let _ = state.delete_habit(id);
        }
    });

    on!(on_reminder_toggled, |state, id, on| {
        if let Some(target) = reminder_target(&id) {
            let _ = state.set_reminder_enabled(target, on);
        }
    });

    on!(on_reminder_shift, |state, id, hours, minutes| {
        if let Some(target) = reminder_target(&id) {
            let _ = state.shift_reminder_time(target, hours, minutes);
        }
    });

    on!(on_reminder_day_toggled, |state, id, day| {
        if let (Some(target), true) = (reminder_target(&id), day >= 0) {
            let _ = state.toggle_reminder_day(target, day as u32);
        }
    });

    on!(on_reminder_kind_picked, |state, id, kind| {
        if let Some(target) = reminder_target(&id) {
            let _ = state.set_reminder_kind(target, ScheduleKind::from_index(kind));
        }
    });

    on!(on_reminder_interval_shift, |state, id, delta| {
        if let Some(target) = reminder_target(&id) {
            let _ = state.shift_reminder_interval(target, delta);
        }
    });

    on!(on_reminder_month_day_shift, |state, id, delta| {
        if let Some(target) = reminder_target(&id) {
            let _ = state.shift_reminder_month_day(target, delta);
        }
    });

    on!(on_reminder_date_shift, |state, id, days, months| {
        if let Some(target) = reminder_target(&id) {
            let _ = state.shift_reminder_date(target, days, months);
        }
    });

    on!(on_theme_mode_picked, |state, index| {
        let _ = state.set_theme_mode(ThemeMode::from_index(index));
    });

    on!(on_accent_picked, |state, index| {
        let _ = state.set_accent_index(if index < 0 { None } else { Some(index as usize) });
    });

    on!(on_graph_weeks_picked, |state, weeks| {
        let _ = state.set_graph_weeks(weeks as i64);
    });

    on!(on_week_start_picked, |state, sunday| {
        let _ = state.set_week_start(if sunday {
            WeekStart::Sunday
        } else {
            WeekStart::Monday
        });
    });

    // Clearing the fields is a UI concern, so these sit outside the macro.
    app.on_add_habit_requested({
        let weak = weak.clone();
        let shared = shared.clone();
        move |name, color_index, emoji_index| {
            let Some(app) = weak.upgrade() else { return };
            let mut state = shared.borrow_mut();
            let trimmed = name.trim().to_string();
            if !trimmed.is_empty() {
                let emoji = (emoji_index >= 0).then(|| emoji_index as usize);
                let reminder = state
                    .draft_reminder
                    .enabled
                    .then(|| state.draft_reminder.clone());
                let _ = state.add_habit(trimmed, color_index.max(0) as usize, emoji, reminder);
            }
            reset_add_form(&app, &mut state);
        }
    });

    app.on_add_cancelled({
        let weak = weak.clone();
        let shared = shared.clone();
        move || {
            let Some(app) = weak.upgrade() else { return };
            reset_add_form(&app, &mut shared.borrow_mut());
        }
    });

    // Bound to a local so it outlives `run()`: a dropped Timer silently stops.
    let tick = Timer::default();
    tick.start(TimerMode::Repeated, TICK, {
        let weak = weak.clone();
        let shared = shared.clone();
        move || {
            let Some(app) = weak.upgrade() else { return };
            // A UI callback may hold the borrow; skip rather than panic.
            let Ok(mut state) = shared.try_borrow_mut() else {
                return;
            };
            let rolled = state.sync_today().unwrap_or(false);
            let changed = state.recompute_due(Local::now().naive_local());
            if rolled || changed {
                push_state(&app, &state);
            }
        }
    });

    app.run().expect("Failed to run app");
}

#[cfg(test)]
mod tests {
    use super::{ordinal, schedule_summary, weekday_summary};
    use crate::model::{Reminder, ScheduleKind, ALL_DAYS};
    use chrono::NaiveDate;
    use uuid::Uuid;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn reminder(kind: ScheduleKind) -> Reminder {
        let mut r = Reminder::new(Uuid::new_v4());
        r.kind = kind;
        r.hour = 7;
        r.minute = 0;
        r
    }

    #[test]
    fn weekday_summaries_read_naturally() {
        assert_eq!(weekday_summary(ALL_DAYS), "Every day");
        assert_eq!(weekday_summary(0b000_0101), "Mon, Wed");
        assert_eq!(weekday_summary(0b110_0000), "Sat, Sun");
        assert_eq!(weekday_summary(0), "No days selected");
    }

    #[test]
    fn ordinals_handle_the_teens() {
        let got: Vec<String> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 23, 31]
            .iter()
            .map(|n| ordinal(*n))
            .collect();
        assert_eq!(
            got,
            [
                "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "23rd", "31st"
            ]
        );
    }

    #[test]
    fn the_time_always_follows_the_recurrence_phrase() {
        let today = day(2026, 9, 11);

        let mut weekly = reminder(ScheduleKind::Weekly);
        weekly.days = 0b000_0101;
        assert_eq!(schedule_summary(&weekly, today), "Mon, Wed at 07:00");

        let mut interval = reminder(ScheduleKind::EveryNDays);
        interval.interval_days = 3;
        assert_eq!(schedule_summary(&interval, today), "Every 3 days at 07:00");

        let mut monthly = reminder(ScheduleKind::Monthly);
        monthly.day_of_month = 15;
        assert_eq!(
            schedule_summary(&monthly, today),
            "The 15th of each month at 07:00"
        );
    }

    #[test]
    fn caveats_are_appended_after_the_time_not_inside_the_phrase() {
        let today = day(2026, 9, 11);

        let mut monthly = reminder(ScheduleKind::Monthly);
        monthly.day_of_month = 31;
        assert_eq!(
            schedule_summary(&monthly, today),
            "The 31st of each month at 07:00 \u{2014} shorter months use their last day"
        );

        let mut passed = reminder(ScheduleKind::Once);
        passed.start_date = day(2026, 9, 1);
        assert_eq!(
            schedule_summary(&passed, today),
            "Once on 1 Sep 2026 at 07:00 \u{2014} already passed"
        );

        let mut upcoming = reminder(ScheduleKind::Once);
        upcoming.start_date = day(2026, 12, 25);
        assert_eq!(
            schedule_summary(&upcoming, today),
            "Once on 25 Dec 2026 at 07:00"
        );
    }
}
