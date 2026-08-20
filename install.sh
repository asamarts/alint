#!/usr/bin/env bash
# alint install script.
#
# Downloads a platform-matched release tarball from GitHub, verifies its
# SHA-256, and installs the `alint` binary to $INSTALL_DIR (default
# $HOME/.local/bin).
#
# Usage:
#   curl -sSL https://alint.org/install.sh | bash
#
# The one-liner fetches this script from the `main` branch (a moving ref). To pin
# and/or read the installer itself before running, use a tag-pinned raw URL:
#   curl -fsSL https://raw.githubusercontent.com/asamarts/alint/<tag>/install.sh -o install.sh
#   less install.sh && bash install.sh
# When `cosign` is present, this script also verifies the release's cosign-signed
# SHA256SUMS before installing (see SECURITY.md, "Verifying release artifacts").
#
# Environment variables:
#   ALINT_VERSION      Tag to install (e.g. v0.1.0). Defaults to the latest release.
#   INSTALL_DIR        Destination directory. Defaults to $HOME/.local/bin.
#   ALINT_REPO         Override repository (for testing forks). Defaults to asamarts/alint.
#   ALINT_SKIP_VERIFY  Set to 1 to skip cosign signature verification (best-effort;
#                      verification is skipped anyway when cosign is absent).

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
  # Fetch first so `set -o pipefail` does not trip on curl's SIGPIPE when
  # awk exits early after the first match.
  RELEASE_JSON=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest")
  VERSION=$(printf '%s\n' "${RELEASE_JSON}" \
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

# ── Optional signature verification (best-effort, cosign) ────────────
# If cosign is present, verify the release's cosign-signed SHA256SUMS and confirm
# this archive's digest is listed in it: authenticity, not just the integrity the
# per-file .sha256 above already checked. NEVER a hard dependency: skipped (with a
# note) when cosign is absent, when the release predates signing (no
# .cosign.bundle asset), or when ALINT_SKIP_VERIFY=1. cosign verifies the bundle
# offline (the Rekor proof is embedded), so a genuine verification FAILURE means a
# bad signature: treated as possible tampering, and aborts the install.
verify_signature() {
  if [[ "${ALINT_SKIP_VERIFY:-}" == "1" ]]; then
    echo "==> Skipping signature verification (ALINT_SKIP_VERIFY=1)"
    return 0
  fi
  if ! command -v cosign >/dev/null 2>&1; then
    echo "note: cosign not found; skipping signature verification (integrity was"
    echo "      checked above). Install cosign v3+ to verify authenticity, or see"
    echo "      https://github.com/${REPO}/blob/main/SECURITY.md#verifying-release-artifacts"
    return 0
  fi
  if ! curl -fsSL -o SHA256SUMS "${BASE_URL}/SHA256SUMS" 2>/dev/null \
     || ! curl -fsSL -o SHA256SUMS.cosign.bundle "${BASE_URL}/SHA256SUMS.cosign.bundle" 2>/dev/null; then
    echo "note: ${VERSION} has no cosign signature (predates release signing); skipping."
    return 0
  fi
  echo "==> Verifying release signature (cosign)"
  if ! cosign verify-blob \
      --bundle SHA256SUMS.cosign.bundle \
      --certificate-identity-regexp "^https://github\\.com/${REPO}/\\.github/workflows/release\\.yml@refs/tags/v" \
      --certificate-oidc-issuer https://token.actions.githubusercontent.com \
      SHA256SUMS >/dev/null 2>&1; then
    echo "error: cosign could not verify SHA256SUMS for ${VERSION}: it is not validly" >&2
    echo "       signed by ${REPO}'s release workflow. Aborting (possible tampering)." >&2
    echo "       Set ALINT_SKIP_VERIFY=1 to bypass." >&2
    exit 1
  fi
  # Confirm this archive's digest appears in the now-authenticated manifest.
  local want got
  want="$(awk -v f="${ARCHIVE}" '{ n = $2; sub(/^\*/, "", n); if (n == f) { print $1; exit } }' SHA256SUMS)"
  if command -v sha256sum >/dev/null 2>&1; then
    got="$(sha256sum "${ARCHIVE}" | awk '{print $1}')"
  else
    got="$(shasum -a 256 "${ARCHIVE}" | awk '{print $1}')"
  fi
  if [[ -z "${want}" || "${want}" != "${got}" ]]; then
    echo "error: ${ARCHIVE} is not the file signed in SHA256SUMS. Aborting (tampering?)." >&2
    exit 1
  fi
  echo "==> Signature OK (SHA256SUMS signed by ${REPO}'s release workflow)"
}
verify_signature

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
