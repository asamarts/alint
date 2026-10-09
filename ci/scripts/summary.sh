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

  echo "Detect Changes: $(status_cell "${CHANGES_RESULT:-}")"
  echo ""
  echo "### Changes Detected"
  echo "| Component | Changed |"
  echo "|-----------|---------|"
  echo "| Rust (crates + xtask) | ${RUST_CHANGED} |"
  echo "| Docs                  | ${DOCS_CHANGED} |"
  echo "| Bench                 | ${BENCH_CHANGED} |"
  echo "| Examples              | ${EXAMPLES_CHANGED} |"
  echo "| Editors               | ${EDITORS_CHANGED} |"
  echo "| Supply chain          | ${SUPPLY_CHAIN_CHANGED} |"
  echo "| Packaging             | ${PACKAGING_CHANGED:-false} |"
  echo ""

  echo "### Rust Pipeline"
  echo "| Check | Result |"
  echo "|-------|--------|"
  row "Format"       "$FMT_RESULT"         "$RUST_CHANGED"
  row "MSRV"         "$MSRV_RESULT"        "$RUST_CHANGED"
  row "Clippy"       "$CLIPPY_RESULT"      "$RUST_CHANGED"
  row "Test"         "$TEST_RESULT"        "$RUST_CHANGED"
  row "Audit"        "$AUDIT_RESULT"       "$RUST_CHANGED"
  row "Deny"         "$DENY_RESULT"        "$( [[ "$RUST_CHANGED" == true || "$SUPPLY_CHAIN_CHANGED" == true ]] && echo true || echo false )"
  row "Supply chain" "$SUPPLY_CHAIN_RESULT" "$SUPPLY_CHAIN_CHANGED"
  row "Build"        "$BUILD_RESULT"       "$RUST_CHANGED"
  row "Docs"         "$DOCS_JOB_RESULT"    "$RUST_CHANGED"
  row "Dogfood"      "$DOGFOOD_RESULT"     "$RUST_CHANGED"
  row "Shell tests"  "$SHELL_TESTS_RESULT" "$RUST_CHANGED"
  row "Secrets inventory" "$SECRETS_INVENTORY_RESULT" "true"
  echo ""

  echo "### Bench Pipeline"
  echo "| Check | Result |"
  echo "|-------|--------|"
  row "Bench smoke" "$BENCH_SMOKE_RESULT" "$BENCH_CHANGED"
  echo ""

  echo "### Examples Pipeline"
  echo "| Check | Result |"
  echo "|-------|--------|"
  row "Examples validate" "$EXAMPLES_RESULT" "$EXAMPLES_CHANGED"
  echo ""

  echo "### Editors Pipeline"
  echo "| Check | Result |"
  echo "|-------|--------|"
  row "Editors (VS Code + Zed)" "$EDITORS_RESULT" "$EDITORS_CHANGED"
  echo ""

  echo "### Packaging Pipeline"
  echo "| Check | Result |"
  echo "|-------|--------|"
  row "install.sh / npm / Docker / pre-commit" "${PACKAGING_RESULT:-skipped}" "${PACKAGING_CHANGED:-false}"
  echo ""
} | tee "${GITHUB_STEP_SUMMARY:-/dev/null}"

# ── Fail if any critical job failed ──────────────────────────────────

FAILED=false
# `changes` gates every pipeline via needs.changes.outputs.*: if it fails (or is
# cancelled), every downstream job is SKIPPED, which would otherwise read as
# "all checks passed (or were skipped)". Anything but success is a failure.
if [[ "${CHANGES_RESULT:-}" != "success" ]]; then
  echo "==> Detect Changes did not succeed (result: ${CHANGES_RESULT:-unset}); downstream skips are not passes" >&2
  FAILED=true
fi
for result in \
  "$SECRETS_INVENTORY_RESULT" \
  "$FMT_RESULT" "$MSRV_RESULT" "$CLIPPY_RESULT" "$TEST_RESULT" "$AUDIT_RESULT" \
  "$DENY_RESULT" "$SUPPLY_CHAIN_RESULT" "$BUILD_RESULT" "$DOCS_JOB_RESULT" \
  "$DOGFOOD_RESULT" "$BENCH_SMOKE_RESULT" "$EXAMPLES_RESULT" \
  "$SHELL_TESTS_RESULT" "$EDITORS_RESULT" "${PACKAGING_RESULT:-skipped}"; do
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
