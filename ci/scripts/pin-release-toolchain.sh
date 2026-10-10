#!/usr/bin/env bash
# Pin the Rust toolchain for the rest of a release.yml job, and prove it.
#
# release.yml installs $RELEASE_RUST (an exact x.y.z, set once at workflow
# level) via dtolnay/rust-toolchain, but that action only runs `rustup default`,
# and rustup gives rust-toolchain.toml (`channel = "stable"`, kept floating for
# dev + CI) precedence over the default. Without this step every cargo call in
# the checkout would quietly build the release with whatever stable is current.
# RUSTUP_TOOLCHAIN beats rust-toolchain.toml (only an explicit `cargo +x` beats
# it, which msrv.sh uses on purpose), so export it for every later step via
# $GITHUB_ENV, then fail the job unless rustc in the checkout really is the pin.
set -euo pipefail

: "${RELEASE_RUST:?RELEASE_RUST must be set (workflow-level env in release.yml)}"
if [[ ! "$RELEASE_RUST" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "::error::RELEASE_RUST must be an exact x.y.z toolchain, got '${RELEASE_RUST}'" >&2
  exit 1
fi

export RUSTUP_TOOLCHAIN="$RELEASE_RUST"
if [[ -n "${GITHUB_ENV:-}" ]]; then
  echo "RUSTUP_TOOLCHAIN=${RELEASE_RUST}" >> "$GITHUB_ENV"
fi

got="$(rustc --version)"
case "$got" in
  "rustc ${RELEASE_RUST} "*) ;;
  *)
    echo "::error::release toolchain is '${got}', expected rustc ${RELEASE_RUST} (RUSTUP_TOOLCHAIN not honoured?)" >&2
    exit 1
    ;;
esac
echo "==> release toolchain pinned: ${got}; $(cargo --version)"
