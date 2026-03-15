use chrono::NaiveDate;
use habit_stack_rs::model::{Habit, HabitStatus};
use habit_stack_rs::storage::Storage;
use tempfile::NamedTempFile;

#[test]
fn create_and_list_habits() {
    let tmp = NamedTempFile::new().unwrap();
    let storage = Storage::new(tmp.path()).unwrap();

    let habit = Habit::new("Test Habit", None);
    storage.create_habit(&habit).unwrap();

    let habits = storage.list_habits(false).unwrap();
    assert_eq!(habits.len(), 1);
    assert_eq!(habits[0].name, "Test Habit");
}

#[test]
fn upsert_and_query_entries() {
    let tmp = NamedTempFile::new().unwrap();
    let storage = Storage::new(tmp.path()).unwrap();

    let habit = Habit::new("Test Habit", None);
    let habit_id = habit.id;
    storage.create_habit(&habit).unwrap();

    let date = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    storage
        .upsert_entry(habit_id, date, HabitStatus::Done, None)
        .unwrap();

    let entries = storage
        .entries_for_period(&[habit_id], date, date)
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].status, HabitStatus::Done);
}

