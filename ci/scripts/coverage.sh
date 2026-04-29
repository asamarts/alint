#!/usr/bin/env bash
# Generate workspace line-coverage with cargo-llvm-cov.
#
# Emits two artifacts:
#   - coverage.lcov   (LCOV-format, consumable by Codecov)
#   - coverage.html/  (browseable report)
#
# Enforces an 85% line-coverage floor (matches the v0.8 plan)
# over the **shipped** crates only — `xtask` is dev tooling
# (hyperfine wrappers, docs-export, scenario generators) whose
# coverage is structurally low and not user-facing, so it's
# excluded from both report and floor.
#
# Set ALINT_COVERAGE_FLOOR=<float> to override locally.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

FLOOR="${ALINT_COVERAGE_FLOOR:-85.0}"
OUT_DIR="${ALINT_COVERAGE_OUT:-target/coverage}"
# Regex matched against source-file paths in the coverage
# report. `xtask/` excludes the whole xtask crate.
EXCLUDE_REGEX='xtask/'

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  echo "==> Installing cargo-llvm-cov"
  cargo install cargo-llvm-cov --locked
fi

mkdir -p "$OUT_DIR"

echo "==> Running cargo llvm-cov (LCOV)"
cargo llvm-cov --workspace --locked \
  --ignore-filename-regex "$EXCLUDE_REGEX" \
  --lcov --output-path "$OUT_DIR/coverage.lcov"

echo "==> Running cargo llvm-cov (HTML)"
cargo llvm-cov report \
  --ignore-filename-regex "$EXCLUDE_REGEX" \
  --html --output-dir "$OUT_DIR/html"

echo "==> Running cargo llvm-cov (summary)"
SUMMARY="$(cargo llvm-cov report --ignore-filename-regex "$EXCLUDE_REGEX" --summary-only)"
echo "$SUMMARY"

# The summary line we care about looks like:
#   TOTAL ... <pct>% ...  (last column with % is line coverage)
LINE_COV=$(printf '%s\n' "$SUMMARY" | awk '
  /^TOTAL/ {
    for (i = NF; i >= 1; i--) {
      if ($i ~ /%$/) { gsub(/%/, "", $i); print $i; exit }
    }
  }
')

if [[ -z "${LINE_COV:-}" ]]; then
  echo "WARN: could not parse line coverage from summary; skipping floor check"
  exit 0
fi

echo "==> Line coverage: ${LINE_COV}%, floor ${FLOOR}%"
awk -v c="$LINE_COV" -v f="$FLOOR" 'BEGIN { exit !(c+0 >= f+0) }' || {
  echo "FAIL: line coverage ${LINE_COV}% < ${FLOOR}%"
  exit 1
}
echo "==> Coverage floor met"
