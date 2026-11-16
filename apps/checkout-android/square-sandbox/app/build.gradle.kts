plugins {
    id("com.android.application")
}

dependencyLocking {
    lockAllConfigurations()
}

val sharedJniPath = providers.environmentVariable("CHECKOUTAPK_OUT").orNull
    ?: throw GradleException("Set CHECKOUTAPK_OUT to the checkout-regtest APK output directory")
val sharedJniDir = file(sharedJniPath)
val sandboxTrial = providers.environmentVariable("OPENBMT_SANDBOX_TRIAL").orNull
if (sandboxTrial != null && !Regex("[a-z][a-z0-9_]{0,15}").matches(sandboxTrial)) {
    throw GradleException("OPENBMT_SANDBOX_TRIAL must match [a-z][a-z0-9_]{0,15}")
}
val callbackDelayText = providers.environmentVariable("OPENBMT_SANDBOX_CALLBACK_DELAY_MS").orNull
val callbackDelayMs = if (callbackDelayText == null) {
    0L
} else {
    if (!Regex("[0-9]{1,5}").matches(callbackDelayText)) {
        throw GradleException("OPENBMT_SANDBOX_CALLBACK_DELAY_MS must be a whole number from 0 to 30000")
    }
    callbackDelayText.toLong().also {
        if (it !in 0L..30_000L) {
            throw GradleException("OPENBMT_SANDBOX_CALLBACK_DELAY_MS must be a whole number from 0 to 30000")
        }
    }
}
val stagedCommonResDir = layout.buildDirectory.dir("generated/shared-res")
val stageCommonRes = tasks.register<org.gradle.api.tasks.Sync>("stageCommonRes") {
    from(file("../../../android-common/res")) {
        // Source documentation stays in the repository but is not an Android resource.
        exclude("**/*.md")
    }
    into(stagedCommonResDir)
}
tasks.configureEach {
    if (name == "preBuild" || name == "mergeDebugResources") {
        dependsOn(stageCommonRes)
    }
}
for (abi in listOf("arm64-v8a", "x86_64")) {
    val abiDir = File(sharedJniDir, "stage/lib/$abi")
    if (!File(abiDir, "libopenbmt_counter_ffi.so").isFile ||
        !File(abiDir, "libopenbmt_qr_jni.so").isFile ||
        !File(abiDir, "libtor.so").isFile
    ) {
        throw GradleException("CHECKOUTAPK_OUT must contain checkout-regtest stage/lib/$abi libraries")
    }
}

android {
    namespace = "org.openbmt.checkout"
    compileSdk {
        version = release(37) { minorApiLevel = 0 }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "org.openbmt.checkout.square.sandbox"
        minSdk = 29
        targetSdk = 36
        versionCode = 1
        versionName = "0.1-sandbox"
    }

    buildFeatures { buildConfig = true }
    buildTypes {
        debug {
            applicationIdSuffix = ".debug" + (sandboxTrial?.let { ".$it" } ?: "")
            buildConfigField("long", "SQUARE_CALLBACK_DELAY_MS", "${callbackDelayMs}L")
        }
        release {
            // There is deliberately no production integration in this isolated project.
            isMinifyEnabled = false
        }
    }

    sourceSets["main"].kotlin.srcDirs(
        "../../src/android",
        "../../src/model",
        "../../../android-common/src/model",
        "../../../android-common/src/android",
        "../../../android-common/tor/src/model",
        "../../../android-common/tor/src/android",
        "../../../../crates/openbmt-counter-ffi/bindings/kotlin",
        "src/main/kotlin",
    )
    // Dependencies are explicit above; AGP's SourceSet API rejects Provider values.
    sourceSets["main"].res.srcDirs("../../res", stagedCommonResDir.get().asFile)
    sourceSets["main"].jniLibs.srcDirs(File(sharedJniDir, "stage/lib"))
}

androidComponents {
    beforeVariants(selector().withBuildType("release")) { variant ->
        variant.enable = false
    }
}

dependencies {
    debugImplementation("com.squareup.sdk:mobile-payments-sdk:2.7.0")
    debugImplementation("com.squareup.sdk:mockreader-ui:2.7.0")
}
