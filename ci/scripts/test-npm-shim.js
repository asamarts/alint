// Gate for the npm distribution's tables and its shim (npm/bin/alint.js).
//
// Tables: npm/platforms.json must list exactly release.yml's build targets, each
// row consistent with its target (os, cpu, package name, binary name), and
// npm/package.json must name every row as an exact-version optionalDependency
// and carry no install script (ADR-0031).
//
// Shim: resolveBinary() against stub resolvers, then the real shim spawned over
// temporary node_modules layouts (npm's flat one and pnpm's symlinked one):
// argument and exit-code forwarding, signal propagation, exec-bit repair, the
// missing-package message, ALINT_BINARY, and unsupported platforms.
//
// Invoked by ci/scripts/test-npm-shim.sh with RELEASE_TARGETS set.

'use strict';

const assert = require('assert');
const child_process = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');

const REPO = path.resolve(__dirname, '../..');
const NPM = path.join(REPO, 'npm');
const pkg = require(path.join(NPM, 'package.json'));
const { packages } = require(path.join(NPM, 'platforms.json'));
const shim = require(path.join(NPM, 'bin/alint.js'));

let checks = 0;
function check(name, fn) {
  try {
    fn();
  } catch (e) {
    console.error(`[test-npm-shim] FAIL ${name}\n${e.stack || e}`);
    process.exit(1);
  }
  checks += 1;
}

// ── tables ──────────────────────────────────────────────────────────────
const built = new Set(process.env.RELEASE_TARGETS.split('\n').filter(Boolean));
const OS_OF = { linux: '-linux-', darwin: '-apple-darwin', win32: '-pc-windows-' };
const CPU_OF = { x64: 'x86_64-', arm64: 'aarch64-' };

check('platforms.json lists exactly the release targets', () => {
  const listed = packages.map((p) => p.target);
  for (const t of listed) assert(built.has(t), `${t} is in platforms.json but release.yml does not build it`);
  for (const t of built) {
    assert(listed.includes(t), `release.yml builds ${t} but npm/platforms.json has no package for it`);
  }
  assert.strictEqual(new Set(listed).size, listed.length, 'a target is listed twice');
});

check('each platform row is consistent with its target', () => {
  const pairs = new Set();
  for (const p of packages) {
    assert.strictEqual(p.name, `@asamarts/alint-${p.os}-${p.cpu}`, `${p.name}: name must be @asamarts/alint-<os>-<cpu>`);
    assert(OS_OF[p.os] && p.target.includes(OS_OF[p.os]), `${p.name}: os ${p.os} does not match ${p.target}`);
    assert(CPU_OF[p.cpu] && p.target.startsWith(CPU_OF[p.cpu]), `${p.name}: cpu ${p.cpu} does not match ${p.target}`);
    assert.strictEqual(p.binary, p.os === 'win32' ? 'alint.exe' : 'alint', `${p.name}: binary name`);
    assert(p.label, `${p.name}: label`);
    assert(!pairs.has(`${p.os}/${p.cpu}`), `${p.os}/${p.cpu} is listed twice`);
    pairs.add(`${p.os}/${p.cpu}`);
  }
});

check('package.json pins every platform package at its own version', () => {
  const deps = pkg.optionalDependencies || {};
  assert.deepStrictEqual(Object.keys(deps).sort(), packages.map((p) => p.name).sort());
  for (const [name, range] of Object.entries(deps)) {
    assert.strictEqual(range, pkg.version, `${name}@${range} must be exactly ${pkg.version}`);
  }
});

