#!/usr/bin/env bash
# R4/V21-8: the test-only `shadow-ir` feature must not reach a shipped or
# benchmarked build. Cargo unifies features across the packages, features,
# targets, and dependency kinds one build selects, so each check names a
# build's exact packages and features, resolves every target (`--target all`),
# and includes dev-dependencies only where that build compiles them (`cargo
# bench`). `cargo tree -i alint-rules -f '{p} [{f}]'` prints the features
# alint-rules resolves to under those choices; grepping the `-e features` tree
# for one edge instead missed a leak through the root package's own features.
#
# SHADOW_TREE_ROOT (default: this repository) names the workspace to inspect;
# test-check-shadow-feature-tree.sh points it at copies with a leak injected.
set -euo pipefail
cd "${SHADOW_TREE_ROOT:-$(git rev-parse --show-toplevel)}" || exit 1

# resolved <label> <cargo tree args...>: sets $features to alint-rules'
# resolved feature lists, one line per resolution (comma-separated).
resolved() {
  local label=$1 tree
  shift
  if ! tree=$(cargo tree --locked -i alint-rules --target all --depth 0 \
    -f '{p} [{f}]' "$@" 2>&1); then
    echo "::error title=shadow-ir feature tree::could not inspect ${label}" >&2
    printf '%s\n' "$tree" >&2
    exit 1
  fi
  if ! grep -q '^alint-rules v' <<<"$tree"; then
    echo "::error title=shadow-ir feature tree::alint-rules is not in ${label}" >&2
    printf '%s\n' "$tree" >&2
    exit 1
  fi
  features=$(sed -n 's/^alint-rules v.*\[\([^]]*\)\]$/\1/p' <<<"$tree")
}
has_shadow() { grep -qE '(^|,)shadow-ir(,|$)' <<<"$features"; }

status=0
check() { # $1 = label, rest = cargo tree args
  local label=$1
  shift
  resolved "$label" "$@"
  if has_shadow; then
    echo "::error title=shadow-ir feature tree::${label} enables shadow-ir" >&2
    printf 'alint-rules features: [%s]\n' "$features" >&2
    status=1
  else
    echo "shadow-ir absent from ${label}"
  fi
}

# The release binary (release-binary.sh builds `-p alint` per target) and
# `cargo install alint`.
check "the release build (-p alint)" -p alint -e normal,build
# bench-record's binaries.
check "the bench-record build (-p alint -p xtask)" -p alint -p xtask -e normal,build
# criterion and gungraun benches compile alint-bench's dev-dependencies, with
# `fs-benches` (micro) or `det-100k` (release-time det_check); all features
# covers every combination.
check "the bench build (-p alint-bench, all features)" \
  -p alint-bench --all-features -e normal,build,dev

# Positive control: the e2e dev-dependency is the one place it must be on, so
# the checks above would see the feature if it were there.
resolved "alint-e2e (the test harness)" -p alint-e2e -e normal,build,dev
if ! has_shadow; then
  echo "::error title=shadow-ir feature tree::alint-e2e does not enable shadow-ir" >&2
  exit 1
fi
echo "shadow-ir present in alint-e2e (the test harness)"

exit "$status"
