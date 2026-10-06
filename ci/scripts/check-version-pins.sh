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
#   - action.yml (the baked GitHub Action binary version)
#   - SECURITY.md
#   - docs/site/integrations/{docker,pre-commit}.md
#   - docs/site/getting-started/installation.md
#
# GitHub Action snippets are release-tag SHA pins and are checked separately
# against ci/action-doc-pin.env. They cannot follow the workspace version in a
# pre-tag version-bump commit because a commit cannot contain its own SHA.
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
#   bash ci/scripts/check-version-pins.sh --verify-action-tag
#
# The default mode is checkout-local and works in shallow consumer clones (the
# script is also run by alint's own command rule). The opt-in tag check proves
# the published documentation SHA and baked binary version against Git history;
# callers using it must fetch tags first.
#
# Fix path: bash ci/scripts/bump-version.sh <new-version>

set -euo pipefail

VERIFY_ACTION_TAG=false
case "${1:-}" in
  "") ;;
  --verify-action-tag) VERIFY_ACTION_TAG=true ;;
  *)
    echo "usage: $0 [--verify-action-tag]" >&2
    exit 2
    ;;
esac
if [[ "$#" -gt 1 ]]; then
  echo "usage: $0 [--verify-action-tag]" >&2
  exit 2
fi

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
  action.yml
  README.md
  SECURITY.md
  docs/site/integrations/docker.md
  docs/site/integrations/pre-commit.md
  docs/site/getting-started/installation.md
)

# npm/package.json is checked separately below — its version field
# is JSON-shaped, not a vX.Y.Z / :X.Y.Z pin embedded in prose, so the
# regex used for the SCOPE files doesn't fit.

# Match any vX.Y.Z or :X.Y.Z (bare-semver-after-colon, e.g. a
# docker tag without the `v` prefix) in scope. In the GitHub Actions
# guide, inspect only alint's own `uses:` lines: versions in comments
# for actions/checkout and github/codeql-action belong to those actions,
# not alint. Exclude anything that matches the workspace version exactly;
# what's left is drift.
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
  pins=$(grep -nE "$PIN_REGEX" "$f" || true)
  drift=$(printf '%s\n' "$pins" | grep -vE "(v|:)${WS_ESCAPED}([^0-9.]|$)" || true)
  if [[ -n "$drift" ]]; then
    echo "[version-pin] $f: stale pin (workspace is $WORKSPACE_VER)" >&2
    printf '    %s\n' "${drift//$'\n'/$'\n    '}" >&2
    failed=1
  fi
done

# GitHub Action documentation has a separate post-release pin. Every checkout,
# including shallow consumer clones, validates its snippets against the
# canonical metadata. The Docs job additionally passes --verify-action-tag with
# full history to prove that the SHA is the declared tag's immutable commit and
# that the tag contains the expected baked binary version.
ACTION_PIN_FILE=ci/action-doc-pin.env
if [[ ! -f "$ACTION_PIN_FILE" ]]; then
  echo "[version-pin] $ACTION_PIN_FILE: NOT FOUND" >&2
  failed=1
