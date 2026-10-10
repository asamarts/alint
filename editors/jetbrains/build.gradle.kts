// alint JetBrains plugin — Gradle build (Kotlin DSL).
//
// Built with the IntelliJ Platform Gradle Plugin 2.x and depends on
// Red Hat's LSP4IJ to host the `alint lsp` language server. One plugin
// covers the whole JetBrains suite (IDEA, PyCharm, GoLand, WebStorm,
// RustRover, CLion, Rider, Android Studio). The version coordinates
// below are validated by the editors CI job (./gradlew buildPlugin
// verifyPlugin).

import java.util.jar.JarInputStream
import java.util.zip.ZipEntry
import java.util.zip.ZipFile

plugins {
    kotlin("jvm") version "2.4.20"
    id("org.jetbrains.intellij.platform") version "2.19.0"
}

group = "org.alint"
// CI stamps the version from the release tag via `-PpluginVersion=…`
// (see release.yml); the committed default can lag like the npm / VS
// Code packages do.
version = (findProperty("pluginVersion") as String?) ?: "0.10.2"

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    intellijPlatform {
        // Base IDE to compile/run against. Community covers the whole
        // platform; the plugin has no IDE-specific code.
        intellijIdeaCommunity("2024.2")
        // LSP4IJ (Red Hat) — the LSP client this plugin registers with.
        // Marketplace id com.redhat.devtools.lsp4ij; verify the version.
        plugin("com.redhat.devtools.lsp4ij", "0.7.0")
        pluginVerifier()
        zipSigner()
        // Platform test fixtures for the headless LSP integration test.
        testFramework(org.jetbrains.intellij.platform.gradle.TestFrameworkType.Platform)
    }
    // Used by the managed-download path to extract the release .tar.gz.
    implementation("org.apache.commons:commons-compress:1.28.0")
    testImplementation("junit:junit:4.13.2")
    // The platform test fixtures (UsefulTestCase) reference opentest4j,
    // which isn't pulled onto the gradle test classpath transitively.
    testImplementation("org.opentest4j:opentest4j:1.3.0")
}

intellijPlatform {
    // No Java sources or GUI (.form) files — this is Kotlin LSP glue —
    // so skip bytecode instrumentation (which would otherwise pull the
    // IntelliJ Java-compiler/ant-tasks dependency).
    instrumentCode = false

    pluginConfiguration {
        id = "org.alint.lsp"
        name = "alint"
        version = project.version.toString()
        ideaVersion {
            sinceBuild = "242"
            // No upper bound — the plugin is LSP glue and shouldn't pin
            // an untilBuild that locks out future IDE releases.
            untilBuild = provider { null }
        }
        vendor {
            name = "alint contributors"
            url = "https://alint.org"
        }
    }
    pluginVerification {
        // Verify against explicit released IDEs (the sinceBuild 242 floor
        // + one recent), NOT recommended(): recommended() reaches for the
        // newest build — including unreleased EAPs (e.g. 2025.3) that
        // aren't downloadable and fail dependency resolution. Runs in the
        // editors CI job so Marketplace-compat regressions surface
        // pre-tag.
        ides {
            create(org.jetbrains.intellij.platform.gradle.IntelliJPlatformType.IntellijIdeaCommunity, "2024.2")
            create(org.jetbrains.intellij.platform.gradle.IntelliJPlatformType.IntellijIdeaCommunity, "2024.3")
        }
        // Fail the build on the same problem classes JetBrains Marketplace
        // moderation rejects. The verifier defaults to compatibility-only
        // (COMPATIBILITY_PROBLEMS + INVALID_PLUGIN); `INTERNAL_API_USAGES`
        // (and friends) emit a *warning* but do not fail the task, so a
        // call to an `@ApiStatus.Internal` API can sail through CI and
        // then get rejected at upload time — which is exactly what bit
        // the v0.11.0 cut (PluginManagerCore.getPlugin). Opting in here
        // closes that gap so the same class of issue fails locally.
        // See https://plugins.jetbrains.com/docs/intellij/api-internal.html
        failureLevel = listOf(
            org.jetbrains.intellij.platform.gradle.tasks.VerifyPluginTask.FailureLevel.COMPATIBILITY_PROBLEMS,
            org.jetbrains.intellij.platform.gradle.tasks.VerifyPluginTask.FailureLevel.INTERNAL_API_USAGES,
            org.jetbrains.intellij.platform.gradle.tasks.VerifyPluginTask.FailureLevel.OVERRIDE_ONLY_API_USAGES,
            org.jetbrains.intellij.platform.gradle.tasks.VerifyPluginTask.FailureLevel.NON_EXTENDABLE_API_USAGES,
            org.jetbrains.intellij.platform.gradle.tasks.VerifyPluginTask.FailureLevel.SCHEDULED_FOR_REMOVAL_API_USAGES,
            org.jetbrains.intellij.platform.gradle.tasks.VerifyPluginTask.FailureLevel.INVALID_PLUGIN,
            org.jetbrains.intellij.platform.gradle.tasks.VerifyPluginTask.FailureLevel.PLUGIN_STRUCTURE_WARNINGS,
            org.jetbrains.intellij.platform.gradle.tasks.VerifyPluginTask.FailureLevel.MISSING_DEPENDENCIES,
        )
    }
    publishing {
        // Set JETBRAINS_MARKETPLACE_TOKEN in CI (release.yml) / locally.
        token = providers.environmentVariable("JETBRAINS_MARKETPLACE_TOKEN")
    }
    signing {
        certificateChain = providers.environmentVariable("JETBRAINS_CERTIFICATE_CHAIN")
        privateKey = providers.environmentVariable("JETBRAINS_PRIVATE_KEY")
        password = providers.environmentVariable("JETBRAINS_PRIVATE_KEY_PASSWORD")
    }
}

