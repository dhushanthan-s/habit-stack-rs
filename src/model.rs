use chrono::{Datelike, Duration, Local, Months, NaiveDate, NaiveDateTime, NaiveTime};
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
    /// A single emoji shown wherever the habit appears, or `None` for the
    /// plain colour dot.
    pub emoji: Option<String>,
    pub created_at: NaiveDate,
    pub archived: bool,
}

impl Habit {
    pub fn new(name: impl Into<String>, color: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            color,
            emoji: None,
            created_at: Local::now().date_naive(),
            archived: false,
        }
    }

    pub fn with_emoji(mut self, emoji: Option<String>) -> Self {
        self.emoji = emoji;
        self
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

/// How far `next_occurrence_after` will scan forward. A little over a year, so
/// it always clears a monthly schedule; `Once` short-circuits instead of
/// scanning, so a far-future one-off is still found.
const SCAN_DAYS: i64 = 400;

/// Last day of the given month, e.g. 29 for February 2028.
fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .and_then(|first| first.pred_opt())
        .map_or(28, |last| last.day())
}

/// A day-of-month pinned to one that exists: "the 31st" means the 30th in
/// April and the 28th or 29th in February, so a monthly reminder never skips
/// a short month.
fn clamped_day(year: i32, month: u32, day_of_month: u32) -> u32 {
    day_of_month.clamp(1, days_in_month(year, month))
}

/// How a reminder repeats. Stored as a string so a future kind does not
/// renumber the existing rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleKind {
    /// Selected weekdays. "Daily" is simply all seven.
    Weekly,
    /// Every N days counted from `start_date`.
    EveryNDays,
    /// One day each month, clamped to the month's length.
    Monthly,
    /// Fires on `start_date` and never again.
    Once,
}

impl ScheduleKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Weekly => "weekly",
            Self::EveryNDays => "interval",
            Self::Monthly => "monthly",
            Self::Once => "once",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "weekly" => Some(Self::Weekly),
            "interval" => Some(Self::EveryNDays),
            "monthly" => Some(Self::Monthly),
            "once" => Some(Self::Once),
            _ => None,
        }
    }

    /// Index the Slint `Segmented` control uses.
    pub fn as_index(&self) -> i32 {
        match self {
            Self::Weekly => 0,
            Self::EveryNDays => 1,
            Self::Monthly => 2,
            Self::Once => 3,
        }
    }

    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Self::EveryNDays,
            2 => Self::Monthly,
            3 => Self::Once,
            _ => Self::Weekly,
        }
    }
}

