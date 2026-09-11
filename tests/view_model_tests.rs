use habit_stack_rs::model::Habit;
use habit_stack_rs::storage::Storage;
use habit_stack_rs::view_model::AppState;
use tempfile::NamedTempFile;

#[test]
fn graphs_are_built_per_habit_in_habit_order() {
    let tmp = NamedTempFile::new().unwrap();
    let storage = Storage::new(tmp.path()).unwrap();

    let h1 = Habit::new("H1", None);
    let h2 = Habit::new("H2", None);
    storage.create_habit(&h1).unwrap();
    storage.create_habit(&h2).unwrap();

    let state = AppState::new(storage).unwrap();

    // One block per habit, aligned index-for-index with `habits`.
    assert_eq!(state.graphs.len(), state.habits.len());
    assert_eq!(state.graphs.len(), 2);
    for (graph, habit) in state.graphs.iter().zip(state.habits.iter()) {
        assert_eq!(graph.habit_id, habit.id);
        assert!(!graph.weeks.is_empty());
    }
}

use chrono::{Duration, Local};
use habit_stack_rs::model::HabitStatus;
use habit_stack_rs::view_model::{
    habit_color, palette_hex, parse_hex, Scope, ThemeMode, WeekStart, HABIT_PALETTE,
};

/// Returns the tempfile alongside the state so the DB outlives the test body.
fn state_with(habits: &[&str]) -> (AppState, Vec<uuid::Uuid>, NamedTempFile) {
    let tmp = NamedTempFile::new().unwrap();
    let storage = Storage::new(tmp.path()).unwrap();
    let mut ids = Vec::new();
    for name in habits {
        let h = Habit::new(*name, None);
        ids.push(h.id);
        storage.create_habit(&h).unwrap();
    }
    (AppState::new(storage).unwrap(), ids, tmp)
}

fn mark(state: &mut AppState, id: uuid::Uuid, days_ago: i64, status: HabitStatus) {
    let date = Local::now().date_naive() - Duration::days(days_ago);
    state.storage.upsert_entry(id, date, status, None).unwrap();
}

#[test]
fn current_streak_counts_back_and_stops_at_a_gap() {
    let (mut state, ids, _db) = state_with(&["Run"]);
    let id = ids[0];
    // Done today, yesterday, 2 days ago; then a gap at 3; then 4 and 5.
    for d in [0, 1, 2, 4, 5] {
        mark(&mut state, id, d, HabitStatus::Done);
    }
    state.refresh().unwrap();

    assert_eq!(state.current_streak(Scope::Habit(id)), 3);
    // The longest run is the 3-day one ending today, not the 2-day one.
    assert_eq!(state.best_streak(Scope::Habit(id)), 3);
}

#[test]
fn today_not_logged_yet_does_not_break_the_streak() {
    let (mut state, ids, _db) = state_with(&["Run"]);
    let id = ids[0];
    for d in [1, 2, 3] {
        mark(&mut state, id, d, HabitStatus::Done);
    }
    state.refresh().unwrap();

    // Today is still in progress, so the streak is measured to yesterday.
    assert!(!state.done_today(id));
    assert_eq!(state.current_streak(Scope::Habit(id)), 3);
}

#[test]
fn best_streak_can_exceed_current_streak() {
    let (mut state, ids, _db) = state_with(&["Run"]);
    let id = ids[0];
    for d in [0, 3, 4, 5, 6, 7] {
        mark(&mut state, id, d, HabitStatus::Done);
    }
    state.refresh().unwrap();

    assert_eq!(state.current_streak(Scope::Habit(id)), 1);
    assert_eq!(state.best_streak(Scope::Habit(id)), 5);
}

#[test]
fn done_today_flips_with_the_toggle() {
    let (mut state, ids, _db) = state_with(&["Run"]);
    let id = ids[0];

    assert!(!state.done_today(id));
    state.toggle_today_for_habit(id).unwrap();
    assert!(state.done_today(id));
    state.toggle_today_for_habit(id).unwrap();
    assert!(!state.done_today(id));
}

