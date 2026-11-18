#!/usr/bin/env bash
# Pin ci/scripts/squash-subject.sh and its workflow:
#   - a conventional title whose squash subject (title and " (#N)") fits in 120
#     characters passes, at exactly 120 too; one character more fails and names
#     the longest title that fits;
#   - the title that put a 126-character subject on main (#563) fails;
#   - a title without a Conventional Commits type fails;
#   - characters count, not bytes;
#   - a missing or non-numeric number is a usage error;
#   - for a pull request of one commit, the commit's subject is checked and the
#     title is not (#491's long title made a short subject; a long commit
#     subject under a short title would land);
#   - the pattern is the range-mode job's, and pull-request-title.yml runs on
#     title edits and pushes, passes the title through the environment and
#     the head commit's subject when the pull request has one commit.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CHECK="$REPO_ROOT/ci/scripts/squash-subject.sh"
WORKFLOW="$REPO_ROOT/.github/workflows/pull-request-title.yml"
RANGE_WORKFLOW="$REPO_ROOT/.github/workflows/issue-26-e2e.yml"
sandbox="$(mktemp -d)"
trap 'rm -rf -- "$sandbox"' EXIT
pass=0
fail=0
ok() { echo "  ok: $1"; pass=$((pass + 1)); }
bad() { echo "  FAIL: $1" >&2; sed 's/^/      | /' "$sandbox/out" >&2; fail=$((fail + 1)); }

expect() {
  local name="$1" want="$2" pattern="$3"
  shift 3
  local status=0
  "$@" > "$sandbox/out" 2>&1 || status=$?
  if [[ "$status" -ne "$want" ]]; then
    bad "$name: expected exit $want, got $status"
  elif [[ -n "$pattern" ]] && ! grep -qF -- "$pattern" "$sandbox/out"; then
    bad "$name: output lacks '$pattern'"
  else
    ok "$name"
  fi
}

# A conventional title of `length` characters.
title() { printf 'fix: %s' "$(head -c "$(($1 - 5))" /dev/zero | tr '\0' x)"; }

expect "a short conventional title passes" 0 "squash subject (17 characters)" \
  "$CHECK" "fix: a thing" 7
expect "a subject of exactly 120 characters passes" 0 "(120 characters)" \
  "$CHECK" "$(title 113)" 563
expect "121 characters fail, naming the longest title that fits" 1 \
  "shorten the title to at most 113 characters" "$CHECK" "$(title 114)" 563
title_563="fix(core): bound entry extraction and evaluator reads, keep non-string reads as notes,"
title_563+=" and redact what may quote values"
expect "#563's title fails" 1 "the subject is 126 characters" "$CHECK" "$title_563" 563
expect "a title without a type fails" 1 "Conventional Commits type" \
  "$CHECK" "Update the README" 12
expect "a scoped breaking title passes" 0 "" \
  "$CHECK" "feat(dsl)!: drop version 0" 12
expect "characters count, not bytes, whatever the caller's locale" 0 "(120 characters)" \
  env LC_ALL=C "$CHECK" "fix: $(printf 'é%.0s' $(seq 1 108))" 563
expect "a missing number is a usage error" 2 "usage" "$CHECK" "fix: a thing"
expect "a non-numeric number is a usage error" 2 "usage" "$CHECK" "fix: a thing" "5; true"
expect "a fourth argument is a usage error" 2 "usage" "$CHECK" "fix: a" 1 "fix: b" "fix: c"
title_491="chore(deps): bump org.jetbrains.intellij.platform from 2.16.0 to 2.17.0 in /editors/jetbrains"
title_491+=" in the jetbrains-gradle group"
expect "one commit: a long title does not matter, the commit's subject does" 0 \
  "(56 characters)" "$CHECK" "$title_491" 491 "chore(deps): bump org.jetbrains.intellij.platform"
expect "one commit: a long commit subject under a short title fails" 1 \
  "amend the commit's subject to at most 113 characters" \
  "$CHECK" "fix: short" 563 "$(title 114)"
expect "one commit: the commit's subject needs a type" 1 "the commit's subject does not" \
  "$CHECK" "fix: short" 12 "Update the README"

# The same pattern as the range-mode job's rule, so a branch's commits and the
# squash subject are held to one rule.
range_pattern="$(sed -n "s/^ *pattern: '\(\^(feat.*\)'$/\1/p" "$RANGE_WORKFLOW" | sort -u)"
check_pattern="$(sed -n "s/^PATTERN='\(.*\)'$/\1/p" "$CHECK")"
echo "$range_pattern / $check_pattern" > "$sandbox/out"
if [[ -n "$check_pattern" && "$range_pattern" == "$check_pattern" ]]; then
  ok "the pattern is the range-mode job's"
else
  bad "the pattern is the range-mode job's"
fi

# Runs on title edits and pushes, with the title in the environment (never in
# the script), and passes a one-commit pull request's head subject.
grep -n 'types:\|PR_\|squash-subject.sh\|fetch-depth' "$WORKFLOW" > "$sandbox/out"
# shellcheck disable=SC2016  # the `$` in these patterns is literal
if grep -qE '^ +types: \[[^]]*\bedited\b' "$WORKFLOW" \
  && grep -qE '^ +types: \[[^]]*\bsynchronize\b' "$WORKFLOW" \
  && grep -qE '^ +PR_TITLE: \$\{\{ github\.event\.pull_request\.title \}\}$' "$WORKFLOW" \
  && grep -qE '^ +PR_COMMITS: \$\{\{ github\.event\.pull_request\.commits \}\}$' "$WORKFLOW" \
  && grep -qE '^ +PR_HEAD: \$\{\{ github\.event\.pull_request\.head\.sha \}\}$' "$WORKFLOW" \
  && grep -qE '^ +fetch-depth: 2$' "$WORKFLOW" \
  && grep -qE '^ +if \[\[ "\$PR_COMMITS" == 1 \]\]; then$' "$WORKFLOW" \
  && grep -qE '^ +"\$\(git log -1 --format=%B "\$PR_HEAD" \| head -n 1\)"$' "$WORKFLOW" \
  && grep -qE '^ +ci/scripts/squash-subject\.sh "\$PR_TITLE" "\$PR_NUMBER"$' "$WORKFLOW"; then
  ok "the workflow passes the title, and a one-commit pull request's head subject"
else
  bad "the workflow passes the title, and a one-commit pull request's head subject"
fi

echo "[test-squash-subject] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
