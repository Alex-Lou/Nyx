plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "io.nyx.browser"
    compileSdk = 35

    defaultConfig {
        applicationId = "io.nyx.browser"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"

        // libnyx_ffi.so est produit par cargo-ndk dans src/main/jniLibs
        // (cf. .github/workflows/android.yml) pour ces deux ABI seulement.
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
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
}
