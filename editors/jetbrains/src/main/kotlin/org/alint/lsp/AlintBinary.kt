package org.alint.lsp

import com.intellij.openapi.application.PathManager
import org.apache.commons.compress.archivers.tar.TarArchiveInputStream
import org.apache.commons.compress.compressors.gzip.GzipCompressorInputStream
import java.io.ByteArrayInputStream
import java.net.URI
import java.net.http.HttpClient
import java.net.http.HttpRequest
import java.net.http.HttpResponse
import java.nio.file.Files
import java.nio.file.Path
import java.security.MessageDigest

/**
 * Binary resolution + managed download, consistent with the other
 * install channels: `alint.path` setting → PATH → previously downloaded
 * copy → (opt-in) download. Mirrors install.sh's conventions exactly so it
 * fetches the byte-identical release artifact.
 */
object AlintBinary {
    private const val REPO = "asamarts/alint"

    private val isWindows: Boolean =
        System.getProperty("os.name").lowercase().contains("win")

    private val binaryName: String = if (isWindows) "alint.exe" else "alint"

    /** First of: the `alint.path` setting, `alint` on PATH, a cached
     * download. `null` when none is present (caller may offer a download). */
    fun resolve(): String? {
        val configured = AlintSettings.getInstance().alintPath.trim()
        if (configured.isNotEmpty() && Path.of(configured).toFile().exists()) {
            return configured
        }
        findOnPath()?.let { return it }
        val cached = cacheDir().resolve(binaryName)
        if (Files.exists(cached)) {
            return cached.toAbsolutePath().toString()
        }
        return null
    }

    /** Download the release matching `version` for this platform into the
     * IDE's system cache, verifying its SHA-256. Returns the binary path. */
    fun download(version: String, log: (String) -> Unit): String {
        val target = resolveTarget()
        val tag = "v$version"
        val archive = "alint-$tag-$target.tar.gz"
        val base = "https://github.com/$REPO/releases/download/$tag"

        log("downloading $archive")
        val tarBytes = try {
            fetch("$base/$archive")
        } catch (ex: Exception) {
            // Most often a 404: the plugin's version is ahead of the
            // published releases (e.g. a locally built plugin still
            // carrying the committed default version). Give an actionable
            // message rather than a bare HTTP status.
            throw IllegalStateException(
                "could not download alint $tag for $target ($archive): ${ex.message}. " +
                    "The plugin version may be ahead of the published releases — set the " +
                    "`alint.path` setting to a local alint binary, or update the plugin.",
                ex,
            )
        }
        val shaText = String(fetch("$base/$archive.sha256"))
        val expected = shaText.trim().split(Regex("\\s+")).first()
        val actual = sha256Hex(tarBytes)
        require(expected == actual) {
            "SHA-256 mismatch for $archive (expected $expected, got $actual)"
        }
        log("SHA-256 verified")

        val dir = cacheDir()
        Files.createDirectories(dir)
        val dest = dir.resolve(binaryName)
        extractBinary(tarBytes, "alint-$tag-$target/$binaryName", dest)
        if (!isWindows) {
            dest.toFile().setExecutable(true)
        }
        log("installed $binaryName ($target)")
        return dest.toAbsolutePath().toString()
    }

    private fun cacheDir(): Path = Path.of(PathManager.getSystemPath(), "alint", "bin")

    private fun findOnPath(): String? {
        val pathVar = System.getenv("PATH") ?: return null
        for (dir in pathVar.split(java.io.File.pathSeparatorChar)) {
            if (dir.isEmpty()) continue
            val candidate = Path.of(dir, binaryName)
            if (Files.exists(candidate)) {
                return candidate.toAbsolutePath().toString()
            }
        }
        return null
    }

    /** Map the JVM os.name/os.arch to the alint release-target triple.
     * Mirrors `npm/platforms.json` and the release.yml build matrix. */
    private fun resolveTarget(): String {
        val os = System.getProperty("os.name").lowercase()
        val arch = when (System.getProperty("os.arch").lowercase()) {
            "amd64", "x86_64" -> "x86_64"
            "aarch64", "arm64" -> "aarch64"
            else -> error("unsupported arch ${System.getProperty("os.arch")}")
        }
        return when {
            os.contains("linux") -> "$arch-unknown-linux-musl"
            os.contains("mac") || os.contains("darwin") -> "$arch-apple-darwin"
            os.contains("win") -> "$arch-pc-windows-msvc"
            else -> error("unsupported OS $os")
        }
    }

    private fun fetch(url: String): ByteArray {
        val client = HttpClient.newBuilder()
            .followRedirects(HttpClient.Redirect.NORMAL)
            .build()
        val request = HttpRequest.newBuilder(URI.create(url)).GET().build()
        val response = client.send(request, HttpResponse.BodyHandlers.ofByteArray())
        require(response.statusCode() == 200) {
            "HTTP ${response.statusCode()} fetching $url"
        }
        return response.body()
    }

    private fun sha256Hex(bytes: ByteArray): String =
        MessageDigest.getInstance("SHA-256").digest(bytes)
            .joinToString("") { "%02x".format(it) }

    private fun extractBinary(tarGz: ByteArray, entryName: String, dest: Path) {
        TarArchiveInputStream(GzipCompressorInputStream(ByteArrayInputStream(tarGz))).use { tar ->
            var entry = tar.nextEntry
            while (entry != null) {
                // Exact match only (the release tarball's known layout is
                // `alint-<tag>-<target>/alint`); matching any nested
                // `*/alint` could pick up an unexpected entry.
                if (entry.name == entryName) {
                    Files.newOutputStream(dest).use { tar.copyTo(it) }
                    return
                }
                entry = tar.nextEntry
            }
        }
        error("extracted tarball missing $entryName")
    }
}
