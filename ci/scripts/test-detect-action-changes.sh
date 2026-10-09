#!/usr/bin/env bash
# Ensure narrowly-scoped changes reach the jobs that actually cover them:
# action-only changes reach the SHA-resolution / pin jobs, the license policy
# reaches cargo-deny, and distribution-packaging files reach the packaging job.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TMP_ROOT=$(mktemp -d)
trap 'rm -rf "$TMP_ROOT"' EXIT

git clone -q --no-local "$REPO_ROOT" "$TMP_ROOT/repo"
cp "$REPO_ROOT/ci/scripts/detect-changes.sh" "$TMP_ROOT/repo/ci/scripts/detect-changes.sh"
cd "$TMP_ROOT/repo"
git config user.name 'alint tests'
git config user.email 'tests@alint.invalid'

pass=0
fail=0

# expect_routes <name> <path> <flag=value>...
expect_routes() {
  local name=$1 path=$2
  shift 2
  local base output want missing=()
  base=$(git rev-parse HEAD)
  mkdir -p "$(dirname "$path")"
  printf '\n# detect-changes fixture\n' >> "$path"
  git add "$path"
  git commit -q -m "fixture: change $path"
  output=$(GH_EVENT=push PUSH_BEFORE_SHA="$base" bash ci/scripts/detect-changes.sh)
  for want in "$@"; do
    grep -qE "(^|[[:space:]])${want}([[:space:]]|$)" <<< "$output" || missing+=("$want")
  done
  if [[ ${#missing[@]} -eq 0 ]]; then
    echo "  ok: $name"
    pass=$((pass + 1))
  else
    echo "  FAIL: $name (missing: ${missing[*]})" >&2
    echo "$output" >&2
    fail=$((fail + 1))
  fi
}

expect_routes "action.yml reaches shell and docs tests" action.yml rust=true docs=true
expect_routes "action helper reaches shell and docs tests" action/resolve-version.sh rust=true docs=true
# deny.toml flips only supply_chain; ci.yml's Deny job must accept that flag
# (asserted statically below), or a PR loosening the policy skips cargo-deny.
expect_routes "deny.toml reaches supply-chain (and Deny)" deny.toml supply_chain=true
expect_routes "install.sh reaches packaging" install.sh packaging=true
expect_routes "npm shim reaches packaging" npm/install.js packaging=true
expect_routes "Dockerfile reaches packaging" Dockerfile packaging=true
expect_routes ".dockerignore reaches packaging" .dockerignore packaging=true
expect_routes "pre-commit manifest reaches packaging" .pre-commit-hooks.yaml packaging=true

# Static half: the jobs those flags feed must gate on them.
python3 - "$REPO_ROOT/.github/workflows/ci.yml" <<'PY' || fail=$((fail + 1))
import re
import sys

ci = open(sys.argv[1], encoding='utf-8').read()


def job_if(name):
    m = re.search(rf'^  {name}:\n(?:    .*\n|\n)*?    if: >-\n(?P<cond>(?:      .*\n)+)', ci, re.MULTILINE)
    return ' '.join(m.group('cond').split()) if m else ''


errors = []
if "needs.changes.outputs.supply_chain == 'true'" not in job_if('deny'):
    errors.append("Deny job must run on supply_chain changes (deny.toml)")
if "needs.changes.outputs.packaging == 'true'" not in job_if('packaging'):
    errors.append("Packaging job must gate on needs.changes.outputs.packaging")
if 'ci/scripts/packaging-check.sh' not in ci:
    errors.append("ci.yml must run ci/scripts/packaging-check.sh")
if 'packaging: ${{ steps.detect.outputs.packaging }}' not in ci:
    errors.append("changes job must export the packaging output")
for e in errors:
    print(f'  FAIL: {e}', file=sys.stderr)
sys.exit(1 if errors else 0)
PY

echo "[test-detect-action-changes] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
