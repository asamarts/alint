#!/usr/bin/env bash
# Regression tests for the GitHub Action's binary-version precedence contract.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

# shellcheck source=action/resolve-version.sh
source action/resolve-version.sh

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

pass=0
fail=0

expect_version() {
  local name=$1
  local expected=$2
  shift 2
  local actual
  if actual=$(resolve_alint_action_version "$@"); then
    if [[ "$actual" == "$expected" ]]; then
      echo "  ok: $name"
      pass=$((pass + 1))
      return
    fi
    echo "  FAIL: $name (expected '$expected', got '$actual')" >&2
  else
    echo "  FAIL: $name (resolver rejected valid inputs)" >&2
  fi
  fail=$((fail + 1))
}

expect_rejected() {
  local name=$1
  shift
  if resolve_alint_action_version "$@" >/dev/null 2>&1; then
    echo "  FAIL: $name (expected rejection)" >&2
    fail=$((fail + 1))
  else
    echo "  ok: $name"
    pass=$((pass + 1))
  fi
}

if [[ "$baked_version" == "v${workspace_version}" ]]; then
  echo "  ok: action.yml bakes the workspace release"
  pass=$((pass + 1))
else
  echo "  FAIL: action.yml bakes '${baked_version:-<empty>}'; workspace is v${workspace_version}" >&2
  fail=$((fail + 1))
fi

sha=0123456789abcdef0123456789abcdef01234567
expect_version "SHA pin selects baked release" "$baked_version" "" "$sha" "$baked_version"
expect_version "exact tag selects itself" v9.8.7 "" v9.8.7 "$baked_version"
expect_version "main branch follows latest" latest "" main "$baked_version"
expect_version "major channel follows latest" latest "" v0 "$baked_version"
expect_version "local action follows latest" latest "" "" "$baked_version"
expect_version "explicit input overrides SHA" v1.2.3 v1.2.3 "$sha" "$baked_version"
expect_version "explicit input overrides exact tag" v1.2.3 v1.2.3 v9.8.7 "$baked_version"
expect_version "explicit latest overrides SHA" latest latest "$sha" "$baked_version"
expect_rejected "SHA pin rejects a missing baked release" "" "$sha" ""
expect_rejected "SHA pin rejects a malformed baked release" "" "$sha" next

# The literal expression is the contract being checked, not a shell expansion.
# shellcheck disable=SC2016
if grep -Fq 'source "$ACTION_PATH/action/resolve-version.sh"' action.yml; then
  echo "  ok: action.yml invokes the tested resolver"
  pass=$((pass + 1))
else
  echo "  FAIL: action.yml does not invoke the tested resolver" >&2
  fail=$((fail + 1))
fi

echo "[test-action-version] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
