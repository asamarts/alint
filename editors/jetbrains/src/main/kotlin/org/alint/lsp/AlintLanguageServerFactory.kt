package org.alint.lsp

import com.intellij.ide.impl.isTrusted
import com.intellij.ide.trustedProjects.TrustedProjectsListener
import com.intellij.notification.NotificationAction
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.thisLogger
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.redhat.devtools.lsp4ij.LanguageServerFactory
import com.redhat.devtools.lsp4ij.LanguageServerManager
import com.redhat.devtools.lsp4ij.ServerStatus
import com.redhat.devtools.lsp4ij.server.ProcessStreamConnectionProvider
import com.redhat.devtools.lsp4ij.server.StreamConnectionProvider

internal const val ALINT_SERVER_ID = "alint"

/**
 * Registers `alint lsp` with LSP4IJ. Wired in `plugin.xml` under the
 * `com.redhat.devtools.lsp4ij` server extension point.
 *
 * Uses LSP4IJ's `ProcessStreamConnectionProvider` (`setCommands` /
 * `setWorkingDirectory` / `getInitializationOptions`) and the
 * `LanguageServerFactory` interface; the editors CI job (`./gradlew buildPlugin
 * verifyPlugin`) catches API drift in the pinned LSP4IJ version.
 */
class AlintLanguageServerFactory : LanguageServerFactory {
    override fun createConnectionProvider(project: Project): StreamConnectionProvider =
        AlintConnectionProvider(project)
}

private class AlintConnectionProvider(private val project: Project) : ProcessStreamConnectionProvider() {
    init {
        val binary = AlintBinary.resolve()
        if (binary != null) {
            super.setCommands(listOf(binary, "lsp"))
            project.basePath?.let { super.setWorkingDirectory(it) }
        } else {
            thisLogger().warn("alint binary not found on PATH or in settings; server not started")
            AlintNotifier.binaryMissing(project)
        }
    }

    /**
     * Forward the IDE's trusted-project state in `initializationOptions` so
     * `alint lsp` unlocks command rules, Git rules, custom facts, remote
     * `extends:`, and `{{env.*}}` only when the user has trusted the project.
     * Computed dynamically on each initialize so restarts pick up changes.
     */
    override fun getInitializationOptions(rootUri: VirtualFile?): Any =
        mapOf("trusted" to project.isTrusted())
}

/**
 * Restarts a running `alint` language server whenever the IDE's trusted-project
 * state changes so the new session re-initializes with updated `trusted`
 * `initializationOptions`. Registered as an application listener in `plugin.xml`
 * because `TrustedProjectsListener.TOPIC` is application-scoped.
 */
class AlintTrustedProjectsListener : TrustedProjectsListener {
    override fun onProjectTrusted(project: Project) {
        restartIfRunning(project)
    }

    override fun onProjectUntrusted(project: Project) {
        restartIfRunning(project)
    }

    private fun restartIfRunning(project: Project) {
        if (project.isDisposed) return
        val manager = LanguageServerManager.getInstance(project)
        val status = manager.getServerStatus(ALINT_SERVER_ID)
        if (status == ServerStatus.starting || status == ServerStatus.started) {
            // In LSP4IJ, `start()` on a running server invokes
            // `LanguageServerWrapper.restart()`, spawning a fresh process that
            // re-reads `getInitializationOptions` with the updated trust state.
            manager.start(ALINT_SERVER_ID)
        }
    }
}

/** Surfaces the "alint not found" prompt with an opt-in download action. */
object AlintNotifier {
    fun binaryMissing(project: Project) {
        val group = NotificationGroupManager.getInstance().getNotificationGroup("alint")
        group
            .createNotification(
                "alint binary not found",
                "Set its path in Settings, install it (brew / cargo / npm), or download the matching release.",
                NotificationType.WARNING,
            )
            .addAction(object : NotificationAction("Download latest") {
                override fun actionPerformed(
                    e: com.intellij.openapi.actionSystem.AnActionEvent,
                    notification: com.intellij.notification.Notification,
                ) {
                    notification.expire()
                    downloadInBackground(project)
                }
            })
            .notify(project)
    }

    private fun downloadInBackground(project: Project) {
        ApplicationManager.getApplication().executeOnPooledThread {
            val group = NotificationGroupManager.getInstance().getNotificationGroup("alint")
            try {
                // Download the alint release matching this plugin's
                // (release-stamped) version — same strategy as the VS
                // Code extension. (The Zed extension uses latest, since
                // its wasm can't read its own version.)
                val version = pluginVersion()
                val path = AlintBinary.download(version) { thisLogger().info("alint: $it") }
                AlintSettings.getInstance().alintPath = path
                group
                    .createNotification(
                        "alint downloaded",
                        "Installed to $path. Restart the alint language server (or the IDE) to use it.",
                        NotificationType.INFORMATION,
                    )
                    .notify(project)
            } catch (ex: Exception) {
                group
                    .createNotification(
                        "alint download failed",
                        ex.message ?: ex.toString(),
                        NotificationType.ERROR,
                    )
                    .notify(project)
            }
        }
    }

    // JetBrains Marketplace validation rejects BOTH `PluginManagerCore.getPlugin(PluginId)`
    // and `PluginManager.getPluginByClass(Class)` as internal-API usage, even though
    // released-IDE bytecode marks neither @ApiStatus.Internal. We sidestep the whole
    // plugin-lookup surface by stamping the version into a generated resource at build
    // time (see `generateVersionResource` in build.gradle.kts) and reading it from our
    // own classloader. No platform API touched.
    // See https://plugins.jetbrains.com/docs/intellij/api-internal.html
    private fun pluginVersion(): String =
        AlintNotifier::class.java.classLoader
            .getResourceAsStream("alint-lsp/version.txt")
            ?.use { it.readBytes().toString(Charsets.UTF_8).trim() }
            ?: error("alint-lsp/version.txt not on the classpath; build.gradle.kts wiring broken")
}
