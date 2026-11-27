plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "com.duonn.axios24"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.duonn.axios24"
        minSdk = 26
        targetSdk = 35
        versionCode = 49
        versionName = "0.4.9"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    buildFeatures {
        compose = true
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.10.01")
    implementation(composeBom)
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.material3:material3")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    testImplementation("junit:junit:4.13.2")
}

tasks.whenTaskAdded {
    if (name == "assembleRelease") {
        doLast {
            val apk = file("build/outputs/apk/release/app-release.apk")
            if (apk.exists()) {
                apk.copyTo(rootProject.file("../axios24_android.apk"), overwrite = true)
            }
        }
    } else if (name == "assembleDebug") {
        doLast {
            val apk = file("build/outputs/apk/debug/app-debug.apk")
            if (apk.exists()) {
                apk.copyTo(rootProject.file("../axios24_android.apk"), overwrite = true)
            }
        }
    }
}
