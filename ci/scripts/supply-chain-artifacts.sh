#!/usr/bin/env bash
set -euo pipefail

# Generate alint's release supply-chain / attribution artifacts into OUT_DIR:
#
#   THIRD-PARTY-LICENSES.html  every third-party crate compiled into the alint
#                              binary, with its full license text (cargo-about,
#                              driven by about.toml + about.hbs at the repo root).
#   alint.cdx.json             a CycloneDX 1.5 software bill of materials for the
#                              shipped `alint` binary and its transitive graph
#                              (cargo-cyclonedx).
#
# Single source of truth for two callers:
#   * .github/workflows/release.yml -- generates both, folds them into
#     SHA256SUMS, and attaches them to every GitHub Release (a later phase
#     signs SHA256SUMS with cosign, so the attribution travels signed too).
#   * .github/workflows/ci.yml -- runs it as a pre-merge gate: cargo-about
#     FAILS generation on any dependency whose license is not in about.toml's
#     `accepted` set, so an out-of-policy license is caught before a release,
#     mirroring (at attribution time) the deny.toml policy gate.
#
# Bootstraps the two cargo subcommands on demand -- like ci/scripts/deny.sh --
# so it works on a bare ubuntu-latest runner. Versions are pinned so the bundle
# is reproducible across runs.
#
# Usage: ci/scripts/supply-chain-artifacts.sh [OUT_DIR]
#        OUT_DIR defaults to target/supply-chain (under the gitignored target/).

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

OUT_DIR="${1:-target/supply-chain}"
mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"   # absolutise before any cwd-sensitive step

CARGO_ABOUT_VERSION="0.9.2"
CARGO_CYCLONEDX_VERSION="0.5.9"

# cargo-cyclonedx has no package filter: it writes an SBOM next to EVERY
# workspace member's Cargo.toml (crates/*/alint.json, xtask/alint.json). Sweep
# those strays on every exit -- success or failure -- so neither a local run nor
# the CI gate leaves the working tree dirty. No alint.json is tracked, so the
# find can only ever match these generated strays (target/ is excluded anyway).
cleanup_sboms() {
  find "$REPO_ROOT" -name 'alint.json' -not -path '*/target/*' -delete 2>/dev/null || true
}
trap cleanup_sboms EXIT

ensure() {
  local bin="$1" crate="$2" version="$3"; shift 3
  if ! command -v "$bin" >/dev/null 2>&1; then
    echo "==> installing ${crate} =${version}"
    cargo install --locked "$crate" --version "=${version}" "$@"
  fi
}

# cargo-about's binary lives behind the non-default `cli` feature (>= 0.9);
# without it `cargo install cargo-about` compiles but installs no binary.
ensure cargo-about     cargo-about     "$CARGO_ABOUT_VERSION" --features cli
ensure cargo-cyclonedx cargo-cyclonedx "$CARGO_CYCLONEDX_VERSION"

echo "==> THIRD-PARTY-LICENSES.html (cargo about generate)"
cargo about generate about.hbs --output-file "$OUT_DIR/THIRD-PARTY-LICENSES.html"

echo "==> alint.cdx.json (cargo cyclonedx, CycloneDX 1.5)"
# --target all   : cover every shipped triple's dependencies in one bundle.
# --no-build-deps: build-time deps are not linked into the shipped binary.
# The alint binary crate's SBOM (crates/alint/alint.json) is the one that
# represents the shipped artifact + its full transitive graph; the rest are
# per-member strays the trap sweeps.
cargo cyclonedx --manifest-path crates/alint/Cargo.toml \
  --format json --target all --no-build-deps --spec-version 1.5 \
  --override-filename alint -q
mv "crates/alint/alint.json" "$OUT_DIR/alint.cdx.json"

echo "==> supply-chain artifacts written to ${OUT_DIR}:"
ls -la "$OUT_DIR/THIRD-PARTY-LICENSES.html" "$OUT_DIR/alint.cdx.json"
