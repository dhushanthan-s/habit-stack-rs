import org.gradle.internal.os.OperatingSystem

plugins {
    id("com.android.application")
}

val ndkDir = "/Users/dhushan-21130/Library/Android/sdk/ndk/27.2.12479018"
val hostTag = "darwin-x86_64"
val rustTarget = "aarch64-linux-android"
val apiLevel = 24
val crateRoot = rootProject.projectDir.parentFile

android {
    namespace = "dev.habitstack"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.habitstack"
        minSdk = apiLevel
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += "arm64-v8a" }
    }

    buildTypes {
        getByName("debug") {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets.getByName("main") {
        jniLibs.srcDir(layout.buildDirectory.dir("rustJniLibs"))
    }
}

// Builds the Rust cdylib for Android and drops a stripped copy where the APK
// picks it up. Stripping matters: the unstripped debug .so is ~190 MB.
val cargoBuild by tasks.registering(Exec::class) {
    workingDir = crateRoot
    environment("ANDROID_NDK_ROOT", ndkDir)
    environment("ANDROID_HOME", android.sdkDirectory.absolutePath)
    commandLine("cargo", "build", "--target", rustTarget, "--lib")
}

val syncJniLibs by tasks.registering {
    dependsOn(cargoBuild)
    val outDir = layout.buildDirectory.dir("rustJniLibs/arm64-v8a")
    // Without this input Gradle calls the task UP-TO-DATE and ships a stale
    // .so even after cargo has rebuilt it.
    inputs.file(providers.provider { File(crateRoot, "target/$rustTarget/debug/libhabit_stack_rs.so") })
        .withPropertyName("rustCdylib")
    outputs.dir(outDir)
    doLast {
        val src = File(crateRoot, "target/$rustTarget/debug/libhabit_stack_rs.so")
        val dstDir = outDir.get().asFile
        dstDir.mkdirs()
        val dst = File(dstDir, "libhabit_stack_rs.so")
        providers.exec {
            commandLine(
                "$ndkDir/toolchains/llvm/prebuilt/$hostTag/bin/llvm-strip",
                "--strip-debug", "-o", dst.absolutePath, src.absolutePath,
            )
        }.result.get()
        logger.lifecycle("jniLibs: ${dst.length() / 1024 / 1024} MB")
    }
}

tasks.matching { it.name.startsWith("merge") && it.name.contains("JniLibFolders") }
    .configureEach { dependsOn(syncJniLibs) }
tasks.named("preBuild") { dependsOn(syncJniLibs) }
