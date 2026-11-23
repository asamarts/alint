package org.alint.lsp

import com.intellij.ide.impl.isTrusted
import com.intellij.ide.impl.setTrusted
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.testFramework.HeavyPlatformTestCase
import com.intellij.testFramework.PsiTestUtil
import com.intellij.util.ui.UIUtil
import com.redhat.devtools.lsp4ij.LSPIJUtils
import com.redhat.devtools.lsp4ij.LanguageServerItem
import com.redhat.devtools.lsp4ij.LanguageServerManager
import com.redhat.devtools.lsp4ij.LanguageServersRegistry
import com.redhat.devtools.lsp4ij.LanguageServiceAccessor
import com.redhat.devtools.lsp4ij.ServerStatus
import org.eclipse.lsp4j.Diagnostic
import java.io.File
import java.net.URI

/**
 * Headless behavioral e2e for the JetBrains integration: opens a file in
 * a real on-disk project, lets LSP4IJ connect + spawn `alint lsp`, and
 * asserts an alint diagnostic arrives — the one thing buildPlugin
 * (headless load) and verifyPlugin (compat) don't cover.
 *
 * Heavy fixture (not light): `alint lsp` walks the real filesystem from
 * the LSP `rootUri` (= the project base path), so the `.alint.yml` +
 * fixture file must exist on disk. Requires the `alint` binary via
 * $ALINT_TEST_BINARY; skips when unset (e.g. a plain unit-test run).
 */
class AlintLspIntegrationTest : HeavyPlatformTestCase() {

    fun testAlintServerPublishesDiagnostics() {
        val alintBinary = System.getenv("ALINT_TEST_BINARY")
        if (alintBinary.isNullOrBlank()) {
            println("SKIP: ALINT_TEST_BINARY not set")
            return
        }

        val base = File(project.basePath!!)
        base.mkdirs()
        File(base, ".alint.yml").writeText(
            """
            version: 1
            rules:
              - id: no-todo
                kind: file_content_forbidden
                paths: "**/*.py"
                pattern: "TODO"
                level: error
            """.trimIndent() + "\n",
        )
        val badPy = File(base, "bad.py").apply { writeText("x = 1  # TODO\n") }
        LocalFileSystem.getInstance().refresh(false)
        val vFile = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(badPy)
            ?: error("could not find bad.py in VFS")

        // Register the project dir as a content root so LSP4IJ sends it as
        // the LSP rootUri/workspaceFolder — alint resolves its workspace
        // (and discovers .alint.yml) from that, not the process CWD.
        val baseDir = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(base)
            ?: error("could not find project base in VFS")
        PsiTestUtil.addContentRoot(module, baseDir)

        val originalAlintPath = AlintSettings.getInstance().alintPath
        try {
            AlintSettings.getInstance().alintPath = alintBinary
            FileEditorManager.getInstance(project).openFile(vFile, true)

            val uri = LSPIJUtils.toUri(vFile)
            // Matching + connecting the file to the "alint" server (mapped to
            // all files) starts the process and sends didOpen. Hold the future
            // and poll it — blocking .get() on the test's EDT would deadlock
            // LSP4IJ's own EDT work.
            val serversFuture = LanguageServiceAccessor.getInstance(project)
                .getLanguageServers(vFile, { true }, { true })

            val ok = waitUntil(60_000) {
                val servers = serversFuture.getNow(null) ?: return@waitUntil false
                servers.any { diagnosticsFor(it, uri).isNotEmpty() }
            }

            val servers = serversFuture.getNow(null)
            val messages = servers.orEmpty().flatMap { diagnosticsFor(it, uri) }.map { it.message ?: "" }
            assertTrue(
                "no diagnostics from alint within timeout " +
                    "(servers=${servers?.size}, statuses=${servers?.map { it.serverWrapper.serverStatus }})",
                ok,
            )
            assertTrue(
                "expected the no-todo diagnostic, got $messages",
                messages.any { it.contains("TODO") || it.contains("forbidden") },
            )
        } finally {
            AlintSettings.getInstance().alintPath = originalAlintPath
        }
    }

