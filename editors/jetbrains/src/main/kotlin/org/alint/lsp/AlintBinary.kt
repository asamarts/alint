package org.alint.lsp

import com.intellij.openapi.application.PathManager
import org.apache.commons.compress.archivers.tar.TarArchiveInputStream
import org.apache.commons.compress.compressors.gzip.GzipCompressorInputStream
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.net.URI
import java.net.http.HttpClient
import java.net.http.HttpRequest
import java.net.http.HttpResponse
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardCopyOption
import java.security.MessageDigest
import java.time.Duration

/**
 * Binary resolution + managed download, consistent with the other
 * install channels: `alint.path` setting → PATH → previously downloaded
 * copy (versioned by plugin version) → (opt-in) download. Mirrors
 * install.sh's conventions exactly so it fetches the byte-identical
 * release artifact.
 */
object AlintBinary {
    private const val REPO = "asamarts/alint"
    internal const val MAX_ARCHIVE_BYTES: Long = 256L * 1024L * 1024L
    internal const val MAX_SHA256_BYTES: Long = 4096L
    private val SHA256_HEX = Regex("^[0-9a-fA-F]{64}$")

    private val isWindows: Boolean =
        System.getProperty("os.name").lowercase().contains("win")

    private val binaryName: String = if (isWindows) "alint.exe" else "alint"

    /** First of: the `alint.path` setting, `alint` on PATH, a cached
     * download for `version`. `null` when none is present (caller may offer
     * a download). */
    fun resolve(version: String? = null): String? {
        val configured = AlintSettings.getInstance().alintPath.trim()
        if (configured.isNotEmpty()) {
            val candidate = runCatching { Path.of(configured) }.getOrNull()
            if (candidate != null && isExecutableFile(candidate, isWindows)) {
                return candidate.toAbsolutePath().toString()
            }
        }
        findOnPath(System.getenv("PATH"), isWindows)?.let { return it }
        if (!version.isNullOrBlank()) {
            val cached = cacheDir(version).resolve(binaryName)
            if (isExecutableFile(cached, isWindows)) {
                return cached.toAbsolutePath().toString()
            }
        }
        return null
    }

    /** Download the release matching `version` for this platform into the
     * IDE's versioned system cache, verifying its SHA-256. Returns the binary
     * path. */
    fun download(version: String, log: (String) -> Unit): String {
        val target = resolveTarget()
        val tag = "v$version"
        val archive = "alint-$tag-$target.tar.gz"
        val base = "https://github.com/$REPO/releases/download/$tag"

        log("downloading $archive")
        val tarBytes = try {
            fetch("$base/$archive", MAX_ARCHIVE_BYTES)
        } catch (ex: Exception) {
            throw IllegalStateException(
                "could not download alint $tag for $target ($archive): ${ex.message}. " +
                    "The plugin version may be ahead of the published releases — set the " +
                    "`alint.path` setting to a local alint binary, or update the plugin.",
                ex,
            )
        }
        val shaText = String(fetch("$base/$archive.sha256", MAX_SHA256_BYTES), Charsets.UTF_8)
        val expected = parseSha256(shaText, archive)
        val actual = sha256Hex(tarBytes)
        require(expected == actual) {
            "SHA-256 mismatch for $archive (expected $expected, got $actual)"
        }
        log("SHA-256 verified")

        val dir = cacheDir(version)
        Files.createDirectories(dir)
        val dest = dir.resolve(binaryName)
        extractBinary(tarBytes, "alint-$tag-$target/$binaryName", dest, MAX_ARCHIVE_BYTES)
        pruneOldCachedVersions(dir.parent, dir.fileName.toString())
        log("installed $binaryName ($target)")
        return dest.toAbsolutePath().toString()
    }

    private fun cacheDir(version: String): Path =
        Path.of(PathManager.getSystemPath(), "alint", "bin", "v$version")

    private fun pruneOldCachedVersions(binRoot: Path?, keepDirName: String) {
        if (binRoot == null || !Files.isDirectory(binRoot)) return
        runCatching {
            Files.list(binRoot).use { stream ->
                stream.forEach { child ->
                    val name = child.fileName?.toString() ?: return@forEach
                    if (name == keepDirName) return@forEach
                    if ((Files.isDirectory(child) && name.startsWith("v")) ||
                        name == "alint" || name == "alint.exe"
                    ) {
                        runCatching { child.toFile().deleteRecursively() }
                    }
                }
            }
        }
    }

