import java.io.File

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// La libreria Rust (crates/rondine-ffi) è compilata da qui con cargo-ndk, e i binding Kotlin
// sono generati da UniFFI: nessun binario precompilato nel repository (requisito F-Droid).
val repoRoot: File = rootProject.projectDir.parentFile
val rustJniDir = layout.buildDirectory.dir("rustJniLibs")
val uniffiDir = layout.buildDirectory.dir("generated/uniffi")
val rustAbis = (findProperty("rondine.abis") as String? ?: "arm64-v8a,x86_64").split(",")

android {
    namespace = "io.github.mtmattia.rondine"
    compileSdk = 37
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "io.github.mtmattia.rondine"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"

        // Nodo proposto al primo avvio sui telefoni veri, da `rondine.nodo` in
        // ~/.gradle/gradle.properties (fuori dal repository). Vuoto: si scrive a mano.
        val defaultNode = (findProperty("rondine.nodo") as String?).orEmpty()
        buildConfigField("String", "DEFAULT_NODE", "\"$defaultNode\"")
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
        debug {
            applicationIdSuffix = ".debug"
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    sourceSets {
        getByName("main") {
            jniLibs.directories.add(rustJniDir.get().asFile.path)
            kotlin.directories.add(uniffiDir.get().asFile.path)
        }
    }

    // Build riproducibili: niente metadati sulle dipendenze dentro l'APK.
    dependenciesInfo {
        includeInApk = false
        includeInBundle = false
    }
}

val ndkDir = androidComponents.sdkComponents.sdkDirectory.map { it.dir("ndk/${android.ndkVersion}") }

val cargoBuild = tasks.register<Exec>("cargoBuild") {
    description = "Compila rondine-ffi per Android con cargo-ndk"
    workingDir = repoRoot
    environment("ANDROID_NDK_HOME", ndkDir.get().asFile.path)
    val args = mutableListOf("cargo", "ndk")
    rustAbis.forEach { args += listOf("-t", it) }
    args += listOf("-P", "26", "-o", rustJniDir.get().asFile.path, "build", "--release", "-p", "rondine-ffi")
    commandLine(args)
    inputs.dir(File(repoRoot, "crates"))
    inputs.file(File(repoRoot, "Cargo.lock"))
    outputs.dir(rustJniDir)
}

// La libreria del computer da cui si leggono i metadati UniFFI: va ricompilata sempre, se no i
// binding Kotlin restano quelli della versione precedente.
val cargoBuildHost = tasks.register<Exec>("cargoBuildHost") {
    description = "Compila rondine-ffi per il computer (metadati dei binding)"
    workingDir = repoRoot
    commandLine("cargo", "build", "-p", "rondine-ffi", "--lib")
}

val uniffiBindgen = tasks.register<Exec>("uniffiBindgen") {
    description = "Genera i binding Kotlin di rondine-ffi"
    dependsOn(cargoBuild, cargoBuildHost)
    workingDir = repoRoot
    // I metadati UniFFI si leggono dalla libreria di debug del computer: quella per Android
    // è compilata con `strip` e non li contiene più.
    val ext = if (System.getProperty("os.name").lowercase().contains("mac")) "dylib" else "so"
    val lib = File(repoRoot, "target/debug/librondine_ffi.$ext").path
    commandLine(
        "cargo", "run", "-p", "rondine-ffi", "--bin", "uniffi-bindgen", "--",
        "generate", "--library", lib, "--language", "kotlin", "--no-format",
        "--out-dir", uniffiDir.get().asFile.path,
    )
    inputs.dir(File(repoRoot, "crates/rondine-ffi/src"))
    // I binding dipendono dalla libreria compilata, non solo dai sorgenti di rondine-ffi.
    inputs.file(lib)
    outputs.dir(uniffiDir)
}

tasks.named("preBuild") { dependsOn(uniffiBindgen) }

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.process)
    implementation(libs.androidx.navigation.compose)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    implementation(libs.compose.icons.extended)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.jna) { artifact { type = "aar" } }
    implementation(libs.unifiedpush.connector)
    implementation(libs.zxing.core)
    // Lettore QR con la fotocamera (Apache-2.0, nessun servizio Google).
    implementation(libs.zxing.embedded)
}
