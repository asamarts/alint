#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

echo "==> Compiling criterion bench targets (no run)"
cargo bench -p alint-bench --no-run --locked

echo "==> Running xtask bench-release --quick"
# Smoke only — we do not gate on numbers on a self-hosted runner. Real
# headline numbers come from bench-release.yml on pinned platforms.
cargo run -p xtask --release --locked -- bench-release --quick
