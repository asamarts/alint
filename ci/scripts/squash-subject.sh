#!/usr/bin/env bash
# Check the commit subject a squash merge of a pull request writes on main: the
# title and " (#N)". It must be a Conventional Commits subject of at most 120
# characters, the rule issue-26-e2e.yml's range-mode job applies to a branch's
# own commits. A title that fits there can still make a subject that does not
# once the merge adds its number. Run by pull-request-title.yml; pinned by
# test-squash-subject.sh.
#
#   squash-subject.sh TITLE NUMBER
set -euo pipefail
# Characters, not bytes, as the range-mode job counts them.
export LC_ALL=C.UTF-8

MAX=120
PATTERN='^(feat|fix|chore|docs|refactor|test|build|ci|perf|style|revert)(\(.+\))?!?: '

if [[ $# -ne 2 || ! "$2" =~ ^[0-9]+$ ]]; then
  echo "usage: $0 TITLE NUMBER" >&2
  exit 2
fi
subject="$1 (#$2)"
suffix=" (#$2)"
status=0
echo "squash subject (${#subject} characters): $subject"
if ! grep -qE -- "$PATTERN" <<< "$subject"; then
  echo "error: the title does not start with a Conventional Commits type," \
    "such as \`fix(core): \`" >&2
  status=1
fi
if (( ${#subject} > MAX )); then
  echo "error: the subject is ${#subject} characters, over ${MAX};" \
    "shorten the title to at most $((MAX - ${#suffix}))" >&2
  status=1
fi
exit "$status"
