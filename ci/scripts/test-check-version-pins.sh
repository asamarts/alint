#!/usr/bin/env bash
# Regression tests for the version/Action documentation pin gate. The test uses
# a disposable local clone so each negative case can corrupt a file without
# touching the caller's worktree.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TMP_ROOT=$(mktemp -d)
trap 'rm -rf "$TMP_ROOT"' EXIT

git clone -q --no-local "$REPO_ROOT" "$TMP_ROOT/repo"
# Exercise working-tree changes before they are committed, not only the clone's
# HEAD copies. The documentation fixtures are already part of this PR; these
# two files are the implementation under test in the same change.
cp "$REPO_ROOT/ci/scripts/check-version-pins.sh" "$TMP_ROOT/repo/ci/scripts/check-version-pins.sh"
cp "$REPO_ROOT/action.yml" "$TMP_ROOT/repo/action.yml"
cd "$TMP_ROOT/repo"

# Read the documented Action pin and the workspace version instead of
# hardcoding one release, so the fixtures survive version bumps (including the
# window where the workspace is bumped but the doc pin still trails it).
DOC_VERSION=$(sed -n 's/^ACTION_DOC_VERSION=//p' ci/action-doc-pin.env)
DOC_SHA=$(sed -n 's/^ACTION_DOC_SHA=//p' ci/action-doc-pin.env)
WS_VERSION=$(sed -n 's/^version = "\([0-9][0-9.]*\)"$/\1/p' Cargo.toml | head -n1)
[[ -n "$DOC_VERSION" && -n "$DOC_SHA" && -n "$WS_VERSION" ]] ||
  { echo "could not read ci/action-doc-pin.env or the workspace version" >&2; exit 1; }
DOC_RE=${DOC_VERSION//./\\.}

pass=0
fail=0

expect_ok() {
  local name=$1
  shift
  if bash ci/scripts/check-version-pins.sh "$@" >/dev/null 2>&1; then
    echo "  ok: $name"
    pass=$((pass + 1))
  else
    echo "  FAIL: $name (expected success)" >&2
    fail=$((fail + 1))
  fi
}

expect_rejected() {
  local name=$1
  shift
  if bash ci/scripts/check-version-pins.sh "$@" >/dev/null 2>&1; then
    echo "  FAIL: $name (expected rejection)" >&2
    fail=$((fail + 1))
  else
    echo "  ok: $name"
    pass=$((pass + 1))
  fi
}

restore() {
  git checkout -q -- ci/action-doc-pin.env docs/rules.md docs/site/integrations/github-actions.md
  rm -f docs/site/new-action-example.md
}

expect_ok "canonical pins"

# Build a hermetic stand-in for the first release that contains the baked
# version. This tests the success path without relying on tags being present in
# the caller's checkout (CI's shell-test job intentionally uses a shallow one).
# Model the post-release state: the doc pin names the workspace version, whose
# tagged action.yml bakes that same version.
git add action.yml
git -c user.name='alint tests' -c user.email='tests@alint.invalid' \
  commit --allow-empty -q -m 'fixture: bake action version'
fixture_sha=$(git rev-parse HEAD)
git tag -f "v$WS_VERSION" HEAD >/dev/null
sed -i "s/^ACTION_DOC_VERSION=.*/ACTION_DOC_VERSION=$WS_VERSION/" ci/action-doc-pin.env
sed -i "s/^ACTION_DOC_SHA=.*/ACTION_DOC_SHA=$fixture_sha/" ci/action-doc-pin.env
sed -i 's/^ACTION_DOC_REQUIRES_EXPLICIT_VERSION=.*/ACTION_DOC_REQUIRES_EXPLICIT_VERSION=false/' \
  ci/action-doc-pin.env
for f in docs/rules.md docs/site/integrations/github-actions.md; do
  sed -i "s/$DOC_SHA # v${DOC_RE}/$fixture_sha # v$WS_VERSION/g" "$f"
  sed -i -E "/^[[:space:]]*version:[[:space:]]*v${DOC_RE}([[:space:]]+#.*)?[[:space:]]*\$/d" "$f"
done
expect_ok "verified release SHA and baked version" --verify-action-tag
restore
git tag -d "v$WS_VERSION" >/dev/null
# The clone carries the real tags; drop the doc pin's tag so verify has none.
git tag -d "v$DOC_VERSION" >/dev/null 2>&1 || true
expect_rejected "verified mode requires the release tag" --verify-action-tag
expect_ok "checkout-local mode does not require release tags"

sed -i '0,/asamarts\/alint@[0-9a-f]\{40\}/s//asamarts\/alint@0000000000000000000000000000000000000000/' \
  docs/site/integrations/github-actions.md
expect_rejected "documented SHA differs from metadata"
restore

sed -i 's/^ACTION_DOC_SHA=.*/ACTION_DOC_SHA=0000000000000000000000000000000000000000/' \
  ci/action-doc-pin.env
expect_rejected "metadata SHA differs from snippets"
restore

cat > docs/site/new-action-example.md <<'EOF'
# New Action example

```yaml
- uses: asamarts/alint@0000000000000000000000000000000000000000 # v0.17.0
  with:
    version: v0.17.0
```
EOF
expect_rejected "new user-facing Action snippets are discovered"
restore

sed -i "0,/^[[:space:]]*version: v${DOC_RE}\$/s//    path: ./" docs/site/integrations/github-actions.md
expect_rejected "legacy Action snippet missing explicit binary version"
restore

sed -i 's/^ACTION_DOC_REQUIRES_EXPLICIT_VERSION=.*/ACTION_DOC_REQUIRES_EXPLICIT_VERSION=false/' \
  ci/action-doc-pin.env
expect_rejected "metadata disagrees with snippet shape"
restore

expect_rejected "unknown option is rejected" --unknown

echo "[test-check-version-pins] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
