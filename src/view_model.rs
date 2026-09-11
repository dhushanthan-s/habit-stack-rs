use crate::model::{Habit, HabitEntry, HabitStatus};
use crate::storage::Storage;
use chrono::{Datelike, Duration, NaiveDate, Utc};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Weeks of history shown in the contribution graph (~4 months).
pub const GRAPH_WEEKS: i64 = 17;

/// Accent colours offered to habits. The UI sends back an index into this
/// table, so hex strings never travel through the markup.
pub const HABIT_PALETTE: [(u8, u8, u8); 8] = [
    (0x2D, 0xD4, 0xBF), // teal
    (0xA7, 0x8B, 0xFA), // violet
    (0xFB, 0x92, 0x3C), // orange
    (0x38, 0xBD, 0xF8), // sky
    (0xF4, 0x72, 0xB6), // pink
    (0x4A, 0xDE, 0x80), // green
    (0xFB, 0xBF, 0x24), // amber
    (0xF8, 0x71, 0x71), // rose
];

pub const SETTING_DARK_THEME: &str = "dark_theme";

pub fn parse_hex(raw: &str) -> Option<(u8, u8, u8)> {
    let s = raw.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    Some((
        u8::from_str_radix(&s[0..2], 16).ok()?,
        u8::from_str_radix(&s[2..4], 16).ok()?,
        u8::from_str_radix(&s[4..6], 16).ok()?,
    ))
}

pub fn hex_of(rgb: (u8, u8, u8)) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb.0, rgb.1, rgb.2)
}

pub fn palette_hex(index: usize) -> String {
    hex_of(HABIT_PALETTE[index % HABIT_PALETTE.len()])
}

/// A habit's accent colour: whatever was stored, else a palette entry derived
/// from its UUID. Deriving rather than backfilling keeps colours stable across
/// sessions without needing a DB migration.
pub fn habit_color(habit: &Habit) -> (u8, u8, u8) {
    habit
        .color
        .as_deref()
        .and_then(parse_hex)
        .unwrap_or_else(|| {
            let bytes = habit.id.as_bytes();
            let idx = (bytes[0] as usize ^ bytes[15] as usize) % HABIT_PALETTE.len();
            HABIT_PALETTE[idx]
        })
}

/// What the graph and the stat blocks are currently describing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Every habit aggregated, so a day's level reflects how many were done.
    All,
    Habit(Uuid),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    pub streak: u32,
    pub best_streak: u32,
    pub rate: u8,
    pub total_done: u32,
}

#[derive(Debug, Clone)]
pub struct DayCell {
    pub date: NaiveDate,
    pub done: bool,
    pub skipped: bool,
    pub has_note: bool,
    pub is_today: bool,
    pub is_future: bool,
}

#[derive(Debug, Clone)]
pub struct WeekColumn {
    pub start_date: NaiveDate,
    /// Abbreviated month name when this week opens a month, else empty.
    pub month_label: String,
    pub days: Vec<DayCell>, // always 7, Mon–Sun
}

/// One block on the Graph page: a habit's grid plus its own stats.
#[derive(Debug, Clone)]
pub struct HabitGraph {
    pub habit_id: Uuid,
    pub weeks: Vec<WeekColumn>,
    pub stats: Stats,
}

pub struct AppState {
    pub storage: Storage,
    pub habits: Vec<Habit>,
    /// Entries for *every* habit across the graph window, so the Today tab can
    /// show per-habit state without another query.
    pub entries: Vec<HabitEntry>,
    /// One per habit, in `habits` order.
    pub graphs: Vec<HabitGraph>,
    /// Aggregated across every habit (`Scope::All`).
    pub totals: Stats,
    pub dark_theme: bool,
    pub today: NaiveDate,
    pub window: (NaiveDate, NaiveDate),
}

impl AppState {
    pub fn new(storage: Storage) -> anyhow::Result<Self> {
        let today = Utc::now().date_naive();
        let mut state = Self {
            storage,
            habits: Vec::new(),
            entries: Vec::new(),
            graphs: Vec::new(),
            totals: Stats::default(),
            dark_theme: true,
            today,
            window: Self::range_for_weeks_ending(today, GRAPH_WEEKS),
        };
        state.initial_load()?;
        Ok(state)
    }

    fn initial_load(&mut self) -> anyhow::Result<()> {
        self.habits = self.storage.list_habits(false)?;
        if self.habits.is_empty() {
            // Create a default habit so the graph is immediately useful.
            let default = Habit::new("Daily Check-in", Some(palette_hex(0)));
            self.storage.create_habit(&default)?;
            self.habits.push(default);
        }
        self.dark_theme = self
            .storage
            .get_setting(SETTING_DARK_THEME)?
            .map(|v| v == "1")
            .unwrap_or(true);
        self.refresh()?;
        Ok(())
    }

