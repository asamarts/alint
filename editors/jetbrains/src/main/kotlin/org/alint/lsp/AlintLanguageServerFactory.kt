package org.alint.lsp

import com.intellij.ide.impl.isTrusted
import com.intellij.ide.trustedProjects.TrustedProjectsListener
import com.intellij.notification.NotificationAction
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.thisLogger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.redhat.devtools.lsp4ij.ConnectDocumentToLanguageServerSetupParticipant
import com.redhat.devtools.lsp4ij.LanguageServerFactory
import com.redhat.devtools.lsp4ij.LanguageServerManager
import com.redhat.devtools.lsp4ij.LanguageServersRegistry
import com.redhat.devtools.lsp4ij.ServerStatus
import com.redhat.devtools.lsp4ij.client.LanguageClientImpl
import com.redhat.devtools.lsp4ij.server.LanguageServerException
import com.redhat.devtools.lsp4ij.server.ProcessStreamConnectionProvider
import com.redhat.devtools.lsp4ij.server.StreamConnectionProvider
import java.util.WeakHashMap
import java.util.concurrent.atomic.AtomicLong

internal const val ALINT_SERVER_ID = "alint"

/**
 * Registers `alint lsp` with LSP4IJ. Wired in `plugin.xml` under the
 * `com.redhat.devtools.lsp4ij` server extension point.
 *
 * Uses LSP4IJ's `ProcessStreamConnectionProvider` (`setCommands` /
 * `setWorkingDirectory` / `getInitializationOptions`), `LanguageClientImpl`,
 * and `LanguageServerFactory`; the editors CI job (`./gradlew buildPlugin
 * verifyPlugin`) catches API drift in the pinned LSP4IJ version.
 */
class AlintLanguageServerFactory : LanguageServerFactory {
    override fun createConnectionProvider(project: Project): StreamConnectionProvider =
        AlintConnectionProvider(project)

    override fun createLanguageClient(project: Project): LanguageClientImpl =
        AlintLanguageClient(project)
}

private class AlintConnectionProvider(private val project: Project) : ProcessStreamConnectionProvider() {
    val generation: Long = AlintTrustCoordinator.nextGeneration()

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

    override fun start() {
        AlintTrustCoordinator.onProviderStarting(project, this)
        try {
            super.start()
        } catch (t: Throwable) {
            AlintTrustCoordinator.onProviderStopped(project, this)
            throw t
        }
    }

    override fun stop() {
        try {
            super.stop()
        } finally {
            AlintTrustCoordinator.onProviderStopped(project, this)
        }
    }

    /**
     * Forward the IDE's trusted-project state in `initializationOptions` so
     * `alint lsp` unlocks command rules, Git rules, custom facts, remote
     * `extends:`, and `{{env.*}}` only when the user has trusted the project.
     * Computed dynamically on each initialize so restarts pick up changes.
     */
    override fun getInitializationOptions(rootUri: VirtualFile?): Any {
        val trusted = project.isTrusted()
        AlintTrustCoordinator.onInitialized(project, this, trusted)
        return mapOf("trusted" to trusted)
    }
}

private class AlintLanguageClient(project: Project) : LanguageClientImpl(project) {
    override fun handleServerStatusChanged(serverStatus: ServerStatus) {
        super.handleServerStatusChanged(serverStatus)
        AlintTrustCoordinator.onServerStatusChanged(project, serverStatus)
    }
}

/**
 * Coordinates workspace-trust restarts per project using only public LSP4IJ
 * and IntelliJ Platform APIs:
 *
 * - Tracks the active [AlintConnectionProvider] and its last initialized
 *   `trusted` value per project, avoiding LSP4IJ 0.7.0's async `stop()`
 *   status-clobber race (where an old process's delayed shutdown can set
 *   `ServerStatus.stopped` while the newly restarted server is alive) and
 *   deduplicating IDEA's double-fired `onProjectTrusted` notifications.
 * - Never starts a server that is not running and never re-enables a server
 *   the user disabled (`StartOptions().setWillEnable(false)`).
 * - Reconnects open editor tabs once a trust-triggered restart reaches
 *   [ServerStatus.started], because LSP4IJ 0.7.0's `restart()` clears
 *   connected documents before starting the fresh process.
 */
private object AlintTrustCoordinator {
    private class ProjectState {
        var activeProvider: AlintConnectionProvider? = null
        var lastInitializedTrusted: Boolean? = null
        var restartInFlight: Boolean = false
        var reconnectOnStart: Boolean = false
    }

    private val nextGen = AtomicLong(0)
    private val lock = Any()
    private val states = WeakHashMap<Project, ProjectState>()

    fun nextGeneration(): Long = nextGen.incrementAndGet()

    private fun stateOf(project: Project): ProjectState =
        states.getOrPut(project) { ProjectState() }

    fun onProviderStarting(project: Project, provider: AlintConnectionProvider) {
        synchronized(lock) {
            val state = stateOf(project)
            val current = state.activeProvider
            if (current == null || provider.generation >= current.generation) {
                state.activeProvider = provider
            }
        }
    }

