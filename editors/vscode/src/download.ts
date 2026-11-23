// Managed binary download — the opt-in third tier of binary
// resolution. Mirrors install.sh: same release URLs, same SHA-256
// verification, same tarball layout, so it fetches the byte-identical
// artifact install.sh and the npm platform packages carry.

import * as https from "node:https";
import * as crypto from "node:crypto";
import * as fs from "node:fs";
import * as path from "node:path";

import { extractSingleFileFromTarGz } from "./tar";
import { binaryName, resolveTarget } from "./target";

const REPO = "asamarts/alint";

export interface DownloadOptions {
  platform?: string;
  arch?: string;
  fetchFn?: (url: string) => Promise<Buffer>;
  maxArchiveBytes?: number;
}

/** Follow up to 5 redirects (GitHub Releases hops through S3). */
function fetch(url: string, redirects = 5): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const req = https.get(url, (res) => {
      const status = res.statusCode ?? 0;
      if (status >= 300 && status < 400 && res.headers.location) {
        if (redirects <= 0) {
          reject(new Error(`too many redirects fetching ${url}`));
          return;
        }
        res.resume();
        resolve(fetch(res.headers.location, redirects - 1));
        return;
      }
      if (status !== 200) {
        res.resume();
        reject(new Error(`HTTP ${status} fetching ${url}`));
        return;
      }
      const chunks: Buffer[] = [];
      res.on("data", (c: Buffer) => chunks.push(c));
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
  const fetchFn = options?.fetchFn ?? fetch;

  const target = resolveTarget(platform, arch);
  const tag = `v${version}`;
  const archive = `alint-${tag}-${target}.tar.gz`;
  const baseUrl = `https://github.com/${REPO}/releases/download/${tag}`;
  const tarUrl = `${baseUrl}/${archive}`;
  const shaUrl = `${tarUrl}.sha256`;

  log(`downloading ${archive}`);
  const [tarBuf, shaBuf] = await Promise.all([fetchFn(tarUrl), fetchFn(shaUrl)]);

  // The .sha256 file is `<hex>  <filename>`; verify the hex column.
  const expected = shaBuf.toString("utf8").trim().split(/\s+/)[0];
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
    options?.maxArchiveBytes,
  );

  fs.mkdirSync(destDir, { recursive: true });
  const destBinary = path.join(destDir, name);
  const tmpBinary = path.join(
    destDir,
    `.${name}.${process.pid}.${crypto.randomBytes(6).toString("hex")}.tmp`,
  );
  try {
    fs.writeFileSync(tmpBinary, binaryBytes);
    if (platform !== "win32" && process.platform !== "win32") {
      fs.chmodSync(tmpBinary, 0o755);
    }
    fs.renameSync(tmpBinary, destBinary);
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
