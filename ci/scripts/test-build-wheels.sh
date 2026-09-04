#!/usr/bin/env bash
# Harness wrapper so ci/scripts/shell-tests.sh picks up the Python gate for
# build_wheels.py (the Path B wheel assembler). The real assertions live in the
# companion test-build-wheels.py (pure stdlib). CI runners always have python3;
# a local box without it skips with a note rather than false-failing.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

if ! command -v python3 >/dev/null 2>&1; then
  echo "[test-build-wheels] python3 not found; skipping (CI runners have it)"
  exit 0
fi

exec python3 ci/scripts/test-build-wheels.py
