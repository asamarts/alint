#!/usr/bin/env bash
# Gate for the npm distribution: npm/platforms.json against release.yml's build
# matrix, npm/package.json's exact-version optionalDependencies and absent
# install script, and the shim's behaviour (ci/scripts/test-npm-shim.js holds
# the assertions).
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

# The build matrix rows are the only `- target:` entries in release.yml
# (plain-text read; no PyYAML dependency on the runner).
targets=$(sed -nE 's/^[[:space:]]+- target:[[:space:]]*([A-Za-z0-9_-]+)[[:space:]]*$/\1/p' \
  .github/workflows/release.yml)
if [[ -z "$targets" ]]; then
  echo "[test-npm-shim] could not read release.yml build-matrix targets" >&2
  exit 1
fi

RELEASE_TARGETS="$targets" exec node ci/scripts/test-npm-shim.js
