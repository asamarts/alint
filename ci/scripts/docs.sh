#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

echo "==> Running cargo doc"
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace
