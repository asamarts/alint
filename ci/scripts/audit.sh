#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

echo "==> Running cargo audit"
# Advisory-only for v0.1: known vulnerabilities in upstream deps should be
# visible but must not block the pipeline until we have a policy.
cargo audit || {
    echo "==> WARNING: cargo audit found vulnerabilities (see above)"
    echo "==> These are in upstream dependencies, not alint code"
}
