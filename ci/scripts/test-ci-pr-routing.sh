#!/usr/bin/env bash
# Model and statically guard the canonical PR routing policy. This is a drift
# test, not the scheduling authority; the disposable broker independently
# validates live GitHub job metadata before provisioning local capacity.
set -euo pipefail

readonly repo_id='1214597864'
readonly asamarts_id='11239806'
readonly kaminsod_id='12991611'

is_approved_human() {
  case "$1" in
    "$asamarts_id"|"$kaminsod_id") return 0 ;;
    *) return 1 ;;
  esac
}

route() {
  local event_name=$1 event_repo=$2 base_repo=$3 head_repo=$4
  local author_id=$5 sender_id=$6 actor_id=$7 local_pr_capacity_enabled=$8

  if [[ "$event_name" != pull_request ]]; then
    printf 'local\n'
  elif [[ "$event_repo" == "$repo_id" &&
          "$base_repo" == "$repo_id" &&
          "$head_repo" == "$repo_id" ]] &&
       is_approved_human "$author_id" &&
       is_approved_human "$sender_id" &&
       is_approved_human "$actor_id" &&
       [[ "$local_pr_capacity_enabled" == true ]]; then
    printf 'local\n'
  else
    printf 'hosted\n'
  fi
}

expect_route() {
  local name=$1 expected=$2
  shift 2
  local actual
  actual=$(route "$@")
  if [[ "$actual" != "$expected" ]]; then
    printf '[ci-pr-routing] %s: expected %s, got %s\n' \
      "$name" "$expected" "$actual" >&2
    return 1
  fi
}

# Existing non-PR behavior is deliberately unchanged by this containment.
expect_route push-local local push '' '' '' '' '' '' false

expect_route asamarts-same-repo-held hosted pull_request \
  "$repo_id" "$repo_id" "$repo_id" \
  "$asamarts_id" "$asamarts_id" "$asamarts_id" false
expect_route kaminsod-same-repo-held hosted pull_request \
  "$repo_id" "$repo_id" "$repo_id" \
  "$kaminsod_id" "$kaminsod_id" "$kaminsod_id" false
# A login rename is intentionally absent from the inputs: stable IDs decide.
expect_route renamed-login-stable-ids-held hosted pull_request \
  "$repo_id" "$repo_id" "$repo_id" \
  "$asamarts_id" "$kaminsod_id" "$kaminsod_id" false

# Model the future capacity switch as well: only the exact admitted identities
# become local if MN-167 replaces the held route with qualified capacity.
expect_route asamarts-qualified-capacity local pull_request \
  "$repo_id" "$repo_id" "$repo_id" \
  "$asamarts_id" "$asamarts_id" "$asamarts_id" true
expect_route kaminsod-qualified-capacity local pull_request \
  "$repo_id" "$repo_id" "$repo_id" \
  "$kaminsod_id" "$kaminsod_id" "$kaminsod_id" true

expect_route dependabot-same-repo hosted pull_request \
  "$repo_id" "$repo_id" "$repo_id" 49699333 49699333 49699333 true
expect_route external-fork hosted pull_request \
  "$repo_id" "$repo_id" 987654321 "$asamarts_id" "$asamarts_id" "$asamarts_id" true
expect_route unapproved-synchronizer hosted pull_request \
  "$repo_id" "$repo_id" "$repo_id" "$asamarts_id" 987654321 987654321 true
expect_route unapproved-author hosted pull_request \
  "$repo_id" "$repo_id" "$repo_id" 987654321 "$asamarts_id" "$asamarts_id" true
expect_route null-head-repository hosted pull_request \
  "$repo_id" "$repo_id" '' "$asamarts_id" "$asamarts_id" "$asamarts_id" true
expect_route wrong-base-repository hosted pull_request \
  "$repo_id" 987654321 "$repo_id" "$asamarts_id" "$asamarts_id" "$asamarts_id" true
expect_route wrong-event-repository hosted pull_request \
  987654321 "$repo_id" "$repo_id" "$asamarts_id" "$asamarts_id" "$asamarts_id" true

assert_contains() {
  local file=$1 needle=$2
  if ! grep -Fq -- "$needle" "$file"; then
    printf '[ci-pr-routing] %s is missing policy token: %s\n' \
      "$file" "$needle" >&2
    return 1
  fi
}

for workflow in .github/workflows/ci.yml .github/workflows/coverage.yml; do
  assert_contains "$workflow" "format('{0}', github.event.repository.id) == '$repo_id'"
  assert_contains "$workflow" "format('{0}', github.event.pull_request.base.repo.id) == '$repo_id'"
  assert_contains "$workflow" "format('{0}', github.event.pull_request.head.repo.id) == '$repo_id'"
  assert_contains "$workflow" \
    "contains(fromJSON('[\"$asamarts_id\",\"$kaminsod_id\"]'), format('{0}', github.event.pull_request.user.id))"
  assert_contains "$workflow" \
    "contains(fromJSON('[\"$asamarts_id\",\"$kaminsod_id\"]'), format('{0}', github.event.sender.id))"
  assert_contains "$workflow" \
    "contains(fromJSON('[\"$asamarts_id\",\"$kaminsod_id\"]'), format('{0}', github.actor_id))"
