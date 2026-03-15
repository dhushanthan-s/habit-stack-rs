## Android build (conceptual)

This project is configured so the Rust crate can be built as a `cdylib`, which is the first step to integrating it into an Android APK via Slint's Android support.

High‑level steps to create an APK:

1. Install the Android SDK + NDK and set the usual environment variables (`ANDROID_HOME`, `ANDROID_NDK_HOME`).
2. Add an Android target for Rust, e.g.:

```bash
rustup target add aarch64-linux-android
```

3. Create a small Gradle Android app in the `android/` directory (follow the Slint \"Android\" documentation) that:
   - Builds the Rust crate as a shared library for the Android target.
   - Loads the Slint UI and your Rust logic from that library.
4. From the `android/` directory, build the APK with:

```bash
./gradlew assembleDebug
```

For day‑to‑day development, you can continue to run the desktop build with:

```bash
cargo run
```

This keeps the feedback loop fast on PC, while still allowing you to wire up an Android build once the toolchain is available.

