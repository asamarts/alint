#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

# cargo-audit may not be on the runner. It is on the self-hosted CI runner
# but NOT on a fresh GitHub-hosted ubuntu-latest (the fork-PR lane — see
# docs/design/v0.14/ci-fork-pr-isolation.md). Install on demand, pinned with
# --locked, so the gate is portable across both. Swatinem/rust-cache persists
# ~/.cargo/bin so the install only pays once per cache window. (Mirrors
# deny.sh.)
if ! command -v cargo-audit >/dev/null 2>&1; then
    echo "==> cargo-audit not found; installing (cargo install --locked)"
    # Clear RUSTFLAGS for the tool build only: CI sets `-D warnings` for
    # *alint's* code, but applying it while compiling a third-party tool
    # would fail the install on any upstream warning.
    RUSTFLAGS= cargo install cargo-audit --locked
fi

echo "==> Running cargo audit"
# Advisory-only for v0.1: known vulnerabilities in upstream deps should be
# visible but must not block the pipeline until we have a policy.
cargo audit || {
    echo "==> WARNING: cargo audit found vulnerabilities (see above)"
    echo "==> These are in upstream dependencies, not alint code"
}
