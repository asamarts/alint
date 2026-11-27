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
  echo "| Shell harnesses       | ${SHELL_CHANGED:-false} |"
  echo "| Relation layer        | ${RELATION_CHANGED:-false} |"
  echo ""

  echo "### Rust Pipeline"
  echo "| Check | Result |"
  echo "|-------|--------|"
  row "Format"       "$FMT_RESULT"         "$RUST_CHANGED"
  row "MSRV"         "$MSRV_RESULT"        "$RUST_CHANGED"
  row "Clippy"       "$CLIPPY_RESULT"      "$RUST_CHANGED"
  row "Test"         "$TEST_RESULT"        "$RUST_CHANGED"
  row "YAML compare" "${YAML_COMPARE_RESULT:-skipped}" "$RUST_CHANGED"
  row "Audit"        "$AUDIT_RESULT"       "$( [[ "$RUST_CHANGED" == true || "$SUPPLY_CHAIN_CHANGED" == true ]] && echo true || echo false )"
  row "Deny"         "$DENY_RESULT"        "$( [[ "$RUST_CHANGED" == true || "$SUPPLY_CHAIN_CHANGED" == true ]] && echo true || echo false )"
  row "Supply chain" "$SUPPLY_CHAIN_RESULT" "$SUPPLY_CHAIN_CHANGED"
  row "Build"        "$BUILD_RESULT"       "$RUST_CHANGED"
  row "Docs"         "$DOCS_JOB_RESULT"    "$( [[ "$RUST_CHANGED" == true || "$DOCS_CHANGED" == true ]] && echo true || echo false )"
  row "Dogfood"      "$DOGFOOD_RESULT"     "$RUST_CHANGED"
  # Shell tests route on their own `shell` flag (detect-changes.sh), which
  # also fires for packaging / docs / supply-chain / repo-config paths.
  row "Shell tests"  "$SHELL_TESTS_RESULT" "${SHELL_CHANGED:-false}"
  row "Plugin contract (witforge)" "${WITFORGE_RESULT:-skipped}" "$RUST_CHANGED"
  row "Secrets inventory" "${SECRETS_INVENTORY_RESULT:-}" "true"
  row "Action pins" "${ACTION_PINS_RESULT:-}" "true"
  echo ""

  echo "### Bench Pipeline"
  echo "| Check | Result |"
  echo "|-------|--------|"
  row "Bench smoke" "$BENCH_SMOKE_RESULT" "$BENCH_CHANGED"
  echo ""

  echo "### Performance (relation layer)"
  echo "| Check | Result |"
  echo "|-------|--------|"
  row "Perf gate (v0.18.2 reference)" "${PERF_REFERENCE_RESULT:-skipped}" "${RELATION_CHANGED:-false}"
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
# Secrets Inventory and Action Pins run on every event (no `needs`, no `if:`),
# so anything but success failed to show the check holds: a cancel (a timeout,
# a stalled `git ls-remote`), a skip, or a result missing from the env.
for check in "Secrets inventory=${SECRETS_INVENTORY_RESULT:-}" "Action pins=${ACTION_PINS_RESULT:-}"; do
  if [[ "${check#*=}" != "success" ]]; then
    echo "==> ${check%%=*} did not succeed (result: ${check#*=}); it runs on every event" >&2
    FAILED=true
  fi
done
# Any job whose outcome is neither `success` nor `skipped` (including `failure`,
# `cancelled` from a timeout/preemption, or unset) fails the aggregate gate.
for job in \
  "Format=${FMT_RESULT:-}" \
  "MSRV=${MSRV_RESULT:-}" \
  "Clippy=${CLIPPY_RESULT:-}" \
  "Test=${TEST_RESULT:-}" \
  "YAML compare=${YAML_COMPARE_RESULT:-skipped}" \
  "Audit=${AUDIT_RESULT:-}" \
  "Deny=${DENY_RESULT:-}" \
  "Supply chain=${SUPPLY_CHAIN_RESULT:-}" \
  "Build=${BUILD_RESULT:-}" \
  "Docs=${DOCS_JOB_RESULT:-}" \
  "Dogfood=${DOGFOOD_RESULT:-}" \
  "Bench smoke=${BENCH_SMOKE_RESULT:-}" \
  "Examples=${EXAMPLES_RESULT:-}" \
  "Shell tests=${SHELL_TESTS_RESULT:-}" \
  "Editors=${EDITORS_RESULT:-}" \
  "Packaging=${PACKAGING_RESULT:-skipped}" \
  "Perf gate (v0.18.2 reference)=${PERF_REFERENCE_RESULT:-skipped}" \
  "Plugin contract (witforge)=${WITFORGE_RESULT:-skipped}"; do
  name="${job%%=*}"
  result="${job#*=}"
  if [[ "$result" != "success" && "$result" != "skipped" ]]; then
    echo "==> ${name} did not pass (result: ${result:-unset})" >&2
    FAILED=true
  fi
done

# Every job whose change trigger fired must have run and succeeded (an
# unexpected skip means a gate went unchecked). `Bench smoke` (skipped on
# hosted runners) and `Perf gate` (PR-only) are checked above for failure/cancel.
AUDIT_TRIGGERED="$( [[ "${RUST_CHANGED:-false}" == true || "${SUPPLY_CHAIN_CHANGED:-false}" == true ]] && echo true || echo false )"
DOCS_TRIGGERED="$( [[ "${RUST_CHANGED:-false}" == true || "${DOCS_CHANGED:-false}" == true ]] && echo true || echo false )"
for check in \
  "Format:${RUST_CHANGED:-false}:${FMT_RESULT:-}" \
  "MSRV:${RUST_CHANGED:-false}:${MSRV_RESULT:-}" \
  "Clippy:${RUST_CHANGED:-false}:${CLIPPY_RESULT:-}" \
  "Test:${RUST_CHANGED:-false}:${TEST_RESULT:-}" \
  "YAML compare:${RUST_CHANGED:-false}:${YAML_COMPARE_RESULT:-skipped}" \
  "Audit:${AUDIT_TRIGGERED}:${AUDIT_RESULT:-}" \
  "Deny:${AUDIT_TRIGGERED}:${DENY_RESULT:-}" \
  "Supply chain:${SUPPLY_CHAIN_CHANGED:-false}:${SUPPLY_CHAIN_RESULT:-}" \
  "Build:${RUST_CHANGED:-false}:${BUILD_RESULT:-}" \
  "Docs:${DOCS_TRIGGERED}:${DOCS_JOB_RESULT:-}" \
  "Dogfood:${RUST_CHANGED:-false}:${DOGFOOD_RESULT:-}" \
  "Examples:${EXAMPLES_CHANGED:-false}:${EXAMPLES_RESULT:-}" \
  "Shell tests:${SHELL_CHANGED:-false}:${SHELL_TESTS_RESULT:-}" \
  "Editors:${EDITORS_CHANGED:-false}:${EDITORS_RESULT:-}" \
  "Packaging:${PACKAGING_CHANGED:-false}:${PACKAGING_RESULT:-skipped}" \
  "Plugin contract (witforge):${RUST_CHANGED:-false}:${WITFORGE_RESULT:-skipped}"; do
  IFS=: read -r name triggered result <<< "$check"
  if [[ "$triggered" == "true" && "$result" != "success" ]]; then
    echo "==> ${name} was triggered but did not succeed (result: ${result:-unset})" >&2
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
