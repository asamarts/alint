#!/usr/bin/env bash
# Compile the whole workspace with the exact minimum supported Rust version.
#
# The MSRV is read from `[workspace.package].rust-version` in Cargo.toml, the
# single source of truth that every published crate inherits, so CI never
# carries a hand-copied toolchain number that can drift from what crates.io
# advertises. Used by ci.yml (`msrv` job, pre-merge) and release.yml
# (`preflight`, so a crate is never published claiming an MSRV it was not
# built on).
#
# Usage:
#   ci/scripts/msrv.sh            # install (if missing) + cargo check on the MSRV
#   ci/scripts/msrv.sh --print    # print the MSRV toolchain (e.g. 1.88.0) and exit
set -euo pipefail

REPO_ROOT="${MSRV_REPO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$REPO_ROOT"

# Only the [workspace.package] table: a member's own rust-version (none today)
# or a dependency table must not be picked up.
msrv="$(awk '
  /^\[/ { in_pkg = ($0 == "[workspace.package]") ; next }
  in_pkg && /^rust-version[[:space:]]*=/ {
    sub(/^[^"]*"/, ""); sub(/".*$/, ""); print; exit
  }
' Cargo.toml)"

if [[ ! "$msrv" =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?$ ]]; then
  echo "[msrv] could not read [workspace.package].rust-version from Cargo.toml (got '${msrv}')" >&2
  exit 1
fi
# rustup wants a full x.y.z for a pinned release; rust-version is usually x.y.
[[ "$msrv" =~ ^[0-9]+\.[0-9]+$ ]] && msrv="${msrv}.0"

if [[ "${1:-}" == "--print" ]]; then
  echo "$msrv"
  exit 0
fi

if ! rustup run "$msrv" rustc --version >/dev/null 2>&1; then
  rustup toolchain install "$msrv" --profile minimal --no-self-update
fi
echo "[msrv] cargo +${msrv} check --locked --workspace --all-targets"
# `+toolchain` overrides rust-toolchain.toml (which pins `stable` for dev).
cargo "+${msrv}" check --locked --workspace --all-targets