check('package.json has no install script and ships the shim', () => {
  assert.strictEqual(pkg.scripts, undefined, 'no scripts: the binary arrives as an optional dependency');
  assert.strictEqual(pkg.dependencies, undefined, 'the shim has no runtime dependencies');
  // No os/cpu on the shim: on any other platform it installs, and ALINT_BINARY
  // or the "no binary is published" message take over.
  assert.strictEqual(pkg.os, undefined);
  assert.strictEqual(pkg.cpu, undefined);
  assert.deepStrictEqual(pkg.bin, { alint: 'bin/alint.js' });
  for (const f of ['bin/alint.js', 'platforms.json', 'README.md']) {
    assert(pkg.files.includes(f), `files must include ${f}`);
    assert(fs.existsSync(path.join(NPM, f)), `npm/${f} is missing`);
  }
  assert(!fs.existsSync(path.join(NPM, 'install.js')), 'npm/install.js is gone (ADR-0031)');
});

// ── resolveBinary (stub resolver) ─────────────────────────────────────────
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'alint-npm-shim-'));
process.on('exit', () => fs.rmSync(tmp, { recursive: true, force: true }));

function stubPackage(dir, p, { binary = true } = {}) {
  fs.mkdirSync(path.join(dir, 'bin'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: p.name, version: pkg.version }));
  if (binary) fs.writeFileSync(path.join(dir, 'bin', p.binary), 'stub');
  return path.join(dir, 'package.json');
}

check('resolveBinary finds every platform package', () => {
  for (const p of packages) {
    const manifest = stubPackage(path.join(tmp, 'stub', p.name), p);
    const got = shim.resolveBinary({
      env: {},
      platform: p.os,
      arch: p.cpu,
      resolve: (id) => {
        assert.strictEqual(id, `${p.name}/package.json`);
        return manifest;
      },
    });
    assert.strictEqual(got, path.join(path.dirname(manifest), 'bin', p.binary));
  }
});

check('a missing platform package explains --omit=optional', () => {
  const p = packages[0];
  const notFound = () => {
    throw Object.assign(new Error('Cannot find module'), { code: 'MODULE_NOT_FOUND' });
  };
  assert.throws(
    () => shim.resolveBinary({ env: {}, platform: p.os, arch: p.cpu, resolve: notFound }),
    (e) => e instanceof shim.ShimError && e.message.includes(p.name) && e.message.includes('--omit=optional') &&
      e.message.includes('ALINT_BINARY'),
  );
  const manifest = stubPackage(path.join(tmp, 'nobin', p.name), p, { binary: false });
  assert.throws(
    () => shim.resolveBinary({ env: {}, platform: p.os, arch: p.cpu, resolve: () => manifest }),
    (e) => e instanceof shim.ShimError && e.message.includes('--omit=optional'),
  );
});

check('an unsupported platform points at other installs', () => {
  assert.throws(
    () => shim.resolveBinary({ env: {}, platform: 'freebsd', arch: 'x64', resolve: () => assert.fail('resolved') }),
    (e) => e instanceof shim.ShimError && /no alint binary is published for freebsd-x64/.test(e.message) &&
      e.message.includes('cargo install alint'),
  );
});

check('ALINT_BINARY wins over the platform package, and must exist', () => {
  const custom = path.join(tmp, 'custom-alint');
  fs.writeFileSync(custom, 'stub');
  const got = shim.resolveBinary({
    env: { ALINT_BINARY: custom },
    platform: 'freebsd',
    arch: 'x64',
    resolve: () => assert.fail('resolved'),
  });
  assert.strictEqual(got, custom);
  assert.throws(
    () => shim.resolveBinary({ env: { ALINT_BINARY: path.join(tmp, 'nope') }, resolve: () => assert.fail() }),
    (e) => e instanceof shim.ShimError && e.message.includes('does not exist'),
  );
});

// ── the real shim, spawned ─────────────────────────────────────────────
function runShim(shimPath, args, env = {}) {
  const childEnv = { ...process.env, ...env };
  if (!('ALINT_BINARY' in env)) delete childEnv.ALINT_BINARY;
  return child_process.spawnSync(process.execPath, [shimPath, ...args], { env: childEnv, encoding: 'utf8' });
}