/// A per-habit nudge at a local wall-clock time, on the days `kind` selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub habit_id: Uuid,
    pub hour: u32,
    pub minute: u32,
    /// Bit 0 = Monday … bit 6 = Sunday. Only read when `kind` is `Weekly`.
    pub days: u8,
    pub kind: ScheduleKind,
    /// Only read when `kind` is `EveryNDays`.
    pub interval_days: u32,
    /// 1..=31, only read when `kind` is `Monthly`.
    pub day_of_month: u32,
    /// The interval anchor, and the date itself for a `Once` reminder.
    pub start_date: NaiveDate,
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
            kind: ScheduleKind::Weekly,
            interval_days: 1,
            day_of_month: 1,
            start_date: Local::now().date_naive(),
            enabled: true,
            last_fired: None,
        }
    }

    pub fn time_of_day(&self) -> NaiveTime {
        NaiveTime::from_hms_opt(self.hour, self.minute, 0).unwrap_or_default()
    }

    /// Whether the schedule lands on `date`, ignoring the time of day.
    pub fn occurs_on(&self, date: NaiveDate) -> bool {
        match self.kind {
            ScheduleKind::Weekly => {
                self.days & (1 << date.weekday().num_days_from_monday()) != 0
            }
            ScheduleKind::EveryNDays => {
                if self.interval_days == 0 || date < self.start_date {
                    return false;
                }
                (date - self.start_date).num_days() % self.interval_days as i64 == 0
            }
            ScheduleKind::Monthly => {
                date.day() == clamped_day(date.year(), date.month(), self.day_of_month)
            }
            ScheduleKind::Once => date == self.start_date,
        }
    }

    /// The next moment this reminder fires, strictly after `now`.
    pub fn next_occurrence_after(&self, now: NaiveDateTime) -> Option<NaiveDateTime> {
        let time = self.time_of_day();

        // A one-off may sit years out, past any scan horizon, so answer it
        // directly rather than walking the calendar to it.
        if self.kind == ScheduleKind::Once {
            let at = self.start_date.and_time(time);
            return (at > now).then_some(at);
        }

        (0..=SCAN_DAYS).find_map(|offset| {
            let date = now.date() + Duration::days(offset);
            if !self.occurs_on(date) {
                return None;
            }
            let at = date.and_time(time);
            (at > now).then_some(at)
        })
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

    /// The day of the month this actually lands on in the month of `date`.
    pub fn effective_day_of_month(&self, date: NaiveDate) -> u32 {
        clamped_day(date.year(), date.month(), self.day_of_month)
    }

    // ------------------------------------------------------------------ edits
    //
    // Each returns whether anything changed, so a caller holding a saved
    // reminder can skip the write. Shared by saved reminders and the add
    // form's unsaved draft.

    /// Nudges the time by whole hours and/or minutes, wrapping within the day.
    pub fn shift_time(&mut self, hours: i32, minutes: i32) -> bool {
        let total = self.hour as i32 * 60 + self.minute as i32 + hours * 60 + minutes;
        let wrapped = total.rem_euclid(24 * 60);
        self.hour = (wrapped / 60) as u32;
        self.minute = (wrapped % 60) as u32;
        // Editing the time re-arms it for today.
        self.last_fired = None;
        true
    }

    /// `day` is 0 = Monday … 6 = Sunday. Refuses to clear the last day, since a
    /// reminder with no days would silently never fire.
    pub fn toggle_day(&mut self, day: u32) -> bool {
        if day > 6 {
            return false;
        }
        let next = self.days ^ (1u8 << day);
        if next == 0 {
            return false;
        }
        self.days = next;
        true
    }

    /// Switches how the reminder repeats. Interval and one-off schedules are
    /// re-anchored to `today`, so "every 3 days" starts counting now rather
    /// than from whenever the reminder happened to be created.
    pub fn set_kind(&mut self, kind: ScheduleKind, today: NaiveDate) -> bool {
        if self.kind == kind {
            return false;
        }
        self.kind = kind;
        if matches!(kind, ScheduleKind::EveryNDays | ScheduleKind::Once) {
            self.start_date = today;
        }
        self.last_fired = None;
        true
    }

    /// Nudges the "every N days" interval, clamped to a sane range.
    pub fn shift_interval(&mut self, delta: i32) -> bool {
        let next = (self.interval_days as i32 + delta).clamp(1, 365);
        if next as u32 == self.interval_days {
            return false;
        }
        self.interval_days = next as u32;
        self.last_fired = None;
        true
    }

    /// Nudges the monthly day-of-month. 31 is kept as 31 and clamped only when
    /// a short month is actually reached, so the setting survives February.
    pub fn shift_month_day(&mut self, delta: i32) -> bool {
        let next = (self.day_of_month as i32 + delta).clamp(1, 31);
        if next as u32 == self.day_of_month {
            return false;
        }
        self.day_of_month = next as u32;
        self.last_fired = None;
        true
    }

    /// Moves a one-off reminder's date by whole days and/or months.
    pub fn shift_date(&mut self, days: i32, months: i32) -> bool {
        let mut date = self.start_date + Duration::days(days as i64);
        // `checked_*_months` clamps to the month end, so 31 Jan + 1 month is
        // 28/29 Feb rather than an overflow.
        date = if months >= 0 {
            date.checked_add_months(Months::new(months as u32))
        } else {
            date.checked_sub_months(Months::new(months.unsigned_abs()))
        }
        .unwrap_or(date);

        if date == self.start_date {
            return false;
        }
        self.start_date = date;
        self.last_fired = None;
        true
    }
}
