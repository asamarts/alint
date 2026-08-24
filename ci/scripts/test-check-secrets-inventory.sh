#!/usr/bin/env bash
# Test harness for ci/scripts/check-secrets-inventory.sh -- the secrets-inventory
# drift gate. Drives the gate against fixtures (via its SECRETS_DOC /
# SECRETS_WORKFLOWS_DIR overrides) to lock in each assertion AND the edge cases an
# adversarial audit surfaced, so a regression in the gate is caught here, not by a
# spuriously red release or a drift that slips through:
#   - empty RETIRED array must not crash the success path under `set -u`
#   - `secrets['NAME']` bracket syntax must be seen by both assertions (fail-open)
#   - the retired marker is scoped to column 1, so prose elsewhere can't misflag an
#     active secret (fail-closed)
#
# shellcheck disable=SC2016  # the `${{ secrets.X }}` fixtures are literal by design
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
GATE="$REPO_ROOT/ci/scripts/check-secrets-inventory.sh"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

pass=0
fail=0

# run_case <name> <ok|fail> <doc-body> <workflow-body>
run_case() {
  local name="$1" expect="$2" doc="$3" wf="$4"
  local d
  d="$(mktemp -d "$tmp/case.XXXXXX")"
  mkdir -p "$d/wf"
  printf '%s\n' "$doc" > "$d/doc.md"
  printf '%s\n' "$wf" > "$d/wf/w.yml"
  local rc=0
  SECRETS_DOC="$d/doc.md" SECRETS_WORKFLOWS_DIR="$d/wf" \
    bash "$GATE" >/dev/null 2>&1 || rc=$?
  local got=ok
  [ "$rc" -eq 0 ] || got=fail
  if [ "$got" = "$expect" ]; then
    echo "  ok: $name (expected $expect)"
    pass=$((pass + 1))
  else
    echo "  FAIL: $name -- expected $expect, got $got (rc=$rc)" >&2
    fail=$((fail + 1))
  fi
}

# Shared valid table header; each case appends its own rows + a closing section.
HDR='## Inventory

| Secret | Channel | Note |
|---|---|---|
| `GITHUB_TOKEN` | ghcr | built-in |'

# A [HIGH regression]: in-sync inventory with NOTHING retired -> PASS. Without the
# `declare -A RETIRED=()` init this crashes on `${#RETIRED[@]}` under `set -u`.
run_case "in-sync, nothing retired" ok \
"$HDR
| \`CARGO_REGISTRY_TOKEN\` | crates.io | active |
## Next" \
'jobs: {a: {steps: [{run: "echo ${{ secrets.CARGO_REGISTRY_TOKEN }}"}]}}'

# B: used-but-undocumented -> FAIL (assertion 1).
run_case "undocumented secret" fail \
"$HDR
## Next" \
'jobs: {a: {steps: [{run: "echo ${{ secrets.MYSTERY_TOKEN }}"}]}}'

# C: retired secret still wired -> FAIL (assertion 2).
run_case "retired still wired" fail \
"$HDR
| \`OLD_TOK\` *(deleted 2026-01-01)* | npm | gone |
## Next" \
'jobs: {a: {steps: [{run: "echo ${{ secrets.OLD_TOK }}"}]}}'

# D [bracket regression]: undocumented via secrets['NAME'] -> FAIL (was fail-open).
run_case "undocumented bracket syntax" fail \
"$HDR
## Next" \
"jobs: {a: {steps: [{run: \"echo \${{ secrets['BRACKET_MYSTERY'] }}\"}]}}"

# E [retired-scope regression]: active secret whose LATER column mentions 'retired'
# must NOT be misflagged -> PASS (was fail-closed on whole-row scan).
run_case "active secret, 'retired' prose in col3" ok \
"$HDR
| \`ACTIVE_TOK\` | npm | replaces the retired PAT flow |
## Next" \
'jobs: {a: {steps: [{run: "echo ${{ secrets.ACTIVE_TOK }}"}]}}'

# F: documented-but-unused ACTIVE secret is allowed -> PASS (no orphan assertion).
run_case "documented-but-unused active" ok \
"$HDR
| \`UNUSED_TOK\` | crates.io | fallback |
## Next" \
'jobs: {a: {steps: [{run: "echo hi"}]}}'

# G: GITHUB_TOKEN is built-in, exempt from assertion 1 -> PASS.
run_case "GITHUB_TOKEN exempt" ok \
"$HDR
## Next" \
'jobs: {a: {steps: [{run: "echo ${{ secrets.GITHUB_TOKEN }}"}]}}'

# H [bracket + retired]: retired secret re-wired via bracket syntax -> FAIL.
run_case "retired re-wired via bracket" fail \
"$HDR
| \`OLD_TOK\` *(deleted 2026-01-01)* | npm | gone |
## Next" \
"jobs: {a: {steps: [{run: \"echo \${{ secrets['OLD_TOK'] }}\"}]}}"

echo "[test-check-secrets-inventory] ${pass} passed, ${fail} failed"
[ "$fail" -eq 0 ]
