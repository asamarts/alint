// Unit tests for the dependency-free `.tar.gz` extractor and `downloadAlint`.

import * as assert from "node:assert/strict";
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
});
