#!/usr/bin/env bash
# Regression tests for the release tag/workspace/Action version tripwire.
set -euo pipefail
# check-release-version.sh falls back to GITHUB_REF_NAME when no tag is passed.
# On a tag-triggered run (the release preflight) that is the release tag, which
# would make the "missing release tag" case pass; every case passes its tag
# explicitly, so clear the fallback.
unset GITHUB_REF_NAME

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TMP_ROOT=$(mktemp -d)
trap 'rm -rf "$TMP_ROOT"' EXIT

git clone -q --no-local "$REPO_ROOT" "$TMP_ROOT/repo"
# The release-preparation command and this test run before the bump is
# committed, while `git clone` sees HEAD only. Overlay every contract/input the
# test exercises so the fixture represents the actual working tree.
cp "$REPO_ROOT/Cargo.toml" "$TMP_ROOT/repo/Cargo.toml"
cp "$REPO_ROOT/ci/scripts/check-release-version.sh" "$TMP_ROOT/repo/ci/scripts/check-release-version.sh"
cp "$REPO_ROOT/ci/scripts/bump-version.sh" "$TMP_ROOT/repo/ci/scripts/bump-version.sh"
cp "$REPO_ROOT/action.yml" "$TMP_ROOT/repo/action.yml"
cd "$TMP_ROOT/repo"

# Derive releases under test from the workspace, so the test does not break on
# every version bump: NEXT is a patch bump, NEXT_MINOR crosses the pre-1.0 Cargo
# compatibility boundary, and PREV differs for the stale-baked case.
CUR=$(sed -n 's/^version = "\([0-9][0-9.]*\)"$/\1/p' Cargo.toml | head -n1)
[[ -n "$CUR" ]] || { echo "could not read workspace version" >&2; exit 1; }
IFS=. read -r cur_major cur_minor cur_patch <<<"$CUR"
NEXT="$cur_major.$cur_minor.$((cur_patch + 1))"
NEXT_MINOR="$cur_major.$((cur_minor + 1)).0"
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
# contract. Stub Cargo resolution and generation: dependency availability is
# unrelated to this shell test, while the real bump script still performs every
# file edit and the fake records which generated contracts it requested.
mkdir -p "$TMP_ROOT/fake-bin"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  '[[ -n "${CARGO_CALL_LOG:-}" ]] && printf "%s\n" "$*" >> "$CARGO_CALL_LOG"' \
  'exit 0' > "$TMP_ROOT/fake-bin/cargo"
chmod +x "$TMP_ROOT/fake-bin/cargo"

# A pre-1.0 minor release must advance every internal dependency floor before
# `cargo metadata`: Cargo's caret semantics reject a 0.18 requirement paired
# with a local 0.19 crate even when their APIs remain compatible.
CARGO_CALL_LOG="$TMP_ROOT/cargo-calls" PATH="$TMP_ROOT/fake-bin:$PATH" \
  bash ci/scripts/bump-version.sh "$NEXT_MINOR" >/dev/null
expected_floor="${cur_major}.$((cur_minor + 1)).0"
floor_count=$(grep -Ec "^alint-[a-z-]+ = \\{ path = \"[^\"]+\", version = \"${expected_floor//./\\.}\" \\}$" Cargo.toml)
if [[ "$floor_count" -eq 7 ]]; then
  echo "  ok: minor bump advances every internal dependency compatibility floor"
  pass=$((pass + 1))
else
  echo "  FAIL: minor bump advanced $floor_count of 7 internal dependency floors" >&2
  fail=$((fail + 1))
fi
if grep -Fxq 'run -q -p xtask -- gen-facts' "$TMP_ROOT/cargo-calls"; then
  echo "  ok: bump-version refreshes the generated facts contract"
  pass=$((pass + 1))
else
  echo "  FAIL: bump-version did not request facts regeneration" >&2
  fail=$((fail + 1))
fi

git reset --hard -q HEAD
overlay=(Cargo.toml action.yml ci/scripts/bump-version.sh ci/scripts/check-release-version.sh)
for file in "${overlay[@]}"; do
  cp "$REPO_ROOT/$file" "$file"
done
# Stage the overlay, so `git diff` below lists only what the bump edits, not an
# uncommitted change to one of these files in the working tree under test.
git add -- "${overlay[@]}"
PATH="$TMP_ROOT/fake-bin:$PATH" bash ci/scripts/bump-version.sh "$NEXT" >/dev/null
expect_ok "bump-version updates workspace and Action together" "v$NEXT"

zed_lock=$(awk -F'"' '/^name = "alint-zed"$/ { want = 1; next } want && /^version = / { print $2; exit }' \
  editors/zed/Cargo.lock)
if [[ "$zed_lock" == "$NEXT" ]]; then
  echo "  ok: bump-version refreshes the Zed lockfile's alint-zed entry"
  pass=$((pass + 1))
else
  echo "  FAIL: the Zed lockfile's alint-zed entry is ${zed_lock:-missing}, not $NEXT" >&2
  fail=$((fail + 1))
fi

# Every file the bump edits must be in RELEASING.md's `git add` list, or the
# release commit made by following it leaves that edit behind (the Zed lockfile
# did, until 2026-11-04). Cargo is stubbed, so Cargo.lock and facts.json do not
# change here; they are listed anyway.
listed=$(awk '/^ *git add / { on = 1 }
  on {
    more = /\\$/
    gsub(/\\/, "")
    for (i = 1; i <= NF; i++) if ($i != "git" && $i != "add") print $i
    if (!more) exit
  }' "$REPO_ROOT/RELEASING.md" | LC_ALL=C sort -u)
unlisted=$(git diff --name-only | LC_ALL=C sort -u | LC_ALL=C comm -23 - <(printf '%s\n' "$listed"))
if [[ -n "$listed" && -z "$unlisted" ]]; then
  echo "  ok: RELEASING.md's git add list covers every file bump-version edits"
  pass=$((pass + 1))
else
  echo "  FAIL: bump-version edits files RELEASING.md's git add list leaves out: ${unlisted:-<could not read the list>}" >&2
  fail=$((fail + 1))
fi

echo "[test-release-version] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
