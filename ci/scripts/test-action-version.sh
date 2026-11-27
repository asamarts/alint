#!/usr/bin/env bash
# Regression tests for the GitHub Action's binary-version precedence contract.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

# Resolve the sourced helper from this script's location so `shellcheck -x`
# follows it from any cwd; SC1091 (info: "not specified as input") only fires
# for a plain `shellcheck` run that was not asked to follow sources.
# shellcheck source-path=SCRIPTDIR/../.. source=action/resolve-version.sh disable=SC1091
source action/resolve-version.sh

workspace_version=$(awk -F'"' '
  /^\[workspace\.package\]/ { in_workspace=1; next }
  in_workspace && /^version =/ { print $2; exit }
' Cargo.toml)
baked_version=$(awk '
  /^[[:space:]]*ALINT_BAKED_VERSION:/ {
    value=$0
    sub(/^[[:space:]]*ALINT_BAKED_VERSION:[[:space:]]*/, "", value)
    gsub(/"/, "", value)
    print value
    exit
  }
' action.yml)

pass=0
fail=0

expect_version() {
  local name=$1
  local expected=$2
  shift 2
  local actual
  if actual=$(resolve_alint_action_version "$@"); then
    if [[ "$actual" == "$expected" ]]; then
      echo "  ok: $name"
      pass=$((pass + 1))
      return
    fi
    echo "  FAIL: $name (expected '$expected', got '$actual')" >&2
  else
    echo "  FAIL: $name (resolver rejected valid inputs)" >&2
  fi
  fail=$((fail + 1))
}

expect_rejected() {
  local name=$1
  shift
  if resolve_alint_action_version "$@" >/dev/null 2>&1; then
    echo "  FAIL: $name (expected rejection)" >&2
    fail=$((fail + 1))
  else
    echo "  ok: $name"
    pass=$((pass + 1))
  fi
}

if [[ "$baked_version" == "v${workspace_version}" ]]; then
  echo "  ok: action.yml bakes the workspace release"
  pass=$((pass + 1))
else
  echo "  FAIL: action.yml bakes '${baked_version:-<empty>}'; workspace is v${workspace_version}" >&2
  fail=$((fail + 1))
fi

sha=0123456789abcdef0123456789abcdef01234567
expect_version "SHA pin selects baked release" "$baked_version" "" "$sha" "$baked_version"
expect_version "exact tag selects itself" v9.8.7 "" v9.8.7 "$baked_version"
expect_version "pre-release tag selects itself" v1.2.3-rc.1 "" v1.2.3-rc.1 "$baked_version"
expect_version "main branch follows latest" latest "" main "$baked_version"
expect_version "major channel follows latest" latest "" v0 "$baked_version"
expect_version "local action follows latest" latest "" "" "$baked_version"
expect_version "explicit input overrides SHA" v1.2.3 v1.2.3 "$sha" "$baked_version"
expect_version "explicit input overrides exact tag" v1.2.3 v1.2.3 v9.8.7 "$baked_version"
expect_version "explicit pre-release plus build is accepted" v1.2.3-rc.1+build.5 \
  v1.2.3-rc.1+build.5 main "$baked_version"
expect_version "explicit latest overrides SHA" latest latest "$sha" "$baked_version"
expect_version "SHA accepts a pre-release baked version" v1.2.3-rc.1 \
  "" "$sha" v1.2.3-rc.1
expect_rejected "explicit branch name is rejected" main main "$sha" "$baked_version"
expect_rejected "explicit incomplete tag is rejected" v1.2 v1.2 "$sha" "$baked_version"
expect_rejected "explicit path-like tag is rejected" v1.2.3/evil \
  v1.2.3/evil "$sha" "$baked_version"
expect_rejected "SHA pin rejects a missing baked release" "" "$sha" ""
expect_rejected "SHA pin rejects a malformed baked release" "" "$sha" next

# Behavioural check of the Install step's install.sh source. Extract the step's
# `run:` script from action.yml and execute it in a sandbox whose `curl` and
# installer are stubs: BOTH local (`uses: ./`) and remote (`uses: org/alint@ref`)
# invocations must run the install.sh bundled beside action.yml in $ACTION_PATH
# and NEVER re-fetch install.sh from raw.githubusercontent.com (M4).
sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
# Plain-text extraction (no PyYAML dependency on the runner): the `run: |`
# block scalar of the `- name: Install alint` step, dedented.
python3 - "$sandbox/install-step.sh" <<'PY'
import re
import sys

lines = open('action.yml', encoding='utf-8').read().splitlines()
start = next(i for i, l in enumerate(lines) if re.match(r'^\s*- name: Install alint\s*$', l))
run = next(i for i in range(start + 1, len(lines)) if re.match(r'^\s*run: \|\s*$', lines[i]))
key_indent = len(lines[run]) - len(lines[run].lstrip())
body = []
for line in lines[run + 1:]:
    if line.strip() and len(line) - len(line.lstrip()) <= key_indent:
        break
    body.append(line)
indent = min(len(l) - len(l.lstrip()) for l in body if l.strip())
open(sys.argv[1], 'w', encoding='utf-8').write('\n'.join(l[indent:] for l in body) + '\n')
PY
mkdir -p "$sandbox/action-path/action" "$sandbox/bin" "$sandbox/tmp"
cp action/resolve-version.sh "$sandbox/action-path/action/"
printf '#!/usr/bin/env bash\necho BUNDLED-INSTALLER\n' > "$sandbox/action-path/install.sh"
cat > "$sandbox/bin/curl" <<'STUB'
#!/usr/bin/env bash
echo "curl $*" >> "$CURL_LOG"
out=""
while [[ $# -gt 0 ]]; do
  if [[ "$1" == "-o" ]]; then out="$2"; shift; fi
  shift
done
printf '#!/usr/bin/env bash\necho FETCHED-INSTALLER\n' > "$out"
STUB
chmod +x "$sandbox/bin/curl"

run_install_step() {
  local action_ref=$1
  : > "$sandbox/curl.log"
  env PATH="$sandbox/bin:$PATH" CURL_LOG="$sandbox/curl.log" \
    ALINT_VERSION="" ALINT_BAKED_VERSION="$baked_version" \
    INSTALL_DIR="$sandbox/install" ACTION_REF="$action_ref" \
    ACTION_PATH="$sandbox/action-path" SOURCE_REPO=asamarts/alint \
    RUNNER_TEMP="$sandbox/tmp" GITHUB_PATH="$sandbox/github-path" \
    bash "$sandbox/install-step.sh" 2>&1
}

local_out=$(run_install_step "")
if grep -q BUNDLED-INSTALLER <<< "$local_out" && [[ ! -s "$sandbox/curl.log" ]]; then
  echo "  ok: local action runs the bundled install.sh (no network fetch)"
  pass=$((pass + 1))
else
  echo "  FAIL: local action did not run the bundled install.sh" >&2
  echo "$local_out" >&2
  cat "$sandbox/curl.log" >&2
  fail=$((fail + 1))
fi

remote_out=$(run_install_step v9.8.7)
if grep -q BUNDLED-INSTALLER <<< "$remote_out" && [[ ! -s "$sandbox/curl.log" ]]; then
  echo "  ok: remote action runs the bundled install.sh (no raw.githubusercontent.com fetch)"
  pass=$((pass + 1))
else
  echo "  FAIL: remote action did not run the bundled install.sh without network fetch" >&2
  echo "$remote_out" >&2
  cat "$sandbox/curl.log" >&2
  fail=$((fail + 1))
fi

if grep -q 'raw\.githubusercontent\.com' action.yml; then
  echo "  FAIL: action.yml still references raw.githubusercontent.com" >&2
  fail=$((fail + 1))
else
  echo "  ok: action.yml does not reference raw.githubusercontent.com"
  pass=$((pass + 1))
fi

echo "[test-action-version] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
