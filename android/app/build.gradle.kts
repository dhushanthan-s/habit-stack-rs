import javax.inject.Inject

plugins {
    id("com.android.application")
}

// The NDK is pinned here and nowhere else: AGP finds this version under the
// SDK, every toolchain path below is derived from it, and CI reads this line
// to know which NDK to install.
val pinnedNdkVersion = "27.2.12479018"
val rustTarget = "aarch64-linux-android"
val androidAbi = "arm64-v8a"
val apiLevel = 24
val crateRoot = rootProject.projectDir.parentFile

// The NDK ships one prebuilt toolchain per host OS. macOS uses the x86_64
// directory on Apple silicon too; its binaries are universal.
val osName = System.getProperty("os.name").lowercase()
val isWindows = osName.startsWith("windows")
val hostTag = when {
    osName.startsWith("mac") -> "darwin-x86_64"
    isWindows -> "windows-x86_64"
    else -> "linux-x86_64"
}
val exeSuffix = if (isWindows) ".exe" else ""
val scriptSuffix = if (isWindows) ".cmd" else ""

// The crate version is the app version, so a release only means bumping
// Cargo.toml. versionCode packs it as major * 10000 + minor * 100 + patch,
// which keeps it rising release over release as in-place upgrades and Google
// Play require; minor and patch must therefore stay below 100.
val crateVersion = Regex("""(?m)^version\s*=\s*"((\d+)\.(\d+)\.(\d+)[^"]*)"""")
    .find(File(crateRoot, "Cargo.toml").readText())
    ?: error("No package version found in Cargo.toml")
val (crateVersionName, major, minor, patch) = crateVersion.destructured

android {
    namespace = "dev.habitstack"
    compileSdk = 35
    ndkVersion = pinnedNdkVersion

    defaultConfig {
        applicationId = "dev.habitstack"
        minSdk = apiLevel
        targetSdk = 35
        versionCode = major.toInt() * 10000 + minor.toInt() * 100 + patch.toInt()
        versionName = crateVersionName
        ndk { abiFilters += androidAbi }
    }

    // Release signing is read from the environment so no key or password is
    // ever committed. Without it, assembleRelease still succeeds but leaves the
    // APK unsigned, and Android refuses to install it.
    val keystorePath = System.getenv("SIGNING_KEYSTORE_PATH")?.takeIf { it.isNotBlank() }
    signingConfigs {
        if (keystorePath != null) {
            create("release") {
                storeFile = file(keystorePath)
                storePassword = System.getenv("SIGNING_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("SIGNING_KEY_ALIAS")
                keyPassword = System.getenv("SIGNING_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        getByName("debug") {
            isMinifyEnabled = false
        }
        getByName("release") {
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("release")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

/**
 * Copies the cargo-built library into a jniLibs layout without its debug info.
 * Stripping matters: the unstripped debug .so is ~190 MB.
 */
abstract class StripRustLibrary : DefaultTask() {
    // Declared as an input so a rebuilt .so is never skipped as UP-TO-DATE.
    @get:InputFile
    abstract val library: RegularFileProperty

    @get:Input
    abstract val stripTool: Property<String>

    @get:Input
    abstract val abi: Property<String>

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @get:Inject
    abstract val execOperations: ExecOperations

    @TaskAction
    fun strip() {
        val source = library.get().asFile
        val abiDir = outputDir.get().dir(abi.get()).asFile.apply { mkdirs() }
        val stripped = File(abiDir, source.name)
        execOperations.exec {
            commandLine(stripTool.get(), "--strip-debug", "-o", stripped.path, source.path)
        }
        logger.lifecycle("jniLibs: ${stripped.length() / 1024 / 1024} MB")
    }
}

// One cargo build per variant: debug APKs carry a debug build of the crate,
// release APKs an optimised one. Each lands in its own generated jniLibs
// directory, which AGP wires into packaging (and orders the tasks for).
androidComponents {
    // Read lazily from the extension rather than sdkComponents.ndkDirectory,
    // which in AGP 8.7 throws unless ndkPath is also set.
    val ndkDirectory = providers.provider { android.ndkDirectory }
    val sdkDirectory = providers.provider { android.sdkDirectory }
    val toolchainBin = ndkDirectory.map { File(it, "toolchains/llvm/prebuilt/$hostTag/bin") }
    val envTarget = rustTarget.replace('-', '_')

    onVariants { variant ->
        val profile = if (variant.buildType == "release") "release" else "debug"
        val suffix = variant.name.replaceFirstChar { it.uppercase() }

        // cargo does its own up-to-date check, so this runs on every build.
        val cargoBuild = tasks.register<Exec>("cargoBuild$suffix") {
            workingDir = crateRoot
            commandLine(
                listOfNotNull(
                    "cargo", "build", "--target", rustTarget, "--lib",
                    "--release".takeIf { profile == "release" },
                ),
            )
            // Resolved at execution time, once AGP has located the NDK. The
            // NDK's clang wrapper doubles as the linker and already knows the
            // sysroot and API level.
            doFirst {
                val bin = toolchainBin.get()
                val clang = File(bin, "$rustTarget$apiLevel-clang$scriptSuffix").path
                val ndk = ndkDirectory.get().path
                environment("ANDROID_HOME", sdkDirectory.get().path)
                environment("ANDROID_NDK_HOME", ndk)
                environment("ANDROID_NDK_ROOT", ndk)
                environment("CARGO_TARGET_${envTarget.uppercase()}_LINKER", clang)
                environment("CC_$envTarget", clang)
                environment("AR_$envTarget", File(bin, "llvm-ar$exeSuffix").path)
            }
        }

        val stripRustLibrary = tasks.register<StripRustLibrary>("stripRustLibrary$suffix") {
            dependsOn(cargoBuild)
            library.set(File(crateRoot, "target/$rustTarget/$profile/libhabit_stack_rs.so"))
            stripTool.set(toolchainBin.map { File(it, "llvm-strip$exeSuffix").path })
            abi.set(androidAbi)
        }

        variant.sources.jniLibs?.addGeneratedSourceDirectory(
            stripRustLibrary,
            StripRustLibrary::outputDir,
        )
    }
}