#[test]
fn completion_rate_is_measured_from_the_creation_date() {
    let (mut state, ids, _db) = state_with(&["Run"]);
    let id = ids[0];
    // The habit was created today, so today is the only eligible day.
    state.toggle_today_for_habit(id).unwrap();
    assert_eq!(state.completion_rate(Scope::Habit(id)), 100);

    state.toggle_today_for_habit(id).unwrap();
    assert_eq!(state.completion_rate(Scope::Habit(id)), 0);
}

#[test]
fn aggregate_scope_requires_every_habit_for_a_perfect_day() {
    let (mut state, ids, _db) = state_with(&["Run", "Read"]);
    mark(&mut state, ids[0], 0, HabitStatus::Done);
    state.refresh().unwrap();

    // Only one of two habits done today, so no perfect-day streak.
    assert_eq!(state.current_streak(Scope::All), 0);

    mark(&mut state, ids[1], 0, HabitStatus::Done);
    state.refresh().unwrap();
    assert_eq!(state.current_streak(Scope::All), 1);
}

#[test]
fn each_graph_only_reflects_its_own_habit() {
    let (mut state, ids, _db) = state_with(&["A", "B"]);
    mark(&mut state, ids[0], 0, HabitStatus::Done);
    state.refresh().unwrap();

    let today = Local::now().date_naive();
    let cell_for = |state: &AppState, habit_id: uuid::Uuid| {
        state
            .graphs
            .iter()
            .find(|g| g.habit_id == habit_id)
            .expect("habit must have a graph")
            .weeks
            .iter()
            .flat_map(|w| w.days.iter())
            .find(|d| d.date == today)
            .expect("today must be in the graph window")
            .clone()
    };

    let a = cell_for(&state, ids[0]);
    assert!(a.done);
    assert!(a.is_today);
    // B's grid must not pick up A's entry.
    assert!(!cell_for(&state, ids[1]).done);

    mark(&mut state, ids[1], 0, HabitStatus::Done);
    state.refresh().unwrap();
    assert!(cell_for(&state, ids[1]).done);
}

#[test]
fn skipped_days_are_neither_done_nor_empty() {
    let (mut state, ids, _db) = state_with(&["Run"]);
    mark(&mut state, ids[0], 1, HabitStatus::Skipped);
    state.refresh().unwrap();

    let date = Local::now().date_naive() - Duration::days(1);
    let cell = state.graphs[0]
        .weeks
        .iter()
        .flat_map(|w| w.days.iter())
        .find(|d| d.date == date)
        .unwrap();
    assert!(cell.skipped);
    assert!(!cell.done);
}

#[test]
fn graph_window_is_monday_aligned_and_labels_each_month_once() {
    let (state, _ids, _db) = state_with(&["Run"]);

    for week in &state.graphs[0].weeks {
        assert_eq!(week.days.len(), 7);
        assert_eq!(
            week.start_date.format("%a").to_string(),
            "Mon",
            "columns must start on Monday"
        );
    }

    // A month's 1st falls in exactly one column, so no label repeats.
    let mut labels: Vec<&str> = state.graphs[0]
        .weeks
        .iter()
        .map(|w| w.month_label.as_str())
        .filter(|l| !l.is_empty())
        .collect();
    let before = labels.len();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(before, labels.len(), "month labels must be unique");
    assert!(before >= 3, "a 17-week window spans at least 3 months");
}

#[test]
fn habit_color_is_stable_and_respects_a_stored_colour() {
    let mut habit = Habit::new("Run", None);
    let derived = habit_color(&habit);
    assert_eq!(derived, habit_color(&habit), "must be deterministic");
    assert!(HABIT_PALETTE.contains(&derived));

    habit.color = Some("#A78BFA".to_string());
    assert_eq!(habit_color(&habit), (0xA7, 0x8B, 0xFA));

    // A malformed value falls back to the derived colour rather than panicking.
    habit.color = Some("not-a-colour".to_string());
    assert_eq!(habit_color(&habit), derived);
}

