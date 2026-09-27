import org.gradle.api.tasks.bundling.Zip
import org.gradle.api.tasks.bundling.ZipEntryCompression

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
            isMinifyEnabled = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
}

dependencies {
    implementation("io.github.skylot:jadx-core:1.5.1")
    implementation("io.github.skylot:jadx-dex-input:1.5.1")
}

tasks.register<Zip>("packageHelper") {
    dependsOn("assembleRelease")
    from(provider {
        zipTree(layout.buildDirectory.file("outputs/apk/release/app-release-unsigned.apk").get().asFile)
    })
    destinationDirectory.set(layout.buildDirectory.dir("distributions"))
    archiveFileName.set("jadx-helper.jar")
    isPreserveFileTimestamps = false
    isReproducibleFileOrder = true
    entryCompression = ZipEntryCompression.DEFLATED
}
