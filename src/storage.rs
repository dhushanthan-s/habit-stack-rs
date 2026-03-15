use crate::model::{Habit, HabitEntry, HabitStatus};
use chrono::NaiveDate;
use directories::ProjectDirs;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct Storage {
    conn: Connection,
}

impl Storage {
    pub fn app_db_path() -> anyhow::Result<PathBuf> {
        let proj = ProjectDirs::from("dev", "HabitStack", "habit_stack_rs")
            .ok_or_else(|| anyhow::anyhow!("Could not determine app data directory"))?;
        let dir = proj.data_dir();
        std::fs::create_dir_all(dir)?;
        Ok(dir.join("habits.db"))
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
            "#,
        )?;
        Ok(())
    }

    pub fn create_habit(&self, habit: &Habit) -> anyhow::Result<()> {
        self.conn.execute(
            r#"INSERT INTO habits (id, name, color, created_at, archived)
               VALUES (?1, ?2, ?3, ?4, ?5)"#,
            params![
                habit.id.to_string(),
                habit.name,
                habit.color,
                habit.created_at.to_string(),
                if habit.archived { 1 } else { 0 },
            ],
        )?;
        Ok(())
    }

    pub fn list_habits(&self, include_archived: bool) -> anyhow::Result<Vec<Habit>> {
        let mut stmt = if include_archived {
            self.conn
                .prepare("SELECT id, name, color, created_at, archived FROM habits")?
        } else {
            self.conn.prepare(
                "SELECT id, name, color, created_at, archived FROM habits WHERE archived = 0",
            )?
        };

        let rows = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            let id = Uuid::parse_str(&id_str).map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
            let name: String = row.get(1)?;
            let color: Option<String> = row.get(2)?;
            let created_at_str: String = row.get(3)?;
            let created_at = NaiveDate::parse_from_str(&created_at_str, "%Y-%m-%d")
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e)))?;
            let archived_int: i64 = row.get(4)?;
            let archived = archived_int != 0;
            Ok(Habit {
                id,
                name,
                color,
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

    pub fn archive_habit(&self, habit_id: Uuid) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE habits SET archived = 1 WHERE id = ?1",
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