check('ALINT_BINARY=node runs the given binary and forwards the exit code', () => {
  const r = runShim(path.join(NPM, 'bin/alint.js'), ['-e', 'console.log(process.argv.slice(1).join("|")); process.exit(7)', 'a', 'b c'], {
    ALINT_BINARY: process.execPath,
  });
  assert.strictEqual(r.status, 7, r.stderr);
  assert.strictEqual(r.stdout.trim(), 'a|b c');
});

const host = shim.platformPackage(process.platform, process.arch);
if (process.platform === 'win32' || !host) {
  console.log(`[test-npm-shim] skip node_modules layouts on ${process.platform}-${process.arch} (they use a shell-script binary)`);
} else {
  const FAKE = [
    '#!/bin/sh',
    'case "$1" in',
    '  --signal) kill -TERM $$ ;;',
    '  *) printf "%s|" "$@"; echo; exit "${ALINT_FAKE_STATUS:-0}" ;;',
    'esac',
    '',
  ].join('\n');

  // npm's flat layout, or pnpm's: the real packages live side by side under
  // .pnpm/<id>/node_modules, and node_modules/@asamarts/alint is a symlink.
  function layout(name, { pnpm = false, platform = true, mode = 0o755 } = {}) {
    const root = path.join(tmp, name);
    const store = pnpm ? path.join(root, 'node_modules/.pnpm/@asamarts+alint@0/node_modules') : path.join(root, 'node_modules');
    const shimDir = path.join(store, '@asamarts/alint');
    fs.mkdirSync(path.join(shimDir, 'bin'), { recursive: true });
    for (const f of ['package.json', 'platforms.json', 'bin/alint.js']) {
      fs.copyFileSync(path.join(NPM, f), path.join(shimDir, f));
    }
    if (platform) {
      const platDir = path.join(store, host.name);
      stubPackage(platDir, host, { binary: false });
      fs.writeFileSync(path.join(platDir, 'bin', host.binary), FAKE, { mode });
      fs.chmodSync(path.join(platDir, 'bin', host.binary), mode);
    }
    if (!pnpm) return path.join(shimDir, 'bin/alint.js');
    fs.mkdirSync(path.join(root, 'node_modules/@asamarts'), { recursive: true });
    fs.symlinkSync(shimDir, path.join(root, 'node_modules/@asamarts/alint'), 'dir');
    return path.join(root, 'node_modules/@asamarts/alint/bin/alint.js');
  }

  check('npm layout: runs the platform binary, forwards args and exit code', () => {
    const r = runShim(layout('flat'), ['check', 'a b', ''], { ALINT_FAKE_STATUS: '3' });
    assert.strictEqual(r.status, 3, r.stderr);
    assert.strictEqual(r.stdout.trim(), 'check|a b||');
  });

  check('pnpm layout: resolves the sibling package through the symlink', () => {
    const r = runShim(layout('pnpm', { pnpm: true }), ['--version']);
    assert.strictEqual(r.status, 0, r.stderr);
    assert.strictEqual(r.stdout.trim(), '--version|');
  });

  check('a binary killed by a signal ends the shim by the same signal', () => {
    const r = runShim(layout('signal'), ['--signal']);
    assert.strictEqual(r.status, null, `status ${r.status}, stderr ${r.stderr}`);
    assert.strictEqual(r.signal, 'SIGTERM');
  });

  check('a binary without its exec bit is repaired and run', () => {
    const r = runShim(layout('noexec', { mode: 0o644 }), ['ok']);
    assert.strictEqual(r.status, 0, r.stderr);
    assert.strictEqual(r.stdout.trim(), 'ok|');
  });

  check('installed without optional dependencies: exit 1 with the reinstall hint', () => {
    const r = runShim(layout('omitted', { platform: false }), ['check']);
    assert.strictEqual(r.status, 1);
    assert.match(r.stderr, /--omit=optional/);
    assert(r.stderr.includes(host.name), r.stderr);
    assert.strictEqual(r.stdout, '');
  });
}

console.log(`[test-npm-shim] OK - ${checks} checks (${packages.length} platform packages)`);
