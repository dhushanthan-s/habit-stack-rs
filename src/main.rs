pub mod model;
pub mod storage;
pub mod view_model;

use crate::view_model::{habit_color, AppState, HABIT_PALETTE};
use slint::{Color, ModelRc, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

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
fn summary_line(stats: &view_model::Stats) -> String {
    let head = if stats.streak > 0 {
        format!("{} streak", plural(stats.streak, "day"))
    } else if stats.best_streak > 0 {
        format!("No streak · best {}", stats.best_streak)
    } else {
        "No streak yet".to_string()
    };
    format!("{head} · {}% · {} done", stats.rate, stats.total_done)
}

fn weeks_model(weeks: &[view_model::WeekColumn]) -> ModelRc<GraphWeek> {
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
            let streak = state.current_streak(view_model::Scope::Habit(h.id));
            HabitItem {
                id: h.id.to_string().into(),
                name: h.name.clone().into(),
                color: color_of(habit_color(h)),
                done_today: state.done_today(h.id),
                streak_label: if streak > 0 {
                    format!("{} streak", plural(streak, "day")).into()
                } else {
                    "No streak yet".into()
                },
                created_label: format!("Since {}", h.created_at.format("%-d %b %Y")).into(),
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
}

fn main() {
    let storage = storage::Storage::new_with_default_path().expect("Failed to open storage");
    let state = AppState::new(storage).expect("Failed to init app state");

    let app = AppWindow::new().expect("Failed to create app window");

    app.set_palette(ModelRc::from(Rc::new(VecModel::from(
        HABIT_PALETTE.iter().copied().map(color_of).collect::<Vec<_>>(),
    ))));
    app.global::<Theme>().set_dark(state.dark_theme);

    push_state(&app, &state);

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

    on!(on_theme_changed, |state, dark| {
        let _ = state.set_dark_theme(dark);
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

    app.run().expect("Failed to run app");
}
