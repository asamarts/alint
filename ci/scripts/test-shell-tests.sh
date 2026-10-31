#!/usr/bin/env bash
# Self-test for shell-tests.sh: a pass counts as passed, an exit 77 from a
# harness in SKIPPABLE as skipped, and an exit 77 from any other harness, or
# any other non-zero status, as a failure.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TMP_ROOT=$(mktemp -d)
trap 'rm -rf "$TMP_ROOT"' EXIT

failures=0
fail() {
  echo "FAIL: $*" >&2
  failures=$((failures + 1))
}

# harness <dir> <name> <exit status>
harness() {
  mkdir -p "$1"
  printf '#!/usr/bin/env bash\nexit %s\n' "$3" > "$1/$2"
  chmod +x "$1/$2"
}

# run <dir> <skippable>: sets $rc and $out.
run() {
  set +e
  out=$(SHELL_TESTS_DIR="$1" SHELL_TESTS_SKIPPABLE="$2" bash "$REPO_ROOT/ci/scripts/shell-tests.sh" 2>&1)
  rc=$?
  set -e
}

# expect <name> <dir> <skippable> <rc> <text expected in the output>
expect() {
  run "$2" "$3"
  if [[ $rc -ne $4 ]]; then
    fail "$1: rc=$rc, expected $4:"
    printf '%s\n' "$out" >&2
  elif ! grep -qF -- "$5" <<<"$out"; then
    fail "$1: output lacks \"$5\":"
    printf '%s\n' "$out" >&2
  else
    echo "ok: $1"
  fi
}

ok="$TMP_ROOT/ok"
harness "$ok" test-pass.sh 0
harness "$ok" test-skips.sh 77
expect "a listed harness that exits 77 is skipped" "$ok" "test-skips.sh" 0 \
  "1 harness(es) passed, 1 skipped"

unlisted="$TMP_ROOT/unlisted"
harness "$unlisted" test-pass.sh 0
harness "$unlisted" test-skips.sh 77
expect "an unlisted harness that exits 77 fails" "$unlisted" "test-other.sh" 1 \
  "test-skips.sh exited 77 but is not in SKIPPABLE"

failing="$TMP_ROOT/failing"
harness "$failing" test-pass.sh 0
harness "$failing" test-fails.sh 3
expect "a harness that exits 3 fails" "$failing" "" 1 "1 of 2 harnesses failed"

if [[ $failures -ne 0 ]]; then
  echo "[shell-tests self-test] $failures case(s) failed" >&2
  exit 1
fi
echo "[shell-tests self-test] OK"
