#!/usr/bin/env bash
# Run every shell-script test harness under ci/scripts/. Each
# harness is a `test-*.sh` that exits 0 on success and any other
# status on failure. A harness listed in SKIPPABLE may instead exit
# 77 when it cannot run here (a missing tool); that is reported as
# skipped, apart from passes. From any other harness 77 is a
# failure, since a tool can exit 77 for its own reasons (curl does
# for a CA-certificate problem). New harnesses just need to follow
# the `test-*.sh` naming convention to be picked up here.
#
# SHELL_TESTS_DIR and SHELL_TESTS_SKIPPABLE override the harness
# directory and the skippable list, for test-shell-tests.sh.
#
# Split out from `ci/scripts/test.sh` so a shell-test failure
# (e.g. a homebrew-formula golden drift) doesn't fail the Rust
# `Test` CI job, which otherwise looks like a Rust regression in
# the workflow summary.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

HARNESS_DIR="${SHELL_TESTS_DIR:-ci/scripts}"
# Harnesses that skip themselves without their tool (by file name).
SKIPPABLE="${SHELL_TESTS_SKIPPABLE-test-witforge-consumer.sh}"

failed=0
checked=0
skipped=()
for harness in "$HARNESS_DIR"/test-*.sh; do
  [[ -f "$harness" ]] || continue
  echo "==> Running $harness"
  status=0
  "$harness" || status=$?
  if [[ "$status" -eq 77 && " $SKIPPABLE " != *" $(basename "$harness") "* ]]; then
    echo "[shell-tests] $harness exited 77 but is not in SKIPPABLE, so that is a failure" >&2
    status=1
  fi
  case "$status" in
    0) checked=$((checked + 1)) ;;
    77) skipped+=("$harness") ;;
    *)
      echo "[shell-tests] FAILED: $harness" >&2
      failed=$((failed + 1))
      ;;
  esac
done

total=$((checked + failed + ${#skipped[@]}))
if [[ "$failed" -ne 0 ]]; then
  echo "[shell-tests] $failed of $total harnesses failed" >&2
  exit 1
fi

if [[ "$total" -eq 0 ]]; then
  echo "[shell-tests] no test-*.sh harnesses found under $HARNESS_DIR/" >&2
  exit 0
fi

if [[ ${#skipped[@]} -ne 0 ]]; then
  echo "[shell-tests] skipped (not run here): ${skipped[*]}"
fi
echo "[shell-tests] OK — $checked harness(es) passed, ${#skipped[@]} skipped"
