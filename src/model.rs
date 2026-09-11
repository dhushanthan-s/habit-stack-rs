use chrono::{Local, NaiveDate, NaiveTime, Weekday};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HabitStatus {
    Done,
    Skipped,
    Missed,
}

impl HabitStatus {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "done" => Some(Self::Done),
            "skipped" => Some(Self::Skipped),
            "missed" => Some(Self::Missed),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Skipped => "skipped",
            Self::Missed => "missed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Habit {
    pub id: Uuid,
    pub name: String,
    pub color: Option<String>,
    pub created_at: NaiveDate,
    pub archived: bool,
}

impl Habit {
    pub fn new(name: impl Into<String>, color: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            color,
            created_at: Local::now().date_naive(),
            archived: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HabitEntry {
    pub id: Uuid,
    pub habit_id: Uuid,
    pub date: NaiveDate,
    pub status: HabitStatus,
    pub note: Option<String>,
}

impl HabitEntry {
    pub fn new(habit_id: Uuid, date: NaiveDate, status: HabitStatus) -> Self {
        Self {
            id: Uuid::new_v4(),
            habit_id,
            date,
            status,
            note: None,
        }
    }
}


/// Every weekday selected.
pub const ALL_DAYS: u8 = 0b0111_1111;

/// A per-habit nudge at a local wall-clock time on selected weekdays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub habit_id: Uuid,
    pub hour: u32,
    pub minute: u32,
    /// Bit 0 = Monday … bit 6 = Sunday.
    pub days: u8,
    pub enabled: bool,
    /// Last date this reminder was announced, so a restart cannot re-announce it.
    pub last_fired: Option<NaiveDate>,
}

impl Reminder {
    pub fn new(habit_id: Uuid) -> Self {
        Self {
            habit_id,
            hour: 9,
            minute: 0,
            days: ALL_DAYS,
            enabled: true,
            last_fired: None,
        }
    }

    pub fn time_of_day(&self) -> NaiveTime {
        NaiveTime::from_hms_opt(self.hour, self.minute, 0).unwrap_or_default()
    }

    pub fn fires_on(&self, weekday: Weekday) -> bool {
        self.days & (1 << weekday.num_days_from_monday()) != 0
    }

    pub fn label(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// "HH:MM" as stored.
    pub fn time_string(&self) -> String {
        self.label()
    }

    pub fn parse_time(raw: &str) -> Option<(u32, u32)> {
        let (h, m) = raw.trim().split_once(':')?;
        let hour: u32 = h.parse().ok()?;
        let minute: u32 = m.parse().ok()?;
        (hour < 24 && minute < 60).then_some((hour, minute))
    }
}