    internal fun isExecutableFile(candidate: Path, windows: Boolean): Boolean =
        runCatching {
            Files.isRegularFile(candidate) && (windows || Files.isExecutable(candidate))
        }.getOrDefault(false)

    internal fun findOnPath(
        pathVar: String?,
        windows: Boolean,
        name: String = if (windows) "alint.exe" else "alint",
        separator: Char = if (windows) ';' else ':',
    ): String? {
        if (pathVar.isNullOrEmpty()) return null
        for (rawDir in pathVar.split(separator)) {
            val dir = if (windows) rawDir.trim().trim('"') else rawDir
            if (dir.isEmpty()) continue
            val candidate = runCatching { Path.of(dir, name) }.getOrNull() ?: continue
            if (isExecutableFile(candidate, windows)) {
                return candidate.toAbsolutePath().toString()
            }
        }
        return null
    }

    internal fun parseSha256(shaText: String, archive: String): String {
        val token = shaText.trim().split(Regex("\\s+")).firstOrNull().orEmpty()
        require(SHA256_HEX.matches(token)) {
            "invalid SHA-256 checksum file for $archive"
        }
        return token.lowercase()
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

    private fun fetch(url: String, maxBytes: Long): ByteArray {
        HttpClient.newBuilder()
            .connectTimeout(Duration.ofSeconds(15))
            .followRedirects(HttpClient.Redirect.NORMAL)
            .build()
            .use { client ->
                val request = HttpRequest.newBuilder(URI.create(url))
                    .timeout(Duration.ofSeconds(30))
                    .GET()
                    .build()
                val response = client.send(request, HttpResponse.BodyHandlers.ofInputStream())
                response.body().use { stream ->
                    require(response.statusCode() == 200) {
                        "HTTP ${response.statusCode()} fetching $url"
                    }
                    return readBounded(stream, maxBytes, url)
                }
            }
    }

    internal fun readBounded(stream: InputStream, maxBytes: Long, label: String): ByteArray {
        val out = ByteArrayOutputStream()
        val buf = ByteArray(16 * 1024)
        var total = 0L
        while (true) {
            val n = stream.read(buf)
            if (n < 0) break
            total += n
            require(total <= maxBytes) {
                "response exceeds $maxBytes bytes ($label)"
            }
            out.write(buf, 0, n)
        }
        return out.toByteArray()
    }

    internal fun sha256Hex(bytes: ByteArray): String =
        MessageDigest.getInstance("SHA-256").digest(bytes)
            .joinToString("") { "%02x".format(it) }

    internal fun normalizeTarEntryName(raw: String): String {
        var name = raw
        while (name.startsWith("./")) {
            name = name.removePrefix("./")
        }
        return name
    }

    internal fun extractBinary(
        tarGz: ByteArray,
        entryName: String,
        dest: Path,
        maxBytes: Long = MAX_ARCHIVE_BYTES,
        windows: Boolean = isWindows,
    ) {
        val expected = normalizeTarEntryName(entryName)
        var found = false
        val tmp = Files.createTempFile(dest.parent, ".${dest.fileName}.", ".tmp")
        try {
            TarArchiveInputStream(GzipCompressorInputStream(ByteArrayInputStream(tarGz))).use { tar ->
                var entry = tar.nextEntry
                while (entry != null) {
                    val normalized = normalizeTarEntryName(entry.name)
                    if (normalized == expected) {
                        require(entry.isFile && !entry.isSymbolicLink && !entry.isLink) {
                            "tar entry $entryName is not a regular file"
                        }
                        Files.newOutputStream(tmp).use { out ->
                            val buf = ByteArray(16 * 1024)
                            var total = 0L
                            while (true) {
                                val n = tar.read(buf)
                                if (n < 0) break
                                total += n
                                require(total <= maxBytes) {
                                    "extracted binary exceeds $maxBytes bytes"
                                }
                                out.write(buf, 0, n)
                            }
                        }
                        found = true
                        break
                    }
                    entry = tar.nextEntry
                }
            }
            if (!found) {
                error("extracted tarball missing $entryName")
            }
            if (!windows) {
                tmp.toFile().setExecutable(true)
            }
            try {
                Files.move(
                    tmp,
                    dest,
                    StandardCopyOption.REPLACE_EXISTING,
                    StandardCopyOption.ATOMIC_MOVE,
                )
            } catch (_: Exception) {
                Files.move(tmp, dest, StandardCopyOption.REPLACE_EXISTING)
            }
        } catch (t: Throwable) {
            runCatching { Files.deleteIfExists(tmp) }
            throw t
        }
    }
}