#[test]
fn palette_index_round_trips_through_hex() {
    for (i, rgb) in HABIT_PALETTE.iter().enumerate() {
        assert_eq!(parse_hex(&palette_hex(i)), Some(*rgb));
    }
    // Indices wrap rather than panic.
    assert_eq!(palette_hex(HABIT_PALETTE.len()), palette_hex(0));
    assert_eq!(parse_hex("#12"), None);
}

#[test]
fn recolouring_a_habit_persists_and_archiving_removes_it() {
    let (mut state, ids, _db) = state_with(&["Run", "Read"]);
    let id = ids[0];

    state.set_habit_color(id, 1).unwrap();
    let stored = state.habits.iter().find(|h| h.id == id).unwrap();
    assert_eq!(stored.color.as_deref(), Some(palette_hex(1).as_str()));
    assert_eq!(habit_color(stored), HABIT_PALETTE[1]);

    state.archive_habit(id).unwrap();
    assert!(!state.habits.iter().any(|h| h.id == id));
    assert_eq!(state.habits.len(), 1);
    // The archived habit's block goes with it.
    assert_eq!(state.graphs.len(), 1);
    assert!(!state.graphs.iter().any(|g| g.habit_id == id));
    assert_eq!(state.totals, state.stats(Scope::All));
}

#[test]
fn every_setting_survives_a_reload() {
    let tmp = NamedTempFile::new().unwrap();
    {
        let storage = Storage::new(tmp.path()).unwrap();
        let mut state = AppState::new(storage).unwrap();
        assert_eq!(state.theme_mode, ThemeMode::Dark, "defaults to dark");
        assert_eq!(state.accent_index, None, "accent follows the first habit");
        assert_eq!(state.graph_weeks, 17);
        assert_eq!(state.week_start, WeekStart::Monday);

        state.set_theme_mode(ThemeMode::System).unwrap();
        state.set_accent_index(Some(3)).unwrap();
        state.set_graph_weeks(26).unwrap();
        state.set_week_start(WeekStart::Sunday).unwrap();
    }

    let storage = Storage::new(tmp.path()).unwrap();
    let state = AppState::new(storage).unwrap();
    assert_eq!(state.theme_mode, ThemeMode::System);
    assert_eq!(state.accent_index, Some(3));
    assert_eq!(state.graph_weeks, 26);
    assert_eq!(state.week_start, WeekStart::Sunday);
    // An explicit accent overrides the first habit's colour.
    assert_eq!(state.accent(), HABIT_PALETTE[3]);
}

#[test]
fn the_old_dark_theme_key_is_carried_over() {
    let tmp = NamedTempFile::new().unwrap();
    {
        // What an install from before theme modes existed looks like.
        let storage = Storage::new(tmp.path()).unwrap();
        storage.set_setting("dark_theme", "0").unwrap();
    }
    let storage = Storage::new(tmp.path()).unwrap();
    let state = AppState::new(storage).unwrap();
    assert_eq!(state.theme_mode, ThemeMode::Light);
}

#[test]
fn graph_range_changes_the_window_length() {
    let (mut state, _ids, _db) = state_with(&["Run"]);
    let weeks_of = |s: &AppState| s.graphs[0].weeks.len();

    state.set_graph_weeks(12).unwrap();
    let short = weeks_of(&state);
    state.set_graph_weeks(26).unwrap();
    let long = weeks_of(&state);
    assert!(long > short, "26 weeks must show more columns than 12");

    // Anything off the menu is ignored rather than applied blindly.
    state.set_graph_weeks(999).unwrap();
    assert_eq!(state.graph_weeks, 26);
}

#[test]
fn week_start_changes_which_day_leads_each_column() {
    let (mut state, _ids, _db) = state_with(&["Run"]);

    state.set_week_start(WeekStart::Monday).unwrap();
    for week in &state.graphs[0].weeks {
        assert_eq!(week.start_date.format("%a").to_string(), "Mon");
    }
    assert_eq!(state.week_start.labels()[0], "Mon");

    state.set_week_start(WeekStart::Sunday).unwrap();
    for week in &state.graphs[0].weeks {
        assert_eq!(week.start_date.format("%a").to_string(), "Sun");
    }
    // Mon/Wed/Fri stay the labelled rows, just shifted down one.
    assert_eq!(state.week_start.labels()[1], "Mon");
    assert_eq!(state.week_start.labels()[3], "Wed");
}

