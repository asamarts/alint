#!/usr/bin/env bash
# Install rustup on a Windows runner image that lacks it (windows-11-arm), so
# the rest of a job can use it as on every other image. A no-op where rustup is
# already on PATH. dtolnay/rust-toolchain installs rustup only off Windows, and
# cross-platform.yml calls rustup directly, so both workflows run this first.
#
# Downloads rustup-init.exe for the runner's own architecture from
# static.rust-lang.org and checks it against the .sha256 published beside it.
set -euo pipefail

if command -v rustup >/dev/null 2>&1; then
  echo "[ensure-rustup] $(rustup --version 2>/dev/null | head -n1)"
  exit 0
fi

case "${RUNNER_OS:-}-${RUNNER_ARCH:-}" in
  Windows-ARM64) host=aarch64-pc-windows-msvc ;;
  Windows-X64) host=x86_64-pc-windows-msvc ;;
  *)
    echo "[ensure-rustup] rustup is missing on ${RUNNER_OS:-?}/${RUNNER_ARCH:-?}; only Windows images are handled" >&2
    exit 1
    ;;
esac

dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
url="https://static.rust-lang.org/rustup/dist/${host}/rustup-init.exe"
curl --proto '=https' --tlsv1.2 -sSfL --retry 5 -o "$dir/rustup-init.exe" "$url"
curl --proto '=https' --tlsv1.2 -sSfL --retry 5 -o "$dir/rustup-init.exe.sha256" "$url.sha256"
expected=$(cut -d' ' -f1 <"$dir/rustup-init.exe.sha256")
actual=$(sha256sum "$dir/rustup-init.exe" | cut -d' ' -f1)
if [[ -z "$expected" || "$expected" != "$actual" ]]; then
  echo "[ensure-rustup] rustup-init.exe checksum mismatch: expected '$expected', got '$actual'" >&2
  exit 1
fi

"$dir/rustup-init.exe" -y --no-modify-path --default-toolchain none --profile minimal
cargo_bin="${CARGO_HOME:-$USERPROFILE/.cargo}/bin"
echo "$cargo_bin" >>"$GITHUB_PATH"
"$cargo_bin/rustup" --version
