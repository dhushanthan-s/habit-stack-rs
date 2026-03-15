pub mod model;
pub mod storage;
pub mod view_model;

use crate::view_model::AppState;
use chrono::NaiveDate;
use slint::{ModelRc, VecModel};
use std::rc::Rc;

slint::include_modules!();

fn main() {
    let storage = storage::Storage::new_with_default_path().expect("Failed to open storage");
    let mut state = AppState::new(storage).expect("Failed to init app state");

    let app = AppWindow::new().expect("Failed to create app window");

    // Helper to rebuild graph_weeks model from state.
    fn rebuild_graph_model(state: &AppState) -> Rc<VecModel<GraphWeek>> {
        Rc::new(VecModel::from(
            state
                .graph_weeks
                .iter()
                .map(|week| {
                    let days_model = week
                        .days
                        .iter()
                        .map(|d| GraphDay {
                            date: d.date.to_string().into(),
                            status: d
                                .status
                                .map(|s| s.as_str())
                                .unwrap_or("")
                                .into(),
                            intensity: d.intensity,
                            has_note: d.has_note,
                        })
                        .collect::<Vec<_>>();
                    GraphWeek {
                        start_label: week.start_date.format("%b %d").to_string().into(),
                        days: Rc::new(VecModel::from(days_model)).into(),
                    }
                })
                .collect::<Vec<_>>(),
        ))
    }

    // Helper to build habit chips model.
    fn build_habit_chips(state: &AppState) -> Rc<VecModel<HabitChip>> {
        Rc::new(VecModel::from(
            state
                .habits
                .iter()
                .map(|h| HabitChip {
                    id: h.id.to_string().into(),
                    name: h.name.clone().into(),
                    color: "".into(),
                    selected: Some(h.id) == state.current_habit_id,
                })
                .collect::<Vec<_>>(),
        ))
    }

    // Initial models.
    let graph_weeks_model = rebuild_graph_model(&state);
    app.set_graph_weeks(ModelRc::from(graph_weeks_model.clone()));
    let habits_model = build_habit_chips(&state);
    app.set_habits(ModelRc::from(habits_model.clone()));

    // Shared mutable state for callbacks.
    let shared_state = Rc::new(std::cell::RefCell::new(state));
    let app_weak = app.as_weak();

    // Tapping a day toggles today for current habit.
    app.on_day_tapped({
        let app_weak = app_weak.clone();
        let shared_state = shared_state.clone();
        move |_date_str| {
            if let Some(app) = app_weak.upgrade() {
                let mut state = shared_state.borrow_mut();
                let current = state
                    .current_habit_id
                    .or_else(|| state.habits.first().map(|h| h.id));
                if let Some(habit_id) = current {
                    if state.toggle_today_for_habit(habit_id).is_ok() {
                        let updated_graph = rebuild_graph_model(&state);
                        app.set_graph_weeks(ModelRc::from(updated_graph));
                        let updated_habits = build_habit_chips(&state);
                        app.set_habits(ModelRc::from(updated_habits));
                    }
                }
            }
        }
    });

    // Selecting a habit chip.
    app.on_habit_selected({
        let app_weak = app_weak.clone();
        let shared_state = shared_state.clone();
        move |id_str| {
            if let Some(app) = app_weak.upgrade() {
                if let Ok(id) = uuid::Uuid::parse_str(&id_str) {
                    let mut state = shared_state.borrow_mut();
                    if state.set_current_habit(id).is_ok() {
                        let updated_graph = rebuild_graph_model(&state);
                        app.set_graph_weeks(ModelRc::from(updated_graph));
                        let updated_habits = build_habit_chips(&state);
                        app.set_habits(ModelRc::from(updated_habits));
                    }
                }
            }
        }
    });

    // Toggling today from the checklist.
    app.on_toggle_today({
        let app_weak = app_weak.clone();
        let shared_state = shared_state.clone();
        move |id_str| {
            if let Some(app) = app_weak.upgrade() {
                if let Ok(id) = uuid::Uuid::parse_str(&id_str) {
                    let mut state = shared_state.borrow_mut();
                    if state.toggle_today_for_habit(id).is_ok() {
                        let updated_graph = rebuild_graph_model(&state);
                        app.set_graph_weeks(ModelRc::from(updated_graph));
                        let updated_habits = build_habit_chips(&state);
                        app.set_habits(ModelRc::from(updated_habits));
                    }
                }
            }
        }
    });

    // Adding a new habit.
    app.on_add_habit_requested({
        move |name| {
            if let Some(app) = app_weak.upgrade() {
                let mut state = shared_state.borrow_mut();
                if state.add_habit(name.to_string()).is_ok() {
                    let updated_graph = rebuild_graph_model(&state);
                    app.set_graph_weeks(ModelRc::from(updated_graph));
                    let updated_habits = build_habit_chips(&state);
                    app.set_habits(ModelRc::from(updated_habits));
                }
            }
        }
    });

    app.run().expect("Failed to run app");
}