else
  # This is a repository-owned, fixed-shape data file.
  # shellcheck disable=SC1090
  source "$ACTION_PIN_FILE"
  if [[ ! "${ACTION_DOC_VERSION:-}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] ||
     [[ ! "${ACTION_DOC_SHA:-}" =~ ^[0-9a-f]{40}$ ]] ||
     [[ ! "${ACTION_DOC_REQUIRES_EXPLICIT_VERSION:-}" =~ ^(true|false)$ ]]; then
    echo "[version-pin] $ACTION_PIN_FILE: malformed action pin metadata" >&2
    failed=1
  else
    if [[ "$VERIFY_ACTION_TAG" == true ]]; then
      tag="v${ACTION_DOC_VERSION}"
      tag_sha=$(git rev-parse --verify "${tag}^{commit}" 2>/dev/null || true)
      if [[ -z "$tag_sha" ]]; then
        echo "[version-pin] $ACTION_PIN_FILE: tag $tag is unavailable; fetch tags before running --verify-action-tag" >&2
        failed=1
      elif [[ "$ACTION_DOC_SHA" != "$tag_sha" ]]; then
        echo "[version-pin] $ACTION_PIN_FILE: SHA $ACTION_DOC_SHA != $tag commit $tag_sha" >&2
        failed=1
      fi
      tagged_baked=$(git show "${tag}:action.yml" 2>/dev/null |
        awk '
          /^[[:space:]]*ALINT_BAKED_VERSION:/ {
          value=$0
          sub(/^[[:space:]]*ALINT_BAKED_VERSION:[[:space:]]*/, "", value)
          gsub(/"/, "", value)
          print value
          exit
        }
        ' || true)
      expected_explicit=true
      if [[ "$tagged_baked" == "v${ACTION_DOC_VERSION}" ]]; then
        expected_explicit=false
      fi
      if [[ "$ACTION_DOC_REQUIRES_EXPLICIT_VERSION" != "$expected_explicit" ]]; then
        echo "[version-pin] $ACTION_PIN_FILE: requires-explicit-version=$ACTION_DOC_REQUIRES_EXPLICIT_VERSION, but $tag action.yml bakes ${tagged_baked:-<nothing>}" >&2
        failed=1
      fi
    fi

    # Keep the two canonical locations required even if their examples are
    # accidentally removed, then discover every additional copy-paste snippet
    # in the user-facing README/docs tree. Historical design/development notes
    # live outside this surface and intentionally retain their original refs.
    action_doc_files=(docs/site/integrations/github-actions.md docs/rules.md)
    while IFS= read -r -d '' f; do
      case "$f" in
        docs/site/integrations/github-actions.md|docs/rules.md) continue ;;
      esac
      if grep -qE 'uses:[[:space:]]*asamarts/alint@' "$f"; then
        action_doc_files+=("$f")
      fi
    done < <(find README.md docs/site -type f \( -name '*.md' -o -name '*.mdx' \) -print0)
    uses_count=0
    explicit_count=0
    for f in "${action_doc_files[@]}"; do
      if [[ ! -f "$f" ]]; then
        echo "[version-pin] $f: NOT FOUND (Action-doc file missing)" >&2
        failed=1
        continue
      fi
      file_uses=$(grep -cE 'uses:[[:space:]]*asamarts/alint@[0-9a-f]{40}[[:space:]]+# v[0-9]+\.[0-9]+\.[0-9]+' "$f" || true)
      uses_count=$((uses_count + file_uses))
      mismatched=$(grep -nE 'uses:[[:space:]]*asamarts/alint@' "$f" |
        grep -vF "asamarts/alint@${ACTION_DOC_SHA} # v${ACTION_DOC_VERSION}" || true)
      if [[ -n "$mismatched" ]]; then
        echo "[version-pin] $f: Action pin differs from $ACTION_PIN_FILE" >&2
        printf '    %s\n' "${mismatched//$'\n'/$'\n    '}" >&2
        failed=1
      fi
      file_explicit=$(grep -cE "^[[:space:]]+version:[[:space:]]+v${ACTION_DOC_VERSION//./\\.}([[:space:]#]|$)" "$f" || true)
      explicit_count=$((explicit_count + file_explicit))
    done
    if [[ "$uses_count" -eq 0 ]]; then
      echo "[version-pin] no documented asamarts/alint Action pins found" >&2
      failed=1
    elif [[ "$ACTION_DOC_REQUIRES_EXPLICIT_VERSION" == true && "$explicit_count" -ne "$uses_count" ]]; then
      echo "[version-pin] v${ACTION_DOC_VERSION} requires one explicit version input per Action snippet ($explicit_count for $uses_count)" >&2
      failed=1
    elif [[ "$ACTION_DOC_REQUIRES_EXPLICIT_VERSION" == false && "$explicit_count" -ne 0 ]]; then
      echo "[version-pin] v${ACTION_DOC_VERSION} bakes its binary version; remove $explicit_count redundant version input(s)" >&2
      failed=1
    fi
  fi
fi

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

if [[ "$VERIFY_ACTION_TAG" == true ]]; then
  action_check="the published Action tag/SHA"
else
  action_check="the documented Action pins"
fi
echo "[version-pin] OK — ${#SCOPE[@]} workspace-version files, $action_check, npm/package.json, and Zed manifests are consistent"
