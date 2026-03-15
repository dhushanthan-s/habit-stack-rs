use habit_stack_rs::model::Habit;
use habit_stack_rs::storage::Storage;
use habit_stack_rs::view_model::AppState;
use tempfile::NamedTempFile;

#[test]
fn set_current_habit_updates_graph() {
    let tmp = NamedTempFile::new().unwrap();
    let storage = Storage::new(tmp.path()).unwrap();

    // create two habits
    let h1 = Habit::new("H1", None);
    let h2 = Habit::new("H2", None);
    storage.create_habit(&h1).unwrap();
    storage.create_habit(&h2).unwrap();

    let mut state = AppState::new(storage).unwrap();

    // current_habit_id should be set and graph_weeks computed
    assert!(state.current_habit_id.is_some());
    let initial_id = state.current_habit_id.unwrap();

    // switch to the second habit
    state.set_current_habit(h2.id).unwrap();
    assert_eq!(state.current_habit_id, Some(h2.id));
    assert!(!state.graph_weeks.is_empty());

    // switching back should also work
    state.set_current_habit(initial_id).unwrap();
    assert_eq!(state.current_habit_id, Some(initial_id));
}

