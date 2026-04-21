#!/usr/bin/env bash
set -euo pipefail

# Publish every member crate in the workspace that isn't marked
# `publish = false`, in dependency order, after a `cargo publish`
# for each waits for crates.io's index to see the new version so
# the next crate in the chain can resolve it.
#
# Usage:
#   ci/scripts/publish-crates.sh             # publish whatever's missing
#   ci/scripts/publish-crates.sh --dry-run   # preview: package + verify-compile
#                                              the foundation crate; package-only
#                                              for downstream crates (they can't
#                                              verify against a not-yet-published
#                                              new version of alint-core).
#
# Preconditions:
#   - Run from a clean checkout of a tagged commit (script will warn
#     otherwise but does not refuse — some patch flows want to publish
#     from a clean-but-untagged `main`).
#   - `CARGO_REGISTRY_TOKEN` set in the environment, or a prior
#     `cargo login` stashed a credential.
#
# The dependency order is hand-maintained here because the workspace
# graph is small and stable; `cargo publish -p` surfaces graph breaks
# loudly if someone reorders without updating this list.

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

DRY_RUN=false
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN=true
fi

# Dependency-ordered list. alint-core is foundational; dsl/rules/output
# each depend only on core; alint (the CLI binary) depends on all four.
CRATES=(
  alint-core
  alint-dsl
  alint-rules
  alint-output
  alint
)

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if [[ -z "$VERSION" ]]; then
  echo "==> ERROR: could not read workspace version from Cargo.toml" >&2
  exit 1
fi

echo "==> Publishing alint workspace crates at version v${VERSION}"
if $DRY_RUN; then
  echo "    (dry-run mode: cargo publish --dry-run, no uploads)"
fi
echo

if [[ -n "$(git status --porcelain 2>/dev/null)" ]]; then
  echo "==> WARNING: working tree is dirty — the published artefacts"
  echo "    will reflect the uncommitted state. Ctrl-C to abort."
  echo
fi

if ! git describe --exact-match --tags HEAD >/dev/null 2>&1; then
  echo "==> WARNING: HEAD is not on a tag. Release tarballs typically"
  echo "    come from a tagged commit; continuing anyway."
  echo
fi

# crates.io lookup. Returns 0 if the exact version is already on the
# index, 1 otherwise. Uses the public v1 API; polite User-Agent so
# rate-limit logs point back at us rather than a generic curl.
crate_version_exists() {
  local crate="$1" version="$2"
  curl -sS -o /dev/null -w '%{http_code}' \
    -A "alint-publish-script/${VERSION} (https://github.com/asamarts/alint)" \
    "https://crates.io/api/v1/crates/${crate}/${version}" \
    | grep -q '^200$'
}

wait_for_index() {
  local crate="$1" version="$2"
  local deadline=$((SECONDS + 300))   # 5 min cap
  local interval=5
  echo "    polling crates.io for ${crate}@${version}…"
  while (( SECONDS < deadline )); do
    if crate_version_exists "$crate" "$version"; then
      echo "    ${crate}@${version} visible on crates.io"
      return 0
    fi
    sleep "$interval"
  done
  echo "==> ERROR: ${crate}@${version} not visible on crates.io after 5 min" >&2
  echo "    This usually means the publish succeeded but index propagation is slow." >&2
  echo "    Re-run the script; already-published crates are skipped." >&2
  return 1
}

for crate in "${CRATES[@]}"; do
  echo "─── ${crate} ────────────────────────────────────────────"
  if crate_version_exists "$crate" "$VERSION"; then
    echo "    ${crate}@${VERSION} already published; skipping"
    echo
    continue
  fi

  if $DRY_RUN; then
    # Only the foundation crate can be fully dry-run:
    # `cargo publish --dry-run` resolves dependencies against
    # crates.io, which can't satisfy a fresh bump of alint-core
    # until the real publish runs. For downstream crates we
    # just announce what *would* happen so the operator can
    # eyeball the order.
    if [[ "$crate" == "${CRATES[0]}" ]]; then
      cargo publish -p "$crate" --dry-run
    else
      echo "    would run: cargo publish -p ${crate}"
      echo "    (skipped — dry-run can't verify downstream crates"
      echo "     against a not-yet-published ${CRATES[0]}@${VERSION})"
    fi
    echo
    continue
  fi

  cargo publish -p "$crate"

  # Skip the index-wait for the last crate — nothing after it needs
  # to resolve it, and waiting there is just dead time for the user.
  if [[ "$crate" != "${CRATES[-1]}" ]]; then
    wait_for_index "$crate" "$VERSION"
  fi
  echo
done

echo "==> Done. Published version: v${VERSION}"
