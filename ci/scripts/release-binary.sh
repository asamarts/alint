#!/usr/bin/env bash
set -euo pipefail

# Build a release binary for the given target triple and package it with
# checksum under target/release-artifacts/. Invoked per matrix entry by
# .github/workflows/release.yml.
#
# Env:
#   TARGET     - Rust target triple (required)
#   VERSION    - version tag (e.g. v0.1.0); defaults to "dev"
#   USE_CROSS  - "true" to invoke `cross` instead of `cargo`; defaults to false

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

TARGET="${TARGET:?TARGET is required (Rust target triple)}"
VERSION="${VERSION:-dev}"
USE_CROSS="${USE_CROSS:-false}"
BIN_NAME="alint"

BIN_EXT=""
if [[ "$TARGET" == *windows* ]]; then
  BIN_EXT=".exe"
fi

ARCHIVE_BASE="alint-${VERSION}-${TARGET}"
ARCHIVE_NAME="${ARCHIVE_BASE}.tar.gz"
STAGING="target/release-staging/${ARCHIVE_BASE}"
ARTIFACTS_DIR="target/release-artifacts"

mkdir -p "$STAGING" "$ARTIFACTS_DIR"

BUILD_CMD="cargo"
if [[ "$USE_CROSS" == "true" ]]; then
  if ! command -v cross >/dev/null 2>&1; then
    echo "==> Installing cross"
    cargo install --locked cross
  fi
  BUILD_CMD="cross"
fi

echo "==> Building ${BIN_NAME} for ${TARGET} (via ${BUILD_CMD})"
rustup target add "$TARGET" 2>/dev/null || true
$BUILD_CMD build --release --locked --target "$TARGET" -p alint-cli

BIN_PATH="target/${TARGET}/release/${BIN_NAME}${BIN_EXT}"
if [[ ! -f "$BIN_PATH" ]]; then
  echo "==> ERROR: expected binary at ${BIN_PATH}"
  exit 1
fi

echo "==> Staging ${ARCHIVE_BASE}"
cp "$BIN_PATH" "$STAGING/"
# Ship optional side-by-side reference docs if present at repo root.
for extra in LICENSE LICENSE-APACHE LICENSE-MIT README.md NOTICE; do
  [[ -f "$extra" ]] && cp "$extra" "$STAGING/" || true
done

# Always include the canonical architecture + methodology docs.
mkdir -p "$STAGING/docs"
for d in docs/design/ARCHITECTURE.md docs/design/ROADMAP.md docs/benchmarks/METHODOLOGY.md; do
  [[ -f "$d" ]] && cp "$d" "$STAGING/docs/" || true
done

echo "==> Packaging ${ARCHIVE_NAME}"
tar -czf "${ARTIFACTS_DIR}/${ARCHIVE_NAME}" -C target/release-staging "$ARCHIVE_BASE"

echo "==> Computing SHA-256"
if command -v sha256sum >/dev/null 2>&1; then
  (cd "$ARTIFACTS_DIR" && sha256sum "$ARCHIVE_NAME" > "${ARCHIVE_NAME}.sha256")
else
  (cd "$ARTIFACTS_DIR" && shasum -a 256 "$ARCHIVE_NAME" > "${ARCHIVE_NAME}.sha256")
fi

echo "==> Artifacts:"
ls -la "$ARTIFACTS_DIR"
