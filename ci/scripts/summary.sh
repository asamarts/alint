#!/usr/bin/env bash
set -euo pipefail

# Generate a CI summary report. All job results are passed via env vars set
# from needs.<job>.result in the workflow.

# ── Helpers ───────────────────────────────────────────────────────────

status_cell() {
  case "$1" in
    success)   echo "pass"      ;;
    failure)   echo "**FAIL**"  ;;
    cancelled) echo "cancelled" ;;
    skipped)   echo "skip"      ;;
    *)         echo "—"         ;;
  esac
}

reason() {
  local result="$1" changed="$2" extra="${3:-}"
  if [[ "$changed" != "true" ]]; then
    echo "no changes"
  elif [[ "$result" == "skipped" && -n "$extra" ]]; then
    echo "$extra"
  else
    echo ""
  fi
}

row() {
  local name="$1" result="$2" changed="$3" extra="${4:-}"
  local st
  st=$(status_cell "$result")
  local r
  r=$(reason "$result" "$changed" "$extra")
  if [[ -n "$r" ]]; then
    st="${st} (${r})"
  fi
  echo "| ${name} | ${st} |"
}

# ── Build report ──────────────────────────────────────────────────────

{
  echo "## CI Report"
  echo ""

  echo "### Changes Detected"
  echo "| Component | Changed |"
  echo "|-----------|---------|"
  echo "| Rust (crates + xtask) | ${RUST_CHANGED} |"
  echo "| Docs                  | ${DOCS_CHANGED} |"
  echo "| Bench                 | ${BENCH_CHANGED} |"
  echo ""

  echo "### Rust Pipeline"
  echo "| Check | Result |"
  echo "|-------|--------|"
  row "Format"    "$FMT_RESULT"     "$RUST_CHANGED"
  row "Clippy"    "$CLIPPY_RESULT"  "$RUST_CHANGED"
  row "Test"      "$TEST_RESULT"    "$RUST_CHANGED"
  row "Audit"     "$AUDIT_RESULT"   "$RUST_CHANGED"
  row "Build"     "$BUILD_RESULT"   "$RUST_CHANGED"
  row "Docs"      "$DOCS_JOB_RESULT" "$RUST_CHANGED"
  row "Dogfood"   "$DOGFOOD_RESULT" "$RUST_CHANGED"
  echo ""

  echo "### Bench Pipeline"
  echo "| Check | Result |"
  echo "|-------|--------|"
  row "Bench smoke" "$BENCH_SMOKE_RESULT" "$BENCH_CHANGED"
  echo ""
} | tee "${GITHUB_STEP_SUMMARY:-/dev/null}"

# ── Fail if any critical job failed ──────────────────────────────────

FAILED=false
for result in \
  "$FMT_RESULT" "$CLIPPY_RESULT" "$TEST_RESULT" "$AUDIT_RESULT" \
  "$BUILD_RESULT" "$DOCS_JOB_RESULT" "$DOGFOOD_RESULT" "$BENCH_SMOKE_RESULT"; do
  if [[ "$result" == "failure" ]]; then
    FAILED=true
  fi
done

echo ""
if [[ "$FAILED" == "true" ]]; then
  echo "==> One or more checks failed"
  exit 1
else
  echo "==> All checks passed (or were skipped)"
fi
