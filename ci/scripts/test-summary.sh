#!/usr/bin/env bash
# Pin ci/scripts/summary.sh's verdict: a failed (or cancelled) `changes` job
# skips every downstream pipeline, and those skips must NOT be reported as
# "all checks passed". Also pin that ci.yml forwards the result of every job in
# the summary's `needs:` list and that summary.sh consumes each one.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

pass=0
fail=0

run_summary() {
  # Every per-job result defaults to "skipped"; the caller overrides some.
  env -i PATH="$PATH" GITHUB_STEP_SUMMARY=/dev/null \
    RUST_CHANGED=false DOCS_CHANGED=false BENCH_CHANGED=false \
    EXAMPLES_CHANGED=false EDITORS_CHANGED=false SUPPLY_CHAIN_CHANGED=false \
    PACKAGING_CHANGED=false SHELL_CHANGED=false \
    SECRETS_INVENTORY_RESULT=success FMT_RESULT=skipped MSRV_RESULT=skipped \
    CLIPPY_RESULT=skipped TEST_RESULT=skipped AUDIT_RESULT=skipped \
    DENY_RESULT=skipped SUPPLY_CHAIN_RESULT=skipped BUILD_RESULT=skipped \
    DOCS_JOB_RESULT=skipped DOGFOOD_RESULT=skipped BENCH_SMOKE_RESULT=skipped \
    EXAMPLES_RESULT=skipped SHELL_TESTS_RESULT=skipped EDITORS_RESULT=skipped \
    PACKAGING_RESULT=skipped \
    "$@" bash ci/scripts/summary.sh >/dev/null 2>&1
}

expect() {
  local name=$1 want=$2
  shift 2
  local got=0
  run_summary "$@" || got=$?
  if { [[ "$want" == pass ]] && [[ "$got" -eq 0 ]]; } ||
     { [[ "$want" == fail ]] && [[ "$got" -ne 0 ]]; }; then
    pass=$((pass + 1))
  else
    echo "  FAIL: $name (expected $want, exit $got)" >&2
    fail=$((fail + 1))
  fi
}

expect "changes ok, all skipped"     pass CHANGES_RESULT=success
expect "changes failed, all skipped" fail CHANGES_RESULT=failure
expect "changes cancelled"           fail CHANGES_RESULT=cancelled
expect "changes result missing"      fail
expect "a job failed"                fail CHANGES_RESULT=success DENY_RESULT=failure
expect "packaging failed"            fail CHANGES_RESULT=success PACKAGING_RESULT=failure
expect "shell-only route, harness failed" fail CHANGES_RESULT=success SHELL_CHANGED=true SHELL_TESTS_RESULT=failure

# A shell-only route must render as a real result, not "(no changes)".
row=$(env -i PATH="$PATH" CHANGES_RESULT=success RUST_CHANGED=false DOCS_CHANGED=false \
  BENCH_CHANGED=false EXAMPLES_CHANGED=false EDITORS_CHANGED=false \
  SUPPLY_CHAIN_CHANGED=false PACKAGING_CHANGED=false SHELL_CHANGED=true \
  SECRETS_INVENTORY_RESULT=success FMT_RESULT=skipped MSRV_RESULT=skipped \
  CLIPPY_RESULT=skipped TEST_RESULT=skipped AUDIT_RESULT=skipped DENY_RESULT=skipped \
  SUPPLY_CHAIN_RESULT=skipped BUILD_RESULT=skipped DOCS_JOB_RESULT=skipped \
  DOGFOOD_RESULT=skipped BENCH_SMOKE_RESULT=skipped EXAMPLES_RESULT=skipped \
  SHELL_TESTS_RESULT=success EDITORS_RESULT=skipped PACKAGING_RESULT=skipped \
  bash ci/scripts/summary.sh 2>/dev/null | grep '^| Shell tests ' || true)
if [[ "$row" == "| Shell tests | pass |" ]]; then
  pass=$((pass + 1))
else
  echo "  FAIL: shell-only route renders as '${row}', expected '| Shell tests | pass |'" >&2
  fail=$((fail + 1))
fi

python3 - <<'PY'
import re
import sys
from pathlib import Path

ci = Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
body = re.search(r'^  summary:\n(?P<body>.*)', ci, re.MULTILINE | re.DOTALL).group('body')
needs_block = re.search(r'^    needs:\n(?P<n>(?:      - .+\n)+)', body, re.MULTILINE).group('n')
jobs = re.findall(r'- ([A-Za-z0-9_-]+)', needs_block)
missing = [j for j in jobs if f'needs.{j}.result' not in body]
if missing:
    print(f'[test-summary] ci.yml summary does not forward the result of: {missing}', file=sys.stderr)
    sys.exit(1)
script = Path('ci/scripts/summary.sh').read_text(encoding='utf-8')
envs = re.findall(r'^\s+([A-Z_]+_RESULT): \$\{\{ needs\.', body, re.MULTILINE)
envs += re.findall(r'^\s+([A-Z_]+_CHANGED): \$\{\{ needs\.changes\.outputs\.', body, re.MULTILINE)
unused = [e for e in envs if f'${e}' not in script and '${' + e not in script]
# Every routing flag the changes job exports must reach the summary.
outputs = re.search(r'^  changes:\n(?:.*\n)*?    outputs:\n(?P<o>(?:      .+\n)+)', ci, re.MULTILINE).group('o')
flags = re.findall(r'^      ([a-z_]+): \$\{\{ steps\.detect\.outputs\.', outputs, re.MULTILINE)
forwarded = re.findall(r'needs\.changes\.outputs\.([a-z_]+)', body)
missing_flags = [f for f in flags if f not in forwarded]
if missing_flags:
    print(f'[test-summary] ci.yml summary does not forward routing flags: {missing_flags}', file=sys.stderr)
    sys.exit(1)
if unused:
    print(f'[test-summary] summary.sh ignores forwarded results: {unused}', file=sys.stderr)
    sys.exit(1)
PY

echo "[test-summary] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
