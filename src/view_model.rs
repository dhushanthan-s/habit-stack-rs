use crate::model::{Habit, HabitEntry, HabitStatus};
use crate::storage::Storage;
use chrono::{Datelike, Duration, NaiveDate, Utc};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct DayCell {
    pub date: NaiveDate,
    pub status: Option<HabitStatus>,
    pub intensity: f32,
    pub has_note: bool,
}

#[derive(Debug, Clone)]
pub struct WeekColumn {
    pub start_date: NaiveDate,
    pub days: Vec<DayCell>, // always 7, Mon–Sun
}

pub struct AppState {
    pub storage: Storage,
    pub habits: Vec<Habit>,
    pub entries: Vec<HabitEntry>,
    pub graph_weeks: Vec<WeekColumn>,
    pub selected_date: Option<NaiveDate>,
    pub current_habit_id: Option<Uuid>,
    pub today: NaiveDate,
}

impl AppState {
    pub fn new(storage: Storage) -> anyhow::Result<Self> {
        let mut state = Self {
            storage,
            habits: Vec::new(),
            entries: Vec::new(),
            graph_weeks: Vec::new(),
            selected_date: None,
            current_habit_id: None,
            today: Utc::now().date_naive(),
        };
        state.initial_load()?;
        Ok(state)
    }

    fn initial_load(&mut self) -> anyhow::Result<()> {
        self.habits = self.storage.list_habits(false)?;
        if self.habits.is_empty() {
            // Create a default habit so the graph is immediately useful.
            let default = Habit::new("Daily Check-in", None);
            self.storage.create_habit(&default)?;
            self.habits.push(default);
        }
        // Select the first habit by default.
        self.current_habit_id = self.habits.first().map(|h| h.id);
        self.refresh_for_current_habit(self.default_range_end(), 12)?;
        Ok(())
    }

    fn default_range_end(&self) -> NaiveDate {
        Utc::now().date_naive()
    }

    fn range_for_weeks_ending(at: NaiveDate, weeks: i64) -> (NaiveDate, NaiveDate) {
        let end = at;
        let start = end - Duration::days(7 * weeks - 1);
        (start, end)
    }

    pub fn refresh_for_current_habit(
        &mut self,
        end: NaiveDate,
        weeks: i64,
    ) -> anyhow::Result<()> {
        let Some(habit_id) = self.current_habit_id else {
            self.entries.clear();
            self.graph_weeks.clear();
            return Ok(());
        };

        let (start, end) = Self::range_for_weeks_ending(end, weeks);
        self.entries = self
            .storage
            .entries_for_period(&[habit_id], start, end)?;
        self.graph_weeks = Self::build_weeks(start, end, &self.entries);
        Ok(())
    }

    fn build_weeks(start: NaiveDate, end: NaiveDate, entries: &[HabitEntry]) -> Vec<WeekColumn> {
        // Index entries per day to compute status and intensity.
        let mut per_day: HashMap<NaiveDate, Vec<&HabitEntry>> = HashMap::new();
        for e in entries {
            per_day.entry(e.date).or_default().push(e);
        }

        // Ensure start is on a Monday to get GitHub-like calendar alignment.
        let mut cursor = start;
        while cursor.weekday().num_days_from_monday() != 0 {
            cursor = cursor - Duration::days(1);
        }

        let mut weeks = Vec::new();
        while cursor <= end {
            let week_start = cursor;
            let mut days = Vec::with_capacity(7);
            for offset in 0..7 {
                let date = week_start + Duration::days(offset);
                let entries_for_day = per_day.get(&date);
                let (status, intensity, has_note) = if let Some(entries) = entries_for_day {
                    let mut done_count = 0usize;
                    let mut any_note = false;
                    for e in entries {
                        if e.status == HabitStatus::Done {
                            done_count += 1;
                        }
                        if e.note.as_ref().map(|s| !s.is_empty()).unwrap_or(false) {
                            any_note = true;
                        }
                    }
                    let total = entries.len().max(1) as f32;
                    let intensity = (done_count as f32 / total).clamp(0.0, 1.0);
                    let status = if done_count > 0 {
                        Some(HabitStatus::Done)
                    } else {
                        // If all are Skipped/Missed we can pick based on first for now.
                        entries.first().map(|e| e.status)
                    };
                    (status, intensity, any_note)
                } else {
                    (None, 0.0, false)
                };

                days.push(DayCell {
                    date,
                    status,
                    intensity,
                    has_note,
                });
            }
            weeks.push(WeekColumn {
                start_date: week_start,
                days,
            });
            cursor = week_start + Duration::days(7);
        }
        weeks
    }

    pub fn set_current_habit(&mut self, habit_id: Uuid) -> anyhow::Result<()> {
        if self.habits.iter().any(|h| h.id == habit_id) {
            self.current_habit_id = Some(habit_id);
            self.refresh_for_current_habit(self.default_range_end(), 12)?;
        }
        Ok(())
    }

    pub fn toggle_today_for_habit(&mut self, habit_id: Uuid) -> anyhow::Result<()> {
        let date = self.today;
        let existing = self
            .entries
            .iter()
            .find(|e| e.habit_id == habit_id && e.date == date)
            .cloned();
        let new_status = match existing {
            Some(e) if e.status == HabitStatus::Done => HabitStatus::Missed,
            _ => HabitStatus::Done,
        };
        self.storage
            .upsert_entry(habit_id, date, new_status, None)?;
        // ensure current habit is the toggled one
        self.current_habit_id = Some(habit_id);
        self.refresh_for_current_habit(self.default_range_end(), 12)?;
        Ok(())
    }

    pub fn add_habit(&mut self, name: String) -> anyhow::Result<Uuid> {
        let habit = Habit::new(name, None);
        let id = habit.id;
        self.storage.create_habit(&habit)?;
        self.habits = self.storage.list_habits(false)?;
        self.current_habit_id = Some(id);
        self.refresh_for_current_habit(self.default_range_end(), 12)?;
        Ok(id)
    }
}

