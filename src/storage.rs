use crate::model::{Habit, HabitEntry, HabitStatus, Reminder, ScheduleKind};
use chrono::NaiveDate;
#[cfg(not(target_os = "android"))]
use directories::ProjectDirs;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Set once from `android_main`, before any storage call.
#[cfg(target_os = "android")]
static ANDROID_DATA_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Records the activity's internal data directory as the home for `habits.db`.
#[cfg(target_os = "android")]
pub fn set_android_data_dir(dir: PathBuf) {
    let _ = ANDROID_DATA_DIR.set(dir);
}

pub struct Storage {
    conn: Connection,
}

impl Storage {
    pub fn app_db_path() -> anyhow::Result<PathBuf> {
        let dir = Self::app_data_dir()?;
        std::fs::create_dir_all(&dir)?;
        Ok(dir.join("habits.db"))
    }

    #[cfg(not(target_os = "android"))]
    fn app_data_dir() -> anyhow::Result<PathBuf> {
        let proj = ProjectDirs::from("dev", "HabitStack", "habit_stack_rs")
            .ok_or_else(|| anyhow::anyhow!("Could not determine app data directory"))?;
        Ok(proj.data_dir().to_path_buf())
    }

    /// On Android `directories` has no writable location to offer, so the path
    /// comes from the activity instead - see `set_android_data_dir`.
    #[cfg(target_os = "android")]
    fn app_data_dir() -> anyhow::Result<PathBuf> {
        ANDROID_DATA_DIR
            .get()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Android data dir not set; call set_android_data_dir"))
    }

    pub fn new_with_default_path() -> anyhow::Result<Self> {
        let path = Self::app_db_path()?;
        Self::new(&path)
    }

    pub fn new(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        let storage = Self { conn };
        storage.init_schema()?;
        Ok(storage)
    }

    fn init_schema(&self) -> anyhow::Result<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS habits (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                color TEXT,
                created_at TEXT NOT NULL,
                archived INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS habit_entries (
                id TEXT PRIMARY KEY,
                habit_id TEXT NOT NULL REFERENCES habits(id) ON DELETE CASCADE,
                date TEXT NOT NULL,
                status TEXT NOT NULL,
                note TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_entries_habit_date
                ON habit_entries (habit_id, date);

            CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS habit_reminders (
                habit_id TEXT PRIMARY KEY
                    REFERENCES habits(id) ON DELETE CASCADE,
                time TEXT NOT NULL,
                days INTEGER NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                last_fired TEXT
            );
            "#,
        )?;
        self.migrate()
    }

    /// Schema upgrades, applied in order and recorded in `PRAGMA user_version`.
    ///
    /// The `CREATE TABLE` batch above deliberately stays frozen at the v0
    /// shape: a fresh database is created at version 0 and then migrated, so
    /// new and existing installs converge on exactly the same columns. Adding
    /// a column to both places instead would make the `ALTER` fail as a
    /// duplicate on first run.
    fn migrate(&self) -> anyhow::Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if version < 1 {
            self.conn.execute_batch(
                r#"
                ALTER TABLE habits ADD COLUMN emoji TEXT;
                ALTER TABLE habit_reminders
                    ADD COLUMN kind TEXT NOT NULL DEFAULT 'weekly';
                ALTER TABLE habit_reminders
                    ADD COLUMN interval_days INTEGER NOT NULL DEFAULT 1;
                ALTER TABLE habit_reminders
                    ADD COLUMN day_of_month INTEGER NOT NULL DEFAULT 1;
                ALTER TABLE habit_reminders ADD COLUMN start_date TEXT;
                PRAGMA user_version = 1;
                "#,
            )?;
        }

