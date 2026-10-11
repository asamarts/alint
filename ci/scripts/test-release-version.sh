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

# Derive the release under test from the workspace, so the test does not break
# on every version bump: CUR is the current version, NEXT a patch bump of it,
# and PREV a version that is guaranteed to differ for the stale-baked case.
CUR=$(sed -n 's/^version = "\([0-9][0-9.]*\)"$/\1/p' Cargo.toml | head -n1)
[[ -n "$CUR" ]] || { echo "could not read workspace version" >&2; exit 1; }
IFS=. read -r cur_major cur_minor cur_patch <<<"$CUR"
NEXT="$cur_major.$cur_minor.$((cur_patch + 1))"
PREV="0.0.1"
[[ "$CUR" != "$PREV" ]] || PREV="0.0.2"
CUR_RE=${CUR//./\\.}

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

expect_ok "matching release contract" "v$CUR"
expect_rejected "tag/workspace mismatch" "v$NEXT"
expect_rejected "malformed major-only tag" v0
expect_rejected "missing release tag"

sed -i "s/ALINT_BAKED_VERSION: v$CUR_RE/ALINT_BAKED_VERSION: v$PREV/" action.yml
expect_rejected "stale Action baked version" "v$CUR"
cp "$REPO_ROOT/action.yml" action.yml

sed -i '/ALINT_BAKED_VERSION:/d' action.yml
expect_rejected "missing Action baked version" "v$CUR"
cp "$REPO_ROOT/action.yml" action.yml

sed -i "0,/^[[:space:]]*default: \"\"\$/s//    default: \"v$CUR\"/" action.yml
expect_rejected "non-empty public Action version default" "v$CUR"
cp "$REPO_ROOT/action.yml" action.yml

sed -i '0,/^[[:space:]]*default: ""$/d' action.yml
expect_rejected "missing public Action version default" "v$CUR"
cp "$REPO_ROOT/action.yml" action.yml

# Prove the normal release-preparation command updates both sides of the
# contract. Stub only cargo metadata: dependency availability is unrelated to
# this shell test, while the real bump script still performs every file edit.
mkdir -p "$TMP_ROOT/fake-bin"
printf '#!/usr/bin/env bash\nexit 0\n' > "$TMP_ROOT/fake-bin/cargo"
chmod +x "$TMP_ROOT/fake-bin/cargo"
PATH="$TMP_ROOT/fake-bin:$PATH" bash ci/scripts/bump-version.sh "$NEXT" >/dev/null
expect_ok "bump-version updates workspace and Action together" "v$NEXT"

echo "[test-release-version] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
