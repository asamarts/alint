#!/usr/bin/env bash
set -euo pipefail

# Regenerate `Formula/alint.rb` for the given VERSION and emit it
# on stdout. Invoked by `.github/workflows/release.yml::homebrew`;
# takes SHA-256s from the release's SHA256SUMS manifest so there's
# no rebuild or network re-download.
#
# Usage:
#   VERSION=v0.4.7 SHA256SUMS=/path/to/SHA256SUMS \
#     ci/scripts/update-homebrew-formula.sh > Formula/alint.rb
#
# Env:
#   VERSION      — git tag for the release (e.g. `v0.4.7`). Required.
#   SHA256SUMS   — path to the combined sums file (see
#                  `.github/workflows/release.yml`'s `aggregate
#                  SHA256SUMS` step). Required.
#
# The emitted formula covers macOS (arm + intel) and Linuxbrew
# (x86_64 + aarch64). Windows users install via cargo or the
# install.sh flow — Homebrew has no Windows story.

VERSION="${VERSION:?VERSION is required (e.g. v0.4.7)}"
SHA256SUMS="${SHA256SUMS:?SHA256SUMS is required (path to the combined manifest)}"

if [[ ! -f "$SHA256SUMS" ]]; then
  echo "ERROR: SHA256SUMS file not found at ${SHA256SUMS}" >&2
  exit 1
fi

# Extract the SHA-256 hash for `alint-<version>-<target>.tar.gz`
# from the combined manifest. Tolerates both `sha256sum`-style
# output (`<hash>  <file>`) and Windows-mode output
# (`<hash> *<file>`).
sha_for() {
  local target="$1"
  local file="alint-${VERSION}-${target}.tar.gz"
  local sum
  sum="$(awk -v t="$file" \
    '{ name = $2; sub(/^\*/, "", name); if (name == t) { print $1; exit } }' \
    "$SHA256SUMS")"
  if [[ -z "$sum" ]]; then
    echo "ERROR: no SHA-256 for ${file} in ${SHA256SUMS}" >&2
    exit 1
  fi
  echo "$sum"
}

V_NO_PREFIX="${VERSION#v}"
SHA_AARCH64_DARWIN="$(sha_for aarch64-apple-darwin)"
SHA_X86_64_DARWIN="$(sha_for x86_64-apple-darwin)"
SHA_AARCH64_LINUX="$(sha_for aarch64-unknown-linux-musl)"
SHA_X86_64_LINUX="$(sha_for x86_64-unknown-linux-musl)"

cat <<EOF
class Alint < Formula
  desc "Language-agnostic linter for repository structure and content"
  homepage "https://github.com/asamarts/alint"
  version "${V_NO_PREFIX}"
  license any_of: ["Apache-2.0", "MIT"]

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/asamarts/alint/releases/download/${VERSION}/alint-${VERSION}-aarch64-apple-darwin.tar.gz"
      sha256 "${SHA_AARCH64_DARWIN}"
    else
      url "https://github.com/asamarts/alint/releases/download/${VERSION}/alint-${VERSION}-x86_64-apple-darwin.tar.gz"
      sha256 "${SHA_X86_64_DARWIN}"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/asamarts/alint/releases/download/${VERSION}/alint-${VERSION}-aarch64-unknown-linux-musl.tar.gz"
      sha256 "${SHA_AARCH64_LINUX}"
    else
      url "https://github.com/asamarts/alint/releases/download/${VERSION}/alint-${VERSION}-x86_64-unknown-linux-musl.tar.gz"
      sha256 "${SHA_X86_64_LINUX}"
    end
  end

  def install
    bin.install "alint"
  end

  test do
    assert_match(/alint \d+\.\d+\.\d+/, shell_output("#{bin}/alint --version"))
  end
end
EOF
