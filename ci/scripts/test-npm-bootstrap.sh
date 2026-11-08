#!/usr/bin/env bash
# Behavioural test for ci/scripts/npm-bootstrap-platform-packages.sh, the
# owner's one-time npm bootstrap, against a stub `npm` that records its argv:
# placeholders are published (as 0.0.0) and deprecated only for packages that do
# not exist, Trusted Publishers are added only where missing, --dry-run changes
# nothing, and a logged-out or too-old npm stops before any change.
#
# Stub bodies are single-quoted on purpose: they expand when the stub runs.
# shellcheck disable=SC2016
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"
SCRIPT=ci/scripts/npm-bootstrap-platform-packages.sh

if ! command -v node >/dev/null 2>&1; then
  if [[ "${CI:-}" == "true" ]]; then
    echo "[test-npm-bootstrap] node is required in CI" >&2
    exit 1
  fi
  echo "[test-npm-bootstrap] skip: node not installed"
  exit 0
fi

sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
mkdir -p "$sandbox/bin"
cat >"$sandbox/bin/npm" <<'STUB'
#!/usr/bin/env bash
echo "npm $*" >> "$STUB_LOG"
case "$1 ${2:-}" in
  "--version "*) echo 11.19.0 ;;
  "trust --help") [[ -z "${NPM_NO_TRUST:-}" ]] || exit 1 ;;
  "whoami "*) [[ -z "${NPM_LOGGED_OUT:-}" ]] || exit 1; echo someone ;;
  "view "*) [[ " ${NPM_EXISTING:-} " == *" $2 "* ]] && echo "$2" || exit 1 ;;
  "trust list")
    if [[ " ${NPM_TRUSTED:-} " == *" $3 "* ]]; then
      echo '[{"type":"github","repository":"asamarts/alint","file":"release.yml"}]'
    else
      echo '[]'
    fi ;;
  "publish "*) echo "published $(node -p 'const p = require("./package.json"); p.name + "@" + p.version + " " + p.os + "/" + p.cpu')" >> "$STUB_LOG" ;;
esac
exit 0
STUB
chmod +x "$sandbox/bin/npm"

pass=0
fail=0
mapfile -t ALL < <(node -p 'require("./npm/platforms.json").packages.map((p) => p.name).join("\n")')
N=${#ALL[@]}

run() {
  : >"$sandbox/log"
  rc=0
  env PATH="$sandbox/bin:$PATH" STUB_LOG="$sandbox/log" "$@" bash "$SCRIPT" ${DRY:+--dry-run} >"$sandbox/out" 2>&1 || rc=$?
  log=$(cat "$sandbox/log")
}
count() { grep -cE -- "$1" <<<"$log" || true; }
check() { # <label> <condition...>
  local label=$1
  shift
  if "$@"; then
    echo "  ok: $label"
    pass=$((pass + 1))
  else
    echo "  FAIL: $label" >&2
    sed 's/^/      out| /' "$sandbox/out" >&2
    printf '%s\n' "$log" | sed 's/^/      log| /' >&2
    fail=$((fail + 1))
  fi
}

DRY='' run
check "fresh: exits 0" test "$rc" -eq 0
check "fresh: publishes a 0.0.0 placeholder of each package with its os/cpu" \
  test "$(count '^published @asamarts/alint-[a-z0-9]+-[a-z0-9]+@0\.0\.0 (linux|darwin|win32)/(x64|arm64)$')" -eq "$N"
check "fresh: the win32-arm64 placeholder carries win32/arm64" \
  grep -qE '^published @asamarts/alint-win32-arm64@0\.0\.0 win32/arm64$' <<<"$log"
check "fresh: deprecates each placeholder" test "$(count '^npm deprecate @asamarts/alint-[a-z0-9-]+@0\.0\.0 ')" -eq "$N"
check "fresh: trusts release.yml of asamarts/alint for each package" \
  test "$(count '^npm trust github @asamarts/alint-[a-z0-9-]+ --file release\.yml --repo asamarts/alint --allow-publish --yes$')" -eq "$N"
check "fresh: never touches the shim package" bash -c '! grep -qE "(publish|deprecate|trust github) @asamarts/alint( |@|$)" <<<"$0"' "$log"

DRY='' run NPM_EXISTING="${ALL[*]}" NPM_TRUSTED="${ALL[*]}"
check "done: re-running changes nothing" bash -c '[[ $0 -eq 0 ]] && ! grep -qE "^(published|npm deprecate|npm trust github)" <<<"$1"' "$rc" "$log"

DRY='' run NPM_EXISTING="${ALL[*]}" NPM_TRUSTED="${ALL[0]}"
check "existing but untrusted: adds only the missing Trusted Publishers" \
  bash -c '[[ $0 -eq 0 ]] && ! grep -q "^published" <<<"$1" && [[ $(grep -c "^npm trust github" <<<"$1") -eq $2 ]]' "$rc" "$log" "$((N - 1))"

DRY=1 run
check "--dry-run: prints the changes but makes none" \
  bash -c '[[ $0 -eq 0 ]] && ! grep -qE "^(published|npm deprecate|npm trust github)" <<<"$1" && grep -q "^+ npm trust github" "$2"' \
  "$rc" "$log" "$sandbox/out"

DRY='' run NPM_LOGGED_OUT=1
check "logged out: stops before any change" bash -c '[[ $0 -ne 0 ]] && ! grep -q "^published" <<<"$1"' "$rc" "$log"
DRY='' run NPM_NO_TRUST=1
check "npm without 'npm trust': stops before any change" bash -c '[[ $0 -ne 0 ]] && ! grep -q "^published" <<<"$1"' "$rc" "$log"

echo "[test-npm-bootstrap] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
