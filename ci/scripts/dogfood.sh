#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

echo "==> Building release binary"
cargo build --release --locked -p alint

echo "==> Running alint against its own repository (dogfood)"
# --fail-on-warning would make this strict; for v0.1 the dogfood config has
# some warnings by design (README/LICENSE are tracked as warnings until they
# are added). Errors will still fail the pipeline.
./target/release/alint check
