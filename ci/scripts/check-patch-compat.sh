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
# Usage: ci/scripts/check-patch-compat.sh <previous-version>
#   e.g.  ci/scripts/check-patch-compat.sh 0.21.0
#
# Needs network access (crates.io downloads, dependency resolution). Scratch
# goes under $TMPDIR; set CARGO_TARGET_DIR to reuse a target directory.
#
# Exit codes:
#   0  every previous crate compiles against this workspace
#   1  at least one does not
#   2  usage or download error

set -euo pipefail

PREVIOUS="${1:-}"
if [[ ! "$PREVIOUS" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "usage: $0 <previous-version, e.g. 0.21.0>" >&2
  exit 2
fi

ROOT="$(git rev-parse --show-toplevel)"
SIBLINGS=(alint-core alint-dsl alint-rules alint-output alint-lsp alint-testkit)
CHECKED=(alint alint-lsp alint-testkit)

WORK="$(mktemp -d "${TMPDIR:-/tmp}/alint-patch-compat.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
TARGET="${CARGO_TARGET_DIR:-$WORK/target}"

failed=0
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
  echo "==> patch-compat: $crate $PREVIOUS against $ROOT"
  if ! (cd "$dir" && CARGO_TARGET_DIR="$TARGET" cargo check --quiet --keep-going); then
    echo "patch-compat: $crate $PREVIOUS does not compile against this workspace" >&2
    failed=1
  fi
done

if [[ "$failed" == 0 ]]; then
  echo "patch-compat: alint, alint-lsp and alint-testkit $PREVIOUS compile against this workspace"
fi
exit "$failed"
