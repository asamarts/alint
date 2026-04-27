#!/usr/bin/env node
// alint npm shim — postinstall script.
//
// Resolves the native alint binary for the running platform/arch
// from `https://github.com/asamarts/alint/releases/<tag>`, verifies
// the SHA-256 hash that ships alongside the tarball, extracts the
// binary, and stages it under `bin-platform/`. The bin shim
// (`bin/alint.js`) execs that binary at runtime.
//
// Mirrors the existing `install.sh` URL conventions exactly so the
// two install paths download byte-identical tarballs.
//
// Skipped during local development checkouts (when this script
// runs from the alint monorepo's own `npm install`) — the upstream
// release for the in-flight version may not exist yet.

'use strict';

const fs = require('fs');
const path = require('path');
const https = require('https');
const crypto = require('crypto');
const os = require('os');
const tar = require('tar');

const REPO = 'asamarts/alint';
const PKG_VERSION = require('./package.json').version;
const VERSION_TAG = `v${PKG_VERSION}`;

// Skip when the consumer set ALINT_SKIP_INSTALL=1, e.g. for CI
// systems that snapshot node_modules and don't want the postinstall
// network hop. Also skip when running from the alint monorepo's own
// pnpm/npm install — the marker is the presence of a Cargo.toml two
// levels up. That keeps `cargo xtask` flows from accidentally trying
// to download a not-yet-released version of itself.
function shouldSkip() {
  if (process.env.ALINT_SKIP_INSTALL === '1') {
    return 'ALINT_SKIP_INSTALL=1';
  }
  const monorepoMarker = path.resolve(__dirname, '..', 'Cargo.toml');
  if (fs.existsSync(monorepoMarker)) {
    return 'detected alint monorepo (../Cargo.toml exists)';
  }
  return null;
}

// Map Node platform/arch to the alint release-target triple.
// The list mirrors install.sh and the release.yml build matrix.
function resolveTarget() {
  const platform = process.platform;
  const arch = process.arch;
  const map = {
    'linux/x64': 'x86_64-unknown-linux-musl',
    'linux/arm64': 'aarch64-unknown-linux-musl',
    'darwin/x64': 'x86_64-apple-darwin',
    'darwin/arm64': 'aarch64-apple-darwin',
    'win32/x64': 'x86_64-pc-windows-msvc',
  };
  const key = `${platform}/${arch}`;
  const target = map[key];
  if (!target) {
    throw new Error(
      `unsupported platform ${platform}/${arch}\n` +
        `       supported: ${Object.keys(map).join(', ')}\n` +
        `       download manually from https://github.com/${REPO}/releases`,
    );
  }
  return target;
}

// Follow up to 5 redirects (GitHub Releases hops through S3).
function fetch(url, redirects = 5) {
  return new Promise((resolve, reject) => {
    const req = https.get(url, (res) => {
      const status = res.statusCode || 0;
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
        reject(new Error(`HTTP ${status} fetching ${url}`));
        res.resume();
        return;
      }
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => resolve(Buffer.concat(chunks)));
      res.on('error', reject);
    });
    req.on('error', reject);
    req.setTimeout(30000, () => {
      req.destroy(new Error(`timeout fetching ${url}`));
    });
  });
}

function sha256Hex(buf) {
  return crypto.createHash('sha256').update(buf).digest('hex');
}

async function main() {
  const skipReason = shouldSkip();
  if (skipReason) {
    process.stderr.write(`@alint/alint: skipping postinstall (${skipReason})\n`);
    return;
  }

  const target = resolveTarget();
  const archive = `alint-${VERSION_TAG}-${target}.tar.gz`;
  const baseUrl = `https://github.com/${REPO}/releases/download/${VERSION_TAG}`;
  const tarUrl = `${baseUrl}/${archive}`;
  const shaUrl = `${tarUrl}.sha256`;

  process.stderr.write(`@alint/alint: downloading ${archive}\n`);
  const [tarBuf, shaBuf] = await Promise.all([fetch(tarUrl), fetch(shaUrl)]);

  // The .sha256 file format mirrors install.sh's expectation:
  //   <hex>  <filename>\n
  // We verify against the hex column, ignoring the trailing
  // filename column.
  const expectedHash = shaBuf.toString('utf8').trim().split(/\s+/)[0];
  const actualHash = sha256Hex(tarBuf);
  if (expectedHash !== actualHash) {
    throw new Error(
      `SHA-256 mismatch for ${archive}\n` +
        `  expected: ${expectedHash}\n` +
        `  got:      ${actualHash}`,
    );
  }
  process.stderr.write(`@alint/alint: SHA-256 verified\n`);

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'alint-npm-'));
  const tarPath = path.join(tmpDir, archive);
  fs.writeFileSync(tarPath, tarBuf);

  // Tarball layout: alint-<tag>-<target>/alint (or alint.exe).
  // We extract the whole thing under tmp, then move just the
  // binary into ./bin-platform/.
  await tar.x({ file: tarPath, cwd: tmpDir });
  const isWindows = process.platform === 'win32';
  const binaryName = isWindows ? 'alint.exe' : 'alint';
  const extractedDir = path.join(tmpDir, `alint-${VERSION_TAG}-${target}`);
  const sourceBinary = path.join(extractedDir, binaryName);
  if (!fs.existsSync(sourceBinary)) {
    throw new Error(
      `extracted tarball missing expected ${binaryName} at ${sourceBinary}`,
    );
  }

  const destDir = path.join(__dirname, 'bin-platform');
  fs.mkdirSync(destDir, { recursive: true });
  const destBinary = path.join(destDir, binaryName);
  fs.copyFileSync(sourceBinary, destBinary);
  if (!isWindows) {
    fs.chmodSync(destBinary, 0o755);
  }

  // Clean up scratch dir; ignore failures (tmp is OS-managed).
  try {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  } catch {
    // ignore — tmp dir is OS-managed
  }

  process.stderr.write(`@alint/alint: installed ${binaryName} for ${target}\n`);
}

main().catch((err) => {
  process.stderr.write(`@alint/alint: postinstall failed: ${err.message}\n`);
  process.exit(1);
});
