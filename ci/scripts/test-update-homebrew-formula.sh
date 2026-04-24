#!/usr/bin/env bash
set -euo pipefail

# Tests for `ci/scripts/update-homebrew-formula.sh`. Each case
# writes a fixture SHA256SUMS, invokes the script, and checks
# the output for the expected substrings.
#
# Run directly:  ci/scripts/test-update-homebrew-formula.sh
# Or via CI:     .github/workflows/ci.yml invokes this.
#
# The harness uses `diff` / grep rather than snapshot files on
# disk so that test expectations live next to the case they
# cover — easier to read than pairs of .in / .stdout fixtures.

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPT="${REPO_ROOT}/ci/scripts/update-homebrew-formula.sh"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fail_count=0
pass_count=0

pass()  { printf '  PASS  %s\n' "$1"; pass_count=$((pass_count + 1)); }
fail()  { printf '  FAIL  %s\n' "$1"; fail_count=$((fail_count + 1)); }

# ---- case: happy path ------------------------------------------------

cat > "$tmp/sums-happy" <<'EOF'
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  alint-v0.4.7-aarch64-apple-darwin.tar.gz
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  alint-v0.4.7-aarch64-unknown-linux-musl.tar.gz
cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc  alint-v0.4.7-x86_64-apple-darwin.tar.gz
dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd *alint-v0.4.7-x86_64-pc-windows-msvc.tar.gz
eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee  alint-v0.4.7-x86_64-unknown-linux-musl.tar.gz
EOF

out="$(VERSION=v0.4.7 SHA256SUMS="$tmp/sums-happy" "$SCRIPT")"

# Version stripped of `v` in the bare `version "..."` declaration.
grep -qE '^[[:space:]]*version "0\.4\.7"$' <<<"$out" \
  && pass "version field is the bare number" \
  || fail "version field should strip the v prefix"

# URL retains the `v` prefix (git-tag form).
grep -q 'releases/download/v0\.4\.7/alint-v0\.4\.7-' <<<"$out" \
  && pass "URLs reference the v-prefixed git tag" \
  || fail "URLs should use the v-prefixed tag"

# Each platform shows up with its expected SHA.
grep -q 'sha256 "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"' <<<"$out" \
  && pass "darwin-arm64 SHA wired" \
  || fail "darwin-arm64 SHA missing"
grep -q 'sha256 "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"' <<<"$out" \
  && pass "linux-arm64 SHA wired" \
  || fail "linux-arm64 SHA missing"
grep -q 'sha256 "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"' <<<"$out" \
  && pass "darwin-x86_64 SHA wired" \
  || fail "darwin-x86_64 SHA missing"
grep -q 'sha256 "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"' <<<"$out" \
  && pass "linux-x86_64 SHA wired" \
  || fail "linux-x86_64 SHA missing"

# Windows SHA isn't needed (Homebrew doesn't ship Windows), so
# it shouldn't leak into the formula.
grep -q 'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd' <<<"$out" \
  && fail "windows SHA should not appear in the Homebrew formula" \
  || pass "windows SHA correctly absent"

# Dual-license metadata preserved.
grep -q 'license any_of: \["Apache-2.0", "MIT"\]' <<<"$out" \
  && pass "dual-license declared" \
  || fail "license line missing"

# Test block present (Homebrew requires it).
grep -q 'alint --version' <<<"$out" \
  && pass "test block invokes --version" \
  || fail "test block missing"

# ---- case: windows-style sha (leading asterisk) handled -----------------

# Same as happy path but every line has the asterisk form.
cat > "$tmp/sums-asterisk" <<'EOF'
1111111111111111111111111111111111111111111111111111111111111111 *alint-v0.4.7-aarch64-apple-darwin.tar.gz
2222222222222222222222222222222222222222222222222222222222222222 *alint-v0.4.7-aarch64-unknown-linux-musl.tar.gz
3333333333333333333333333333333333333333333333333333333333333333 *alint-v0.4.7-x86_64-apple-darwin.tar.gz
4444444444444444444444444444444444444444444444444444444444444444 *alint-v0.4.7-x86_64-unknown-linux-musl.tar.gz
EOF

out_ast="$(VERSION=v0.4.7 SHA256SUMS="$tmp/sums-asterisk" "$SCRIPT")"
if grep -q 'sha256 "1111111111111111111111111111111111111111111111111111111111111111"' <<<"$out_ast" \
   && grep -q 'sha256 "4444444444444444444444444444444444444444444444444444444444444444"' <<<"$out_ast"; then
  pass "windows-style asterisk prefix is stripped"
else
  fail "asterisk-prefixed sums were not parsed"
fi

# ---- case: missing target errors out ------------------------------

cat > "$tmp/sums-incomplete" <<'EOF'
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  alint-v0.4.7-aarch64-apple-darwin.tar.gz
EOF

if VERSION=v0.4.7 SHA256SUMS="$tmp/sums-incomplete" "$SCRIPT" >/dev/null 2>"$tmp/err"; then
  fail "script should fail when a platform's SHA is missing"
else
  if grep -q "no SHA-256" "$tmp/err"; then
    pass "script errors clearly when a platform is missing"
  else
    fail "missing-platform error should mention 'no SHA-256'; got: $(cat "$tmp/err")"
  fi
fi

# ---- case: missing env vars fail fast -----------------------------

if SHA256SUMS=/dev/null "$SCRIPT" >/dev/null 2>"$tmp/err"; then
  fail "script should reject missing VERSION"
else
  grep -q 'VERSION' "$tmp/err" \
    && pass "missing VERSION fails fast with a clear error" \
    || fail "missing-VERSION error should mention VERSION; got: $(cat "$tmp/err")"
fi

if VERSION=v0.4.7 "$SCRIPT" >/dev/null 2>"$tmp/err"; then
  fail "script should reject missing SHA256SUMS"
else
  grep -q 'SHA256SUMS' "$tmp/err" \
    && pass "missing SHA256SUMS fails fast with a clear error" \
    || fail "missing-SHA256SUMS error should mention SHA256SUMS; got: $(cat "$tmp/err")"
fi

# ---- case: missing SHA file path errors out -----------------------

if VERSION=v0.4.7 SHA256SUMS=/nonexistent/path "$SCRIPT" >/dev/null 2>"$tmp/err"; then
  fail "script should reject a non-existent SHA256SUMS path"
else
  grep -q 'not found' "$tmp/err" \
    && pass "non-existent SHA256SUMS path errors cleanly" \
    || fail "non-existent-path error should mention 'not found'; got: $(cat "$tmp/err")"
fi

# ---- summary ------------------------------------------------------

echo
echo "Results: ${pass_count} passed, ${fail_count} failed."
if [[ $fail_count -gt 0 ]]; then
  exit 1
fi