    fn range_for_weeks_ending(at: NaiveDate, weeks: i64) -> (NaiveDate, NaiveDate) {
        (at - Duration::days(7 * weeks - 1), at)
    }

    /// Reloads every habit's entries for the window, then rebuilds one graph
    /// per habit plus the aggregate totals.
    pub fn refresh(&mut self) -> anyhow::Result<()> {
        self.window = Self::range_for_weeks_ending(self.today, GRAPH_WEEKS);
        let (start, end) = self.window;
        let ids: Vec<Uuid> = self.habits.iter().map(|h| h.id).collect();
        self.entries = self.storage.entries_for_period(&ids, start, end)?;

        // Built into locals first: `stats` borrows self immutably.
        let graphs: Vec<HabitGraph> = self
            .habits
            .iter()
            .map(|habit| HabitGraph {
                habit_id: habit.id,
                weeks: Self::build_weeks(start, end, &self.entries, habit.id, self.today),
                stats: self.stats(Scope::Habit(habit.id)),
            })
            .collect();
        let totals = self.stats(Scope::All);

        self.graphs = graphs;
        self.totals = totals;
        Ok(())
    }

    fn build_weeks(
        start: NaiveDate,
        end: NaiveDate,
        entries: &[HabitEntry],
        habit_id: Uuid,
        today: NaiveDate,
    ) -> Vec<WeekColumn> {
        let mut per_day: HashMap<NaiveDate, &HabitEntry> = HashMap::new();
        for e in entries.iter().filter(|e| e.habit_id == habit_id) {
            per_day.insert(e.date, e);
        }

        // Snap back to Monday for GitHub-like calendar alignment.
        let mut cursor = start;
        while cursor.weekday().num_days_from_monday() != 0 {
            cursor -= Duration::days(1);
        }

        let mut weeks = Vec::new();
        while cursor <= end {
            let week_start = cursor;
            let mut days = Vec::with_capacity(7);
            let mut month_label = String::new();

            for offset in 0..7 {
                let date = week_start + Duration::days(offset);
                // One label per month: the 1st falls in exactly one week.
                if date.day() == 1 {
                    month_label = date.format("%b").to_string();
                }

                let entry = per_day.get(&date);
                let status = entry.map(|e| e.status);
                days.push(DayCell {
                    date,
                    done: status == Some(HabitStatus::Done),
                    skipped: status == Some(HabitStatus::Skipped),
                    has_note: entry
                        .and_then(|e| e.note.as_ref())
                        .is_some_and(|n| !n.is_empty()),
                    is_today: date == today,
                    is_future: date > today,
                });
            }

            weeks.push(WeekColumn {
                start_date: week_start,
                month_label,
                days,
            });
            cursor = week_start + Duration::days(7);
        }
        weeks
    }

    // ------------------------------------------------------------------ stats

    /// Dates that count as completed for a scope. Under `Scope::All` a day
    /// qualifies only once every habit that existed then was done, so habits
    /// added later do not retroactively spoil past days.
    fn done_dates(&self, scope: Scope) -> HashSet<NaiveDate> {
        match scope {
            Scope::Habit(id) => self
                .entries
                .iter()
                .filter(|e| e.habit_id == id && e.status == HabitStatus::Done)
                .map(|e| e.date)
                .collect(),
            Scope::All => {
                let mut per_day: HashMap<NaiveDate, HashSet<Uuid>> = HashMap::new();
                for e in &self.entries {
                    if e.status == HabitStatus::Done {
                        per_day.entry(e.date).or_default().insert(e.habit_id);
                    }
                }
                per_day
                    .into_iter()
                    .filter(|(date, done)| {
                        let active = self.habits.iter().filter(|h| h.created_at <= *date).count();
                        active > 0 && done.len() >= active
                    })
                    .map(|(date, _)| date)
                    .collect()
            }
        }
    }

    pub fn done_today(&self, habit_id: Uuid) -> bool {
        self.entries
            .iter()
            .any(|e| e.habit_id == habit_id && e.date == self.today && e.status == HabitStatus::Done)
    }

    /// Consecutive completed days ending today. Today counts as still in
    /// progress: if it is not logged yet the streak is measured to yesterday
    /// rather than reported as broken.
    pub fn current_streak(&self, scope: Scope) -> u32 {
        let done = self.done_dates(scope);
        let mut cursor = if done.contains(&self.today) {
            self.today
        } else {
            self.today - Duration::days(1)
        };
        let mut count = 0;
        while done.contains(&cursor) {
            count += 1;
            cursor -= Duration::days(1);
        }
        count
    }