    fun onInitialized(project: Project, provider: AlintConnectionProvider, trusted: Boolean) {
        synchronized(lock) {
            val state = stateOf(project)
            val current = state.activeProvider
            if (current == null || provider.generation >= current.generation) {
                state.activeProvider = provider
                state.lastInitializedTrusted = trusted
            }
        }
    }

    fun onProviderStopped(project: Project, provider: AlintConnectionProvider) {
        synchronized(lock) {
            val state = states[project] ?: return
            if (state.activeProvider === provider) {
                state.activeProvider = null
                state.lastInitializedTrusted = null
                state.restartInFlight = false
            }
        }
    }

    fun onTrustChanged(project: Project) {
        if (project.isDisposed || project.isDefault) return
        val definition = LanguageServersRegistry.getInstance().getServerDefinition(ALINT_SERVER_ID)
            ?: return
        if (!definition.isEnabled(project)) return

        val manager = LanguageServerManager.getInstance(project)
        val status = manager.getServerStatus(ALINT_SERVER_ID)
        val shouldDispatch = synchronized(lock) {
            val state = stateOf(project)
            val hasLiveProvider = state.activeProvider?.isAlive == true
            val isActiveOrStopping = hasLiveProvider ||
                state.activeProvider != null ||
                status == ServerStatus.starting ||
                status == ServerStatus.started ||
                status == ServerStatus.stopping
            if (!isActiveOrStopping) {
                return@synchronized false
            }
            val desiredTrusted = project.isTrusted()
            if (!state.restartInFlight &&
                state.lastInitializedTrusted == desiredTrusted &&
                status != ServerStatus.stopping
            ) {
                return@synchronized false
            }
            state.reconnectOnStart = true
            if (state.restartInFlight) {
                // Re-checked once the in-flight restart reaches `started`.
                return@synchronized false
            }
            state.restartInFlight = true
            true
        }
        if (!shouldDispatch) return

        ApplicationManager.getApplication().executeOnPooledThread {
            if (project.isDisposed || project.isDefault) {
                clearRestartInFlight(project)
                return@executeOnPooledThread
            }
            val currentDef = LanguageServersRegistry.getInstance().getServerDefinition(ALINT_SERVER_ID)
            if (currentDef == null || !currentDef.isEnabled(project)) {
                clearRestartInFlight(project)
                return@executeOnPooledThread
            }
            try {
                // In LSP4IJ, `start()` on a started server invokes
                // `LanguageServerWrapper.restart()`, spawning a fresh process
                // that re-reads `getInitializationOptions`. Setting
                // `willEnable = false` avoids silently re-enabling a server
                // the user disabled in settings.
                val options = LanguageServerManager.StartOptions()
                    .setWillEnable(false)
                    .setForceStart(false)
                LanguageServerManager.getInstance(project).start(currentDef, options)
            } catch (_: LanguageServerException) {
                clearRestartInFlight(project)
            } catch (ex: Throwable) {
                clearRestartInFlight(project)
                thisLogger().warn("failed to restart alint language server after trust change", ex)
            }
        }
    }

    fun onServerStatusChanged(project: Project, status: ServerStatus) {
        if (project.isDisposed || project.isDefault) return
        if (status != ServerStatus.started) return

        val (shouldReconnect, needsFollowupRestart) = synchronized(lock) {
            val state = states[project] ?: return
            state.restartInFlight = false
            val currentTrusted = project.isTrusted()
            val drifted = state.lastInitializedTrusted != null &&
                state.lastInitializedTrusted != currentTrusted
            if (drifted) {
                state.reconnectOnStart = true
                false to true
            } else if (state.reconnectOnStart) {
                state.reconnectOnStart = false
                true to false
            } else {
                false to false
            }
        }

        if (needsFollowupRestart) {
            onTrustChanged(project)
        } else if (shouldReconnect) {
            reconnectOpenFiles(project)
        }
    }

    private fun clearRestartInFlight(project: Project) {
        synchronized(lock) {
            states[project]?.restartInFlight = false
        }
    }

    private fun reconnectOpenFiles(project: Project) {
        ApplicationManager.getApplication().invokeLater {
            if (project.isDisposed || project.isDefault) return@invokeLater
            val editorManager = FileEditorManager.getInstance(project)
            val openFiles = editorManager.openFiles
            if (openFiles.isEmpty()) return@invokeLater
            val connector = ConnectDocumentToLanguageServerSetupParticipant()
            for (file in openFiles) {
                if (file.isValid) {
                    connector.fileOpened(editorManager, file)
                }
            }
        }
    }
}

/**
 * Restarts a running `alint` language server whenever the IDE's trusted-project
 * state changes so the new session re-initializes with updated `trusted`
 * `initializationOptions`. Registered as an application listener in `plugin.xml`
 * because `TrustedProjectsListener.TOPIC` is application-scoped.
 */
class AlintTrustedProjectsListener : TrustedProjectsListener {
    override fun onProjectTrusted(project: Project) {
        AlintTrustCoordinator.onTrustChanged(project)
    }

    override fun onProjectUntrusted(project: Project) {
        AlintTrustCoordinator.onTrustChanged(project)
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
