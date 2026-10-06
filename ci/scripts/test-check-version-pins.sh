#!/usr/bin/env bash
# Regression tests for the version/Action documentation pin gate. The test uses
# a disposable local clone so each negative case can corrupt a file without
# touching the caller's worktree.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TMP_ROOT=$(mktemp -d)
trap 'rm -rf "$TMP_ROOT"' EXIT

git clone -q --no-local "$REPO_ROOT" "$TMP_ROOT/repo"
cd "$TMP_ROOT/repo"

pass=0
fail=0

expect_ok() {
  local name=$1
  if bash ci/scripts/check-version-pins.sh >/dev/null 2>&1; then
    echo "  ok: $name"
    pass=$((pass + 1))
  else
    echo "  FAIL: $name (expected success)" >&2
    fail=$((fail + 1))
  fi
}

expect_rejected() {
  local name=$1
  if bash ci/scripts/check-version-pins.sh >/dev/null 2>&1; then
    echo "  FAIL: $name (expected rejection)" >&2
    fail=$((fail + 1))
  else
    echo "  ok: $name"
    pass=$((pass + 1))
  fi
}

restore() {
  git checkout -q -- ci/action-doc-pin.env docs/rules.md docs/site/integrations/github-actions.md
}

expect_ok "canonical pins"

sed -i '0,/asamarts\/alint@[0-9a-f]\{40\}/s//asamarts\/alint@0000000000000000000000000000000000000000/' \
  docs/site/integrations/github-actions.md
expect_rejected "unresolvable documented SHA"
restore

sed -i 's/^ACTION_DOC_SHA=.*/ACTION_DOC_SHA=0000000000000000000000000000000000000000/' \
  ci/action-doc-pin.env
expect_rejected "metadata SHA differs from release tag"
restore

sed -i '0,/^[[:space:]]*version: v0\.17\.0$/s//    path: ./' docs/site/integrations/github-actions.md
expect_rejected "legacy Action snippet missing explicit binary version"
restore

sed -i 's/^ACTION_DOC_REQUIRES_EXPLICIT_VERSION=.*/ACTION_DOC_REQUIRES_EXPLICIT_VERSION=false/' \
  ci/action-doc-pin.env
expect_rejected "metadata disagrees with tagged action default"
restore

echo "[test-check-version-pins] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
