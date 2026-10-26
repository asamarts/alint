#!/usr/bin/env bash
# R4/V21-8: the test-only `shadow-ir` feature must not reach a release or bench
# build. `release-binary.sh` builds `-p alint`; bench-record builds
# `-p alint -p xtask`, and its criterion build is `-p alint-bench`. Cargo
# feature unification means checking `alint-rules` alone is not enough: the
# selected package set is what can accidentally enable it.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)" || exit 1

status=0
check() { # $1 = label, rest = cargo tree package args
  local label=$1
  shift
  local tree
  if ! tree=$(cargo tree -e features --locked "$@" 2>&1); then
    echo "::error title=shadow-ir feature tree::could not inspect ${label}" >&2
    printf '%s\n' "$tree" >&2
    exit 1
  fi
  if grep -q 'alint-rules feature "shadow-ir"' <<<"$tree"; then
    echo "::error title=shadow-ir feature tree::${label} enables shadow-ir" >&2
    grep 'alint-rules feature "shadow-ir"' <<<"$tree" >&2 || true
    status=1
  else
    echo "shadow-ir absent from ${label}"
  fi
}

# The release binary and the bench-record invocation.
check "release build (-p alint)" -p alint
check "bench-record build (-p alint -p xtask)" -p alint -p xtask
check "criterion bench build (-p alint-bench)" -p alint-bench

# Positive control: the e2e dev-dependency is the one place it must be on.
if ! cargo tree -e features --locked -p alint-e2e 2>/dev/null \
  | grep -q 'alint-rules feature "shadow-ir"'; then
  echo "::error title=shadow-ir feature tree::alint-e2e does not enable shadow-ir" >&2
  exit 1
fi
echo "shadow-ir present in alint-e2e (the test harness)"

exit "$status"
