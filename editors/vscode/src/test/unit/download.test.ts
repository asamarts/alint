// Unit tests for the dependency-free `.tar.gz` extractor and `downloadAlint`.

import * as assert from "node:assert/strict";
import * as childProcess from "node:child_process";
import * as crypto from "node:crypto";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import * as zlib from "node:zlib";

import { downloadAlint } from "../../download";
import { extractSingleFileFromTarGz } from "../../tar";

interface TarEntrySpec {
  name: string;
  prefix?: string;
  typeflag?: string;
  content?: Buffer;
  linkname?: string;
  pax?: Record<string, string>;
  gnuLongName?: string;
}

function writeOctal(buf: Buffer, offset: number, length: number, value: number): void {
  const digits = value.toString(8).padStart(length - 1, "0");
  buf.write(digits, offset, length - 1, "ascii");
  buf[offset + length - 1] = 0;
}

function buildHeader(
  name: string,
  size: number,
  typeflag: string,
  prefix = "",
  linkname = "",
): Buffer {
  const header = Buffer.alloc(512, 0);
  header.write(name, 0, Math.min(Buffer.byteLength(name), 100), "utf8");
  writeOctal(header, 100, 8, typeflag === "5" ? 0o755 : 0o644);
  writeOctal(header, 108, 8, 0);
  writeOctal(header, 116, 8, 0);
  writeOctal(header, 124, 12, size);
  writeOctal(header, 136, 12, 1_700_000_000);
  header.fill(0x20, 148, 156);
  header[156] = typeflag.charCodeAt(0);
  if (linkname) {
    header.write(linkname, 157, Math.min(Buffer.byteLength(linkname), 100), "utf8");
  }
  header.write("ustar\0", 257, 6, "ascii");
  header.write("00", 263, 2, "ascii");
  if (prefix) {
    header.write(prefix, 345, Math.min(Buffer.byteLength(prefix), 155), "utf8");
  }
  let sum = 0;
  for (let i = 0; i < 512; i++) {
    sum += header[i];
  }
  const chksum = sum.toString(8).padStart(6, "0");
  header.write(chksum, 148, 6, "ascii");
  header[154] = 0;
  header[155] = 0x20;
  return header;
}

function padToBlock(buf: Buffer): Buffer {
  const rem = buf.length % 512;
  if (rem === 0) {
    return buf;
  }
  return Buffer.concat([buf, Buffer.alloc(512 - rem, 0)]);
}

function encodePax(records: Record<string, string>): Buffer {
  const parts: Buffer[] = [];
  for (const [k, v] of Object.entries(records)) {
    const rest = Buffer.from(` ${k}=${v}\n`, "utf8");
    let len = rest.length + 1;
    while (true) {
      const candidate = Buffer.byteLength(String(len), "ascii") + rest.length;
      if (candidate === len) {
        break;
      }
      len = candidate;
    }
    parts.push(Buffer.concat([Buffer.from(String(len), "ascii"), rest]));
  }
  return Buffer.concat(parts);
}

function buildTarGz(entries: TarEntrySpec[]): Buffer {
  const chunks: Buffer[] = [];
  for (const entry of entries) {
    if (entry.pax) {
      const paxData = encodePax(entry.pax);
      chunks.push(buildHeader("PaxHeaders.0/entry", paxData.length, "x"));
      chunks.push(padToBlock(paxData));
    }
    if (entry.gnuLongName !== undefined) {
      const nameData = Buffer.from(`${entry.gnuLongName}\0`, "utf8");
      chunks.push(buildHeader("././@LongLink", nameData.length, "L"));
      chunks.push(padToBlock(nameData));
    }
    const content = entry.content ?? Buffer.alloc(0);
    const typeflag = entry.typeflag ?? "0";
    chunks.push(
      buildHeader(
        entry.name,
        content.length,
        typeflag,
        entry.prefix ?? "",
        entry.linkname ?? "",
      ),
    );
    if (content.length > 0) {
      chunks.push(padToBlock(content));
    }
  }
  chunks.push(Buffer.alloc(1024, 0));
  return zlib.gzipSync(Buffer.concat(chunks));
}

function sha256File(archiveName: string, data: Buffer): Buffer {
  const hex = crypto.createHash("sha256").update(data).digest("hex");
  return Buffer.from(`${hex}  ${archiveName}\n`, "utf8");
}