    fun testInitializationOptionsReflectTrustedProjectState() {
        val originalTrusted = project.isTrusted()
        try {
            val provider = AlintLanguageServerFactory().createConnectionProvider(project)
            project.setTrusted(false)
            assertEquals(mapOf("trusted" to false), provider.getInitializationOptions(null))
            project.setTrusted(true)
            assertEquals(mapOf("trusted" to true), provider.getInitializationOptions(null))
        } finally {
            project.setTrusted(originalTrusted)
        }
    }

    fun testTrustChangeDoesNotStartStoppedServerOrReenableDisabledServer() {
        val originalTrusted = project.isTrusted()
        val definition = LanguageServersRegistry.getInstance().getServerDefinition(ALINT_SERVER_ID)
        assertNotNull("alint server definition must be registered", definition)
        val originalEnabled = definition!!.isEnabled(project)
        try {
            // Ensure server is stopped / not started.
            val manager = LanguageServerManager.getInstance(project)
            val initialStatus = manager.getServerStatus(ALINT_SERVER_ID)
            assertTrue(
                "expected server not running before test, got $initialStatus",
                initialStatus == ServerStatus.none || initialStatus == ServerStatus.stopped,
            )

            // Flipping trust while no server is running must NOT spawn `alint lsp`.
            project.setTrusted(false)
            project.setTrusted(true)
            UIUtil.dispatchAllInvocationEvents()
            Thread.sleep(200)
            UIUtil.dispatchAllInvocationEvents()
            val afterFlipStatus = manager.getServerStatus(ALINT_SERVER_ID)
            assertTrue(
                "trust change must not start a stopped server, got $afterFlipStatus",
                afterFlipStatus == ServerStatus.none || afterFlipStatus == ServerStatus.stopped,
            )

            // Disabling the server and flipping trust must NOT re-enable it.
            definition.setEnabled(false, project)
            project.setTrusted(false)
            UIUtil.dispatchAllInvocationEvents()
            Thread.sleep(200)
            UIUtil.dispatchAllInvocationEvents()
            assertFalse("trust change must not re-enable a disabled server", definition.isEnabled(project))
        } finally {
            definition.setEnabled(originalEnabled, project)
            project.setTrusted(originalTrusted)
        }
    }

