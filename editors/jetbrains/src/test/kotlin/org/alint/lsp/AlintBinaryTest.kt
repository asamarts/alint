package org.alint.lsp

import org.apache.commons.compress.archivers.tar.TarArchiveEntry
import org.apache.commons.compress.archivers.tar.TarArchiveOutputStream
import org.apache.commons.compress.compressors.gzip.GzipCompressorOutputStream
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayOutputStream
import java.nio.file.Files

class AlintBinaryTest {
    private fun makeTarGz(entries: List<Triple<String, ByteArray?, Byte>>): ByteArray {
        val out = ByteArrayOutputStream()
        GzipCompressorOutputStream(out).use { gz ->
            TarArchiveOutputStream(gz).use { tar ->
                tar.setLongFileMode(TarArchiveOutputStream.LONGFILE_POSIX)
                for ((name, bytes, linkFlag) in entries) {
                    val entry = TarArchiveEntry(name, linkFlag)
                    if (bytes != null) {
                        entry.size = bytes.size.toLong()
                    }
                    tar.putArchiveEntry(entry)
                    if (bytes != null) {
                        tar.write(bytes)
                    }
                    tar.closeArchiveEntry()
                }
            }
        }
        return out.toByteArray()
    }

    @Test
    fun parseSha256AcceptsUppercaseAndRejectsMalformed() {
        val upper = "A".repeat(64)
        assertEquals("a".repeat(64), AlintBinary.parseSha256("$upper  alint.tar.gz\n", "alint.tar.gz"))
        var failed = false
        try {
            AlintBinary.parseSha256("not-hex  alint.tar.gz", "alint.tar.gz")
        } catch (_: IllegalArgumentException) {
            failed = true
        }
        assertTrue("malformed sha256 should fail", failed)
    }

    @Test
    fun extractBinaryHandlesDotSlashAtomicReplaceAndRejectsSymlinkOrOversize() {
        val dir = Files.createTempDirectory("alint-jb-bin-")
        try {
            val dest = dir.resolve("alint")
            Files.write(dest, "old".toByteArray())
            val payload = "new-binary".toByteArray()
            val tarGz = makeTarGz(
                listOf(
                    Triple("./alint-v0.23.0-x86_64-unknown-linux-musl/alint", payload, TarArchiveEntry.LF_NORMAL),
                ),
            )
            AlintBinary.extractBinary(tarGz, "alint-v0.23.0-x86_64-unknown-linux-musl/alint", dest, 1024, false)
            assertArrayEquals(payload, Files.readAllBytes(dest))

            // Symlink entry with matching name must be rejected.
            val symlinkTar = makeTarGz(
                listOf(
                    Triple("alint-v0.23.0-x86_64-unknown-linux-musl/alint", null, TarArchiveEntry.LF_SYMLINK),
                ),
            )
            var symlinkFailed = false
            try {
                AlintBinary.extractBinary(symlinkTar, "alint-v0.23.0-x86_64-unknown-linux-musl/alint", dest, 1024, false)
            } catch (_: IllegalArgumentException) {
                symlinkFailed = true
            }
            assertTrue("symlink entry should be rejected", symlinkFailed)

            // Oversized payload must be rejected and leave existing dest untouched.
            var oversizeFailed = false
            try {
                AlintBinary.extractBinary(tarGz, "alint-v0.23.0-x86_64-unknown-linux-musl/alint", dest, 4, false)
            } catch (_: IllegalArgumentException) {
                oversizeFailed = true
            }
            assertTrue("oversized entry should be rejected", oversizeFailed)
            assertArrayEquals(payload, Files.readAllBytes(dest))
        } finally {
            dir.toFile().deleteRecursively()
        }
    }

    @Test
    fun findOnPathStripsWindowsQuotesSkipsDirsAndInvalidChars() {
        val dir = Files.createTempDirectory("alint-jb-path-")
        try {
            val d1 = Files.createDirectories(dir.resolve("d1"))
            Files.createDirectories(d1.resolve("alint")) // directory named alint
            val d2 = Files.createDirectories(dir.resolve("Program Files").resolve("alint"))
            val exe = d2.resolve("alint.exe")
            Files.write(exe, "MZ".toByteArray())

            val winPath = "\"${d1}\";\"${d2}\""
            val found = AlintBinary.findOnPath(winPath, windows = true, name = "alint.exe", separator = ';')
            assertEquals(exe.toAbsolutePath().toString(), found)

            // Null byte causes InvalidPathException — must be skipped, not crash.
            val badPath = "bad\u0000path"
            assertNull(AlintBinary.findOnPath(badPath, windows = true, name = "alint.exe", separator = ';'))
            assertFalse(AlintBinary.isExecutableFile(d1.resolve("alint"), windows = false))
        } finally {
            dir.toFile().deleteRecursively()
        }
    }
}