function makeTempDir(): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), "alint-vscode-unit-"));
}

describe("downloadAlint + USTAR extractor", () => {
  const releaseShapes: Array<{
    label: string;
    platform: string;
    arch: string;
    target: string;
    binName: string;
    leadDotSlash?: boolean;
  }> = [
    {
      label: "Linux x64",
      platform: "linux",
      arch: "x64",
      target: "x86_64-unknown-linux-musl",
      binName: "alint",
    },
    {
      label: "macOS arm64 (with ./ prefix)",
      platform: "darwin",
      arch: "arm64",
      target: "aarch64-apple-darwin",
      binName: "alint",
      leadDotSlash: true,
    },
    {
      label: "Windows x64",
      platform: "win32",
      arch: "x64",
      target: "x86_64-pc-windows-msvc",
      binName: "alint.exe",
    },
  ];

  for (const shape of releaseShapes) {
    it(`downloads, verifies SHA-256, and extracts ${shape.label} release archive`, async () => {
      const version = "0.24.0";
      const baseDir = `alint-v${version}-${shape.target}`;
      const prefix = shape.leadDotSlash ? "./" : "";
      const payload = Buffer.from(`fake-binary-${shape.target}\x00\x01\x02`, "binary");
      const archive = buildTarGz([
        { name: `${prefix}${baseDir}/`, typeflag: "5" },
        {
          name: `${prefix}${baseDir}/README.md`,
          content: Buffer.from("# alint\n", "utf8"),
        },
        {
          name: `${prefix}${baseDir}/THIRD-PARTY-LICENSES.html`,
          content: Buffer.from("<html>licenses</html>", "utf8"),
        },
        {
          name: `${prefix}${baseDir}/${shape.binName}`,
          content: payload,
        },
        {
          name: `${prefix}${baseDir}/docs/ARCHITECTURE.md`,
          content: Buffer.from("# arch\n", "utf8"),
        },
      ]);

      const archiveName = `${baseDir}.tar.gz`;
      const expectedTarUrl = `https://github.com/asamarts/alint/releases/download/v${version}/${archiveName}`;
      const expectedShaUrl = `${expectedTarUrl}.sha256`;
      const shaBuf = sha256File(archiveName, archive);

      const destDir = makeTempDir();
      try {
        const logs: string[] = [];
        const installed = await downloadAlint(
          version,
          destDir,
          (msg) => logs.push(msg),
          {
            platform: shape.platform,
            arch: shape.arch,
            fetchFn: async (url) => {
              if (url === expectedTarUrl) {
                return archive;
              }
              if (url === expectedShaUrl) {
                return shaBuf;
              }
              throw new Error(`unexpected URL: ${url}`);
            },
          },
        );

        assert.equal(installed, path.join(destDir, shape.binName));
        assert.deepEqual(fs.readFileSync(installed), payload);
        // Only the target binary lands in destDir (no unpacked README/docs/temp files).
        assert.deepEqual(fs.readdirSync(destDir), [shape.binName]);
        if (shape.platform !== "win32" && process.platform !== "win32") {
          const mode = fs.statSync(installed).mode & 0o777;
          assert.equal(mode, 0o755);
        }
        assert.ok(logs.some((l) => l.includes("SHA-256 verified")));
      } finally {
        fs.rmSync(destDir, { recursive: true, force: true });
      }
    });
  }

  it("supports USTAR prefix, PAX extended headers, and GNU long names", () => {
    const payload = Buffer.from("pax-binary-bytes", "utf8");
    const ustarPrefixArchive = buildTarGz([
      {
        prefix: "alint-v0.24.0-x86_64-unknown-linux-musl",
        name: "alint",
        content: payload,
      },
    ]);
    assert.deepEqual(
      extractSingleFileFromTarGz(
        ustarPrefixArchive,
        "alint-v0.24.0-x86_64-unknown-linux-musl/alint",
      ),
      payload,
    );

    const paxArchive = buildTarGz([
      {
        name: "truncated",
        pax: {
          path: "alint-v0.24.0-aarch64-apple-darwin/alint",
          size: String(payload.length),
        },
        content: payload,
      },
    ]);
    assert.deepEqual(
      extractSingleFileFromTarGz(
        paxArchive,
        "alint-v0.24.0-aarch64-apple-darwin/alint",
      ),
      payload,
    );

    const gnuArchive = buildTarGz([
      {
        name: "short",
        gnuLongName: "alint-v0.24.0-x86_64-pc-windows-msvc/alint.exe",
        content: payload,
      },
    ]);
    assert.deepEqual(
      extractSingleFileFromTarGz(
        gnuArchive,
        "alint-v0.24.0-x86_64-pc-windows-msvc/alint.exe",
      ),
      payload,
    );
  });

  it("rejects SHA-256 mismatches before touching destDir", async () => {
    const destDir = makeTempDir();
    try {
      const archive = buildTarGz([
        {
          name: "alint-v0.24.0-x86_64-unknown-linux-musl/alint",
          content: Buffer.from("ok"),
        },
      ]);
      await assert.rejects(
        () =>
          downloadAlint("0.24.0", destDir, () => {}, {
            platform: "linux",
            arch: "x64",
            fetchFn: async (url) =>
              url.endsWith(".sha256")
                ? Buffer.from(`${"0".repeat(64)}  alint.tar.gz\n`)
                : archive,
          }),
        /SHA-256 mismatch/,
      );
      assert.deepEqual(fs.readdirSync(destDir), []);
    } finally {
      fs.rmSync(destDir, { recursive: true, force: true });
    }
  });

  it("rejects archives missing the expected binary", () => {
    const archive = buildTarGz([
      {
        name: "alint-v0.24.0-x86_64-unknown-linux-musl/README.md",
        content: Buffer.from("hi"),
      },
    ]);
    assert.throws(
      () =>
        extractSingleFileFromTarGz(
          archive,
          "alint-v0.24.0-x86_64-unknown-linux-musl/alint",
        ),
      /missing expected/,
    );
  });

  it("rejects symlinks, hardlinks, traversal, absolute paths, and duplicates", () => {
    const expected = "alint-v0.24.0-x86_64-unknown-linux-musl/alint";
    const goodEntry: TarEntrySpec = { name: expected, content: Buffer.from("bin") };

    // Symlink placed AFTER the valid binary must still be rejected.
    const withTrailingSymlink = buildTarGz([
      goodEntry,
      {
        name: "alint-v0.24.0-x86_64-unknown-linux-musl/evil",
        typeflag: "2",
        linkname: "/etc/passwd",
      },
    ]);
    assert.throws(
      () => extractSingleFileFromTarGz(withTrailingSymlink, expected),
      /unsupported link entry/,
    );

    const withHardlink = buildTarGz([
      {
        name: expected,
        typeflag: "1",
        linkname: "other",
      },
    ]);
    assert.throws(
      () => extractSingleFileFromTarGz(withHardlink, expected),
      /unsupported link entry/,
    );

    for (const badPath of [
      "../escape",
      "alint-v0.24.0-x86_64-unknown-linux-musl/../../escape",
      "/etc/passwd",
      "C:/Windows/win.ini",
      "alint-v0.24.0-x86_64-unknown-linux-musl\\alint",
    ]) {
      const archive = buildTarGz([
        goodEntry,
        { name: badPath, content: Buffer.from("x") },
      ]);
      assert.throws(
        () => extractSingleFileFromTarGz(archive, expected),
        /traversal|absolute|backslash/,
        `expected rejection for ${badPath}`,
      );
    }

    const withDuplicate = buildTarGz([goodEntry, goodEntry]);
    assert.throws(
      () => extractSingleFileFromTarGz(withDuplicate, expected),
      /duplicate entry/,
    );
  });

  it("rejects corrupt header checksums, truncated archives, and oversized gzip bombs", () => {
    const expected = "alint-v0.24.0-x86_64-unknown-linux-musl/alint";
    const goodTar = zlib.gunzipSync(
      buildTarGz([{ name: expected, content: Buffer.from("bin") }]),
    );

    const badChecksum = Buffer.from(goodTar);
    badChecksum[10] ^= 0xff;
    assert.throws(
      () => extractSingleFileFromTarGz(zlib.gzipSync(badChecksum), expected),
      /checksum/,
    );

    const truncated = goodTar.subarray(0, 600);
    assert.throws(
      () => extractSingleFileFromTarGz(zlib.gzipSync(truncated), expected),
      /truncated/,
    );

    // Gzip bomb exceeding maxUncompressedBytes is rejected by node:zlib.
    const bigPayload = Buffer.alloc(64 * 1024, 0x41);
    const bigArchive = buildTarGz([{ name: expected, content: bigPayload }]);
    assert.throws(
      () => extractSingleFileFromTarGz(bigArchive, expected, 8 * 1024),
      /maxOutputLength|Cannot create a Buffer|ERR_BUFFER_TOO_LARGE|length/i,
    );
  });

  it("extracts archives produced by the real system `tar -czf` tool", function () {
    const tmp = makeTempDir();
    try {
      const version = "0.24.0";
      const baseDir = `alint-v${version}-x86_64-unknown-linux-musl`;
      const stageDir = path.join(tmp, baseDir);
      fs.mkdirSync(path.join(stageDir, "docs"), { recursive: true });
      const payload = crypto.randomBytes(4096);
      fs.writeFileSync(path.join(stageDir, "alint"), payload, { mode: 0o755 });
      fs.writeFileSync(path.join(stageDir, "README.md"), "# alint\n");
      fs.writeFileSync(path.join(stageDir, "THIRD-PARTY-LICENSES.html"), "<html></html>");
      fs.writeFileSync(path.join(stageDir, "docs", "ARCHITECTURE.md"), "# arch\n");

      const archivePath = path.join(tmp, `${baseDir}.tar.gz`);
      try {
        childProcess.execFileSync("tar", ["-czf", archivePath, "-C", tmp, baseDir], {
          stdio: "ignore",
        });
      } catch {
        this.skip();
        return;
      }
      const archiveBytes = fs.readFileSync(archivePath);
      const extracted = extractSingleFileFromTarGz(archiveBytes, `${baseDir}/alint`);
      assert.deepEqual(extracted, payload);
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("hardens path normalization, typeflags, PAX/GNU metadata, and archive framing", () => {
    const expected = "alint-v0.24.0-x86_64-unknown-linux-musl/alint";
    const goodEntry: TarEntrySpec = { name: expected, content: Buffer.from("bin") };

    // Post-dot-slash absolute/drive paths, empty segments, internal `.`, control chars, trailing slash on file.
    for (const badPath of [
      ".//etc/passwd",
      "././C:/Windows/win.ini",
      "alint-v0.24.0-x86_64-unknown-linux-musl/./alint",
      "alint-v0.24.0-x86_64-unknown-linux-musl//alint",
      "alint-v0.24.0-x86_64-unknown-linux-musl/file/",
      "alint-v0.24.0-x86_64-unknown-linux-musl/bad\x07name",
      "",
    ]) {
      const archive = buildTarGz([goodEntry, { name: badPath, content: Buffer.from("x") }]);
      assert.throws(
        () => extractSingleFileFromTarGz(archive, expected),
        /empty|absolute|traversal|control|non-canonical/,
        `expected rejection for ${JSON.stringify(badPath)}`,
      );
    }

    // Case-colliding duplicate entry against target path.
    const caseDup = buildTarGz([
      goodEntry,
      { name: "alint-v0.24.0-x86_64-unknown-linux-musl/ALINT", content: Buffer.from("evil") },
    ]);
    assert.throws(
      () => extractSingleFileFromTarGz(caseDup, expected),
      /duplicate entry/,
    );

    // Disallowed typeflags (char/block/FIFO/sparse) and non-zero-sized directory.
    for (const flag of ["3", "4", "6", "S"]) {
      const badType = buildTarGz([goodEntry, { name: "dir/special", typeflag: flag }]);
      assert.throws(
        () => extractSingleFileFromTarGz(badType, expected),
        /unsupported.*typeflag/,
        `expected rejection for typeflag ${flag}`,
      );
    }
    const nonZeroDir = buildTarGz([
      goodEntry,
      { name: "dir/", typeflag: "5", content: Buffer.from("data") },
    ]);
    assert.throws(
      () => extractSingleFileFromTarGz(nonZeroDir, expected),
      /non-zero size/,
    );

    // PAX global header smuggling `path` or `size`, PAX `linkpath`, sparse, empty path.
    const paxGlobal = zlib.gzipSync(
      Buffer.concat([
        buildHeader("pax_global_header", encodePax({ path: expected }).length, "g"),
        padToBlock(encodePax({ path: expected })),
        buildHeader("other", 3, "0"),
        padToBlock(Buffer.from("bin")),
        Buffer.alloc(1024, 0),
      ]),
    );
    assert.throws(
      () => extractSingleFileFromTarGz(paxGlobal, expected),
      /unsupported PAX global key/,
    );

    const badPaxCases: Array<Record<string, string>> = [
      { linkpath: "/etc/passwd" },
      { "GNU.sparse.major": "1" },
      { "SCHILY.realsize": "999" },
      { path: "" },
    ];
    for (const badPax of badPaxCases) {
      const archive = buildTarGz([
        goodEntry,
        { name: "x", pax: badPax, content: Buffer.from("x") },
      ]);
      assert.throws(
        () => extractSingleFileFromTarGz(archive, expected),
        /unsupported PAX|empty path override/,
      );
    }

    // Stacked duplicate PAX 'x' or GNU 'L' headers, dangling 'x' before EOF, non-NUL tail in 'L'.
    const paxData = encodePax({ path: expected });
    const stackedPax = zlib.gzipSync(
      Buffer.concat([
        buildHeader("PaxHeaders/1", paxData.length, "x"),
        padToBlock(paxData),
        buildHeader("PaxHeaders/2", paxData.length, "x"),
        padToBlock(paxData),
        buildHeader("alint", 3, "0"),
        padToBlock(Buffer.from("bin")),
        Buffer.alloc(1024, 0),
      ]),
    );
    assert.throws(() => extractSingleFileFromTarGz(stackedPax, expected), /stacked PAX/);

    const danglingPax = zlib.gzipSync(
      Buffer.concat([
        buildHeader("alint-v0.24.0-x86_64-unknown-linux-musl/alint", 3, "0"),
        padToBlock(Buffer.from("bin")),
        buildHeader("PaxHeaders/dangle", paxData.length, "x"),
        padToBlock(paxData),
        Buffer.alloc(1024, 0),
      ]),
    );
    assert.throws(() => extractSingleFileFromTarGz(danglingPax, expected), /dangling/);

    const embeddedNulLong = Buffer.from("a/b\0extra\0", "utf8");
    const badLong = zlib.gzipSync(
      Buffer.concat([
        buildHeader("././@LongLink", embeddedNulLong.length, "L"),
        padToBlock(embeddedNulLong),
        buildHeader("short", 3, "0"),
        padToBlock(Buffer.from("bin")),
        Buffer.alloc(1024, 0),
      ]),
    );
    assert.throws(() => extractSingleFileFromTarGz(badLong, expected), /embedded NUL/);

    // High-bit byte in PAX length (0xB1 instead of ASCII '1').
    const highBitPax = Buffer.from([0xb1, 0x30, 0x20, 0x6b, 0x3d, 0x76, 0x0a, 0x0a, 0x0a, 0x0a]);
    const badPaxLen = zlib.gzipSync(
      Buffer.concat([
        buildHeader("PaxHeaders/hi", highBitPax.length, "x"),
        padToBlock(highBitPax),
        buildHeader(expected, 3, "0"),
        padToBlock(Buffer.from("bin")),
        Buffer.alloc(1024, 0),
      ]),
    );
    assert.throws(() => extractSingleFileFromTarGz(badPaxLen, expected), /PAX/);

    // GNU tar `"ustar  \0"` magic with non-zero bytes at offset 345 (`atime`) must NOT be read as USTAR prefix.
    const gnuHeader = buildHeader(expected, 3, "0");
    gnuHeader.write("ustar  \0", 257, 8, "ascii");
    gnuHeader.write("12345678901\0", 345, 12, "ascii");
    gnuHeader.fill(0x20, 148, 156);
    let sum = 0;
    for (let i = 0; i < 512; i++) {
      sum += gnuHeader[i];
    }
    gnuHeader.write(sum.toString(8).padStart(6, "0"), 148, 6, "ascii");
    gnuHeader[154] = 0;
    gnuHeader[155] = 0x20;
    const gnuMagicArchive = zlib.gzipSync(
      Buffer.concat([gnuHeader, padToBlock(Buffer.from("bin")), Buffer.alloc(1024, 0)]),
    );
    assert.deepEqual(extractSingleFileFromTarGz(gnuMagicArchive, expected), Buffer.from("bin"));

    // Trailing non-zero garbage after a zero block and non-512-multiple framing.
    const trailingGarbage = zlib.gzipSync(
      Buffer.concat([
        buildHeader(expected, 3, "0"),
        padToBlock(Buffer.from("bin")),
        Buffer.alloc(512, 0),
        Buffer.alloc(512, 0x41),
      ]),
    );
    assert.throws(
      () => extractSingleFileFromTarGz(trailingGarbage, expected),
      /trailing data after end-of-archive/,
    );

    const shortTar = zlib.gzipSync(Buffer.alloc(200, 0));
    assert.throws(
      () => extractSingleFileFromTarGz(shortTar, expected),
      /truncated tar archive header/,
    );
  });

  it("handles uppercase/malformed .sha256, maxBytes passthrough, and atomic overwrite", async () => {
    const version = "0.24.0";
    const target = "x86_64-unknown-linux-musl";
    const baseDir = `alint-v${version}-${target}`;
    const archiveName = `${baseDir}.tar.gz`;
    const payload = Buffer.from("new-binary-v2", "utf8");
    const archive = buildTarGz([{ name: `${baseDir}/alint`, content: payload }]);
    const upperHex = crypto.createHash("sha256").update(archive).digest("hex").toUpperCase();

    const destDir = makeTempDir();
    try {
      // Pre-existing binary must be atomically replaced.
      fs.writeFileSync(path.join(destDir, "alint"), Buffer.from("old-binary-v1"));
      const seenLimits: Array<{ url: string; maxBytes?: number }> = [];
      const installed = await downloadAlint(version, destDir, () => {}, {
        platform: "linux",
        arch: "x64",
        maxArchiveBytes: 123_456,
        fetchFn: async (url, maxBytes) => {
          seenLimits.push({ url, maxBytes });
          if (url.endsWith(".sha256")) {
            return Buffer.from(`${upperHex}  ${archiveName}\n`, "utf8");
          }
          return archive;
        },
      });
      assert.deepEqual(fs.readFileSync(installed), payload);
      assert.deepEqual(fs.readdirSync(destDir), ["alint"]);
      assert.equal(seenLimits.find((x) => x.url.endsWith(".tar.gz"))?.maxBytes, 123_456);
      assert.equal(seenLimits.find((x) => x.url.endsWith(".sha256"))?.maxBytes, 4096);

      // Malformed .sha256 digest is rejected cleanly.
      await assert.rejects(
        () =>
          downloadAlint(version, destDir, () => {}, {
            platform: "linux",
            arch: "x64",
            fetchFn: async (url) =>
              url.endsWith(".sha256")
                ? Buffer.from("not-a-valid-hex-digest  alint.tar.gz\n")
                : archive,
          }),
        /invalid SHA-256 checksum file/,
      );
    } finally {
      fs.rmSync(destDir, { recursive: true, force: true });
    }
  });

  it("rejects truncated tar archives missing a POSIX zero-block EOF trailer", () => {
    const expected = "alint-v0.24.0-x86_64-unknown-linux-musl/alint";
    const payload = Buffer.from("truncated-without-eof", "utf8");
    const noZeroTrailer = zlib.gzipSync(
      Buffer.concat([buildHeader(expected, payload.length, "0"), padToBlock(payload)]),
    );
    assert.throws(
      () => extractSingleFileFromTarGz(noZeroTrailer, expected),
      /tar archive truncated before end-of-archive zero block/,
    );
  });

  it("supports GNU base-256 encoded entry sizes", () => {
    const expected = "alint-v0.24.0-x86_64-unknown-linux-musl/alint";
    const payload = Buffer.from("base256-payload-bytes", "utf8");
    const hdr = buildHeader(expected, 0, "0");
    // Encode payload.length using GNU base-256 (0x80 high bit + big-endian length).
    hdr.fill(0, 124, 136);
    hdr[124] = 0x80;
    hdr[135] = payload.length;
    hdr.fill(0x20, 148, 156);
    let sum = 0;
    for (let i = 0; i < 512; i++) {
      sum += hdr[i];
    }
    hdr.write(sum.toString(8).padStart(6, "0"), 148, 6, "ascii");
    hdr[154] = 0;
    hdr[155] = 0x20;
    const archive = zlib.gzipSync(
      Buffer.concat([hdr, padToBlock(payload), Buffer.alloc(1024, 0)]),
    );
    assert.deepEqual(extractSingleFileFromTarGz(archive, expected), payload);
  });
});