    pub fn best_streak(&self, scope: Scope) -> u32 {
        let mut dates: Vec<NaiveDate> = self.done_dates(scope).into_iter().collect();
        dates.sort_unstable();
        let mut best = 0;
        let mut run = 0;
        let mut prev: Option<NaiveDate> = None;
        for date in dates {
            run = match prev {
                Some(p) if date == p + Duration::days(1) => run + 1,
                _ => 1,
            };
            best = best.max(run);
            prev = Some(date);
        }
        best
    }

    /// Percent of the tracked span completed. The span starts at the later of
    /// the graph window and the habit's creation date, so a new habit is not
    /// penalised for days before it existed.
    pub fn completion_rate(&self, scope: Scope) -> u8 {
        let (window_start, _) = self.window;
        let (done, total) = match scope {
            Scope::Habit(id) => {
                let Some(habit) = self.habits.iter().find(|h| h.id == id) else {
                    return 0;
                };
                let start = window_start.max(habit.created_at);
                let days = (self.today - start).num_days() + 1;
                let done = self
                    .entries
                    .iter()
                    .filter(|e| {
                        e.habit_id == id
                            && e.status == HabitStatus::Done
                            && e.date >= start
                            && e.date <= self.today
                    })
                    .count() as i64;
                (done, days)
            }
            Scope::All => {
                let earliest = self.habits.iter().map(|h| h.created_at).min();
                let Some(earliest) = earliest else { return 0 };
                let start = window_start.max(earliest);
                // Sum each habit's own eligible days rather than days × habits.
                let total: i64 = self
                    .habits
                    .iter()
                    .map(|h| {
                        let from = start.max(h.created_at);
                        ((self.today - from).num_days() + 1).max(0)
                    })
                    .sum();
                let done = self
                    .entries
                    .iter()
                    .filter(|e| {
                        e.status == HabitStatus::Done && e.date >= start && e.date <= self.today
                    })
                    .count() as i64;
                (done, total)
            }
        };
        if total <= 0 {
            return 0;
        }
        ((done * 100) / total).clamp(0, 100) as u8
    }

    pub fn total_done(&self, scope: Scope) -> u32 {
        self.entries
            .iter()
            .filter(|e| e.status == HabitStatus::Done)
            .filter(|e| match scope {
                Scope::Habit(id) => e.habit_id == id,
                Scope::All => true,
            })
            .count() as u32
    }

    pub fn stats(&self, scope: Scope) -> Stats {
        Stats {
            streak: self.current_streak(scope),
            best_streak: self.best_streak(scope),
            rate: self.completion_rate(scope),
            total_done: self.total_done(scope),
        }
    }

    // ----------------------------------------------------------------- intents

    pub fn toggle_today_for_habit(&mut self, habit_id: Uuid) -> anyhow::Result<()> {
        let date = self.today;
        let new_status = if self.done_today(habit_id) {
            HabitStatus::Missed
        } else {
            HabitStatus::Done
        };
        self.storage.upsert_entry(habit_id, date, new_status, None)?;
        self.refresh()?;
        Ok(())
    }

    pub fn add_habit(&mut self, name: String, color_index: usize) -> anyhow::Result<Uuid> {
        let habit = Habit::new(name, Some(palette_hex(color_index)));
        let id = habit.id;
        self.storage.create_habit(&habit)?;
        self.habits = self.storage.list_habits(false)?;
        self.refresh()?;
        Ok(id)
    }

    pub fn set_habit_color(&mut self, habit_id: Uuid, color_index: usize) -> anyhow::Result<()> {
        self.storage
            .update_habit_color(habit_id, &palette_hex(color_index))?;
        self.habits = self.storage.list_habits(false)?;
        self.refresh()
    }

    pub fn archive_habit(&mut self, habit_id: Uuid) -> anyhow::Result<()> {
        self.storage.archive_habit(habit_id)?;
        self.habits = self.storage.list_habits(false)?;
        self.refresh()
    }

    pub fn set_dark_theme(&mut self, dark: bool) -> anyhow::Result<()> {
        self.dark_theme = dark;
        self.storage
            .set_setting(SETTING_DARK_THEME, if dark { "1" } else { "0" })
    }

    /// Colour for app chrome (tab indicator, totals). Taken from the first
    /// habit, since nothing is "selected" any more.
    pub fn accent(&self) -> (u8, u8, u8) {
        self.habits
            .first()
            .map(habit_color)
            .unwrap_or(HABIT_PALETTE[0])
    }
}
