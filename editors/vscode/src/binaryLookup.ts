// Pure filesystem binary-resolution helpers (no `vscode` dependency so unit
// tests can exercise them directly under Node/Mocha).

import * as fs from "node:fs";
import * as path from "node:path";

import { binaryName } from "./target";

/** True when `filePath` exists, is a regular file (following symlinks), and is
 * executable (on POSIX). Rejects directories and unreadable/non-executable files. */
export function isExecutableFile(
  filePath: string,
  platform: string = process.platform,
): boolean {
  try {
    const stat = fs.statSync(filePath);
    if (!stat.isFile()) {
      return false;
    }
    if (platform !== "win32" && process.platform !== "win32") {
      fs.accessSync(filePath, fs.constants.X_OK);
    }
    return true;
  } catch {
    return false;
  }
}

/** Versioned cache path: `<globalStorage>/bin/v<version>/alint(.exe)`. */
export function versionedCacheBinaryPath(
  globalStoragePath: string,
  version: string,
  platform: string = process.platform,
): string {
  return path.join(globalStoragePath, "bin", `v${version}`, binaryName(platform));
}

/** Best-effort cleanup of older `bin/v*` cache directories and legacy unversioned
 * `bin/alint(.exe)` files after installing `currentVersion`. */
export function pruneOldCachedVersions(
  globalStoragePath: string,
  currentVersion: string,
): void {
  const binRoot = path.join(globalStoragePath, "bin");
  const keepDir = `v${currentVersion}`;
  let entries: fs.Dirent[];
  try {
    entries = fs.readdirSync(binRoot, { withFileTypes: true });
  } catch {
    return;
  }
  for (const entry of entries) {
    if (entry.name === keepDir) {
      continue;
    }
    if (
      (entry.isDirectory() && entry.name.startsWith("v")) ||
      entry.name === "alint" ||
      entry.name === "alint.exe"
    ) {
      try {
        fs.rmSync(path.join(binRoot, entry.name), { recursive: true, force: true });
      } catch {
        // Ignore cleanup failures (e.g. a running Windows binary).
      }
    }
  }
}

/** First directory on `pathVar` that contains an executable `alint(.exe)` file. */
export function findBinaryOnPath(
  pathVar: string = process.env.PATH ?? "",
  platform: string = process.platform,
): string | undefined {
  const name = binaryName(platform);
  const sep = platform === "win32" ? ";" : ":";
  for (const rawDir of pathVar.split(sep)) {
    const dir =
      platform === "win32" ? rawDir.trim().replace(/^"+|"+$/g, "") : rawDir;
    if (!dir) {
      continue;
    }
    const candidate = path.join(dir, name);
    if (isExecutableFile(candidate, platform)) {
      return candidate;
    }
  }
  return undefined;
}
