## Android build

This directory is a regular Gradle Android project. Gradle drives the whole
build: it cross-compiles the Rust crate for `aarch64-linux-android`, strips the
library, and packages it with the small Java shim in `app/src/main/java`.

### Prerequisites

- JDK 17
- Android SDK with platform 35 and the NDK version pinned as
  `pinnedNdkVersion` in `app/build.gradle.kts` (currently 27.2.12479018):

  ```bash
  sdkmanager "platforms;android-35" "ndk;27.2.12479018"
  ```

- Rust with the Android target:

  ```bash
  rustup target add aarch64-linux-android
  ```

Point Gradle at the SDK with `ANDROID_HOME` or `sdk.dir` in
`local.properties`. The NDK is found inside the SDK, and the linker and C
compiler are derived from it, so no machine-specific path is configured
anywhere else.

### Building

From `android/`:

| Command | Output |
| --- | --- |
| `./gradlew assembleDebug` | `app/build/outputs/apk/debug/app-debug.apk`: debug Rust build, signed with the local debug key |
| `./gradlew assembleRelease` | `app/build/outputs/apk/release/app-release.apk`: optimised Rust build (`app-release-unsigned.apk` without signing, below) |
| `./gradlew bundleRelease` | `app/build/outputs/bundle/release/app-release.aab`, for Google Play |

Install on a connected device with `./gradlew installDebug`, or
`adb install -r <apk>`.

The app's `versionName` is the `version` in the root `Cargo.toml`, and its
`versionCode` is derived from it as `major * 10000 + minor * 100 + patch`.

A bare `cargo build --target aarch64-linux-android` will not link, because
the NDK paths come from Gradle. To build only the library, run
`./gradlew cargoBuildDebug` (or `cargoBuildRelease`).

### Release signing

Release builds are signed only when these environment variables are set:

| Variable | Meaning |
| --- | --- |
| `SIGNING_KEYSTORE_PATH` | Path to the `.jks` keystore |
| `SIGNING_KEYSTORE_PASSWORD` | Keystore password |
| `SIGNING_KEY_ALIAS` | Alias of the key inside it |
| `SIGNING_KEY_PASSWORD` | Password of that key |

Without them the release APK is left unsigned, and Android will not install it.

Create the key once:

```bash
keytool -genkeypair -v -keystore habit-stack-release.jks -alias habit-stack \
  -keyalg RSA -keysize 4096 -validity 10000
```

Keep the keystore and its passwords backed up somewhere safe, and out of this
repository. Android only accepts an update signed with the same key, so losing
it means existing installs can never be upgraded.

### CI/CD

`.github/workflows/android.yml` runs on GitHub Actions:

- **Every push to `master` and every pull request:** `cargo test`, then a
  debug APK, uploaded as the `habit-stack-debug` workflow artifact. Each run
  signs with a freshly generated debug key, so uninstall the previous CI build
  before installing a newer one.
- **Pushing a `vX.Y.Z` tag:** `cargo test`, then a signed release APK and App
  Bundle, published as a GitHub Release named after the tag. The job fails if
  the tag does not match the version in `Cargo.toml`.

Releases need four repository secrets (**Settings → Secrets and variables →
Actions**):

| Secret | Value |
| --- | --- |
| `SIGNING_KEYSTORE_BASE64` | The keystore, base64-encoded: `base64 -i habit-stack-release.jks` |
| `SIGNING_KEYSTORE_PASSWORD` | Keystore password |
| `SIGNING_KEY_ALIAS` | Key alias, e.g. `habit-stack` |
| `SIGNING_KEY_PASSWORD` | Key password |

### Cutting a release

1. Bump `version` in the root `Cargo.toml` (for example to `0.2.0`) and commit.
2. Tag and push:

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

3. The APK and AAB appear on the repository's Releases page when the workflow
   finishes.
