#!/usr/bin/env bash
# Check the commit subject a squash merge of a pull request writes on main: the
# title and " (#N)", or, for a pull request of one commit, that commit's
# subject and " (#N)" (the repository's squash setting is
# COMMIT_OR_PR_TITLE). It must be a Conventional Commits subject of at most 120
# characters, the rule issue-26-e2e.yml's range-mode job applies to a branch's
# own commits. A subject that fits there can still make one that does not once
# the merge adds its number. Run by pull-request-title.yml; pinned by
# test-squash-subject.sh.
#
#   squash-subject.sh TITLE NUMBER [COMMIT_SUBJECT]
set -euo pipefail
# Characters, not bytes, as the range-mode job counts them.
export LC_ALL=C.UTF-8

MAX=120
PATTERN='^(feat|fix|chore|docs|refactor|test|build|ci|perf|style|revert)(\(.+\))?!?: '

if [[ $# -lt 2 || $# -gt 3 || ! "$2" =~ ^[0-9]+$ ]]; then
  echo "usage: $0 TITLE NUMBER [COMMIT_SUBJECT]" >&2
  exit 2
fi
suffix=" (#$2)"
if [[ $# -eq 3 ]]; then
  # One commit: the merge writes its subject, whatever the title says.
  source="the commit's subject"
  fix="amend the commit's subject to at most"
  subject="$3$suffix"
else
  source="the title"
  fix="shorten the title to at most"
  subject="$1$suffix"
fi
status=0
echo "squash subject (${#subject} characters): $subject"
if ! grep -qE -- "$PATTERN" <<< "$subject"; then
  echo "error: $source does not start with a Conventional Commits type," \
    "such as \`fix(core): \`" >&2
  status=1
fi
if (( ${#subject} > MAX )); then
  echo "error: the subject is ${#subject} characters, over ${MAX};" \
    "$fix $((MAX - ${#suffix})) characters" >&2
  status=1
fi
exit "$status"
