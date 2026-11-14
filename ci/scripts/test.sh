#!/usr/bin/env bash
# Run the Rust workspace test suite. Kept narrowly scoped to
# `cargo test`; shell-script test harnesses live in
# `ci/scripts/shell-tests.sh` and run in their own CI job so a
# homebrew-formula golden drift doesn't fail the Rust "Test" job.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

echo "==> Running cargo test --workspace --no-fail-fast"
cargo test --workspace --locked --no-fail-fast
