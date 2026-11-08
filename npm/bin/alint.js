#!/usr/bin/env node
// alint npm shim: runs the native alint binary.
//
// The binary ships in a per-platform package (`@asamarts/alint-<os>-<cpu>`,
// listed in ../platforms.json) that @asamarts/alint names as an exact-version
// optionalDependency. The package manager installs only the one whose `os`
// and `cpu` match, so nothing here downloads anything and no package has an
// install script. ALINT_BINARY=<path> runs a binary of the caller's choosing
// instead (mirrors, air-gapped installs).

'use strict';

const fs = require('fs');
const path = require('path');
const child_process = require('child_process');

const { packages } = require('../platforms.json');

class ShimError extends Error {}

function platformPackage(platform, arch) {
  return packages.find((p) => p.os === platform && p.cpu === arch);
}

function missingPackageMessage(pkg, platform, arch) {
  return (
    `@asamarts/alint: the alint binary for ${platform}-${arch} is not installed.\n` +
    `  It ships in the optional dependency ${pkg.name}, which this install left out.\n` +
    `  Usual causes: installing with --omit=optional or --no-optional, or a lockfile\n` +
    `  written on another platform without its optional dependencies.\n` +
    `  Fix: reinstall without omitting optional dependencies, for example\n` +
    `    npm install @asamarts/alint\n` +
    `  or set ALINT_BINARY to the path of an alint binary.\n`
  );
}

// Returns the binary to run. `resolve` is require.resolve; tests pass a stub.
function resolveBinary({
  env = process.env,
  platform = process.platform,
  arch = process.arch,
  resolve = require.resolve,
} = {}) {
  if (env.ALINT_BINARY) {
    if (!fs.existsSync(env.ALINT_BINARY)) {
      throw new ShimError(`@asamarts/alint: ALINT_BINARY=${env.ALINT_BINARY} does not exist.\n`);
    }
    return env.ALINT_BINARY;
  }
  const pkg = platformPackage(platform, arch);
  if (!pkg) {
    throw new ShimError(
      `@asamarts/alint: no alint binary is published for ${platform}-${arch}.\n` +
        `  Install with \`cargo install alint\`, or download a release from\n` +
        `  https://github.com/asamarts/alint/releases\n`,
    );
  }
  let manifest;
  try {
    manifest = resolve(`${pkg.name}/package.json`);
  } catch (e) {
    throw new ShimError(missingPackageMessage(pkg, platform, arch));
  }
  const binary = path.join(path.dirname(manifest), 'bin', pkg.binary);
  if (!fs.existsSync(binary)) {
    throw new ShimError(missingPackageMessage(pkg, platform, arch));
  }
  return binary;
}

// npm keeps a packed file's executable bit, but some extractors do not.
function ensureExecutable(binary) {
  if (process.platform === 'win32') return;
  try {
    fs.accessSync(binary, fs.constants.X_OK);
  } catch (e) {
    try {
      fs.chmodSync(binary, 0o755);
    } catch (chmodError) {
      // Leave it: spawning reports the permission error below.
    }
  }
}

function main() {
  let binary;
  try {
    binary = resolveBinary();
  } catch (e) {
    if (!(e instanceof ShimError)) throw e;
    process.stderr.write(e.message);
    process.exit(1);
  }
  ensureExecutable(binary);
  const result = child_process.spawnSync(binary, process.argv.slice(2), { stdio: 'inherit' });
  if (result.error) {
    process.stderr.write(`@asamarts/alint: failed to run ${binary}: ${result.error.message}\n`);
    process.exit(1);
  }
  if (result.status === null) {
    // Killed by a signal (e.g. ^C): end the same way, so a parent sees it.
    process.kill(process.pid, result.signal || 'SIGINT');
    // Not every signal ends Node (SIGINT does unless handled); fall back.
    process.exit(128 + (require('os').constants.signals[result.signal] || 2));
  }
  process.exit(result.status);
}

module.exports = { platformPackage, resolveBinary, missingPackageMessage, ShimError };

if (require.main === module) {
  main();
}
