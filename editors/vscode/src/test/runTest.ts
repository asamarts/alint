// Headless e2e entrypoint: downloads a real VS Code, copies the fixture
// workspace to a temp dir (writing `alint.path` into its settings so the
// extension launches the prebuilt server), and runs the mocha suite
// inside the extension host.
//
// Run with `npm run test:e2e` (locally use `xvfb-run -a npm run test:e2e`).
// The `alint` binary is taken from $ALINT_TEST_BINARY, falling back to
// `alint` on PATH.

import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';

import { runTests } from '@vscode/test-electron';

async function main(): Promise<void> {
  // out/test/runTest.js -> ../../ is the extension root (editors/vscode).
  const extensionDevelopmentPath = path.resolve(__dirname, '../../');
  const extensionTestsPath = path.resolve(__dirname, './suite/index');
  const fixtureSrc = path.resolve(extensionDevelopmentPath, 'src/test/fixtures/workspace');

  // Copy the fixture to a writable temp workspace and point the
  // extension at the binary under test via workspace settings.
  const workspace = fs.mkdtempSync(path.join(os.tmpdir(), 'alint-vscode-e2e-'));
  for (const name of fs.readdirSync(fixtureSrc)) {
    fs.copyFileSync(path.join(fixtureSrc, name), path.join(workspace, name));
  }
  fs.mkdirSync(path.join(workspace, '.vscode'), { recursive: true });
  const alintPath = process.env.ALINT_TEST_BINARY ?? 'alint';
  fs.writeFileSync(
    path.join(workspace, '.vscode', 'settings.json'),
    JSON.stringify(
      {
        'alint.path': alintPath,
        // Quiet VS Code's own network features. Under a headless,
        // no-network runner these calls fail and the resulting
        // "Unexpected SIGPIPE" in the extension host can tear down the
        // language client's stream mid-activation — unrelated to alint.
        'telemetry.telemetryLevel': 'off',
        'update.mode': 'none',
        'extensions.autoCheckUpdates': false,
        'extensions.autoUpdate': false,
        'workbench.enableExperiments': false,
        'npm.fetchOnlinePackageInfo': false,
      },
      null,
      2,
    ),
  );

  try {
    await runTests({
      // Pin a known-stable VS Code (the extension's min engine) for
      // reproducible CI rather than chasing `stable`/latest.
      version: '1.91.0',
      extensionDevelopmentPath,
      extensionTestsPath,
      launchArgs: [
        workspace,
        '--disable-workspace-trust',
        '--disable-extensions',
        // Electron's sandbox needs privileges absent under headless
        // xvfb / containers; without this the extension host can crash
        // on startup (the IPC "Sending request failed" / SIGPIPE path).
        '--no-sandbox',
      ],
    });
  } catch (err) {
    console.error('VS Code e2e failed:', err);
    process.exit(1);
  } finally {
    fs.rmSync(workspace, { recursive: true, force: true });
  }
}

main();
