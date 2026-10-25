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
// AAB per Google Play (`-Prondine.negozio=true`): niente aggiornamenti da GitHub, che Play vieta,
// e niente permesso di installare app.
val playStore = findProperty("rondine.negozio") == "true"
val rustAbis = (findProperty("rondine.abis") as String? ?: "arm64-v8a,x86_64").split(",")

android {
    namespace = "io.github.mtmattia.rondine"
    compileSdk = 37
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "io.github.mtmattia.rondine"
        minSdk = 26
        targetSdk = 36
        versionCode = 21
        versionName = "0.1.20"
        // Solo le architetture per cui c'è la libreria Rust: le altre librerie native (WebRTC,
        // JNA) non servirebbero a niente e allungherebbero l'APK.
        ndk { abiFilters += rustAbis }

        // Nodo proposto al primo avvio sui telefoni veri (facoltativo, si può cancellare o cambiare
        // dopo in Impostazioni): il nido pubblico di Mattia. Un altro con `rondine.nodo` in
        // ~/.gradle/gradle.properties o `-Prondine.nodo=…`; vuoto per non proporne nessuno.
        val defaultNode = (findProperty("rondine.nodo") as String?) ?: "https://macbook-pro-di-mattia-4.tail9cc49c.ts.net"
        buildConfigField("String", "DEFAULT_NODE", "\"$defaultNode\"")
        // Ripetitore FCM della versione Play (SPEC §8.1): il nido con le credenziali Firebase.
        val relay = (findProperty("rondine.ripetitore") as String?) ?: "https://macbook-pro-di-mattia-4.tail9cc49c.ts.net"
        buildConfigField("String", "PUSH_RELAY", "\"$relay\"")
        buildConfigField("boolean", "PLAY_STORE", "$playStore")
    }

    // Il permesso di installare (per gli aggiornamenti da GitHub) sta in un manifest a parte, usato
    // da tutte le build tranne l'AAB per Google Play.
    if (!playStore) {
        sourceSets.getByName("debug").manifest.srcFile("src/sideload/AndroidManifest.xml")
        sourceSets.getByName("release").manifest.srcFile("src/sideload/AndroidManifest.xml")
    }

    // Il modello del ritaglio si legge così com'è dall'APK, senza decomprimerlo.
    androidResources { noCompress += listOf("tar") }

    // Due versioni con lo stesso pacchetto e la stessa firma, quindi aggiornabili a vicenda
    // (docs/PUBBLICAZIONE.md):
    // - foss: GitHub e F-Droid, solo software libero, notifiche con UnifiedPush;
    // - play: Google Play, in più FCM (non libero) quando manca un distributore UnifiedPush.
    flavorDimensions += "canale"
    productFlavors {
        create("foss") { dimension = "canale" }
        create("play") {
            dimension = "canale"
            // Dati pubblici del progetto Firebase, letti da src/play/google-services.json senza il
            // plugin di Google (che vorrebbe un'app Firebase anche per il pacchetto .debug).
            @Suppress("UNCHECKED_CAST")
            val gs = groovy.json.JsonSlurper().parse(file("src/play/google-services.json")) as Map<String, Any>
            val project = gs["project_info"] as Map<String, String>
            val client = (gs["client"] as List<Map<String, Any>>).first()
            val appId = (client["client_info"] as Map<String, String>)["mobilesdk_app_id"]
            val apiKey = (client["api_key"] as List<Map<String, String>>).first()["current_key"]
            buildConfigField("String", "FIREBASE_PROJECT_ID", "\"${project["project_id"]}\"")
            buildConfigField("String", "FIREBASE_SENDER_ID", "\"${project["project_number"]}\"")
            buildConfigField("String", "FIREBASE_APP_ID", "\"$appId\"")
            buildConfigField("String", "FIREBASE_API_KEY", "\"$apiKey\"")
        }
    }

    // Firma della versione release per Google Play (chiave di caricamento). Chiave e password
    // stanno fuori dal repository: in ~/.gradle/gradle.properties (`rondine.firma.*`) o nelle
    // variabili d'ambiente `RONDINE_FIRMA_*` (per GitHub Actions). Senza, la release resta non
    // firmata, come la vuole F-Droid, che la firma da sé.
    fun secret(name: String): String? =
        (findProperty("rondine.firma.$name") as String?) ?: System.getenv("RONDINE_FIRMA_${name.uppercase()}")
    val keystore = secret("keystore")
    signingConfigs {
        if (keystore != null) {
            create("release") {
                storeFile = file(keystore)
                storePassword = secret("password")
                keyAlias = secret("alias") ?: "rondine"
                keyPassword = secret("password_chiave") ?: secret("password")
            }
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.findByName("release")
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

// La versione Play di prova ha il pacchetto vero: l'app Firebase è registrata per quello, e FCM
// lo controlla. Non si installa quindi accanto alla release Play (la foss di prova sì).
androidComponents {
    onVariants(selector().withBuildType("debug").withFlavor("canale" to "play")) { v ->
        v.applicationId.set("io.github.mtmattia.rondine")
    }
}

val ndkDir = androidComponents.sdkComponents.sdkDirectory.map { it.dir("ndk/${android.ndkVersion}") }

val cargoBuild = tasks.register<Exec>("cargoBuild") {
    description = "Compila rondine-ffi per Android con cargo-ndk"
    workingDir = repoRoot
    environment("ANDROID_NDK_HOME", ndkDir.get().asFile.path)
    // Build riproducibile: Rust mette nella libreria i percorsi dei sorgenti delle dipendenze
    // (~/.cargo/registry/…), diversi su ogni computer. Si sostituiscono con nomi fissi.
    val cargoHome = System.getenv("CARGO_HOME") ?: "${System.getProperty("user.home")}/.cargo"
    val rustflags = "--remap-path-prefix=$cargoHome=/cargo --remap-path-prefix=${repoRoot.path}=/rondine"
    environment("RUSTFLAGS", rustflags)
    // Se cambiano le opzioni, la libreria va ricompilata.
    inputs.property("rustflags", rustflags)
    inputs.property("abis", rustAbis.joinToString(","))
    val args = mutableListOf("cargo", "ndk")
    rustAbis.forEach { args += listOf("-t", it) }
    args += listOf("-P", "26", "-o", rustJniDir.get().asFile.path, "build", "--release", "-p", "rondine-ffi")
    commandLine(args)
    inputs.dir(File(repoRoot, "crates"))
    inputs.file(File(repoRoot, "Cargo.lock"))
    inputs.file(File(repoRoot, "Cargo.toml"))
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
    // Mappe nell'app (Apache-2.0): tessere OpenStreetMap, niente Google.
    implementation(libs.osmdroid)
    // Lettore QR con la fotocamera (Apache-2.0, nessun servizio Google).
    implementation(libs.zxing.embedded)
    // Chiamate voce e video (SPEC §7.9): WebRTC compilato dai sorgenti dal progetto Conversations
    // (codice BSD-3, pacchetto Apache-2.0 o MIT), lo stesso usato da app su F-Droid.
    implementation(libs.webrtc)
    // Tutte le emoji con categorie, recenti e tonalità della pelle (AndroidX, Apache-2.0).
    implementation(libs.emoji.picker)
    // Solo versione Play: FCM tramite il ripetitore del nido (SPEC §8.1).
    "playImplementation"(libs.firebase.messaging)
    "playImplementation"(libs.androidx.fragment)
}
