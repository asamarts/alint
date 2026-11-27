// Managed binary download — the opt-in third tier of binary
// resolution. Mirrors install.sh: same release URLs, same SHA-256
// verification, same tarball layout, so it fetches the byte-identical
// artifact install.sh and the npm platform packages carry.

import * as http from "node:http";
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
 * strict upper bound on buffered response bytes. Exported with an injectable
 * `getFn` so unit tests can exercise premature close and `maxBytes` limits
 * against a local server. */
export function fetchWithGet(
  url: string,
  maxBytes: number,
  redirects = 5,
  getFn: (url: string, cb: (res: http.IncomingMessage) => void) => http.ClientRequest = https.get,
  allowHttp = false,
): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    let settled = false;
    const finishOk = (value: Buffer | Promise<Buffer>) => {
      if (settled) {
        return;
      }
      settled = true;
      resolve(value);
    };
    const finishErr = (err: Error) => {
      if (settled) {
        return;
      }
      settled = true;
      reject(err);
    };

    const req = getFn(url, (res) => {
      const status = res.statusCode ?? 0;
      if (status >= 300 && status < 400 && res.headers.location) {
        res.resume();
        if (redirects <= 0) {
          finishErr(new Error(`too many redirects fetching ${url}`));
          return;
        }
        let nextUrl: URL;
        try {
          nextUrl = new URL(res.headers.location, url);
        } catch {
          finishErr(new Error(`invalid redirect URL fetching ${url}`));
          return;
        }
        const allowedProtocol =
          nextUrl.protocol === "https:" || (allowHttp && nextUrl.protocol === "http:");
        if (!allowedProtocol) {
          finishErr(
            new Error(
              `refusing non-HTTPS redirect (${nextUrl.protocol}) fetching ${url}`,
            ),
          );
          return;
        }
        finishOk(
          fetchWithGet(nextUrl.toString(), maxBytes, redirects - 1, getFn, allowHttp),
        );
        return;
      }
      if (status !== 200) {
        res.resume();
        finishErr(new Error(`HTTP ${status} fetching ${url}`));
        return;
      }
      const chunks: Buffer[] = [];
      let total = 0;
      res.on("data", (c: Buffer) => {
        total += c.length;
        if (total > maxBytes) {
          const err = new Error(`response exceeds ${maxBytes} bytes fetching ${url}`);
          finishErr(err);
          res.destroy();
          req.destroy();
          return;
        }
        chunks.push(c);
      });
      res.on("end", () => {
        if (!res.complete) {
          finishErr(new Error(`truncated response fetching ${url}`));
          return;
        }
        finishOk(Buffer.concat(chunks));
      });
      res.on("close", () => {
        if (!settled && !res.complete) {
          finishErr(new Error(`connection closed prematurely fetching ${url}`));
        }
      });
      res.on("error", (err) => finishErr(err));
    });
    req.on("error", (err) => finishErr(err));
    req.setTimeout(30000, () => {
      const err = new Error(`timeout fetching ${url}`);
      finishErr(err);
      req.destroy();
    });
  });
}

function fetch(url: string, maxBytes: number): Promise<Buffer> {
  return fetchWithGet(url, maxBytes);
}

function sha256Hex(buf: Buffer): string {
  return crypto.createHash("sha256").update(buf).digest("hex");
}

async function renameWithWindowsRetry(src: string, dest: string): Promise<void> {
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
      // Brief async backoff for Windows Defender / file-handle release without
      // blocking the extension host event loop.
      await new Promise((resolve) => setTimeout(resolve, 50 * attempt));
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
    await renameWithWindowsRetry(tmpBinary, destBinary);
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