        Ok(())
    }

    pub fn create_habit(&self, habit: &Habit) -> anyhow::Result<()> {
        self.conn.execute(
            r#"INSERT INTO habits (id, name, color, emoji, created_at, archived)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6)"#,
            params![
                habit.id.to_string(),
                habit.name,
                habit.color,
                habit.emoji,
                habit.created_at.to_string(),
                if habit.archived { 1 } else { 0 },
            ],
        )?;
        Ok(())
    }

    pub fn list_habits(&self, include_archived: bool) -> anyhow::Result<Vec<Habit>> {
        let mut stmt = if include_archived {
            self.conn
                .prepare("SELECT id, name, color, emoji, created_at, archived FROM habits")?
        } else {
            self.conn.prepare(
                "SELECT id, name, color, emoji, created_at, archived FROM habits WHERE archived = 0",
            )?
        };

        let rows = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            let id = Uuid::parse_str(&id_str).map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
            let name: String = row.get(1)?;
            let color: Option<String> = row.get(2)?;
            let emoji: Option<String> = row.get(3)?;
            let created_at_str: String = row.get(4)?;
            let created_at = NaiveDate::parse_from_str(&created_at_str, "%Y-%m-%d")
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?;
            let archived_int: i64 = row.get(5)?;
            let archived = archived_int != 0;
            Ok(Habit {
                id,
                name,
                color,
                emoji: emoji.filter(|e| !e.is_empty()),
                created_at,
                archived,
            })
        })?;

        let mut habits = Vec::new();
        for h in rows {
            habits.push(h?);
        }
        Ok(habits)
    }

    /// Deletes the habit outright. Its entries and reminder go with it through
    /// `ON DELETE CASCADE`, which `foreign_keys = ON` in `init_schema` enables.
    pub fn delete_habit(&self, habit_id: Uuid) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM habits WHERE id = ?1",
            params![habit_id.to_string()],
        )?;
        Ok(())
    }

    pub fn upsert_entry(
        &self,
        habit_id: Uuid,
        date: NaiveDate,
        status: HabitStatus,
        note: Option<String>,
    ) -> anyhow::Result<()> {
        let existing_id: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM habit_entries WHERE habit_id = ?1 AND date = ?2",
                params![habit_id.to_string(), date.to_string()],
                |row| row.get(0),
            )
            .optional()?;

        let entry_id = if let Some(id_str) = existing_id {
            let id = Uuid::parse_str(&id_str)?;
            self.conn.execute(
                "UPDATE habit_entries SET status = ?1, note = ?2 WHERE id = ?3",
                params![status.as_str(), note, id.to_string()],
            )?;
            id
        } else {
            let entry = HabitEntry::new(habit_id, date, status);
            self.conn.execute(
                r#"INSERT INTO habit_entries (id, habit_id, date, status, note)
                   VALUES (?1, ?2, ?3, ?4, ?5)"#,
                params![
                    entry.id.to_string(),
                    entry.habit_id.to_string(),
                    entry.date.to_string(),
                    entry.status.as_str(),
                    entry.note,
                ],
            )?;
            entry.id
        };

        let _ = entry_id;
        Ok(())
    }

    pub fn update_habit_color(&self, habit_id: Uuid, hex: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE habits SET color = ?1 WHERE id = ?2",
            params![hex, habit_id.to_string()],
        )?;
        Ok(())
    }

    /// `None` clears the emoji, restoring the plain colour dot.
    pub fn update_habit_emoji(&self, habit_id: Uuid, emoji: Option<&str>) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE habits SET emoji = ?1 WHERE id = ?2",
            params![emoji, habit_id.to_string()],
        )?;
        Ok(())
    }

    pub fn list_reminders(&self) -> anyhow::Result<Vec<Reminder>> {
        let mut stmt = self.conn.prepare(
            "SELECT habit_id, time, days, enabled, last_fired,
                    kind, interval_days, day_of_month, start_date
             FROM habit_reminders",
        )?;
        let rows = stmt.query_map([], |row| {
            let habit_id_str: String = row.get(0)?;
            let habit_id = Uuid::parse_str(&habit_id_str).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            let time_str: String = row.get(1)?;
            let (hour, minute) = Reminder::parse_time(&time_str).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    "invalid reminder time".into(),
                )
            })?;
            let days: i64 = row.get(2)?;
            let enabled: i64 = row.get(3)?;
            let last_fired: Option<String> = row.get(4)?;
            let kind: String = row.get(5)?;
            let interval_days: i64 = row.get(6)?;
            let day_of_month: i64 = row.get(7)?;
            let start_date: Option<String> = row.get(8)?;
            Ok(Reminder {
                habit_id,
                hour,
                minute,
                days: days as u8,
                kind: ScheduleKind::parse(&kind).unwrap_or(ScheduleKind::Weekly),
                interval_days: (interval_days.max(1)) as u32,
                day_of_month: (day_of_month.clamp(1, 31)) as u32,
                // Rows migrated from v0 have no anchor; today is the only
                // sensible one, and it is unread until the kind changes.
                start_date: start_date
                    .and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok())
                    .unwrap_or_else(|| chrono::Local::now().date_naive()),
                enabled: enabled != 0,
                last_fired: last_fired
                    .and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()),
            })
        })?;

        let mut reminders = Vec::new();
        for r in rows {
            reminders.push(r?);
        }
        Ok(reminders)
    }

    pub fn upsert_reminder(&self, reminder: &Reminder) -> anyhow::Result<()> {
        self.conn.execute(
            r#"INSERT INTO habit_reminders
                   (habit_id, time, days, enabled, last_fired,
                    kind, interval_days, day_of_month, start_date)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
               ON CONFLICT(habit_id) DO UPDATE SET
                   time = excluded.time,
                   days = excluded.days,
                   enabled = excluded.enabled,
                   last_fired = excluded.last_fired,
                   kind = excluded.kind,
                   interval_days = excluded.interval_days,
                   day_of_month = excluded.day_of_month,
                   start_date = excluded.start_date"#,
            params![
                reminder.habit_id.to_string(),
                reminder.time_string(),
                reminder.days as i64,
                if reminder.enabled { 1 } else { 0 },
                reminder.last_fired.map(|d| d.to_string()),
                reminder.kind.as_str(),
                reminder.interval_days as i64,
                reminder.day_of_month as i64,
                reminder.start_date.to_string(),
            ],
        )?;
        Ok(())
    }

    pub fn clear_reminder(&self, habit_id: Uuid) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM habit_reminders WHERE habit_id = ?1",
            params![habit_id.to_string()],
        )?;
        Ok(())
    }

    pub fn mark_reminder_fired(&self, habit_id: Uuid, date: NaiveDate) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE habit_reminders SET last_fired = ?1 WHERE habit_id = ?2",
            params![date.to_string(), habit_id.to_string()],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> anyhow::Result<Option<String>> {
        let value = self
            .conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> anyhow::Result<()> {
        self.conn.execute(
            r#"INSERT INTO app_settings (key, value) VALUES (?1, ?2)
               ON CONFLICT(key) DO UPDATE SET value = excluded.value"#,
            params![key, value],
        )?;
        Ok(())
    }

    pub fn entries_for_period(
        &self,
        habit_ids: &[Uuid],
        start: NaiveDate,
        end: NaiveDate,
    ) -> anyhow::Result<Vec<HabitEntry>> {
        if habit_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut placeholders = String::new();
        for (idx, _) in habit_ids.iter().enumerate() {
            if idx > 0 {
                placeholders.push_str(", ");
            }
            placeholders.push('?');
            placeholders.push_str(&(idx + 1).to_string());
        }

        let sql = format!(
            "SELECT id, habit_id, date, status, note
             FROM habit_entries
             WHERE habit_id IN ({})
               AND date >= ?
               AND date <= ?",
            placeholders
        );
        // rusqlite needs params as a slice; assemble dynamically.
        let mut params_vec: Vec<String> = habit_ids.iter().map(|id| id.to_string()).collect();
        params_vec.push(start.to_string());
        params_vec.push(end.to_string());

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(params_vec.iter()),
            |row| {
                let id_str: String = row.get(0)?;
                let id = Uuid::parse_str(&id_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
                let habit_id_str: String = row.get(1)?;
                let habit_id = Uuid::parse_str(&habit_id_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e)))?;
                let date_str: String = row.get(2)?;
                let date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e)))?;
                let status_str: String = row.get(3)?;
                let status =
                    HabitStatus::from_str(&status_str).ok_or_else(|| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            "invalid status".into(),
                        )
                    })?;
                let note: Option<String> = row.get(4)?;
                Ok(HabitEntry {
                    id,
                    habit_id,
                    date,
                    status,
                    note,
                })
            },
        )?;

        let mut entries = Vec::new();
        for e in rows {
            entries.push(e?);
        }
        Ok(entries)
    }
}

