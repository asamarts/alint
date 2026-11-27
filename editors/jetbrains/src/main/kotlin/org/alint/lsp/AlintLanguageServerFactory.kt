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
import java.lang.ref.WeakReference
import java.util.Collections
import java.util.WeakHashMap
import java.util.concurrent.atomic.AtomicLong

internal const val ALINT_SERVER_ID = "alint"

/**
 * Registers `alint lsp` with LSP4IJ. Wired in `plugin.xml` under the
 * `com.redhat.devtools.lsp4ij` server extension point.
 */
class AlintLanguageServerFactory : LanguageServerFactory {
    override fun createConnectionProvider(project: Project): StreamConnectionProvider =
        AlintConnectionProvider(project)

    override fun createLanguageClient(project: Project): LanguageClientImpl =
        AlintLanguageClient(project)
}

internal class AlintConnectionProvider(private val project: Project) : ProcessStreamConnectionProvider() {
    val generation: Long = AlintTrustCoordinator.nextGeneration()
    private val hasBinary: Boolean

    init {
        val version = runCatching { AlintNotifier.pluginVersion() }.getOrNull()
        val binary = AlintBinary.resolve(version)
        if (binary != null) {
            hasBinary = true
            AlintNotifier.resetMissingNotice(project)
            super.setCommands(listOf(binary, "lsp"))
            project.basePath?.let { super.setWorkingDirectory(it) }
        } else {
            hasBinary = false
            thisLogger().warn("alint binary not found on PATH or in settings; server not started")
            AlintNotifier.binaryMissing(project)
        }
    }

