use chrono::{NaiveDate, Utc};
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
            created_at: Utc::now().date_naive(),
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

