#!/usr/bin/env bash
set -euo pipefail

# Each crates.io-published member carries its OWN copy of the dual-license texts
# + NOTICE, so its published `.crate` is self-contained: cargo inherits the SPDX
# `license` field from the workspace root but NOT the license *files*, and a
# `../`-escaping `include` is rejected (see the include_str-stays-in-crate rule).
# Symlinks are ruled out by the repo's own `no_symlinks` dogfood rule. So the
# copies are committed -- and this harness asserts they stay byte-identical to the
# root originals (no drift) and that every published crate has them.
#
# The published-crate list is DERIVED from ci/scripts/publish-crates.sh, so a
# newly-added published crate that forgets its license copies fails here.

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

FILES=(LICENSE-APACHE LICENSE-MIT NOTICE)

# Parse the CRATES=( ... ) array out of publish-crates.sh (single source of truth).
mapfile -t CRATES < <(
  awk '/^CRATES=\(/ { f = 1; next } f && /^\)/ { f = 0 } f { gsub(/[ \t]/, ""); if ($0 != "") print }' \
    ci/scripts/publish-crates.sh
)

if [ "${#CRATES[@]}" -eq 0 ]; then
  echo "[crate-license] FAIL: parsed no crates from ci/scripts/publish-crates.sh CRATES=()" >&2
  exit 1
fi

rc=0
for crate in "${CRATES[@]}"; do
  for f in "${FILES[@]}"; do
    if [ ! -f "crates/${crate}/${f}" ]; then
      echo "[crate-license] FAIL: crates/${crate}/${f} is missing (published crate must ship it)" >&2
      echo "  -> cp ${f} crates/${crate}/${f}" >&2
      rc=1
    elif ! cmp -s "${f}" "crates/${crate}/${f}"; then
      echo "[crate-license] FAIL: crates/${crate}/${f} differs from the root ${f}" >&2
      echo "  -> re-copy: cp ${f} crates/${crate}/${f}" >&2
      rc=1
    fi
  done
done

if [ "$rc" -eq 0 ]; then
  echo "[crate-license] OK -- all ${#CRATES[@]} published crates carry root-identical ${FILES[*]}"
fi
exit "$rc"
