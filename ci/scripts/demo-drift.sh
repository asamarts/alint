#!/usr/bin/env bash
# demo-drift.sh — keep README's demo GIF honest.
#
# The GIF (demo/alint.gif) is generated from demo/alint.tape, which runs a fixed
# command sequence against demo/fixture. If a bundled rule changes, a fixer is
# added, or output shape shifts, the GIF silently becomes a LIE on our primary
# discovery surface, and nothing else in CI would notice.
#
# This gate replays the tape's exact sequence and pins alint's real behaviour.
# When it fails, the fix is usually: re-record the GIF (`demo/render.sh`)
# and update the expectations below in the same commit.
#
# Deliberately a TEXT-level gate, not a byte-diff of the GIF: GIF encoding is not
# reproducible across vhs/ttyd/ffmpeg versions, so a byte-diff would flake
# constantly and teach everyone to ignore it. What actually matters is that the
# demo still tells the truth about what alint does.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ALINT="${ALINT_BIN:-$REPO_ROOT/target/release/alint}"
[ -x "$ALINT" ] || { echo "demo-drift: no alint binary at $ALINT (build it first)"; exit 1; }

# --- expectations (keep in lockstep with demo/alint.tape + the recorded GIF) ---
EXPECT_ERRORS=3          # tracked target/ (x2 rules) + .DS_Store
EXPECT_FIXABLE=4         # final newline, trailing ws (x2), .DS_Store removal
EXPECT_APPLIED=4
EXPECT_ERRORS_AFTER=2    # the tracked target/ pair; a human decision, not a rewrite

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cp -r "$REPO_ROOT/demo/fixture/." "$work/"

# The tape synthesizes these two at record time rather than committing them
# (a tracked target/ and a .DS_Store in git would be gross, and would trip
# alint's own dogfood). Mirror that setup exactly. The .DS_Store must carry the
# real "Bud1" magic (00 00 00 01 42 75 64 31): hygiene-no-macos-junk is
# content-verified, so a bare-newline stub is correctly NOT flagged and the demo
# would silently lose its .DS_Store finding (the 3 -> 2 drift this gate caught).
mkdir -p "$work/target/debug" && printf 'binary\n' > "$work/target/debug/acme"
printf '\000\000\000\001Bud1' > "$work/.DS_Store"
git -C "$work" init -q .
git -C "$work" add -A

cd "$work"

fail() { echo "DEMO DRIFT: $1"; echo; echo "The README GIF now misrepresents alint."; \
         echo "Re-record it (demo/render.sh) and update ci/scripts/demo-drift.sh."; exit 1; }

# --- 1. check: the state the GIF opens on -------------------------------------
before="$("$ALINT" check --no-docs --color never 2>&1 || true)"
errors=$(printf '%s' "$before"  | grep -oE '[0-9]+ errors?'        | head -1 | grep -oE '[0-9]+' || echo 0)
fixable=$(printf '%s' "$before" | grep -oE '[0-9]+ auto-fixable'   | head -1 | grep -oE '[0-9]+' || echo 0)
[ "$errors" = "$EXPECT_ERRORS" ]   || fail "check reports $errors errors, GIF shows $EXPECT_ERRORS"
[ "$fixable" = "$EXPECT_FIXABLE" ] || fail "check reports $fixable auto-fixable, GIF shows $EXPECT_FIXABLE"

# --- 2. fix: the payoff frame --------------------------------------------------
fixed="$("$ALINT" fix --color never 2>&1 || true)"
applied=$(printf '%s' "$fixed" | grep -oE '[0-9]+ applied' | head -1 | grep -oE '[0-9]+' || echo 0)
[ "$applied" = "$EXPECT_APPLIED" ] || fail "fix applied $applied, GIF shows $EXPECT_APPLIED"

# --- 3. check again: what a human still has to decide --------------------------
after="$("$ALINT" check --no-docs --color never 2>&1 || true)"
errors_after=$(printf '%s' "$after" | grep -oE '[0-9]+ errors?' | head -1 | grep -oE '[0-9]+' || echo 0)
[ "$errors_after" = "$EXPECT_ERRORS_AFTER" ] \
  || fail "post-fix check reports $errors_after errors, GIF shows $EXPECT_ERRORS_AFTER"

echo "demo-drift: OK (${EXPECT_ERRORS} errors -> fix ${EXPECT_APPLIED} -> ${EXPECT_ERRORS_AFTER} remain); GIF still tells the truth"
