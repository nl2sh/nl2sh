plugins {
    id("com.android.application")
}

android {
    namespace = "com.nl2sh.jadx"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.nl2sh.jadx.helper"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        multiDexEnabled = false
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
}

dependencies {
    implementation("io.github.skylot:jadx-core:1.5.3")
    implementation("io.github.skylot:jadx-dex-input:1.5.3")
    implementation("com.github.tony19:logback-android:3.0.0")
}
