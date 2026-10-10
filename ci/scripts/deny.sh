#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

# cargo-deny may not be on the runner. It is on the self-hosted CI
# runner, but NOT on GitHub-hosted ubuntu-latest (used by
# release.yml preflight). Install on demand, pinned with --locked,
# so the gate is portable across both. Swatinem/rust-cache persists
# ~/.cargo/bin so the install only pays once per cache window.
#
# The version is PINNED because deny.toml's schema and the lint codes passed
# below are cargo-deny-version specific; a runner with a different version
# gets the pinned one installed. Bump it deliberately (and re-check deny.toml
# against that release's config docs).
CARGO_DENY_VERSION="0.19.9"
if [[ "$(cargo deny --version 2>/dev/null || true)" != "cargo-deny ${CARGO_DENY_VERSION}" ]]; then
    echo "==> cargo-deny ${CARGO_DENY_VERSION} not found; installing (cargo install --locked)"
    # Clear RUSTFLAGS for the tool build only: CI sets `-D warnings`
    # for *alint's* code, but applying it while compiling a
    # third-party tool would fail the install on any upstream
    # warning under the current toolchain.
    RUSTFLAGS='' cargo install cargo-deny --locked --version "${CARGO_DENY_VERSION}"
fi

echo "==> Running cargo deny check licenses bans sources (blocking)"
# Licenses is the v0.9.22-audit gap (no deny.toml meant the license
# gate allowlisted nothing). bans (no `*` version reqs) + sources
# (crates.io only) are cheap supply-chain gates. A violation here
# fails CI and blocks a release. Policy lives in deny.toml.
cargo deny check licenses bans sources

echo "==> Running cargo deny check advisories (blocking on vulnerabilities)"
# BLOCKING: a RustSec vulnerability advisory against any crate in the graph
# fails CI and the release preflight. The informational `unmaintained` and
# `unsound` advisories are demoted to warnings here (-W; cargo-deny has no
# config-file level for them) and yanked crates warn via deny.toml
# `yanked = "warn"`. Waivers live in deny.toml [advisories].ignore, the single
# list ci/scripts/audit.sh also honours.
cargo deny check -W unmaintained -W unsound advisories