// Target Java 21 (the IntelliJ 2024.2+ runtime baseline; IntelliJ Platform
// Gradle Plugin 2.12+ also pins the Java toolchain to the platform's, so a
// JDK 17 build fails to find a 21 toolchain). Deliberately NOT
// `jvmToolchain(21)`, which makes Gradle try to *provision* a toolchain and
// fails on boxes without toolchain auto-detection or a download repo
// configured — build with a JDK 21+.
kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_21
    }
}

java {
    sourceCompatibility = JavaVersion.VERSION_21
    targetCompatibility = JavaVersion.VERSION_21
}

// --- Generated version resource -------------------------------------------
//
// AlintNotifier.pluginVersion() reads this file instead of any platform
// plugin-lookup API (PluginManagerCore.getPlugin / PluginManager.getPluginByClass
// are both rejected by Marketplace validation as internal-API usage). The
// gradle build already knows the version via -PpluginVersion or the committed
// default, so we just stamp it as a classpath resource our own classloader
// can read at runtime — zero platform API surface.
val generateVersionResource = tasks.register("generateVersionResource") {
    group = "build"
    description = "Write the plugin version into a classpath resource for runtime self-lookup."
    val outDir = layout.buildDirectory.dir("generated/resources/version")
    val versionString = project.version.toString()
    inputs.property("version", versionString)
    outputs.dir(outDir)
    doLast {
        val f = outDir.get().asFile.resolve("alint-lsp/version.txt")
        f.parentFile.mkdirs()
        f.writeText(versionString)
    }
}
sourceSets["main"].resources.srcDir(generateVersionResource.map { it.outputs.files })

// Make the build's idea of the version visible to PluginVersionResourceTest so
// it can cross-check the embedded resource. Decoupling the test from gradle's
// internal `project.version` re-evaluation matters because the resource is
// generated once at build time and shouldn't drift from what tests see.
tasks.named<Test>("test") {
    systemProperty("alint.test.plugin.version", project.version.toString())
}

