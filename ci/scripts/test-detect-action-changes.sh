#!/usr/bin/env bash
# Ensure action-only changes reach the jobs that cover SHA resolution and pins.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TMP_ROOT=$(mktemp -d)
trap 'rm -rf "$TMP_ROOT"' EXIT

git clone -q --no-local "$REPO_ROOT" "$TMP_ROOT/repo"
cp "$REPO_ROOT/ci/scripts/detect-changes.sh" "$TMP_ROOT/repo/ci/scripts/detect-changes.sh"
cd "$TMP_ROOT/repo"
git config user.name 'alint tests'
git config user.email 'tests@alint.invalid'

pass=0
fail=0

expect_action_routes() {
  local name=$1
  local path=$2
  local base output
  base=$(git rev-parse HEAD)
  printf '\n# detect-changes fixture\n' >> "$path"
  git add "$path"
  git commit -q -m "fixture: change $path"
  output=$(GH_EVENT=push PUSH_BEFORE_SHA="$base" bash ci/scripts/detect-changes.sh)
  if grep -q 'rust=true' <<< "$output" && grep -q 'docs=true' <<< "$output"; then
    echo "  ok: $name"
    pass=$((pass + 1))
  else
    echo "  FAIL: $name (expected rust=true and docs=true)" >&2
    echo "$output" >&2
    fail=$((fail + 1))
  fi
}

expect_action_routes "action.yml reaches shell and docs tests" action.yml
expect_action_routes "action helper reaches shell and docs tests" action/resolve-version.sh

echo "[test-detect-action-changes] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
