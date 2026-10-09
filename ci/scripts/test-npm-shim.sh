#!/usr/bin/env bash
# The npm shim's postinstall maps process.platform/arch to a release tarball.
# npm admits the package on every os x cpu combination package.json lists, so
# each combination must resolve to a target that release.yml's build matrix
# actually produces (win32/arm64 once had no mapping: postinstall failed), and
# every mapped target must exist in that matrix.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

if ! command -v node >/dev/null 2>&1; then
  if [[ "${CI:-}" == "true" ]]; then
    echo "[test-npm-shim] node is required in CI" >&2
    exit 1
  fi
  echo "[test-npm-shim] skip: node not installed"
  exit 0
fi

targets=$(python3 - <<'PY'
import yaml
wf = yaml.safe_load(open('.github/workflows/release.yml', encoding='utf-8'))
for row in wf['jobs']['build']['strategy']['matrix']['include']:
    print(row['target'])
PY
)

RELEASE_TARGETS="$targets" node - <<'JS'
'use strict';
const path = require('path');
const pkg = require(path.resolve('npm/package.json'));
const { TARGETS, resolveTarget } = require(path.resolve('npm/install.js'));
const built = new Set(process.env.RELEASE_TARGETS.split('\n').filter(Boolean));
const errors = [];
for (const os of pkg.os) {
  for (const cpu of pkg.cpu) {
    let target;
    try {
      target = resolveTarget(os, cpu);
    } catch (e) {
      errors.push(`${os}/${cpu} is admitted by package.json os/cpu but has no target`);
      continue;
    }
    if (!built.has(target)) {
      errors.push(`${os}/${cpu} -> ${target}, which release.yml does not build`);
    }
  }
}
for (const [key, target] of Object.entries(TARGETS)) {
  if (!built.has(target)) errors.push(`TARGETS[${key}] = ${target} is not a release target`);
}
let rejected = false;
try { resolveTarget('freebsd', 'x64'); } catch (e) { rejected = /unsupported platform/.test(e.message); }
if (!rejected) errors.push('an unsupported platform must fail with a clear message');
if (errors.length) {
  for (const e of errors) console.error(`[test-npm-shim] ${e}`);
  process.exit(1);
}
console.log(`[test-npm-shim] OK - ${pkg.os.length * pkg.cpu.length} os/cpu combinations map to release targets`);
JS
