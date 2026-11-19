#!/usr/bin/env bash
# Pin check-docs-overlay-since.py against a fixture repository with a release
# tag and a docs-bundle.yml that overlays reference and concept pages:
#   - since-blocks of any parsable version, blank lines, the Action pin, counts
#     the tag's facts.json renders, the main-tracked ROADMAP and a new rule
#     kind's section all pass;
#   - an unwrapped added or changed line, a fenced block outside a block, a
#     counts-page prose change, a released kind's section or the fix-operations
#     section in docs/rules.md, and a marker whose version does not parse fail,
#     naming the file and line;
#   - no reachable tag, or a workflow without the strip-since command, exits 2.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CHECKER="$REPO_ROOT/ci/scripts/check-docs-overlay-since.py"
PINNER="$REPO_ROOT/ci/scripts/pin-docs-counts.py"

sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
pass=0
fail=0
ok() { echo "  ok: $1"; pass=$((pass + 1)); }
bad() { echo "  FAIL: $1" >&2; fail=$((fail + 1)); }

repo="$sandbox/repo"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
g() {
  git -C "$repo" -c user.name=test -c user.email=test@example.invalid \
    -c commit.gpgsign=false -c tag.gpgsign=false -c core.hooksPath=/dev/null "$@"
}
put() { mkdir -p "$(dirname "$repo/$1")"; printf '%s' "$2" > "$repo/$1"; }
add() { printf '%s' "$2" >> "$repo/$1"; }
run() { python3 "$CHECKER" --repo "$repo" > "$sandbox/out" 2>&1; }
pristine() { g reset -q --hard; g clean -qfd; }

# expect_fail <label> <file:line the output must name>
expect_fail() {
  local status=0
  run || status=$?
  if [[ "$status" -ne 1 ]]; then
    bad "$1: exit $status, expected 1"
    cat "$sandbox/out" >&2
  elif ! grep -Fq "  $2:" "$sandbox/out"; then
    bad "$1: output does not name $2"
    cat "$sandbox/out" >&2
  else
    ok "$1"
  fi
  pristine
}

git init -q "$repo"
put .github/workflows/docs-bundle.yml 'jobs:
  bundle:
    steps:
      - run: |
          node ci/scripts/strip-since.mjs --released-version "${BUNDLE_SOURCE#v}" \
            docs/site/reference \
            docs/site/concepts \
            docs/design/ROADMAP.md
          python3 ci/scripts/pin-docs-counts.py facts.json \
            docs/site/concepts/start-here/kinds-families-categories.md
'
put docs/site/reference/formats.md '# Formats

Released line.

- uses: asamarts/alint@8d7a60f6642cb1b59537202ae81df7d951a078ad # v0.1.0
'
put docs/site/concepts/page.md '# Page

A released paragraph.
'
put docs/design/ROADMAP.md '# Roadmap
'
put docs/rules.md '# Rules

## Existence

### `file_exists`

Released prose.

## Fix operations

Released fix prose.

## Bundled rulesets

### `alint://bundled/x@v1`

Ruleset prose.
'
mkdir -p "$repo/docs/site/concepts/start-here"
cp "$REPO_ROOT/facts.json" "$repo/facts.json"
cp "$REPO_ROOT/docs/site/concepts/start-here/kinds-families-categories.md" \
  "$repo/docs/site/concepts/start-here/kinds-families-categories.md"
g add -A
g commit -q -m release
g tag v0.1.0

# Main after the release: everything below is marked or exempt.
add docs/site/reference/formats.md '
<!-- alint:since=0.2.0 -->
An unreleased line.

```yaml
inside: a fence
```
<!-- /alint:since -->

<!-- alint:since=0.1 -->
A correction of released behaviour.
<!-- /alint:since -->
'
sed -i 's|8d7a60f6642cb1b59537202ae81df7d951a078ad # v0.1.0|5054852d00d47affc9ea71440240147d9999f4d3 # v0.1.1|' \
  "$repo/docs/site/reference/formats.md"
add docs/design/ROADMAP.md '
## v0.2: planned work, tracking main
'
add docs/rules.md '
## Cross-file

### `brand_new_kind`

A new kind is withheld from the site with its page.

### `file_absent`

