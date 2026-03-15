## Habit Stack – Project Docs

### Overview

Habit Stack is a privacy-first habit tracker written in Rust. All data is stored locally on the device, and the main visualization is a GitHub-style calendar graph that shows your habit activity over time in a modern, minimal UI built with Slint.

### Architecture

- **Language**: Rust
- **UI framework**: Slint (`src/ui.slint`, `AppWindow`)
- **Core modules**:
  - `src/model.rs` – domain types: `Habit`, `HabitEntry`, `HabitStatus`
  - `src/storage.rs` – SQLite-backed persistence via `rusqlite`
  - `src/view_model.rs` – `AppState` and Git-graph-style calendar view model
  - `src/main.rs` – app entrypoint, Slint wiring, callbacks
  - `src/lib.rs` – library entrypoint exporting core modules
- **Build script**: `build.rs` compiles the Slint UI at build time.

### Data & Privacy

- All habit data is stored **locally only** in a SQLite database named `habits.db`.
- The DB path is resolved via the `directories` crate to a platform-appropriate app-data directory.
- No networking crates (HTTP, WebSockets, analytics, telemetry) are used.
- Optional backup/export can be implemented as writing JSON files to local storage; no cloud integration is required for basic usage.

### Git-Style Habit Graph

- The calendar visualization is inspired by the GitHub contribution graph:
  - **Columns** represent weeks.
  - **Rows** represent days of the week (Mon–Sun).
  - **Cells** represent a day’s completion status for the default habit (or, in future, aggregated habits).
- The view-model (`AppState` in `src/view_model.rs`) builds:
  - `DayCell` – a single day with `date`, `status`, and derived properties.
  - `WeekColumn` – a week containing 7 `DayCell` items.
- The Slint UI defines:
  - Structs `GraphDay` and `GraphWeek` mirroring the Rust view-model.
  - A scrollable row of week columns rendered as small rounded rectangles.

### Running the App

From the project root:

```bash
cargo run
```

This will:

- Build the Slint UI via `build.rs`.
- Create or open the local `habits.db` database.
- Create a default habit (`Daily Check-in`) on first launch.
- Show the Git-style graph for the last several weeks and allow tapping a day to toggle completion.

### Testing

Run tests with:

```bash
cargo test
```

Current coverage:

- Persistence: `tests/model_storage_tests.rs` exercises habit creation and entry upsert/query.
- View-model and UI compile paths are validated by building and running the app.

### Future Extensions

- Multi-habit selection and per-habit filtering in the graph.
- Habit management screens (create/edit/archive, color selection, frequency goals).
- Day detail bottom sheet with notes and bulk status updates.
- Simple export/import of data via local JSON files.