#[test]
fn reminders_round_trip_and_follow_the_habit() {
    let (mut state, ids, _db) = state_with(&["Run", "Read"]);
    let id = ids[0];

    assert!(state.reminder_for(id).is_none(), "none by default");

    state.set_reminder_enabled(id, true).unwrap();
    let r = state.reminder_for(id).expect("created on enable");
    assert!(r.enabled);
    assert_eq!(r.label(), "09:00", "sensible default time");

    state.shift_reminder_time(id, -2, 30).unwrap();
    assert_eq!(state.reminder_for(id).unwrap().label(), "07:30");

    // Wraps within the day rather than going negative.
    state.shift_reminder_time(id, -8, 0).unwrap();
    assert_eq!(state.reminder_for(id).unwrap().label(), "23:30");

    // Clearing the last remaining day is refused: it would never fire.
    for day in 0..7 {
        state.toggle_reminder_day(id, day).unwrap();
    }
    assert_ne!(state.reminder_for(id).unwrap().days, 0);

    // Archiving the habit takes its reminder with it (ON DELETE CASCADE is on
    // delete; archive keeps the row, so it must not surface as due).
    state.archive_habit(id).unwrap();
    assert!(!state.due.iter().any(|d| d.habit_id == id));
}

#[test]
fn disabling_a_reminder_that_never_existed_is_a_no_op() {
    let (mut state, ids, _db) = state_with(&["Run"]);
    state.set_reminder_enabled(ids[0], false).unwrap();
    assert!(state.reminder_for(ids[0]).is_none(), "no empty row written");
}

#[test]
fn a_due_reminder_clears_once_the_habit_is_done() {
    let (mut state, ids, _db) = state_with(&["Run"]);
    let id = ids[0];

    state.set_reminder_enabled(id, true).unwrap();
    // Force the time into the past so it is unambiguously due.
    state.shift_reminder_time(id, -9, 0).unwrap();
    state.recompute_due(Local::now().naive_local());
    assert_eq!(state.due.len(), 1, "overdue and not done");

    state.toggle_today_for_habit(id).unwrap();
    state.recompute_due(Local::now().naive_local());
    assert!(state.due.is_empty(), "ticking it clears the reminder");
}

#[test]
fn sync_today_is_a_no_op_when_the_date_has_not_changed() {
    let (mut state, _ids, _db) = state_with(&["Run"]);
    assert_eq!(state.today, Local::now().date_naive());
    assert!(!state.sync_today().unwrap(), "same day, no refresh");

    // A stale date (an app left open past midnight) rolls forward.
    state.today -= Duration::days(1);
    assert!(state.sync_today().unwrap(), "rolled over");
    assert_eq!(state.today, Local::now().date_naive());
}

#[test]
fn first_launch_creates_a_default_habit_with_a_colour() {
    let (state, _ids, _db) = state_with(&[]);
    assert_eq!(state.habits.len(), 1);
    assert_eq!(state.habits[0].name, "Daily Check-in");
    assert!(state.habits[0].color.is_some());
    assert_eq!(state.graphs.len(), 1);
    assert_eq!(state.stats(Scope::All).total_done, 0);
    assert_eq!(state.totals, state.stats(Scope::All));
}

#[test]
fn future_days_are_flagged_but_never_marked_today() {
    let (state, _ids, _db) = state_with(&["Run"]);
    let today = Local::now().date_naive();
    let mut future = 0;
    for day in state.graphs[0].weeks.iter().flat_map(|w| w.days.iter()) {
        assert_eq!(day.is_today, day.date == today);
        assert_eq!(day.is_future, day.date > today);
        if day.is_future {
            future += 1;
            assert!(!day.done);
        }
    }
    // The window ends today, padded out to the end of the current week.
    assert!(future < 7);
}
