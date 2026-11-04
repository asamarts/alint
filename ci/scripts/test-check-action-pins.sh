#!/usr/bin/env bash
# Regression tests for check-action-pins.sh against local fixture repositories
# (ACTION_PINS_GIT_BASE), so no network is needed: exact, annotated and series
# tags, a branch comment, sub-path Actions, and every way a pin can disagree with
# its comment.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
pass=0
fail=0

g() { git -c user.name='alint tests' -c user.email='tests@alint.invalid' -c init.defaultBranch=master "$@"; }

# acme/tagged: c1 (v1.0.0, lightweight), c2 (v1.1.0 annotated, and the moving
# major tag v1), c3 (v2.0.0).
work="$sandbox/work-tagged"
g init -q "$work"
for n in 1 2 3; do g -C "$work" commit -q --allow-empty -m "c$n"; done
c1=$(g -C "$work" rev-parse HEAD~2)
c2=$(g -C "$work" rev-parse HEAD~1)
c3=$(g -C "$work" rev-parse HEAD)
g -C "$work" tag v1.0.0 "$c1"
g -C "$work" tag -a -m v1.1.0 v1.1.0 "$c2"
g -C "$work" tag v1 "$c2"
g -C "$work" tag v2.0.0 "$c3"
mkdir -p "$sandbox/remotes/acme"
g clone -q --bare "$work" "$sandbox/remotes/acme/tagged"

# acme/branchy: b1, b2 on master; b3 only on `feature`.
work="$sandbox/work-branchy"
g init -q "$work"
g -C "$work" commit -q --allow-empty -m b1
b1=$(g -C "$work" rev-parse HEAD)
g -C "$work" commit -q --allow-empty -m b2
g -C "$work" checkout -q -b feature
g -C "$work" commit -q --allow-empty -m b3
b3=$(g -C "$work" rev-parse HEAD)
g -C "$work" checkout -q master
g clone -q --bare "$work" "$sandbox/remotes/acme/branchy"

root="$sandbox/root"
mkdir -p "$root/.github/workflows"
cp "$REPO_ROOT/ci/scripts/check-action-pins.sh" "$sandbox/check.sh"

# check <name> <ok|rejected> <workflow steps...>: one workflow holding the steps.
check() {
  local name=$1 want=$2
  shift 2
  rm -f "$root/action.yml"
  {
    echo "on: push"
    echo "jobs:"
    echo "  j:"
    echo "    runs-on: ubuntu-latest"
    echo "    steps:"
    for step in "$@"; do echo "      - $step"; done
  } > "$root/.github/workflows/w.yml"
  verdict
}

verdict() {
  local got=ok
  ACTION_PINS_REPO_ROOT="$root" ACTION_PINS_GIT_BASE="file://$sandbox/remotes" ACTION_PINS_ATTEMPTS=1 \
    bash "$sandbox/check.sh" > "$sandbox/out" 2>&1 || got=rejected
  if [[ "$got" == "$want" ]]; then
    echo "  ok: $name"
    pass=$((pass + 1))
  else
    echo "  FAIL: $name (expected $want, got $got)" >&2
    sed 's/^/      | /' "$sandbox/out" >&2
    fail=$((fail + 1))
  fi
}

check "an exact lightweight tag" ok "uses: acme/tagged@$c1 # v1.0.0"
check "an annotated tag, peeled to its commit" ok "uses: acme/tagged@$c2 # v1.1.0"
check "a major tag that moved on accepts a release in its series" ok "uses: acme/tagged@$c1 # v1"
check "a minor series" ok "uses: acme/tagged@$c1 # v1.0"
check "a sub-path Action resolves its repository" ok "uses: acme/tagged/sub/dir@$c3 # v2.0.0"
check "a commit in the named branch's history" ok "uses: acme/branchy@$b1 # master"
check "local and docker references are skipped" ok "uses: ./" "uses: docker://alpine:3.20" \
  "uses: acme/tagged@$c3 # v2.0.0"

check "a tag naming another commit" rejected "uses: acme/tagged@$c2 # v1.0.0"
check "a commit outside the named series" rejected "uses: acme/tagged@$c3 # v1"
check "a commit not on the named branch" rejected "uses: acme/branchy@$b3 # master"
check "a pin without a comment" rejected "uses: acme/tagged@$c1"
check "a comment with extra words" rejected "uses: acme/tagged@$c1 # v1.0.0 (latest)"
check "an unpinned tag" rejected "uses: acme/tagged@v1.0.0 # v1.0.0"
check "a short SHA" rejected "uses: acme/tagged@${c1:0:12} # v1.0.0"
check "a comment naming no tag or branch" rejected "uses: acme/tagged@$c1 # v9"
check "an unreadable repository" rejected "uses: acme/missing@$c1 # v1.0.0"
check "one bad pin among good ones" rejected "uses: acme/tagged@$c1 # v1.0.0" "uses: acme/tagged@$c1 # v2.0.0"

# action.yml is checked too.
check "setup for action.yml" ok "uses: acme/tagged@$c1 # v1.0.0"
printf 'runs:\n  using: composite\n  steps:\n    - uses: acme/tagged@%s # v1.1.0\n' "$c1" > "$root/action.yml"
want=rejected
name="a bad pin in action.yml"
verdict

echo "[test-check-action-pins] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