// --- Marketplace-deny bytecode scan ---------------------------------------
//
// JetBrains Marketplace's moderation validator rejects references to certain
// platform-internal classes even when those classes are NOT annotated
// `@ApiStatus.Internal` in released-IDE bytecode (verified: 2024.2 -> 2025.2
// none carry the annotation on `PluginManagerCore.getPlugin(PluginId)`).
// `verifyPlugin`'s `INTERNAL_API_USAGES` therefore cannot catch this class
// — the v0.11.0 cut shipped with such a call, passed CI, and was rejected
// at upload. This second-layer check scans our authored jar's constant pool
// for an explicit deny-list of class names; extend the list as we hit new
// Marketplace-rejection patterns.
//
// See https://plugins.jetbrains.com/docs/intellij/api-internal.html
val verifyNoMarketplaceDeniedApis = tasks.register("verifyNoMarketplaceDeniedApis") {
    group = "verification"
    description = "Fail if the built plugin references Marketplace-rejected platform internals."

    dependsOn("buildPlugin")
    // Inputs/outputs so this re-runs only when the built zip changes.
    val zipFile = tasks.named("buildPlugin", Zip::class).flatMap { it.archiveFile }
    inputs.file(zipFile)
    val marker = layout.buildDirectory.file("verification/marketplace-denied-apis.ok")
    outputs.file(marker)

    // (FQN slashed form). Whole-class bans: any reference at all is a fail,
    // including method calls, field reads, and superclass declarations. The
    // two PluginManager* classes were both rejected by Marketplace moderation
    // on v0.11.0 uploads — PluginManagerCore.getPlugin(PluginId) on the first
    // build, then PluginManager.getPluginByClass(Class) on the second — so
    // both surfaces are off-limits. Look up plugin metadata via a build-time
    // resource (see generateVersionResource above) or, if you need someone
    // else's plugin descriptor, ApplicationManager-routed extension lookups.
    val deniedClasses = listOf(
        "com/intellij/ide/plugins/PluginManagerCore",
        "com/intellij/ide/plugins/PluginManager",
    )

    // Only scan jars produced by THIS project (not bundled third-party libs
    // we don't author — commons-*, etc. — which we're not responsible for).
    val authoredJarPrefix = "alint-jetbrains/lib/alint-jetbrains-"

    doLast {
        val violations = mutableListOf<String>()
        ZipFile(zipFile.get().asFile).use { outer: ZipFile ->
            val entries: List<ZipEntry> = outer.entries().toList()
                .filter { e: ZipEntry -> e.name.startsWith(authoredJarPrefix) && e.name.endsWith(".jar") }
                .filter { e: ZipEntry -> !e.name.endsWith("-searchableOptions.jar") }
            for (jarEntry: ZipEntry in entries) {
                JarInputStream(outer.getInputStream(jarEntry)).use { jin: JarInputStream ->
                    var ce: ZipEntry? = jin.nextJarEntry
                    while (ce != null) {
                        if (ce.name.endsWith(".class")) {
                            val bytes: ByteArray = jin.readAllBytes()
                            // Class names live in the constant pool as
                            // UTF-8 entries; scanning the raw bytes for
                            // the slashed FQN is reliable for a "class
                            // is referenced anywhere" check.
                            val asLatin: String = bytes.toString(Charsets.ISO_8859_1)
                            for (denied: String in deniedClasses) {
                                if (asLatin.contains(denied)) {
                                    violations.add("${jarEntry.name} -> ${ce.name} references $denied")
                                }
                            }
                        }
                        ce = jin.nextJarEntry
                    }
                }
            }
        }
        if (violations.isNotEmpty()) {
            throw GradleException(
                buildString {
                    appendLine("Plugin references Marketplace-rejected platform internals:")
                    violations.forEach { appendLine("  $it") }
                    appendLine()
                    appendLine("These classes pass intellij-plugin-verifier (not @ApiStatus.Internal in")
                    appendLine("released IDE bytecode) but JetBrains Marketplace moderation rejects them.")
                    appendLine("See https://plugins.jetbrains.com/docs/intellij/api-internal.html for")
                    appendLine("the documented public alternatives (e.g. PluginManagerCore ->")
                    appendLine("PluginManager.getPluginByClass / .findEnabledPlugin).")
                }
            )
        }
        val markerFile = marker.get().asFile
        markerFile.parentFile.mkdirs()
        markerFile.writeText("ok: scanned ${deniedClasses.size} denied class(es); no references found\n")
        logger.lifecycle("✓ verifyNoMarketplaceDeniedApis: ${deniedClasses.size} class(es) on the deny-list; no references in authored jar")
    }
}

// Wire the deny-list scan onto `buildPlugin` itself so every path that
// produces the zip runs it — including `verifyPlugin` (depends on
// `buildPlugin`) AND `publishPlugin` (depends on `signPlugin` -> `buildPlugin`).
// `release.yml` invokes `./gradlew publishPlugin` directly without
// `verifyPlugin`, so attaching the gate to verifyPlugin alone would not
// cover the release path. `check` also depends on it for `./gradlew check`.
tasks.named("buildPlugin") { finalizedBy(verifyNoMarketplaceDeniedApis) }
tasks.named("check") { dependsOn(verifyNoMarketplaceDeniedApis) }
