#!/usr/bin/env bash
# Regression tests for the release tag/workspace/Action version tripwire.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TMP_ROOT=$(mktemp -d)
trap 'rm -rf "$TMP_ROOT"' EXIT

git clone -q --no-local "$REPO_ROOT" "$TMP_ROOT/repo"
cp "$REPO_ROOT/ci/scripts/check-release-version.sh" "$TMP_ROOT/repo/ci/scripts/check-release-version.sh"
cp "$REPO_ROOT/ci/scripts/bump-version.sh" "$TMP_ROOT/repo/ci/scripts/bump-version.sh"
cp "$REPO_ROOT/action.yml" "$TMP_ROOT/repo/action.yml"
cd "$TMP_ROOT/repo"

pass=0
fail=0

expect_ok() {
  local name=$1
  shift
  if bash ci/scripts/check-release-version.sh "$@" >/dev/null 2>&1; then
    echo "  ok: $name"
    pass=$((pass + 1))
  else
    echo "  FAIL: $name (expected success)" >&2
    fail=$((fail + 1))
  fi
}

expect_rejected() {
  local name=$1
  shift
  if bash ci/scripts/check-release-version.sh "$@" >/dev/null 2>&1; then
    echo "  FAIL: $name (expected rejection)" >&2
    fail=$((fail + 1))
  else
    echo "  ok: $name"
    pass=$((pass + 1))
  fi
}

expect_ok "matching release contract" v0.17.0
expect_rejected "tag/workspace mismatch" v0.17.1
expect_rejected "malformed major-only tag" v0
expect_rejected "missing release tag"

sed -i 's/ALINT_BAKED_VERSION: v0\.17\.0/ALINT_BAKED_VERSION: v0.16.1/' action.yml
expect_rejected "stale Action baked version" v0.17.0
cp "$REPO_ROOT/action.yml" action.yml

sed -i '/ALINT_BAKED_VERSION:/d' action.yml
expect_rejected "missing Action baked version" v0.17.0
cp "$REPO_ROOT/action.yml" action.yml

# Prove the normal release-preparation command updates both sides of the
# contract. Stub only cargo metadata: dependency availability is unrelated to
# this shell test, while the real bump script still performs every file edit.
mkdir -p "$TMP_ROOT/fake-bin"
printf '#!/usr/bin/env bash\nexit 0\n' > "$TMP_ROOT/fake-bin/cargo"
chmod +x "$TMP_ROOT/fake-bin/cargo"
PATH="$TMP_ROOT/fake-bin:$PATH" bash ci/scripts/bump-version.sh 0.17.1 >/dev/null
expect_ok "bump-version updates workspace and Action together" v0.17.1

echo "[test-release-version] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
