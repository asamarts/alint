#!/usr/bin/env bash
# Harness wrapper so ci/scripts/shell-tests.sh picks up the Python gate for
# stage_npm_packages.py (the npm package stager). The real assertions live in the
# companion test-stage-npm-packages.py (pure stdlib). CI runners always have python3;
# a local box without it skips with a note rather than false-failing.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

if ! command -v python3 >/dev/null 2>&1; then
  echo "[test-stage-npm-packages] python3 not found; skipping (CI runners have it)"
  exit 0
fi

exec python3 ci/scripts/test-stage-npm-packages.py