done

# Compare the complete normalized expressions, not merely their component
# tokens. This rejects an accidental `|| true`, missing conjunction, regrouping
# or drift between the ordinary and coverage workflows.
python3 - <<'PY'
from pathlib import Path
import re
import sys

ci = Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
coverage = Path('.github/workflows/coverage.yml').read_text(encoding='utf-8')

identity = r'''format('{0}', github.event.repository.id) == '1214597864' &&
format('{0}', github.event.pull_request.base.repo.id) == '1214597864' &&
format('{0}', github.event.pull_request.head.repo.id) == '1214597864' &&
contains(fromJSON('["11239806","12991611"]'), format('{0}', github.event.pull_request.user.id)) &&
contains(fromJSON('["11239806","12991611"]'), format('{0}', github.event.sender.id)) &&
contains(fromJSON('["11239806","12991611"]'), format('{0}', github.actor_id))'''

expected_ci = "${{ github.event_name == 'pull_request' && (" + identity + ") }}"
expected_coverage = "${{ github.event_name != 'pull_request' || (false && " + identity + ") }}"

def normalize(value: str) -> str:
    return ''.join(value.split())

def expression(document: str, start: str, end: str, name: str) -> str:
    match = re.search(start + r'(?P<body>.*?)' + end, document, re.MULTILINE | re.DOTALL)
    if match is None:
        raise SystemExit(f'[ci-pr-routing] could not extract {name} policy')
    return normalize(match.group('body'))

ci_policy = expression(
    ci,
    r'^\s{10}IS_ADMITTED_PR: >-\n',
    r'^\s{10}# Capacity switch',
    'ci.yml',
)
coverage_policy = expression(
    coverage,
    r'^\s{4}if: >-\n',
    r'^\s{4}runs-on:',
    'coverage.yml',
)

for name, actual, expected in (
    ('ci.yml', ci_policy, normalize(expected_ci)),
    ('coverage.yml', coverage_policy, normalize(expected_coverage)),
):
    if actual != expected:
        print(f'[ci-pr-routing] {name} full policy differs from the canonical expression', file=sys.stderr)
        raise SystemExit(1)

allowed_ci_selectors = {
    'ubuntu-latest',
    '${{ fromJSON(needs.changes.outputs.runner) }}',
    "${{ needs.changes.outputs.runner && fromJSON(needs.changes.outputs.runner) || 'ubuntu-latest' }}",
}
ci_selectors = re.findall(r'^\s+runs-on:\s*(.+?)\s*$', ci, re.MULTILINE)
unexpected = sorted(set(ci_selectors) - allowed_ci_selectors)
if unexpected:
    print(f'[ci-pr-routing] ci.yml has a runner selector outside the hosted/route outputs: {unexpected}', file=sys.stderr)
    raise SystemExit(1)

coverage_selectors = re.findall(r'^\s+runs-on:\s*(.+?)\s*$', coverage, re.MULTILINE)
if coverage_selectors != ['[self-hosted, linux, alint]']:
    print(f'[ci-pr-routing] coverage runner selector drifted: {coverage_selectors}', file=sys.stderr)
    raise SystemExit(1)
PY

assert_contains .github/workflows/ci.yml 'LOCAL_PR_CAPACITY_ENABLED: "false"'
assert_contains .github/workflows/ci.yml 'needs.changes.outputs.hosted'
assert_contains .github/workflows/coverage.yml '(false &&'

if grep -Fq 'outputs.untrusted' .github/workflows/ci.yml; then
  printf '[ci-pr-routing] trust and executor selection became conflated again\n' >&2
  exit 1
fi

route_line=$(grep -n -- '- id: route' .github/workflows/ci.yml | cut -d: -f1)
checkout_line=$(grep -n -- 'uses: actions/checkout@' .github/workflows/ci.yml | head -n 1 | cut -d: -f1)
if [[ -z "$route_line" || -z "$checkout_line" || "$route_line" -ge "$checkout_line" ]]; then
  printf '[ci-pr-routing] route must exist and precede the first checkout\n' >&2
  exit 1
fi

if grep -Eq 'head\.repo\.(full_name|fork)' \
    .github/workflows/ci.yml .github/workflows/coverage.yml; then
  printf '[ci-pr-routing] mutable/name/boolean repository trust predicate returned\n' >&2
  exit 1
fi

printf '[ci-pr-routing] OK — identity fixtures and workflow guards passed\n'