    fun testWorkspaceTrustControlsCommandRulesAndRestartsServer() {
        val alintBinary = System.getenv("ALINT_TEST_BINARY")
        if (alintBinary.isNullOrBlank()) {
            println("SKIP: ALINT_TEST_BINARY not set")
            return
        }

        val originalAlintPath = AlintSettings.getInstance().alintPath
        val originalTrusted = project.isTrusted()
        try {
            // Start explicitly untrusted before LSP4IJ spawns `alint lsp`.
            project.setTrusted(false)

            val base = File(project.basePath!!)
            base.mkdirs()
            val quotedBinary = alintBinary.replace("\\", "\\\\").replace("\"", "\\\"")
            File(base, ".alint.yml").writeText(
                """
                version: 1
                rules:
                  - id: no-todo
                    kind: file_content_forbidden
                    paths: "**/*.py"
                    pattern: "TODO"
                    level: error
                  - id: cmd-probe
                    kind: command
                    paths: "**/*.py"
                    command: ["$quotedBinary", "--bogus-trust-probe"]
                    level: error
                    message: "command-probe-ran"
                """.trimIndent() + "\n",
            )
            val badPy = File(base, "bad.py").apply { writeText("x = 1  # TODO\n") }
            LocalFileSystem.getInstance().refresh(false)
            val vFile = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(badPy)
                ?: error("could not find bad.py in VFS")
            val baseDir = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(base)
                ?: error("could not find project base in VFS")
            PsiTestUtil.addContentRoot(module, baseDir)

            AlintSettings.getInstance().alintPath = alintBinary
            FileEditorManager.getInstance(project).openFile(vFile, true)

            val uri = LSPIJUtils.toUri(vFile)
            val serversFuture = LanguageServiceAccessor.getInstance(project)
                .getLanguageServers(vFile, { true }, { true })

            // 1. Untrusted: `no-todo` publishes while spawning `cmd-probe` is withheld.
            val untrustedOk = waitUntil(60_000) {
                val servers = serversFuture.getNow(null) ?: return@waitUntil false
                servers.any { srv ->
                    diagnosticsFor(srv, uri).any { (it.message ?: "").contains("TODO") || (it.message ?: "").contains("forbidden") }
                }
            }
            val untrustedServers = serversFuture.getNow(null).orEmpty()
            val untrustedMsgs = untrustedServers
                .flatMap { diagnosticsFor(it, uri) }
                .map { it.message ?: "" }
            assertTrue("expected no-todo diagnostic while untrusted, got $untrustedMsgs", untrustedOk)
            assertFalse(
                "command rule must NOT run in an untrusted project, got $untrustedMsgs",
                untrustedMsgs.any { it.contains("command-probe-ran") },
            )
            val wrapper = untrustedServers.first().serverWrapper
            val untrustedPid = wrapper.currentProcessId
            assertNotNull("expected live untrusted alint lsp PID", untrustedPid)

            // 2. Trust the project: `AlintTrustedProjectsListener` restarts `alint lsp`
            //    with `initializationOptions = { "trusted": true }` and automatically
            //    reconnects open editor tabs so `cmd-probe` publishes without manual
            //    re-open.
            project.setTrusted(true)

            val trustedOk = waitUntil(60_000) {
                val pid = wrapper.currentProcessId
                if (pid == null || pid == untrustedPid) return@waitUntil false
                val data = wrapper.getLSPVirtualFileData(uri) ?: return@waitUntil false
                data.diagnosticsForServer.diagnostics.any { (it.message ?: "").contains("command-probe-ran") }
            }
            val trustedPid = wrapper.currentProcessId
            val trustedMsgs = wrapper.getLSPVirtualFileData(uri)
                ?.diagnosticsForServer?.diagnostics?.map { it.message ?: "" }.orEmpty()
            assertTrue(
                "expected automatic reconnect and command-probe diagnostic after trusting project " +
                    "(oldPid=$untrustedPid, pid=$trustedPid, status=${wrapper.serverStatus}, msgs=$trustedMsgs)",
                trustedOk,
            )

            // 3. Revoke trust: server restarts back into untrusted mode, reconnects
            //    open tabs, and clears `cmd-probe` while keeping `no-todo`.
            project.setTrusted(false)

            val revokedOk = waitUntil(60_000) {
                val pid = wrapper.currentProcessId
                if (pid == null || pid == trustedPid) return@waitUntil false
                val data = wrapper.getLSPVirtualFileData(uri) ?: return@waitUntil false
                val msgs = data.diagnosticsForServer.diagnostics.map { it.message ?: "" }
                msgs.any { it.contains("TODO") || it.contains("forbidden") } &&
                    msgs.none { it.contains("command-probe-ran") }
            }
            val revokedMsgs = wrapper.getLSPVirtualFileData(uri)
                ?.diagnosticsForServer?.diagnostics?.map { it.message ?: "" }.orEmpty()
            assertTrue(
                "expected automatic reconnect and command-probe cleared after revoking trust " +
                    "(trustedPid=$trustedPid, pid=${wrapper.currentProcessId}, status=${wrapper.serverStatus}, msgs=$revokedMsgs)",
                revokedOk,
            )
        } finally {
            AlintSettings.getInstance().alintPath = originalAlintPath
            project.setTrusted(originalTrusted)
        }
    }

    private fun diagnosticsFor(item: LanguageServerItem, uri: URI): List<Diagnostic> {
        val data = item.serverWrapper.getLSPVirtualFileData(uri) ?: return emptyList()
        return data.diagnosticsForServer.diagnostics.toList()
    }

    /** Poll `cond`, pumping the IDE event queue (LSP4IJ is async on the EDT). */
    private fun waitUntil(timeoutMs: Long, cond: () -> Boolean): Boolean {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline) {
            if (cond()) return true
            UIUtil.dispatchAllInvocationEvents()
            Thread.sleep(100)
        }
        return cond()
    }
}
