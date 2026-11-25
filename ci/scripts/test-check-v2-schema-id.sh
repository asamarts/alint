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

# 9. First v2 release (prior tag exists WITHOUT schemas/v2/config.json):
#    passes when RELEASE == tag, fails when mismatched.
r9="$TMP_BASE/r9"
mkdir -p "$r9"
git -C "$r9" init -q
git -C "$r9" config user.name "test"
git -C "$r9" config user.email "test@example.invalid"
echo "v1" > "$r9/README.md"
git -C "$r9" add -A && git -C "$r9" commit -q -m "pre-v2"
git -C "$r9" tag "v0.22.0"
make_repo "$r9" "v0.23.0" "first"
expect_pass "first v2 release passes when RELEASE matches tag" "$r9" --release v0.23.0
expect_fail "first v2 release fails when RELEASE differs from tag" "$r9" "no prior v*.*.* tag contains" --release v0.24.0

# 10. Prerelease tags in git history are ignored when finding prev_tag, and
#     --release rejects prerelease tags.
r10="$TMP_BASE/r10"
make_repo "$r10" "v0.23.0" "old"
git -C "$r10" tag "v0.23.0"
make_repo "$r10" "v0.23.0" "changed"
git -C "$r10" tag "v0.24.0-rc.1"
expect_fail "release mode ignores prerelease tag and compares against v0.23.0" "$r10" "changed since v0.23.0" --release v0.24.0
expect_fail "release mode rejects prerelease --release argument" "$r10" "invalid --release tag" --release v0.24.0-rc.1

# 11. Backport release (--release v0.23.1 when v0.24.0 tag exists) compares
#     strictly against v0.23.0 (< v0.23.1).
r11="$TMP_BASE/r11"
make_repo "$r11" "v0.23.0" "base"
git -C "$r11" tag "v0.23.0"
make_repo "$r11" "v0.24.0" "future"
git -C "$r11" tag "v0.24.0"
make_repo "$r11" "v0.23.0" "base"
expect_pass "backport release compares against v0.23.0 ignoring newer v0.24.0 tag" "$r11" --release v0.23.1

# 12. Type-faithful JSON diff: changing integer 1 to boolean true MUST be
#     detected as a schema change (Python bool == int trap).
r12="$TMP_BASE/r12"
make_repo "$r12" "v0.23.0" "x"
cat > "$r12/schemas/v2/config.json" <<'EOF'
{
  "$id": "https://raw.githubusercontent.com/asamarts/alint/v0.23.0/schemas/v2/config.json",
  "properties": { "flag": { "default": 1 } }
}
EOF
git -C "$r12" add -A && git -C "$r12" commit -q -m "int default"
git -C "$r12" tag "v0.23.0"
cat > "$r12/schemas/v2/config.json" <<'EOF'
{
  "$id": "https://raw.githubusercontent.com/asamarts/alint/v0.23.0/schemas/v2/config.json",
  "properties": { "flag": { "default": true } }
}
EOF
expect_fail "release mode detects JSON 1 vs true type change" "$r12" "changed since v0.23.0" --release v0.24.0

# 13. Modeline strictness: indented stale modeline, trailing comment, and
#     `main`-pointing modeline are all caught.
r13="$TMP_BASE/r13"
make_repo "$r13" "v0.23.0" "x"
cat >> "$r13/docs/site/configuration/version-2.md" <<'EOF'
  # yaml-language-server: $schema=https://raw.githubusercontent.com/asamarts/alint/main/schemas/v2/config.json
EOF
expect_fail "dev mode catches indented main-pointing second modeline" "$r13" "main/schemas/v2/config.json"

r13b="$TMP_BASE/r13b"
make_repo "$r13b" "v0.23.0" "x"
cat > "$r13b/docs/development/CONFIG-AUTHORING.md" <<'EOF'
# yaml-language-server: $schema=https://raw.githubusercontent.com/asamarts/alint/v0.23.0/schemas/v2/config.json # trailing
EOF
expect_fail "dev mode rejects malformed trailing-comment modeline" "$r13b" "malformed v2 schema modeline"

# 14. Non-object JSON and conflicting CLI mode flags are rejected cleanly.
r14="$TMP_BASE/r14"
make_repo "$r14" "v0.23.0" "x"
echo '[]' > "$r14/schemas/v2/config.json"
expect_fail "dev mode rejects non-object schema JSON cleanly" "$r14" "must be an object"
expect_fail "conflicting --release and --dev flags are rejected" "$r1" "conflicting or duplicate mode flag" --release v0.24.0 --dev

# 15. Live repository passes dev mode right now.
expect_pass "live repo passes dev mode" "$REPO_ROOT"

echo "[test-check-v2-schema-id] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
