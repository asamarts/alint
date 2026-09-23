#!/usr/bin/env bash
# Prove that the workflow-permission policy rejects representative real-file
# regressions rather than merely matching its embedded regex fixtures.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
POLICY="$REPO_ROOT/ci/scripts/test-workflow-permissions.sh"
TMP_ROOT="$(mktemp -d)"
trap 'rm -rf -- "$TMP_ROOT"' EXIT

passed=0

# This is a literal workflow mutation anchor, not shell code.
# shellcheck disable=SC2016
latest_tag_anchor='          LATEST_TAG="$(gh api "repos/${GITHUB_REPOSITORY}/releases/latest" --jq .tag_name)"'

expect_rejection() {
  local label="$1"
  local relative_path="$2"
  local needle="$3"
  local replacement="$4"
  local expected="$5"
  local fixture="$TMP_ROOT/$label"
  local output

  mkdir -p "$fixture/.github" "$fixture/ci"
  cp -a "$REPO_ROOT/.github/workflows" "$fixture/.github/workflows"
  cp -a "$REPO_ROOT/ci/scripts" "$fixture/ci/scripts"

  python3 - "$fixture/$relative_path" "$needle" "$replacement" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
needle = sys.argv[2]
replacement = sys.argv[3]
text = path.read_text(encoding='utf-8')
if text.count(needle) != 1:
    raise SystemExit(
        f'{path}: mutation anchor must occur exactly once; found {text.count(needle)}'
    )
path.write_text(text.replace(needle, replacement), encoding='utf-8')
PY

  if output="$(WORKFLOW_PERMISSIONS_REPO_ROOT="$fixture" bash "$POLICY" 2>&1)"; then
    printf '[workflow-permissions-mutations] %s: policy unexpectedly passed\n' "$label" >&2
    exit 1
  fi
  if ! grep -Fq -- "$expected" <<<"$output"; then
    printf '[workflow-permissions-mutations] %s: wrong rejection\n%s\n' \
      "$label" "$output" >&2
    exit 1
  fi
  printf '  ok: %s\n' "$label"
  passed=$((passed + 1))
}

expect_rejection \
  missing-top-level-declaration \
  .github/workflows/ci.yml \
  $'permissions:\n  contents: read\n' \
  '' \
  'expected one explicit top-level permissions block, found 0'

expect_rejection \
  broad-ordinary-authority \
  .github/workflows/ci.yml \
  $'permissions:\n  contents: read\n' \
  $'permissions:\n  contents: write\n' \
  "top-level permissions {'contents': 'write'} != expected {'contents': 'read'}"

expect_rejection \
  undeclared-write-operation \
  .github/workflows/ci.yml \
  '      - run: bash ci/scripts/check-secrets-inventory.sh' \
  $'      - run: bash ci/scripts/check-secrets-inventory.sh\n      - run: git push origin HEAD:audit-fixture' \
  "'git push' requires contents: write"

expect_rejection \
  review-approval-operation \
  .github/workflows/bench-record.yml \
  '          gh pr create' \
  $'          gh pr review 7 \\\n            --approve\n          gh pr create' \
  'pull-request approval operation is prohibited'

expect_rejection \
  raw-api-mutation \
  .github/workflows/docs-bundle.yml \
  "$latest_tag_anchor" \
  $'          gh api "repos/${GITHUB_REPOSITORY}/issues/1" \\\n            --method PATCH -f title=audit-fixture\n          LATEST_TAG="$(gh api "repos/${GITHUB_REPOSITORY}/releases/latest" --jq .tag_name)"' \
  'raw GitHub API mutation requires an explicit policy mapping'

expect_rejection \
  implicit-api-post \
  .github/workflows/docs-bundle.yml \
  "$latest_tag_anchor" \
  $'          gh api "repos/${GITHUB_REPOSITORY}/issues" -f title=audit-fixture\n          LATEST_TAG="$(gh api "repos/${GITHUB_REPOSITORY}/releases/latest" --jq .tag_name)"' \
  'raw GitHub API mutation requires an explicit policy mapping'

expect_rejection \
  graphql-mutation \
  .github/workflows/docs-bundle.yml \
  "$latest_tag_anchor" \
  $'          gh api graphql -f query="mutation { auditFixture }"\n          LATEST_TAG="$(gh api "repos/${GITHUB_REPOSITORY}/releases/latest" --jq .tag_name)"' \
  'raw GitHub API mutation requires an explicit policy mapping'

expect_rejection \
  curl-implicit-post \
  .github/workflows/docs-bundle.yml \
  "$latest_tag_anchor" \
  $'          curl https://api.github.com/repos/o/r/issues -d title=audit-fixture\n          LATEST_TAG="$(gh api "repos/${GITHUB_REPOSITORY}/releases/latest" --jq .tag_name)"' \
  'raw GitHub API mutation requires an explicit policy mapping'

printf '[workflow-permissions-mutations] OK — %d real-file regressions rejected\n' "$passed"