Not released either: the tag has no section for it.
'
sed -i 's|^Ruleset prose.$|Ruleset prose, edited.|' "$repo/docs/rules.md"
sed -i 's|^Released prose.$|Released prose.\n\n<!-- alint:since=0.2 -->\nNew prose.\n<!-- /alint:since -->|' \
  "$repo/docs/rules.md"
# A new kind on main moves every count; the tag's facts.json renders them back.
python3 - "$repo/facts.json" "$sandbox/facts-main.json" <<'PY'
import json
import sys

facts = json.load(open(sys.argv[1], encoding="utf-8"))
facts["rule_kinds"].append("zz_new_kind")
facts["rule_categories"]["zz_new_kind"] = ["existence"]
facts["counts"]["rule_kinds"] += 1
json.dump(facts, open(sys.argv[2], "w", encoding="utf-8"))
PY
python3 "$PINNER" "$sandbox/facts-main.json" \
  "$repo/docs/site/concepts/start-here/kinds-families-categories.md" > /dev/null
if g diff --quiet v0.1.0 -- docs/site/concepts/start-here/kinds-families-categories.md; then
  bad "fixture: the count page did not change"
fi
g add -A
g commit -q -m main

if run; then ok "marked, exempt and withheld lines pass"; else
  bad "marked, exempt and withheld lines pass"
  cat "$sandbox/out" >&2
fi

add docs/site/reference/formats.md '
An unwrapped line.
'
expect_fail "an unwrapped added line fails" "docs/site/reference/formats.md:19"

sed -i 's|^A released paragraph.$|A released paragraph, now saying more.|' "$repo/docs/site/concepts/page.md"
expect_fail "a changed line fails" "docs/site/concepts/page.md:3"

add docs/site/concepts/page.md '
```yaml
new: example
```
'
expect_fail "a fenced block outside a since-block fails" "docs/site/concepts/page.md:6"

sed -i 's|^Every rule names a `kind`|Every rule, new or old, names a `kind`|' \
  "$repo/docs/site/concepts/start-here/kinds-families-categories.md"
line=$(grep -n '^Every rule, new or old' "$repo/docs/site/concepts/start-here/kinds-families-categories.md" | cut -d: -f1)
expect_fail "prose on the count page fails" \
  "docs/site/concepts/start-here/kinds-families-categories.md:$line"

sed -i 's|^Released prose.$|Released prose, rewritten.|' "$repo/docs/rules.md"
expect_fail "a released kind's section in docs/rules.md fails" "docs/rules.md:7"

sed -i 's|^Released fix prose.$|Released fix prose, rewritten.|' "$repo/docs/rules.md"
line=$(grep -n '^Released fix prose, rewritten' "$repo/docs/rules.md" | cut -d: -f1)
expect_fail "the fix-operations section in docs/rules.md fails" "docs/rules.md:$line"

sed -i 's|<!-- alint:since=0.2.0 -->|<!-- alint:since=O.2 -->|' "$repo/docs/site/reference/formats.md"
expect_fail "a marker whose version does not parse fails" "docs/site/reference/formats.md:7"

# Setup errors exit 2.
status=0
python3 "$CHECKER" --repo "$repo" --base no-such-ref > "$sandbox/out" 2>&1 || status=$?
if [[ "$status" -eq 2 ]] && grep -Fq 'no-such-ref is not a commit' "$sandbox/out"; then
  ok "a --base that is not a commit exits 2"
else
  bad "a --base that is not a commit: exit $status"
  cat "$sandbox/out" >&2
fi
g tag -d v0.1.0 > /dev/null
status_untagged=0
python3 "$CHECKER" --repo "$repo" > "$sandbox/out" 2>&1 || status_untagged=$?
if [[ "$status_untagged" -eq 2 ]] && grep -Fq 'no v* release tag' "$sandbox/out"; then
  ok "no reachable tag exits 2"
else
  bad "no reachable tag: exit $status_untagged"
  cat "$sandbox/out" >&2
fi
g tag v0.1.0 HEAD~1
sed -i 's|node ci/scripts/strip-since.mjs|node ci/scripts/other.mjs|' "$repo/.github/workflows/docs-bundle.yml"
status=0
run || status=$?
if [[ "$status" -eq 2 ]] && grep -Fq 'expected one strip-since.mjs command' "$sandbox/out"; then
  ok "a workflow without the strip-since command exits 2"
else
  bad "a workflow without the strip-since command: exit $status"
  cat "$sandbox/out" >&2
fi
pristine

echo "[check-docs-overlay-since] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
