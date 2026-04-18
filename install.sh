#!/usr/bin/env bash
# alint install script.
#
# Downloads a platform-matched release tarball from GitHub, verifies its
# SHA-256, and installs the `alint` binary to $INSTALL_DIR (default
# $HOME/.local/bin).
#
# Usage:
#   curl -sSL https://raw.githubusercontent.com/asamarts/alint/main/install.sh | bash
#
# Environment variables:
#   ALINT_VERSION   Tag to install (e.g. v0.1.0). Defaults to the latest release.
#   INSTALL_DIR     Destination directory. Defaults to $HOME/.local/bin.
#   ALINT_REPO      Override repository (for testing forks). Defaults to asamarts/alint.

set -euo pipefail

REPO="${ALINT_REPO:-asamarts/alint}"
VERSION="${ALINT_VERSION:-latest}"
INSTALL_DIR="${INSTALL_DIR:-${HOME}/.local/bin}"
BINARY="alint"

# ── Platform detection ───────────────────────────────────────────────

OS="$(uname -s)"
ARCH="$(uname -m)"
case "${OS}-${ARCH}" in
  Linux-x86_64)        TARGET="x86_64-unknown-linux-musl" ;;
  Linux-aarch64|Linux-arm64) TARGET="aarch64-unknown-linux-musl" ;;
  Darwin-x86_64)       TARGET="x86_64-apple-darwin" ;;
  Darwin-arm64)        TARGET="aarch64-apple-darwin" ;;
  *)
    echo "error: unsupported platform ${OS}/${ARCH}"
    echo "       on Windows, download the release tarball manually from:"
    echo "       https://github.com/${REPO}/releases"
    exit 1
    ;;
esac

echo "==> Detected platform: ${OS}/${ARCH} → ${TARGET}"

# ── Resolve version ──────────────────────────────────────────────────

if [[ "${VERSION}" == "latest" ]]; then
  echo "==> Resolving latest release tag"
  VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | awk -F'"' '/"tag_name":/ {print $4; exit}')
  if [[ -z "${VERSION}" ]]; then
    echo "error: could not resolve latest release tag from github api"
    echo "       try specifying ALINT_VERSION=v0.1.0 explicitly."
    exit 1
  fi
  echo "==> Latest version: ${VERSION}"
fi

ARCHIVE="alint-${VERSION}-${TARGET}.tar.gz"
BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
ARCHIVE_URL="${BASE_URL}/${ARCHIVE}"
SHA_URL="${ARCHIVE_URL}.sha256"

# ── Download + verify ────────────────────────────────────────────────

TMPDIR=$(mktemp -d)
trap 'rm -rf "${TMPDIR}"' EXIT

echo "==> Downloading ${ARCHIVE_URL}"
curl -fsSL -o "${TMPDIR}/${ARCHIVE}" "${ARCHIVE_URL}"
curl -fsSL -o "${TMPDIR}/${ARCHIVE}.sha256" "${SHA_URL}"

echo "==> Verifying SHA-256"
cd "${TMPDIR}"
if command -v sha256sum >/dev/null 2>&1; then
  sha256sum -c "${ARCHIVE}.sha256"
elif command -v shasum >/dev/null 2>&1; then
  shasum -a 256 -c "${ARCHIVE}.sha256"
else
  echo "error: neither sha256sum nor shasum is available — cannot verify download"
  exit 1
fi

# ── Extract + install ────────────────────────────────────────────────

echo "==> Extracting"
tar -xzf "${ARCHIVE}"

STAGED_DIR="alint-${VERSION}-${TARGET}"
if [[ ! -f "${STAGED_DIR}/${BINARY}" ]]; then
  echo "error: binary not found at ${TMPDIR}/${STAGED_DIR}/${BINARY}"
  exit 1
fi

mkdir -p "${INSTALL_DIR}"
cp "${STAGED_DIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
chmod +x "${INSTALL_DIR}/${BINARY}"

echo "==> Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"

# Post-install sanity
"${INSTALL_DIR}/${BINARY}" --version 2>/dev/null || true

# Helpful PATH hint
if ! echo ":${PATH}:" | grep -q ":${INSTALL_DIR}:"; then
  echo ""
  echo "note: ${INSTALL_DIR} is not in your PATH. Add it to your shell rc, e.g.:"
  echo "      echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.bashrc"
fi
