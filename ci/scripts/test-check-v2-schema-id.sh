#!/usr/bin/env bash
# ci/scripts/test-check-v2-schema-id.sh -- hermetic regression suite for
# ci/scripts/check-v2-schema-id.sh (#570). Auto-discovered by shell-tests.sh.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPT="$REPO_ROOT/ci/scripts/check-v2-schema-id.sh"

pass=0
fail=0
TMP_BASE=$(mktemp -d "${TMPDIR:-/tmp}/test-v2-schema-id.XXXXXX")
trap 'rm -rf "$TMP_BASE"' EXIT

make_repo() {
  local dir="$1"
  local rel="$2"
  local prop_val="$3"
  local id_rel="${4:-$rel}"
  local doc1_rel="${5:-$rel}"
  local doc2_rel="${6:-$rel}"
  mkdir -p "$dir/xtask/src" "$dir/schemas/v2" "$dir/docs/site/configuration" "$dir/docs/development"
  cat > "$dir/xtask/src/gen_schema_v2.rs" <<EOF
pub(crate) const RELEASE: &str = "${rel}";
EOF
  cat > "$dir/schemas/v2/config.json" <<EOF
{
  "\$id": "https://raw.githubusercontent.com/asamarts/alint/${id_rel}/schemas/v2/config.json",
  "title": "alint v2",
  "properties": { "example": { "const": "${prop_val}" } }
}
EOF
  cat > "$dir/docs/site/configuration/version-2.md" <<EOF
\`\`\`yaml
# yaml-language-server: \$schema=https://raw.githubusercontent.com/asamarts/alint/${doc1_rel}/schemas/v2/config.json
version: 2
\`\`\`
EOF
  cat > "$dir/docs/development/CONFIG-AUTHORING.md" <<EOF
\`\`\`yaml
# yaml-language-server: \$schema=https://raw.githubusercontent.com/asamarts/alint/${doc2_rel}/schemas/v2/config.json
version: 2
\`\`\`
EOF
  if [[ ! -d "$dir/.git" ]]; then
    git -C "$dir" init -q
    git -C "$dir" config user.name "test"
    git -C "$dir" config user.email "test@example.invalid"
  fi
  git -C "$dir" add -A
  git -C "$dir" commit -q -m "state ${rel} ${prop_val}" --allow-empty
}

expect_pass() {
  local name="$1"
  local dir="$2"
  shift 2
  if out=$(V2_SCHEMA_REPO_ROOT="$dir" bash "$SCRIPT" "$@" 2>&1); then
    echo "  PASS: $name"
    pass=$((pass + 1))
  else
    echo "  FAIL: $name (expected pass, got failure):" >&2
    echo "$out" >&2
    fail=$((fail + 1))
  fi
}

expect_fail() {
  local name="$1"
  local dir="$2"
  local needle="$3"
  shift 3
  if out=$(V2_SCHEMA_REPO_ROOT="$dir" bash "$SCRIPT" "$@" 2>&1); then
    echo "  FAIL: $name (expected failure, got pass): $out" >&2
    fail=$((fail + 1))
  elif [[ "$out" != *"$needle"* ]]; then
    echo "  FAIL: $name (missing '$needle' in output): $out" >&2
    fail=$((fail + 1))
  else
    echo "  PASS: $name"
    pass=$((pass + 1))
  fi
}

echo "[test-check-v2-schema-id] running hermetic tests..."

# 1. Dev mode passes when RELEASE, $id, and both docs modelines agree, even
#    when the schema body changed since the previous tag.
r1="$TMP_BASE/r1"
make_repo "$r1" "v0.23.0" "old"
git -C "$r1" tag "v0.23.0"
make_repo "$r1" "v0.23.0" "new-in-dev"
expect_pass "dev mode passes when consistent even if schema body changed" "$r1"
expect_pass "dev mode explicit --dev passes" "$r1" --dev

# 2. Dev mode fails when schemas/v2/config.json $id drifts from RELEASE.
r2="$TMP_BASE/r2"
make_repo "$r2" "v0.24.0" "new" "v0.23.0" "v0.24.0" "v0.24.0"
expect_fail "dev mode rejects schema \$id mismatch" "$r2" "expected"

# 3. Dev mode fails when a doc modeline drifts from RELEASE.
r3="$TMP_BASE/r3"
make_repo "$r3" "v0.24.0" "new" "v0.24.0" "v0.23.0" "v0.24.0"
expect_fail "dev mode rejects version-2.md modeline drift" "$r3" "version-2.md"

r3b="$TMP_BASE/r3b"
make_repo "$r3b" "v0.24.0" "new" "v0.24.0" "v0.24.0" "v0.23.0"
expect_fail "dev mode rejects CONFIG-AUTHORING.md modeline drift" "$r3b" "CONFIG-AUTHORING.md"

# 4. Release mode passes when schema body is unchanged since previous tag
#    (no spurious $id bump required).
r4="$TMP_BASE/r4"
make_repo "$r4" "v0.23.0" "same"
git -C "$r4" tag "v0.23.0"
expect_pass "release mode passes when schema unchanged since prior tag" "$r4" --release v0.24.0

# 5. Release mode fails when schema body changed since previous tag and
#    RELEASE was not bumped to the release tag.
r5="$TMP_BASE/r5"
make_repo "$r5" "v0.23.0" "old"
git -C "$r5" tag "v0.23.0"
make_repo "$r5" "v0.23.0" "changed"
expect_fail "release mode fails on unbumped schema change" "$r5" "changed since v0.23.0" --release v0.24.0

# 6. Tagged-HEAD self-comparison regression: when HEAD is already tagged
#    v0.24.0 (as in release.yml on tag push), --release v0.24.0 MUST still
#    compare against v0.23.0 and FAIL if RELEASE was left at v0.23.0!
git -C "$r5" tag "v0.24.0"
expect_fail "release mode excludes HEAD tag itself and compares against v0.23.0" "$r5" "changed since v0.23.0" --release v0.24.0

# 7. Release mode passes when schema body changed AND RELEASE/$id/modelines
#    were bumped to v0.24.0 (including when HEAD is tagged v0.24.0).
r7="$TMP_BASE/r7"
make_repo "$r7" "v0.23.0" "old"
git -C "$r7" tag "v0.23.0"
make_repo "$r7" "v0.24.0" "changed"
git -C "$r7" tag "v0.24.0"
expect_pass "release mode passes when schema changed and RELEASE bumped" "$r7" --release v0.24.0

# 8. Release mode rejects RELEASE ahead of the release tag.
r8="$TMP_BASE/r8"
make_repo "$r8" "v0.25.0" "same"
git -C "$r8" tag "v0.23.0"
expect_fail "release mode rejects RELEASE ahead of release tag" "$r8" "is ahead of" --release v0.24.0

# 9. Live repository passes dev mode right now.
expect_pass "live repo passes dev mode" "$REPO_ROOT"

echo "[test-check-v2-schema-id] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
