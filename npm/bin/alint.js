#!/usr/bin/env node
// alint npm shim — runtime wrapper.
//
// Locates the platform-matched native binary that `install.js`
// staged under `bin-platform/` at install time, then spawns it
// with the user's argv. On Unix we'd ideally `execvp` to replace
// the Node process (zero overhead), but that's not portable to
// Windows; spawnSync+stdio:inherit is the universal path.

'use strict';

const path = require('path');
const fs = require('fs');
const child_process = require('child_process');

const isWindows = process.platform === 'win32';
const binaryName = isWindows ? 'alint.exe' : 'alint';
const binaryPath = path.join(__dirname, '..', 'bin-platform', binaryName);

if (!fs.existsSync(binaryPath)) {
  process.stderr.write(
    `@alint/alint: binary not found at ${binaryPath}\n` +
      `  this usually means the postinstall step failed; try:\n` +
      `    npm uninstall -g @alint/alint && npm install -g @alint/alint\n` +
      `  or set ALINT_SKIP_INSTALL=0 if your CI suppressed it.\n`,
  );
  process.exit(1);
}

const result = child_process.spawnSync(binaryPath, process.argv.slice(2), {
  stdio: 'inherit',
});

if (result.error) {
  process.stderr.write(`@alint/alint: failed to spawn ${binaryPath}: ${result.error.message}\n`);
  process.exit(1);
}

// `spawnSync` returns null for status when killed by a signal
// (e.g. user ^C). Forward exit code 130 (SIGINT-conventional) so
// CI runners interpret it correctly.
process.exit(result.status === null ? 130 : result.status);
