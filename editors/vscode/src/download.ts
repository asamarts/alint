// Managed binary download — the opt-in third tier of binary
// resolution. Mirrors install.sh: same release URLs, same SHA-256
// verification, same tarball layout, so it fetches the byte-identical
// artifact install.sh and the npm platform packages carry.

import * as https from "node:https";
import * as crypto from "node:crypto";
import * as fs from "node:fs";
import * as path from "node:path";

import {
  DEFAULT_MAX_ARCHIVE_BYTES,
  extractSingleFileFromTarGz,
} from "./tar";
import { binaryName, resolveTarget } from "./target";

const REPO = "asamarts/alint";
const MAX_SHA256_BYTES = 4096;

export interface DownloadOptions {
  platform?: string;
  arch?: string;
  fetchFn?: (url: string, maxBytes: number) => Promise<Buffer>;
  maxArchiveBytes?: number;
}

/** Follow up to 5 HTTPS redirects (GitHub Releases hops through S3) with a
 * strict upper bound on buffered response bytes. */
function fetch(url: string, maxBytes: number, redirects = 5): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const req = https.get(url, (res) => {
      const status = res.statusCode ?? 0;
      if (status >= 300 && status < 400 && res.headers.location) {
        res.resume();
        if (redirects <= 0) {
          reject(new Error(`too many redirects fetching ${url}`));
          return;
        }
        let nextUrl: URL;
        try {
          nextUrl = new URL(res.headers.location, url);
        } catch {
          reject(new Error(`invalid redirect URL fetching ${url}`));
          return;
        }
        if (nextUrl.protocol !== "https:") {
          reject(
            new Error(
              `refusing non-HTTPS redirect (${nextUrl.protocol}) fetching ${url}`,
            ),
          );
          return;
        }
        resolve(fetch(nextUrl.toString(), maxBytes, redirects - 1));
        return;
      }
      if (status !== 200) {
        res.resume();
        reject(new Error(`HTTP ${status} fetching ${url}`));
        return;
      }
      const chunks: Buffer[] = [];
      let total = 0;
      res.on("data", (c: Buffer) => {
        total += c.length;
        if (total > maxBytes) {
          res.destroy();
          req.destroy(
            new Error(`response exceeds ${maxBytes} bytes fetching ${url}`),
          );
          return;
        }
        chunks.push(c);
      });
      res.on("end", () => resolve(Buffer.concat(chunks)));
      res.on("error", reject);
    });
    req.on("error", reject);
    req.setTimeout(30000, () => {
      req.destroy(new Error(`timeout fetching ${url}`));
    });
  });
}

function sha256Hex(buf: Buffer): string {
  return crypto.createHash("sha256").update(buf).digest("hex");
}

function renameWithWindowsRetry(src: string, dest: string): void {
  const maxAttempts = process.platform === "win32" ? 4 : 1;
  for (let attempt = 1; ; attempt++) {
    try {
      fs.renameSync(src, dest);
      return;
    } catch (err) {
      const code = (err as NodeJS.ErrnoException).code;
      if (
        attempt >= maxAttempts ||
        (code !== "EPERM" && code !== "EBUSY" && code !== "EACCES")
      ) {
        throw err;
      }
      // Brief synchronous backoff for Windows Defender / file-handle release.
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 50 * attempt);
    }
  }
}

/** Download the alint release matching `version` for the current
 * platform into `destDir`, verifying its SHA-256. Returns the path to
 * the extracted binary. */
export async function downloadAlint(
  version: string,
  destDir: string,
  log: (msg: string) => void,
  options?: DownloadOptions,
): Promise<string> {
  const platform = options?.platform ?? process.platform;
  const arch = options?.arch ?? process.arch;
  const maxArchiveBytes = options?.maxArchiveBytes ?? DEFAULT_MAX_ARCHIVE_BYTES;
  const doFetch = options?.fetchFn ?? fetch;

  const target = resolveTarget(platform, arch);
  const tag = `v${version}`;
  const archive = `alint-${tag}-${target}.tar.gz`;
  const baseUrl = `https://github.com/${REPO}/releases/download/${tag}`;
  const tarUrl = `${baseUrl}/${archive}`;
  const shaUrl = `${tarUrl}.sha256`;

  log(`downloading ${archive}`);
  const [tarBuf, shaBuf] = await Promise.all([
    doFetch(tarUrl, maxArchiveBytes),
    doFetch(shaUrl, MAX_SHA256_BYTES),
  ]);

  // The .sha256 file is `<64-hex>  <filename>`; validate and compare hex.
  const rawExpected = shaBuf.toString("utf8").trim().split(/\s+/)[0] ?? "";
  if (!/^[0-9a-fA-F]{64}$/.test(rawExpected)) {
    throw new Error(`invalid SHA-256 checksum file for ${archive}`);
  }
  const expected = rawExpected.toLowerCase();
  const actual = sha256Hex(tarBuf);
  if (expected !== actual) {
    throw new Error(
      `SHA-256 mismatch for ${archive} (expected ${expected}, got ${actual})`,
    );
  }
  log("SHA-256 verified");

  const name = binaryName(platform);
  const expectedEntry = `alint-${tag}-${target}/${name}`;
  const binaryBytes = extractSingleFileFromTarGz(
    tarBuf,
    expectedEntry,
    maxArchiveBytes,
  );

  fs.mkdirSync(destDir, { recursive: true });
  const destBinary = path.join(destDir, name);
  const tmpBinary = path.join(
    destDir,
    `.${name}.${process.pid}.${crypto.randomBytes(6).toString("hex")}.tmp`,
  );
  try {
    fs.writeFileSync(tmpBinary, binaryBytes, { flag: "wx", mode: 0o600 });
    if (platform !== "win32" && process.platform !== "win32") {
      fs.chmodSync(tmpBinary, 0o755);
    }
    renameWithWindowsRetry(tmpBinary, destBinary);
  } catch (err) {
    try {
      fs.rmSync(tmpBinary, { force: true });
    } catch {
      // Ignore cleanup errors when surfacing the original failure.
    }
    throw err;
  }

  log(`installed ${name} (${target})`);
  return destBinary;
}
