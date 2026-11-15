#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

echo "==> Running cargo clippy"
cargo clippy --workspace --all-targets -- -D warnings

# The reference parsers behind alint-core's test-only feature (ADR-0041).
echo "==> Running cargo clippy -p alint-core --features yaml-reference"
cargo clippy -p alint-core --all-targets --features yaml-reference -- -D warnings
