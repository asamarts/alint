#!/usr/bin/env bash
# Patch-release crate compatibility: compile the previous release's published
# crates against this workspace's libraries.
#
# Every published alint crate depends on its siblings with a caret
# requirement (`^0.21.0`). Once a patch release is on crates.io, an unlocked
# `cargo install alint --version <previous>` resolves those requirements to
# the NEW libraries and builds the previous CLI against them. A public API
# the previous crates call that this release changed (a signature, a struct
# a previous crate builds with a literal) then fails that install. This check
# downloads the previous release's `alint`, `alint-lsp` and `alint-testkit`
# and runs `cargo check` on each with `[patch.crates-io]` pointing every
# sibling at this workspace. A failure means: keep the old API (a
# `#[deprecated]` shim) rather than ship.
#
# Not covered: a consumer that pins one library with `=` to the previous
# version while taking newer siblings. Cargo never resolves that on its own.
#
# Usage: ci/scripts/check-patch-compat.sh <previous-version>...
#   e.g.  ci/scripts/check-patch-compat.sh 0.21.0 0.21.1
#
# Every earlier patch on the line resolves to this release, so pass them all.
#
# Each `.crate` ships the previous release's Cargo.lock, which pins every
# sibling at <previous-version>; cargo then ignores a `[patch]` whose version
# differs and only warns. The check deletes it, as an unlocked install does,
# with all features on, and fails unless every sibling in the build graph is
# this workspace's copy.
#
# Needs network access (crates.io downloads, dependency resolution). Scratch
# goes under $TMPDIR; set CARGO_TARGET_DIR to reuse a target directory.
#
# Exit codes:
#   0  every previous crate compiles against this workspace
#   1  at least one does not, or one did not build against this workspace
#   2  usage or download error, a <previous-version> that is not an earlier
#      release on this workspace's caret line, or a prerelease workspace

set -euo pipefail

if (($# == 0)); then
  echo "usage: $0 <previous-version>... (e.g. 0.21.0 0.21.1)" >&2
  exit 2
fi

# The workspace this script belongs to, wherever it is run from.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CURRENT="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' "$ROOT/Cargo.toml")"
# A caret requirement does not select a prerelease, so an unlocked install of
# a previous release never resolves to one: there is nothing to check.
if [[ ! "$CURRENT" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "patch-compat: the workspace version $CURRENT is not a plain x.y.z release" >&2
  exit 2
fi
IFS=. read -r cur_major cur_minor cur_patch <<<"$CURRENT"
for PREVIOUS in "$@"; do
  if [[ ! "$PREVIOUS" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "usage: $0 <previous-version>... (e.g. 0.21.0 0.21.1)" >&2
    exit 2
  fi
  # A caret requirement on 0.y.z accepts 0.y.z' for z' >= z: the same 0.y line.
  IFS=. read -r prev_major prev_minor prev_patch <<<"$PREVIOUS"
  if [[ "$prev_major.$prev_minor" != "$cur_major.$cur_minor" || "$prev_major" != 0 ]] \
    || ((10#$prev_patch >= 10#$cur_patch)); then
    echo "patch-compat: $PREVIOUS is not an earlier 0.y.z on this workspace's line ($CURRENT)" >&2
    exit 2
  fi
done
SIBLINGS=(alint-core alint-dsl alint-rules alint-output alint-lsp alint-testkit)
CHECKED=(alint alint-lsp alint-testkit)

WORK="$(mktemp -d "${TMPDIR:-/tmp}/alint-patch-compat.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
TARGET="${CARGO_TARGET_DIR:-$WORK/target}"

failed=0
for PREVIOUS in "$@"; do
  for crate in "${CHECKED[@]}"; do
    url="https://static.crates.io/crates/$crate/$crate-$PREVIOUS.crate"
    if ! curl -fsSL --retry 3 -A "alint-patch-compat (https://github.com/asamarts/alint)" "$url" \
      | tar -xz -C "$WORK"; then
      echo "patch-compat: could not download $crate $PREVIOUS" >&2
      exit 2
    fi
    dir="$WORK/$crate-$PREVIOUS"
    {
      echo
      echo "[patch.crates-io]"
      for sibling in "${SIBLINGS[@]}"; do
        [[ "$sibling" == "$crate" ]] && continue
        echo "$sibling = { path = \"$ROOT/crates/$sibling\" }"
      done
    } >>"$dir/Cargo.toml"
    rm -f "$dir/Cargo.lock"
    echo "==> patch-compat: $crate $PREVIOUS against $ROOT ($CURRENT)"
    if ! (cd "$dir" && CARGO_TARGET_DIR="$TARGET" cargo check --quiet --keep-going --all-features); then
      echo "patch-compat: $crate $PREVIOUS does not compile against this workspace" >&2
      failed=1
      continue
    fi
    # Every sibling the build used must be this workspace's copy (`name vX (path)`).
    if ! graph="$(cd "$dir" && cargo tree --quiet --all-features --prefix none -e normal,build \
      --format '{p}')"; then
      echo "patch-compat: could not read $crate $PREVIOUS's dependency graph" >&2
      failed=1
      continue
    fi
    others=()
    for sibling in "${SIBLINGS[@]}"; do
      [[ "$sibling" != "$crate" ]] && others+=("$sibling")
    done
    pattern="^($(IFS='|'; echo "${others[*]}")) v"
    unpatched="$(grep -E "$pattern" <<<"$graph" | grep -vF "($ROOT/crates/" | sort -u || true)"
    if [[ -n "$unpatched" ]]; then
      echo "patch-compat: $crate $PREVIOUS built against siblings outside this workspace:" >&2
      echo "$unpatched" >&2
      failed=1
    fi
  done
done

if [[ "$failed" == 0 ]]; then
  echo "patch-compat: alint, alint-lsp and alint-testkit $* compile against this workspace"
fi
exit "$failed"
