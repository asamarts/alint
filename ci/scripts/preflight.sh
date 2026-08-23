#!/usr/bin/env bash
# ci/scripts/preflight.sh — local preflight bundle.
#
# Runs the same gates CI's ci.yml workflow runs against a push to
# main, in the order most-likely-to-fail first (fast-fail). On a
# warm cargo cache the whole bundle finishes in well under a
# minute. Designed to wrap into the pre-push git hook at
# ci/githooks/pre-push so a typo or unformatted block bounces
# locally instead of consuming a CI minute.
#
# Skipped from the default preflight (CI runs them, but they're
# too slow / network-dependent for routine push):
#   - cargo audit (network-dependent; CI runs the `audit` job, so
#     a CVE that landed minutes ago can fail CI even though
#     preflight just passed — opt in with `bash ci/scripts/audit.sh`
#     before a tag push if you want belt-and-suspenders)
#   - bench-smoke (~3 min; bench harness re-runs the smoke set)
#   - coverage (~5 min; not load-bearing for correctness)
#   - cross-platform build matrix (~10 min wall-clock for the five
#     release targets — release.yml is the gate that matters)
#   - shell-tests (~1s; CI runs `ci/scripts/shell-tests.sh` as its
#     own job — opt in locally with that script if you've changed
#     anything under ci/scripts/*.sh)
#   - examples-validate (~30s build + run; CI runs it as its own
#     job when examples/ or schemas/ change)
#
# Skip a specific check:
#   PREFLIGHT_SKIP=clippy,doc ./ci/scripts/preflight.sh
#
# Skip the whole hook for one push:
#   git push --no-verify

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

skip_set=",${PREFLIGHT_SKIP:-},"
failed=0

run() {
  local name=$1
  shift
  if [[ "$skip_set" == *",$name,"* ]]; then
    echo "==> [preflight] skip $name (PREFLIGHT_SKIP)"
    return 0
  fi
  echo "==> [preflight] $name"
  if ! "$@"; then
    echo
    echo "==> [preflight] FAILED on '$name'"
    failed=1
    return 1
  fi
}

# Order: cheapest-and-most-likely-to-fail first.
#   - fmt: ~1s, catches single-space / trailing-comma drift
#   - clippy: ~10s warm; pedantic-strict, catches the stuff
#     that bites in CI
#   - test: ~30s warm; the workspace test suite + trycmd
#     snapshots
#   - doc: ~10s; rustdoc warnings would otherwise bypass
#     check / test / clippy
#   - dogfood: ~5s if release binary is fresh; runs alint
#     against its own repo
#
# We deliberately fall through on failure (rather than `set -e`-
# exit on the first run) so a developer sees the FULL set of
# things broken in one pass instead of fix-rerun-fix-rerun.
run fmt           bash ci/scripts/fmt.sh                          || true
run clippy        bash ci/scripts/clippy.sh                       || true
run test          bash ci/scripts/test.sh                         || true
run doc           bash ci/scripts/docs.sh                         || true
run version-pins  bash ci/scripts/check-version-pins.sh           || true
run dep-floors    bash ci/scripts/check-workspace-dep-floors.sh   || true
run secrets-inv   bash ci/scripts/check-secrets-inventory.sh      || true
run dogfood       bash ci/scripts/dogfood.sh                      || true

if [[ "$failed" -ne 0 ]]; then
  echo
  echo "==> [preflight] ONE OR MORE CHECKS FAILED — see output above"
  exit 1
fi

echo
echo "==> [preflight] all checks passed; ok to push"
