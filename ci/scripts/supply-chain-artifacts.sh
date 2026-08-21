#!/usr/bin/env bash
set -euo pipefail

# Generate alint's release supply-chain / attribution artifacts into OUT_DIR,
# both scoped to the SHIPPED `alint` binary (crates/alint) so the two artifacts
# describe the same graph -- not the whole dev workspace (which would attribute
# bench/test/xtask-only crates that never ship in the binary):
#
#   THIRD-PARTY-LICENSES.html  every crate compiled into the alint binary, with
#                              its full license text (cargo-about, driven by
#                              about.toml + about.hbs at the repo root).
#   alint.cdx.json             a CycloneDX 1.5 software bill of materials for the
#                              same binary + its transitive graph (cargo-cyclonedx).
#
# Single source of truth for two callers:
#   * .github/workflows/release.yml -- generates both, folds them into
#     SHA256SUMS, stages them into every tarball + the ghcr image, and attaches
#     them to every GitHub Release (a later phase signs SHA256SUMS with cosign,
#     so the attribution travels signed too).
#   * .github/workflows/ci.yml -- runs this whole script as a pre-merge gate (the
#     `supply-chain` job), so a broken about.toml / about.hbs / lockfile, or an
#     out-of-policy license, is caught on the PR rather than at release time.
#
# Bootstraps the two cargo subcommands on demand -- like ci/scripts/deny.sh --
# so it works on a bare ubuntu-latest runner. Tool versions are pinned AND
# SOURCE_DATE_EPOCH is set, so both artifacts are byte-reproducible across runs
# (a property P1-b's cosign signing over SHA256SUMS relies on).
#
# Usage: ci/scripts/supply-chain-artifacts.sh [OUT_DIR]
#        OUT_DIR defaults to target/supply-chain (under the gitignored target/).

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

OUT_DIR="${1:-target/supply-chain}"
mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"   # absolutise before any cwd-sensitive step

# The shipped artifact: release-binary.sh builds `-p alint`, so both the license
# bundle and the SBOM scope to this manifest, keeping them on the same graph.
BINARY_MANIFEST="crates/alint/Cargo.toml"

CARGO_ABOUT_VERSION="0.9.2"
CARGO_CYCLONEDX_VERSION="0.5.9"

# Byte-reproducible artifacts: pin cargo-cyclonedx's metadata.timestamp to the
# tagged commit's date. Without this each run embeds now(), so a verifier
# regenerating from the tag would compute a different hash than the signed one.
# (cargo-cyclonedx OMITS the optional CycloneDX serialNumber under
# SOURCE_DATE_EPOCH -- a random per-run UUID cannot be reproducible -- so a
# deterministic serialNumber is injected after generation below; actions/attest
# rejects an SBOM that carries none.)
SOURCE_DATE_EPOCH="$(git -C "$REPO_ROOT" log -1 --format=%ct 2>/dev/null || echo 0)"
export SOURCE_DATE_EPOCH

# cargo-cyclonedx has no package filter: it writes an SBOM next to EVERY
# workspace member's Cargo.toml (crates/*/alint.json, xtask/alint.json). Sweep
# those strays on every exit -- success or failure -- so neither a local run nor
# the CI gate leaves the working tree dirty. Scoped to the member roots
# (crates/, xtask/) rather than the whole repo, so it can only ever match
# cargo-cyclonedx's own output, never an unrelated alint.json elsewhere.
cleanup_sboms() {
  find "$REPO_ROOT/crates" "$REPO_ROOT/xtask" -maxdepth 2 -name 'alint.json' \
    -not -path '*/target/*' -delete 2>/dev/null || true
}
trap cleanup_sboms EXIT

# Install a pinned cargo subcommand if it is absent OR a different version is
# active. A presence-only check would let a stale binary that rust-cache's
# cache-bin persisted across a version bump silently defeat the pin; the version
# probe uses cargo's subcommand resolution so it works regardless of PATH.
ensure() {
  local bin="$1" crate="$2" version="$3"; shift 3
  if ! cargo "${bin#cargo-}" --version 2>/dev/null | grep -qF "$version"; then
    echo "==> installing ${crate} =${version}"
    cargo install --locked --force "$crate" --version "=${version}" "$@"
  fi
}

# cargo-about's binary lives behind the non-default `cli` feature (>= 0.9);
# without it `cargo install cargo-about` compiles but installs no binary.
ensure cargo-about     cargo-about     "$CARGO_ABOUT_VERSION" --features cli
ensure cargo-cyclonedx cargo-cyclonedx "$CARGO_CYCLONEDX_VERSION"

echo "==> THIRD-PARTY-LICENSES.html (cargo about, scoped to the alint binary)"
cargo about generate --manifest-path "$BINARY_MANIFEST" about.hbs \
  --output-file "$OUT_DIR/THIRD-PARTY-LICENSES.html"

echo "==> alint.cdx.json (cargo cyclonedx, CycloneDX 1.5, scoped to the alint binary)"
# --target all   : cover every shipped triple's dependencies in one bundle.
# --no-build-deps: build-time deps are not linked into the shipped binary.
cargo cyclonedx --manifest-path "$BINARY_MANIFEST" \
  --format json --target all --no-build-deps --spec-version 1.5 \
  --override-filename alint -q
mv "crates/alint/alint.json" "$OUT_DIR/alint.cdx.json"

# actions/attest's checkIsCycloneDX treats serialNumber as MANDATORY, though the
# CycloneDX spec marks it optional -- and cargo-cyclonedx emits none under
# SOURCE_DATE_EPOCH (see above), so `attest-sbom` rejects the bundle with
# "Unsupported SBOM format. Must be valid SPDX or CycloneDX JSON." Inject a
# DETERMINISTIC RFC-4122 v5 URN derived from the crate version: reproducible
# across runs (preserving the byte-identity cosign-over-SHA256SUMS relies on) yet
# unique per release. python3's uuid5 is portable across Linux + macOS dev boxes
# (uuidgen's v5 flags are Linux-only).
sbom_version="$(jq -r '.metadata.component.version' "$OUT_DIR/alint.cdx.json")"
serial_uuid="$(python3 -c 'import uuid, sys; print(uuid.uuid5(uuid.NAMESPACE_URL, "https://alint.org/sbom/alint@" + sys.argv[1]))' "$sbom_version")"
sbom_tmp="$(mktemp)"
jq --arg sn "urn:uuid:${serial_uuid}" '.serialNumber = $sn' "$OUT_DIR/alint.cdx.json" > "$sbom_tmp"
mv "$sbom_tmp" "$OUT_DIR/alint.cdx.json"
echo "==> injected deterministic serialNumber (urn:uuid:${serial_uuid})"

# Gate the invariant this whole fix rests on: actions/attest's checkIsCycloneDX
# requires bomFormat + specVersion + a serialNumber. Assert the generated SBOM
# carries a urn:uuid: serialNumber HERE -- run by the ci.yml `supply-chain` job
# and the release preflight -- so a regression (e.g. a cargo-cyclonedx bump that
# changes its serialNumber behaviour) fails a PR or the preflight, never
# mid-release the way v0.15.1's attest-sbom did.
jq -e '(.serialNumber // "") | startswith("urn:uuid:")' "$OUT_DIR/alint.cdx.json" >/dev/null \
  || { echo "FATAL: alint.cdx.json lacks a urn:uuid: serialNumber; attest-sbom would reject it" >&2; exit 1; }

echo "==> supply-chain artifacts written to ${OUT_DIR}:"
ls -la "$OUT_DIR/THIRD-PARTY-LICENSES.html" "$OUT_DIR/alint.cdx.json"
