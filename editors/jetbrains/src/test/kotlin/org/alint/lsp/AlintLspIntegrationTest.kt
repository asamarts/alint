package org.alint.lsp

import com.intellij.ide.impl.setTrusted
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.testFramework.HeavyPlatformTestCase
import com.intellij.testFramework.PsiTestUtil
import com.intellij.util.ui.UIUtil
import com.redhat.devtools.lsp4ij.LSPIJUtils
import com.redhat.devtools.lsp4ij.LanguageServerItem
import com.redhat.devtools.lsp4ij.LanguageServiceAccessor
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
    }

    fun testInitializationOptionsReflectTrustedProjectState() {
        val provider = AlintLanguageServerFactory().createConnectionProvider(project)
        project.setTrusted(false)
        assertEquals(mapOf("trusted" to false), provider.getInitializationOptions(null))
        project.setTrusted(true)
        assertEquals(mapOf("trusted" to true), provider.getInitializationOptions(null))
    }

    fun testWorkspaceTrustControlsCommandRulesAndRestartsServer() {
        val alintBinary = System.getenv("ALINT_TEST_BINARY")
        if (alintBinary.isNullOrBlank()) {
            println("SKIP: ALINT_TEST_BINARY not set")
            return
        }

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
        val untrustedWrapper = untrustedServers.first().serverWrapper
        val untrustedPid = untrustedWrapper.currentProcessId
        assertNotNull("expected live untrusted alint lsp PID", untrustedPid)

        // 2. Trust the project: `AlintTrustedProjectsListener` restarts `alint lsp`
        //    with `initializationOptions = { "trusted": true }`, unlocking `cmd-probe`.
        project.setTrusted(true)

        val restartedForTrust = waitUntil(60_000) {
            val pid = untrustedWrapper.currentProcessId
            pid != null && pid != untrustedPid
        }
        assertTrue(
            "expected AlintTrustedProjectsListener to restart alint lsp on trust " +
                "(pid stayed $untrustedPid, status=${untrustedWrapper.serverStatus})",
            restartedForTrust,
        )
        val trustedPid = untrustedWrapper.currentProcessId

        // Reconnect `bad.py` to the restarted server (LSP4IJ 0.7.0's `restart()`
        // disconnects open documents before restarting).
        val trustedServersFuture = LanguageServiceAccessor.getInstance(project)
            .getLanguageServers(vFile, { true }, { true })
        val trustedOk = waitUntil(60_000) {
            val servers = trustedServersFuture.getNow(null) ?: return@waitUntil false
            servers.any { srv ->
                diagnosticsFor(srv, uri).any { (it.message ?: "").contains("command-probe-ran") }
            }
        }
        val trustedMsgs = trustedServersFuture.getNow(null).orEmpty()
            .flatMap { diagnosticsFor(it, uri) }
            .map { it.message ?: "" }
        assertTrue(
            "expected command rule diagnostic after trusting project, got $trustedMsgs",
            trustedOk,
        )

        // 3. Revoke trust: server restarts back into untrusted mode and clears `cmd-probe`.
        project.setTrusted(false)

        val restartedForUntrust = waitUntil(60_000) {
            val pid = untrustedWrapper.currentProcessId
            pid != null && pid != trustedPid
        }
        assertTrue(
            "expected AlintTrustedProjectsListener to restart alint lsp when trust is revoked " +
                "(pid stayed $trustedPid, status=${untrustedWrapper.serverStatus})",
            restartedForUntrust,
        )

        val revokedServersFuture = LanguageServiceAccessor.getInstance(project)
            .getLanguageServers(vFile, { true }, { true })
        val revokedOk = waitUntil(60_000) {
            val servers = revokedServersFuture.getNow(null) ?: return@waitUntil false
            servers.isNotEmpty() && servers.all { srv ->
                val msgs = diagnosticsFor(srv, uri).map { it.message ?: "" }
                msgs.any { it.contains("TODO") || it.contains("forbidden") } &&
                    msgs.none { it.contains("command-probe-ran") }
            }
        }
        val revokedMsgs = revokedServersFuture.getNow(null).orEmpty()
            .flatMap { diagnosticsFor(it, uri) }
            .map { it.message ?: "" }
        assertTrue(
            "expected command rule diagnostic to clear after revoking trust, got $revokedMsgs",
            revokedOk,
        )
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