    override fun start() {
        if (!hasBinary) {
            throw LanguageServerException("alint binary not found on PATH or in Settings -> Tools -> alint")
        }
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
 * Coordinates workspace-trust and settings restarts per project using only
 * public LSP4IJ and IntelliJ Platform APIs.
 */
internal object AlintTrustCoordinator {
    private class ProjectState {
        // WeakReference prevents a strong value -> key chain from defeating
        // WeakHashMap GC of disposed Projects.
        var activeProviderRef: WeakReference<AlintConnectionProvider>? = null
        var lastInitializedTrusted: Boolean? = null
        var restartInFlight: Boolean = false
        var trustRestartInProgress: Boolean = false
        var reconnectOnStart: Boolean = false

        fun activeProvider(): AlintConnectionProvider? = activeProviderRef?.get()
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
            val current = state.activeProvider()
            if (current == null || provider.generation >= current.generation) {
                state.activeProviderRef = WeakReference(provider)
            }
        }
    }

    fun onInitialized(project: Project, provider: AlintConnectionProvider, trusted: Boolean) {
        synchronized(lock) {
            val state = stateOf(project)
            val current = state.activeProvider()
            if (current == null || provider.generation >= current.generation) {
                state.activeProviderRef = WeakReference(provider)
                state.lastInitializedTrusted = trusted
            }
        }
    }

    fun onProviderStopped(project: Project, provider: AlintConnectionProvider) {
        synchronized(lock) {
            val state = states[project] ?: return
            if (state.activeProvider() === provider) {
                state.activeProviderRef = null
                state.lastInitializedTrusted = null
                if (!state.trustRestartInProgress) {
                    state.restartInFlight = false
                    state.reconnectOnStart = false
                }
            }
        }
    }

    fun onSettingsChanged(project: Project) {
        restartRunningServer(project, requireTrustChange = false)
    }

    fun onTrustChanged(project: Project) {
        restartRunningServer(project, requireTrustChange = true)
    }

    private fun restartRunningServer(project: Project, requireTrustChange: Boolean) {
        if (project.isDisposed || project.isDefault) return
        val definition = LanguageServersRegistry.getInstance().getServerDefinition(ALINT_SERVER_ID)
            ?: return
        if (!definition.isEnabled(project)) return

        val manager = LanguageServerManager.getInstance(project)
        val status = manager.getServerStatus(ALINT_SERVER_ID)
        val shouldDispatch = synchronized(lock) {
            val state = stateOf(project)
            val isActiveOrStopping = state.activeProvider() != null ||
                status == ServerStatus.starting ||
                status == ServerStatus.started ||
                status == ServerStatus.stopping
            if (!isActiveOrStopping) {
                return@synchronized false
            }
            if (requireTrustChange) {
                val desiredTrusted = project.isTrusted()
                if (!state.restartInFlight &&
                    state.lastInitializedTrusted == desiredTrusted &&
                    status != ServerStatus.stopping
                ) {
                    return@synchronized false
                }
            }
            state.reconnectOnStart = true
            if (state.restartInFlight) {
                return@synchronized false
            }
            state.restartInFlight = true
            state.trustRestartInProgress = true
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
            val currentManager = LanguageServerManager.getInstance(project)
            val currentStatus = currentManager.getServerStatus(ALINT_SERVER_ID)
            val stillActive = synchronized(lock) {
                val state = states[project] ?: return@synchronized false
                state.activeProvider() != null ||
                    currentStatus == ServerStatus.starting ||
                    currentStatus == ServerStatus.started ||
                    currentStatus == ServerStatus.stopping
            }
            if (!stillActive) {
                clearRestartInFlight(project)
                return@executeOnPooledThread
            }
            try {
                val options = LanguageServerManager.StartOptions()
                    .setWillEnable(false)
                    .setForceStart(false)
                currentManager.start(currentDef, options)
                // If the server finished stopping right before `start()` ran,
                // `forceStart = false` can become a silent no-op and no
                // `ServerStatus.started` event will arrive. Clear the in-flight
                // flags so future restarts cannot wedge permanently.
                val postStatus = currentManager.getServerStatus(ALINT_SERVER_ID)
                if (postStatus == ServerStatus.stopped || postStatus == ServerStatus.none) {
                    clearRestartInFlight(project)
                }
            } catch (_: LanguageServerException) {
                clearRestartInFlight(project)
            } catch (ex: Throwable) {
                clearRestartInFlight(project)
                thisLogger().warn("failed to restart alint language server", ex)
            }
        }
    }

    fun onServerStatusChanged(project: Project, status: ServerStatus) {
        if (project.isDisposed || project.isDefault) return
        if (status == ServerStatus.stopped) {
            synchronized(lock) {
                val state = states[project] ?: return
                if (!state.trustRestartInProgress) {
                    state.restartInFlight = false
                    state.reconnectOnStart = false
                }
            }
            return
        }
        if (status != ServerStatus.started) return

        val (shouldReconnect, needsFollowupRestart) = synchronized(lock) {
            val state = states[project] ?: return
            state.restartInFlight = false
            state.trustRestartInProgress = false
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

    internal fun clearRestartInFlight(project: Project) {
        synchronized(lock) {
            states[project]?.let { state ->
                state.restartInFlight = false
                state.trustRestartInProgress = false
                state.reconnectOnStart = false
            }
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
 * `initializationOptions`.
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
    private val notifiedMissing: MutableSet<Project> =
        Collections.synchronizedSet(Collections.newSetFromMap(WeakHashMap()))

    fun resetMissingNotice(project: Project) {
        notifiedMissing.remove(project)
    }

    fun binaryMissing(project: Project) {
        if (!notifiedMissing.add(project)) {
            return
        }
        val group = NotificationGroupManager.getInstance().getNotificationGroup("alint")
        group
            .createNotification(
                "alint binary not found",
                "Set its path in Settings -> Tools -> alint, install it (brew / cargo / npm), or download the matching release.",
                NotificationType.WARNING,
            )
            .addAction(object : NotificationAction("Download matching release") {
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
                val version = pluginVersion()
                val path = AlintBinary.download(version) { thisLogger().info("alint: $it") }
                // Do NOT write the managed cache path into `AlintSettings.alintPath`:
                // `AlintBinary.resolve(version)` probes the versioned cache dir
                // automatically, so future plugin upgrades or a newly installed
                // PATH binary are picked up without getting stuck on an old path.
                resetMissingNotice(project)
                group
                    .createNotification(
                        "alint downloaded",
                        "Installed to $path. Restart the alint language server (or the IDE) to use it.",
                        NotificationType.INFORMATION,
                    )
                    .notify(project)
            } catch (ex: Exception) {
                resetMissingNotice(project)
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

    // Stamped into a classpath resource at build time (`generateVersionResource`
    // in build.gradle.kts) so we avoid Marketplace-denied PluginManager* APIs.
    internal fun pluginVersion(): String =
        AlintNotifier::class.java.classLoader
            .getResourceAsStream("alint-lsp/version.txt")
            ?.use { it.readBytes().toString(Charsets.UTF_8).trim() }
            ?: error("alint-lsp/version.txt not on the classpath; build.gradle.kts wiring broken")
}
