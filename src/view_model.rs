use crate::model::{Habit, HabitEntry, HabitStatus, Reminder, ScheduleKind};
use crate::reminders::{self, DueReminder};
use crate::storage::Storage;
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, Weekday};
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

/// Emoji offered to habits. Like `HABIT_PALETTE` the UI sends back an index,
/// with `None` meaning "no emoji, show the colour dot". A curated set rather
/// than free text: it renders the same on desktop and Android and needs no
/// grapheme-cluster validation.
pub const EMOJI_PALETTE: [&str; 24] = [
    "\u{1F3C3}", // runner
    "\u{1F4A7}", // droplet
    "\u{1F4DA}", // books
    "\u{1F9D8}", // meditation
    "\u{1F6CF}", // bed
    "\u{1F957}", // salad
    "\u{1F48A}", // pill
    "\u{1F3B8}", // guitar
    "\u{1F3CB}", // weightlifter
    "\u{1F6B6}", // walker
    "\u{1F9F9}", // broom
    "\u{270D}",  // writing hand
    "\u{1F3A8}", // palette
    "\u{2615}",  // coffee
    "\u{1F331}", // seedling
    "\u{1F4B0}", // money bag
    "\u{1F9E0}", // brain
    "\u{1F9B7}", // tooth
    "\u{1F6AD}", // no smoking
    "\u{1F4F5}", // no phones
    "\u{1F64F}", // folded hands
    "\u{1F415}", // dog
    "\u{1F3AF}", // target
    "\u{2B50}",  // star
];

/// Superseded by `SETTING_THEME_MODE`; still read once so an existing install
/// keeps the theme it was using.
pub const SETTING_DARK_THEME: &str = "dark_theme";
pub const SETTING_THEME_MODE: &str = "theme_mode";
pub const SETTING_ACCENT_INDEX: &str = "accent_index";
pub const SETTING_GRAPH_WEEKS: &str = "graph_weeks";
pub const SETTING_WEEK_START: &str = "week_start";

/// Selectable graph window lengths, in weeks.
pub const GRAPH_WEEK_CHOICES: [i64; 3] = [12, 17, 26];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
    System,
}

impl ThemeMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::System => "system",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            "system" => Some(Self::System),
            _ => None,
        }
    }

    /// Index the Slint side uses.
    pub fn as_index(&self) -> i32 {
        match self {
            Self::Dark => 0,
            Self::Light => 1,
            Self::System => 2,
        }
    }

    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Self::Light,
            2 => Self::System,
            _ => Self::Dark,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeekStart {
    Monday,
    Sunday,
}

impl WeekStart {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Monday => "mon",
            Self::Sunday => "sun",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "mon" => Some(Self::Monday),
            "sun" => Some(Self::Sunday),
            _ => None,
        }
    }

    /// Row index of `weekday` within a column that starts on this day.
    pub fn row_of(&self, weekday: Weekday) -> i64 {
        let from_monday = weekday.num_days_from_monday() as i64;
        match self {
            Self::Monday => from_monday,
            Self::Sunday => (from_monday + 1) % 7,
        }
    }

    /// Gutter captions, labelling Mon/Wed/Fri whichever day leads the column.
    pub fn labels(&self) -> [&'static str; 7] {
        match self {
            Self::Monday => ["Mon", "", "Wed", "", "Fri", "", ""],
            Self::Sunday => ["", "Mon", "", "Wed", "", "Fri", ""],
        }
    }
}

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

/// `None`, or an out-of-range index, means no emoji.
pub fn emoji_of(index: Option<usize>) -> Option<String> {
    index
        .and_then(|i| EMOJI_PALETTE.get(i))
        .map(|e| (*e).to_string())
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

/// Which reminder an edit applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReminderTarget {
    /// The add form's unsaved reminder, stored once the habit is created.
    Draft,
    Habit(Uuid),
}

