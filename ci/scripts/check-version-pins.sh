#!/usr/bin/env bash
# Verify install-snippet version pins match Cargo.toml's
# `[workspace.package].version`.
#
# The workspace version is the source of truth. Every user-facing
# install snippet (README, SECURITY, docs/site/integrations,
# docs/site/getting-started) must pin to the same version. The
# release pipeline can drift if we forget to sweep these by hand;
# this script + the matching alint rule (`install-snippets-match
# -workspace-version` in .alint.yml) close the loop.
#
# Files in scope (must pin to the workspace version):
#   - README.md
#   - SECURITY.md
#   - docs/site/integrations/{docker,github-actions,pre-commit}.md
#   - docs/site/getting-started/installation.md
#
# Files deliberately excluded (intentional historical refs):
#   - CHANGELOG.md (per-version release entries)
#   - docs/benchmarks/HISTORY.md (per-version perf rows)
#   - docs/design/** (historical architecture / scope_filter
#     introduction markers)
#   - examples/*/README.md (case-study captures at a fixed SHA +
#     alint version)
#
# Exit codes:
#   0  all in-scope files pin to the workspace version
#   1  drift detected (stale pin somewhere)
#   2  could not read workspace version (broken Cargo.toml)
#
# Usage:
#   bash ci/scripts/check-version-pins.sh
#
# Fix path: bash ci/scripts/bump-version.sh <new-version>

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

WORKSPACE_VER=$(awk -F'"' '
  /^\[workspace\.package\]/ { f=1 }
  f && /^version =/ { print $2; exit }
' Cargo.toml)

if [[ -z "${WORKSPACE_VER:-}" ]]; then
  echo "[version-pin] could not read [workspace.package].version from Cargo.toml" >&2
  exit 2
fi

SCOPE=(
  README.md
  SECURITY.md
  docs/site/integrations/docker.md
  docs/site/integrations/github-actions.md
  docs/site/integrations/pre-commit.md
  docs/site/getting-started/installation.md
)

# npm/package.json is checked separately below — its version field
# is JSON-shaped, not a vX.Y.Z / :X.Y.Z pin embedded in prose, so the
# regex used for the SCOPE files doesn't fit.

# Match any vX.Y.Z or :X.Y.Z (bare-semver-after-colon, e.g. a
# docker tag without the `v` prefix) in scope. Exclude anything
# that matches the workspace version exactly — what's left is
# drift.
#
# We deliberately do NOT match bare `X.Y.Z` (no `v` and no `:`
# anchor) because the integration docs sometimes mention
# minor/major channels in prose ("the `:0.9` channel") that we
# don't want to police.
PIN_REGEX='(v|:)[0-9]+\.[0-9]+\.[0-9]+'
# Escape dots for the exclude regex so e.g. "v0.9.20" doesn't
# match a hypothetical "v0a9b20".
WS_ESCAPED="${WORKSPACE_VER//./\\.}"

failed=0
for f in "${SCOPE[@]}"; do
  if [[ ! -f "$f" ]]; then
    echo "[version-pin] $f: NOT FOUND (in-scope file missing)" >&2
    failed=1
    continue
  fi
  drift=$(grep -nE "$PIN_REGEX" "$f" | grep -vE "(v|:)${WS_ESCAPED}([^0-9.]|$)" || true)
  if [[ -n "$drift" ]]; then
    echo "[version-pin] $f: stale pin (workspace is $WORKSPACE_VER)" >&2
    echo "$drift" | sed 's/^/    /' >&2
    failed=1
  fi
done

if [[ -f npm/package.json ]]; then
  NPM_VER=$(awk -F'"' '/^[[:space:]]*"version":/ { print $4; exit }' npm/package.json)
  if [[ -z "$NPM_VER" ]]; then
    echo "[version-pin] npm/package.json: could not parse version field" >&2
    failed=1
  elif [[ "$NPM_VER" != "$WORKSPACE_VER" ]]; then
    echo "[version-pin] npm/package.json: version $NPM_VER != workspace $WORKSPACE_VER" >&2
    failed=1
  fi
fi

# Zed extension manifests: the registry builds from source at the committed
# version (no publish-time stamp), so the committed value ships and must match
# the workspace. (VS Code/JetBrains are version-stamped at publish, so their
# committed versions are intentionally allowed to lag and are NOT gated here.)
for zf in editors/zed/extension.toml editors/zed/Cargo.toml; do
  if [[ -f "$zf" ]]; then
    ZED_VER=$(awk -F'"' '/^version = / { print $2; exit }' "$zf")
    if [[ "$ZED_VER" != "$WORKSPACE_VER" ]]; then
      echo "[version-pin] $zf: version $ZED_VER != workspace $WORKSPACE_VER" >&2
      failed=1
    fi
  fi
done

if [[ "$failed" -ne 0 ]]; then
  echo "" >&2
  echo "Fix: after a version bump, run 'bash ci/scripts/bump-version.sh <new-version>'." >&2
  echo "     If the workspace is already at $WORKSPACE_VER, hand-edit the flagged file(s)" >&2
  echo "     to match (bump-version.sh no-ops when current == target)." >&2
  exit 1
fi

echo "[version-pin] OK — all ${#SCOPE[@]} install-snippet files + npm/package.json + Zed manifests pin to $WORKSPACE_VER"
