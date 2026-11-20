plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// Version unique, lue dans le fichier VERSION du dépôt (« v1.2.3 »).
// versionCode = 1*10000 + 2*100 + 3 → toujours croissant, mises à jour OK.
val nyxVersion = rootProject.file("../VERSION").takeIf { it.exists() }
    ?.readText()?.trim()?.removePrefix("v") ?: "0.1.0"
val nyxVersionCode = nyxVersion.split(".").map { it.toIntOrNull() ?: 0 }
    .let { (it.getOrElse(0) { 0 } * 10000) + (it.getOrElse(1) { 0 } * 100) + it.getOrElse(2) { 0 } }
    .coerceAtLeast(1)

// Clé de signature permanente (CI) : fournie par variables d'environnement,
// jamais dans le dépôt. Absente → APK release non signé (la CI fait alors
// un debug, cf. android.yml).
val nyxKeystore = System.getenv("NYX_KEYSTORE")?.let { file(it) }?.takeIf { it.exists() }

android {
    namespace = "io.nyx.browser"
    compileSdk = 35

    defaultConfig {
        applicationId = "io.nyx.browser"
        minSdk = 26
        targetSdk = 35
        versionCode = nyxVersionCode
        versionName = nyxVersion

        // libnyx_ffi.so est produit par cargo-ndk dans src/main/jniLibs
        // (cf. .github/workflows/android.yml) pour ces deux ABI seulement.
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    signingConfigs {
        if (nyxKeystore != null) {
            create("nyx") {
                storeFile = nyxKeystore
                storeType = "pkcs12"
                storePassword = System.getenv("NYX_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("NYX_KEY_ALIAS")
                keyPassword = System.getenv("NYX_KEYSTORE_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            if (nyxKeystore != null) signingConfig = signingConfigs.getByName("nyx")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    sourceSets {
        getByName("main") {
            // Les assets du dépôt (newtab.html, filterlists/*.txt.gz) sont
            // servis tels quels — pas de copie, source unique avec le desktop.
            assets.setSrcDirs(listOf("../../assets"))
        }
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.webkit:webkit:1.12.1")
    implementation("androidx.core:core-splashscreen:1.0.1")
    // « Afficher » / « Copier » un mot de passe : verrouillage de l'appareil
    // (biométrie ou code) via BiometricPrompt. 1.1.0 = dernière stable.
    implementation("androidx.biometric:biometric:1.1.0")
    // Appareils liés : scan du QR d'appairage. Lecteur ZXing embarqué
    // (caméra via l'API Android, aucun Google Play Services, aucun réseau).
    // 4.3.0 = dernière stable ; tire com.google.zxing:core 3.4.1.
    implementation("com.journeyapps:zxing-android-embedded:4.3.0")
}
