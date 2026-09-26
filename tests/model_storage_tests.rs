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


// ------------------------------------------------------- emoji & schedules

use habit_stack_rs::model::{Reminder, ScheduleKind};
use rusqlite::Connection;

#[test]
fn a_habits_emoji_round_trips_and_can_be_cleared() {
    let tmp = NamedTempFile::new().unwrap();
    let storage = Storage::new(tmp.path()).unwrap();

    let habit = Habit::new("Hydrate", None).with_emoji(Some("\u{1F4A7}".to_string()));
    let id = habit.id;
    storage.create_habit(&habit).unwrap();
    assert_eq!(
        storage.list_habits(false).unwrap()[0].emoji.as_deref(),
        Some("\u{1F4A7}")
    );

    storage.update_habit_emoji(id, Some("\u{1F3C3}")).unwrap();
    assert_eq!(
        storage.list_habits(false).unwrap()[0].emoji.as_deref(),
        Some("\u{1F3C3}")
    );

    storage.update_habit_emoji(id, None).unwrap();
    assert_eq!(storage.list_habits(false).unwrap()[0].emoji, None);
}

#[test]
fn every_schedule_field_survives_a_round_trip() {
    let tmp = NamedTempFile::new().unwrap();
    let storage = Storage::new(tmp.path()).unwrap();

    let habit = Habit::new("Rent", None);
    storage.create_habit(&habit).unwrap();

    let mut reminder = Reminder::new(habit.id);
    reminder.kind = ScheduleKind::Monthly;
    reminder.day_of_month = 28;
    reminder.interval_days = 9;
    reminder.start_date = NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
    reminder.hour = 17;
    reminder.minute = 45;
    storage.upsert_reminder(&reminder).unwrap();

    let read_back = storage.list_reminders().unwrap();
    assert_eq!(read_back.len(), 1);
    assert_eq!(read_back[0], reminder);

    // Upserting again must update in place rather than duplicate.
    reminder.kind = ScheduleKind::Once;
    storage.upsert_reminder(&reminder).unwrap();
    let read_back = storage.list_reminders().unwrap();
    assert_eq!(read_back.len(), 1);
    assert_eq!(read_back[0].kind, ScheduleKind::Once);
}

#[test]
fn deleting_a_habit_cascades_to_its_entries_and_reminder() {
    let tmp = NamedTempFile::new().unwrap();
    let storage = Storage::new(tmp.path()).unwrap();

    let doomed = Habit::new("Doomed", None);
    let kept = Habit::new("Kept", None);
    storage.create_habit(&doomed).unwrap();
    storage.create_habit(&kept).unwrap();

    let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
    for habit in [&doomed, &kept] {
        storage
            .upsert_entry(habit.id, date, HabitStatus::Done, None)
            .unwrap();
        storage.upsert_reminder(&Reminder::new(habit.id)).unwrap();
    }

    storage.delete_habit(doomed.id).unwrap();

    let habits = storage.list_habits(true).unwrap();
    assert_eq!(habits.len(), 1, "gone outright, not archived");
    assert_eq!(habits[0].id, kept.id);

    let entries = storage
        .entries_for_period(&[doomed.id, kept.id], date, date)
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].habit_id, kept.id);

    let reminders = storage.list_reminders().unwrap();
    assert_eq!(reminders.len(), 1);
    assert_eq!(reminders[0].habit_id, kept.id);
}

/// Writes a database in the pre-migration shape, then opens it through
/// `Storage` and checks the upgrade lands without losing the existing rows.
#[test]
fn a_v0_database_migrates_to_the_current_schema() {
    let tmp = NamedTempFile::new().unwrap();
    let habit_id = uuid::Uuid::new_v4();

    {
        let conn = Connection::open(tmp.path()).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE habits (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                color TEXT,
                created_at TEXT NOT NULL,
                archived INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE habit_reminders (
                habit_id TEXT PRIMARY KEY REFERENCES habits(id) ON DELETE CASCADE,
                time TEXT NOT NULL,
                days INTEGER NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                last_fired TEXT
            );
            "#,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO habits (id, name, color, created_at, archived)
             VALUES (?1, 'Legacy', '#2DD4BF', '2026-01-01', 0)",
            [habit_id.to_string()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO habit_reminders (habit_id, time, days, enabled, last_fired)
             VALUES (?1, '07:30', 5, 1, NULL)",
            [habit_id.to_string()],
        )
        .unwrap();
    }

    let storage = Storage::new(tmp.path()).unwrap();

    let habits = storage.list_habits(false).unwrap();
    assert_eq!(habits.len(), 1, "the existing habit survives");
    assert_eq!(habits[0].name, "Legacy");
    assert_eq!(habits[0].emoji, None, "the new column defaults to empty");

    let reminders = storage.list_reminders().unwrap();
    assert_eq!(reminders.len(), 1);
    assert_eq!(reminders[0].kind, ScheduleKind::Weekly, "defaults to weekly");
    assert_eq!(reminders[0].days, 5, "the old bitmask is untouched");
    assert_eq!(reminders[0].hour, 7);
    assert_eq!(reminders[0].interval_days, 1);
    assert_eq!(reminders[0].day_of_month, 1);

    let version: i64 = Connection::open(tmp.path())
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);

    // Reopening must be a no-op rather than re-running the ALTERs.
    assert!(Storage::new(tmp.path()).is_ok(), "migration is idempotent");
}

#[test]
fn a_fresh_database_lands_on_the_same_schema_as_a_migrated_one() {
    let tmp = NamedTempFile::new().unwrap();
    Storage::new(tmp.path()).unwrap();

    let conn = Connection::open(tmp.path()).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);

    let columns: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('habit_reminders')")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for expected in ["kind", "interval_days", "day_of_month", "start_date"] {
        assert!(columns.contains(&expected.to_string()), "missing {expected}");
    }
}
