#!/usr/bin/env bash
# Fail closed unless a release tag, Cargo workspace version, and the binary
# version baked into action.yml all agree. release.yml runs this before any
# publishing job; it is also covered by test-release-version.sh.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

release_tag="${1:-${GITHUB_REF_NAME:-}}"
if [[ ! "$release_tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "[release-version] expected an exact vX.Y.Z release tag, got '${release_tag:-<empty>}'" >&2
  exit 2
fi

workspace_version=$(awk -F'"' '
  /^\[workspace\.package\]/ { in_workspace=1; next }
  in_workspace && /^version =/ { print $2; exit }
' Cargo.toml)
baked_version=$(awk '
  /^[[:space:]]*ALINT_BAKED_VERSION:/ {
    value=$0
    sub(/^[[:space:]]*ALINT_BAKED_VERSION:[[:space:]]*/, "", value)
    gsub(/"/, "", value)
    print value
    exit
  }
' action.yml)

failed=0
if [[ "v${workspace_version:-}" != "$release_tag" ]]; then
  echo "[release-version] $release_tag != Cargo.toml workspace version v${workspace_version:-<unreadable>}" >&2
  failed=1
fi
if [[ "$baked_version" != "$release_tag" ]]; then
  echo "[release-version] $release_tag != action.yml baked version ${baked_version:-<missing>}" >&2
  failed=1
fi
if [[ "$failed" -ne 0 ]]; then
  echo "[release-version] run ci/scripts/bump-version.sh before tagging the release commit" >&2
  exit 1
fi

echo "[release-version] OK — tag, workspace, and Action binary are $release_tag"
