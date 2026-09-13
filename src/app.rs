//! Slint wiring: builds the UI models from `AppState` and owns the callbacks.

use crate::view_model::{habit_color, AppState, ThemeMode, WeekStart, HABIT_PALETTE};
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
            HabitItem {
                id: h.id.to_string().into(),
                name: h.name.clone().into(),
                color: color_of(habit_color(h)),
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
                reminder_on: reminder.map(|r| r.enabled).unwrap_or(false),
                reminder_label: reminder.map(|r| r.label()).unwrap_or_default().into(),
                reminder_days: ModelRc::from(Rc::new(VecModel::from(
                    (0..7)
                        .map(|d| reminder.map(|r| r.days & (1 << d) != 0).unwrap_or(false))
                        .collect::<Vec<bool>>(),
                ))),
            }
        })
        .collect();
    app.set_habits(ModelRc::from(Rc::new(VecModel::from(habits))));

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
        1 => format!("{} is due now", state.due[0].habit_name),
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

    on!(on_archive_habit, |state, id| {
        if let Ok(id) = uuid::Uuid::parse_str(&id) {
            let _ = state.archive_habit(id);
        }
    });

    on!(on_reminder_toggled, |state, id, on| {
        if let Ok(id) = uuid::Uuid::parse_str(&id) {
            let _ = state.set_reminder_enabled(id, on);
        }
    });

    on!(on_reminder_shift, |state, id, hours, minutes| {
        if let Ok(id) = uuid::Uuid::parse_str(&id) {
            let _ = state.shift_reminder_time(id, hours, minutes);
        }
    });

    on!(on_reminder_day_toggled, |state, id, day| {
        if let (Ok(id), true) = (uuid::Uuid::parse_str(&id), day >= 0) {
            let _ = state.toggle_reminder_day(id, day as u32);
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

    // Clearing the field is a UI concern, so it sits outside the macro.
    app.on_add_habit_requested({
        let weak = weak.clone();
        let shared = shared.clone();
        move |name, color_index| {
            let Some(app) = weak.upgrade() else { return };
            let trimmed = name.trim().to_string();
            if !trimmed.is_empty() {
                let mut state = shared.borrow_mut();
                let _ = state.add_habit(trimmed, color_index.max(0) as usize);
                push_state(&app, &state);
            }
            app.set_new_habit_name("".into());
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