/// The add form's starting reminder: off, so a new habit gets none unless it
/// is switched on. The nil id is replaced when the habit is created.
fn blank_draft() -> Reminder {
    Reminder {
        enabled: false,
        ..Reminder::new(Uuid::nil())
    }
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
    pub reminders: Vec<Reminder>,
    /// Reminder being configured in the add form. Never in `reminders`, so it
    /// cannot surface as due.
    pub draft_reminder: Reminder,
    /// Recomputed each tick; drives the badges and banner.
    pub due: Vec<DueReminder>,
    pub theme_mode: ThemeMode,
    /// `None` = follow the first habit's colour.
    pub accent_index: Option<usize>,
    pub graph_weeks: i64,
    pub week_start: WeekStart,
    pub today: NaiveDate,
    pub window: (NaiveDate, NaiveDate),
}

impl AppState {
    pub fn new(storage: Storage) -> anyhow::Result<Self> {
        let today = Local::now().date_naive();
        let mut state = Self {
            storage,
            habits: Vec::new(),
            entries: Vec::new(),
            graphs: Vec::new(),
            totals: Stats::default(),
            reminders: Vec::new(),
            draft_reminder: blank_draft(),
            due: Vec::new(),
            theme_mode: ThemeMode::Dark,
            accent_index: None,
            graph_weeks: GRAPH_WEEKS,
            week_start: WeekStart::Monday,
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
        self.load_settings()?;
        self.refresh()?;
        Ok(())
    }

    fn load_settings(&mut self) -> anyhow::Result<()> {
        self.theme_mode = match self.storage.get_setting(SETTING_THEME_MODE)? {
            Some(raw) => ThemeMode::parse(&raw).unwrap_or(ThemeMode::Dark),
            // Carry over the old boolean key from before modes existed.
            None => match self.storage.get_setting(SETTING_DARK_THEME)?.as_deref() {
                Some("0") => ThemeMode::Light,
                _ => ThemeMode::Dark,
            },
        };
        self.accent_index = self
            .storage
            .get_setting(SETTING_ACCENT_INDEX)?
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|i| *i >= 0)
            .map(|i| i as usize % HABIT_PALETTE.len());
        self.graph_weeks = self
            .storage
            .get_setting(SETTING_GRAPH_WEEKS)?
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|w| GRAPH_WEEK_CHOICES.contains(w))
            .unwrap_or(GRAPH_WEEKS);
        self.week_start = self
            .storage
            .get_setting(SETTING_WEEK_START)?
            .and_then(|v| WeekStart::parse(&v))
            .unwrap_or(WeekStart::Monday);
        Ok(())
    }

    fn range_for_weeks_ending(at: NaiveDate, weeks: i64) -> (NaiveDate, NaiveDate) {
        (at - Duration::days(7 * weeks - 1), at)
    }

    /// Reloads every habit's entries for the window, then rebuilds one graph
    /// per habit plus the aggregate totals.
    pub fn refresh(&mut self) -> anyhow::Result<()> {
        self.window = Self::range_for_weeks_ending(self.today, self.graph_weeks);
        let (start, end) = self.window;
        let ids: Vec<Uuid> = self.habits.iter().map(|h| h.id).collect();
        self.entries = self.storage.entries_for_period(&ids, start, end)?;
        self.reminders = self.storage.list_reminders()?;

        // Built into locals first: `stats` borrows self immutably.
        let graphs: Vec<HabitGraph> = self
            .habits
            .iter()
            .map(|habit| HabitGraph {
                habit_id: habit.id,
                weeks: Self::build_weeks(
                    start,
                    end,
                    &self.entries,
                    habit.id,
                    self.today,
                    self.week_start,
                ),
                stats: self.stats(Scope::Habit(habit.id)),
            })
            .collect();
        let totals = self.stats(Scope::All);

        self.graphs = graphs;
        self.totals = totals;
        self.recompute_due(Local::now().naive_local());
        Ok(())
    }

    fn build_weeks(
        start: NaiveDate,
        end: NaiveDate,
        entries: &[HabitEntry],
        habit_id: Uuid,
        today: NaiveDate,
        week_start: WeekStart,
    ) -> Vec<WeekColumn> {
        let mut per_day: HashMap<NaiveDate, &HabitEntry> = HashMap::new();
        for e in entries.iter().filter(|e| e.habit_id == habit_id) {
            per_day.insert(e.date, e);
        }

        // Snap back to the configured first day of the week so columns align.
        let mut cursor = start;
        while week_start.row_of(cursor.weekday()) != 0 {
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

    /// `reminder` is saved against the new habit, so one configured in the add
    /// form arrives with it.
    pub fn add_habit(
        &mut self,
        name: String,
        color_index: usize,
        emoji_index: Option<usize>,
        reminder: Option<Reminder>,
    ) -> anyhow::Result<Uuid> {
        let habit =
            Habit::new(name, Some(palette_hex(color_index))).with_emoji(emoji_of(emoji_index));
        let id = habit.id;
        self.storage.create_habit(&habit)?;
        if let Some(reminder) = reminder {
            self.storage.upsert_reminder(&Reminder {
                habit_id: id,
                last_fired: None,
                ..reminder
            })?;
        }
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

    /// `None` clears the emoji and restores the colour dot.
    pub fn set_habit_emoji(
        &mut self,
        habit_id: Uuid,
        emoji_index: Option<usize>,
    ) -> anyhow::Result<()> {
        let emoji = emoji_of(emoji_index);
        self.storage
            .update_habit_emoji(habit_id, emoji.as_deref())?;
        self.habits = self.storage.list_habits(false)?;
        self.refresh()
    }

    /// Removes the habit for good, along with its entries and reminder.
    pub fn delete_habit(&mut self, habit_id: Uuid) -> anyhow::Result<()> {
        self.storage.delete_habit(habit_id)?;
        self.habits = self.storage.list_habits(false)?;
        self.refresh()
    }

    // ------------------------------------------------------------- settings

    pub fn set_theme_mode(&mut self, mode: ThemeMode) -> anyhow::Result<()> {
        self.theme_mode = mode;
        self.storage.set_setting(SETTING_THEME_MODE, mode.as_str())
    }

    /// `None` reverts to following the first habit's colour.
    pub fn set_accent_index(&mut self, index: Option<usize>) -> anyhow::Result<()> {
        self.accent_index = index.map(|i| i % HABIT_PALETTE.len());
        let raw = self.accent_index.map_or(-1, |i| i as i64).to_string();
        self.storage.set_setting(SETTING_ACCENT_INDEX, &raw)
    }

    pub fn set_graph_weeks(&mut self, weeks: i64) -> anyhow::Result<()> {
        if !GRAPH_WEEK_CHOICES.contains(&weeks) {
            return Ok(());
        }
        self.graph_weeks = weeks;
        self.storage
            .set_setting(SETTING_GRAPH_WEEKS, &weeks.to_string())?;
        self.refresh()
    }

    pub fn set_week_start(&mut self, week_start: WeekStart) -> anyhow::Result<()> {
        self.week_start = week_start;
        self.storage
            .set_setting(SETTING_WEEK_START, week_start.as_str())?;
        self.refresh()
    }

    // ------------------------------------------------------------ reminders

    pub fn reminder_for(&self, habit_id: Uuid) -> Option<&Reminder> {
        self.reminders.iter().find(|r| r.habit_id == habit_id)
    }

    /// Reads back the stored reminder, or a sensible unsaved default so the
    /// editor always has something to show.
    fn reminder_or_default(&self, habit_id: Uuid) -> Reminder {
        self.reminder_for(habit_id)
            .cloned()
            .unwrap_or_else(|| Reminder::new(habit_id))
    }

    fn save_reminder(&mut self, reminder: Reminder) -> anyhow::Result<()> {
        self.storage.upsert_reminder(&reminder)?;
        self.reminders = self.storage.list_reminders()?;
        self.recompute_due(Local::now().naive_local());
        Ok(())
    }

    /// Starts the add form's draft over: off, at the default schedule.
    pub fn reset_draft_reminder(&mut self) {
        self.draft_reminder = blank_draft();
    }

    /// Applies `edit` to the draft in memory, or to a habit's reminder, which
    /// is written back only if the edit changed something.
    fn edit_reminder(
        &mut self,
        target: ReminderTarget,
        edit: impl FnOnce(&mut Reminder) -> bool,
    ) -> anyhow::Result<()> {
        match target {
            ReminderTarget::Draft => {
                edit(&mut self.draft_reminder);
                Ok(())
            }
            ReminderTarget::Habit(habit_id) => {
                let mut reminder = self.reminder_or_default(habit_id);
                if edit(&mut reminder) {
                    self.save_reminder(reminder)
                } else {
                    Ok(())
                }
            }
        }
    }

    pub fn set_reminder_enabled(
        &mut self,
        target: ReminderTarget,
        enabled: bool,
    ) -> anyhow::Result<()> {
        if let ReminderTarget::Habit(habit_id) = target {
            if !enabled && self.reminder_for(habit_id).is_none() {
                return Ok(());
            }
        }
        self.edit_reminder(target, |r| {
            r.enabled = enabled;
            true
        })
    }

    pub fn shift_reminder_time(
        &mut self,
        target: ReminderTarget,
        hours: i32,
        minutes: i32,
    ) -> anyhow::Result<()> {
        self.edit_reminder(target, |r| r.shift_time(hours, minutes))
    }

    pub fn toggle_reminder_day(&mut self, target: ReminderTarget, day: u32) -> anyhow::Result<()> {
        self.edit_reminder(target, |r| r.toggle_day(day))
    }

    pub fn set_reminder_kind(
        &mut self,
        target: ReminderTarget,
        kind: ScheduleKind,
    ) -> anyhow::Result<()> {
        let today = self.today;
        self.edit_reminder(target, |r| r.set_kind(kind, today))
    }

    pub fn shift_reminder_interval(
        &mut self,
        target: ReminderTarget,
        delta: i32,
    ) -> anyhow::Result<()> {
        self.edit_reminder(target, |r| r.shift_interval(delta))
    }

    pub fn shift_reminder_month_day(
        &mut self,
        target: ReminderTarget,
        delta: i32,
    ) -> anyhow::Result<()> {
        self.edit_reminder(target, |r| r.shift_month_day(delta))
    }

    pub fn shift_reminder_date(
        &mut self,
        target: ReminderTarget,
        days: i32,
        months: i32,
    ) -> anyhow::Result<()> {
        self.edit_reminder(target, |r| r.shift_date(days, months))
    }

    fn done_today_set(&self) -> HashSet<Uuid> {
        self.habits
            .iter()
            .filter(|h| self.done_today(h.id))
            .map(|h| h.id)
            .collect()
    }

    /// Refreshes `due`; returns whether it changed, so the caller can avoid a
    /// pointless UI push on a quiet tick.
    pub fn recompute_due(&mut self, now: NaiveDateTime) -> bool {
        let done = self.done_today_set();
        let due = reminders::due_now(&self.reminders, &self.habits, &done, now);
        let changed = due != self.due;
        self.due = due;
        changed
    }

    /// Reminders that should be announced now, marking each fired so a restart
    /// or a throttled tick cannot announce it twice.
    pub fn take_announcements(&mut self, now: NaiveDateTime) -> anyhow::Result<Vec<DueReminder>> {
        let done = self.done_today_set();
        let pending = reminders::to_announce(&self.reminders, &self.habits, &done, now);
        for due in &pending {
            self.storage.mark_reminder_fired(due.habit_id, now.date())?;
            if let Some(r) = self
                .reminders
                .iter_mut()
                .find(|r| r.habit_id == due.habit_id)
            {
                r.last_fired = Some(now.date());
            }
        }
        Ok(pending)
    }

    /// Re-reads the local date, refreshing if it rolled over. Called from the
    /// tick, which is what keeps a long-running window honest.
    pub fn sync_today(&mut self) -> anyhow::Result<bool> {
        let now = Local::now().date_naive();
        if now == self.today {
            return Ok(false);
        }
        self.today = now;
        self.refresh()?;
        Ok(true)
    }

    /// Colour for app chrome (tab indicator, totals): the Settings override if
    /// one is set, else the first habit's colour.
    pub fn accent(&self) -> (u8, u8, u8) {
        if let Some(index) = self.accent_index {
            return HABIT_PALETTE[index % HABIT_PALETTE.len()];
        }
        self.habits
            .first()
            .map(habit_color)
            .unwrap_or(HABIT_PALETTE[0])
    }
}
