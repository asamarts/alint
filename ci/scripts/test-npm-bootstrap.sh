#!/usr/bin/env bash
# Behavioural test for ci/scripts/npm-bootstrap-platform-packages.sh, the
# owner's one-time npm bootstrap, against a stub `npm` that records its argv:
# placeholders are published (as 0.0.0) only for packages that do not exist, a
# placeholder is deprecated once the registry serves it (a new package answers
# 404 for a while after its publish is accepted, as npmjs.org does), an earlier
# run's undeprecated placeholder is deprecated on a re-run, Trusted Publishers
# are added only where missing, each step runs for every package before the
# next, --dry-run changes nothing, and a logged-out or too-old npm stops before
# any change. Like npm with 2FA on, the stub refuses publish, deprecate and trust
# (EOTP) unless stdin and stdout are a terminal, and refuses a second Trusted
# Publisher; the script runs on a pseudo-terminal, as the owner runs it.
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
# Registry model: NPM_EXISTING packages exist with a deprecated 0.0.0 placeholder,
# except NPM_UNDEPRECATED ones (placeholder not deprecated) and NPM_NO_PLACEHOLDER
# ones (no 0.0.0). A package published in the run stays invisible for
# NPM_VISIBLE_AFTER `npm view` polls, and until then deprecate/trust answer 404.
cat >"$sandbox/bin/npm" <<'STUB'
#!/usr/bin/env bash
echo "npm $*" >> "$STUB_LOG"
listed() { [[ " ${!1:-} " == *" $2 "* ]]; }
key() { echo "$STUB_STATE/${1//\//_}"; }
visible() { # <pkg>: exists before the run, or published and processed
  listed NPM_EXISTING "$1" && return 0
  [[ -e "$(key "$1").published" && ! -s "$(key "$1").published" ]]
}
not_found() { echo "npm error code E404" >&2; exit 1; }
case "$1 ${2:-}" in
  "publish "* | "deprecate "* | "trust list" | "trust github")
    if [[ ! -t 0 || ! -t 1 ]]; then
      echo "EOTP $1 ${2:-}" >> "$STUB_LOG"
      echo '{"error":{"code":"EOTP","summary":"This operation requires a one-time password."}}'
      exit 1
    fi ;;
esac
case "$1 ${2:-}" in
  "--version "*) echo 11.19.0 ;;
  "trust --help") [[ -z "${NPM_NO_TRUST:-}" ]] || exit 1 ;;
  "whoami "*) [[ -z "${NPM_LOGGED_OUT:-}" ]] || exit 1; echo someone ;;
  "view "*)
    pkg=${2%@0.0.0}
    if [[ -s "$(key "$pkg").published" ]]; then # still processing: one poll fewer to go
      left=$(($(cat "$(key "$pkg").published") - 1))
      if ((left > 0)); then echo "$left" >"$(key "$pkg").published"; else : >"$(key "$pkg").published"; fi
    fi
    visible "$pkg" || not_found
    case "$3" in
      name) echo "$pkg" ;;
      version)
        if [[ "$2" == *@0.0.0 ]] && ! listed NPM_NO_PLACEHOLDER "$pkg"; then echo 0.0.0; fi ;;
      deprecated)
        if [[ -e "$(key "$pkg").deprecated" ]] ||
           { listed NPM_EXISTING "$pkg" && ! listed NPM_UNDEPRECATED "$pkg" && ! listed NPM_NO_PLACEHOLDER "$pkg"; }; then
          echo "Placeholder"
        fi ;;
    esac ;;
  "deprecate "*)
    visible "${2%@0.0.0}" || not_found
    touch "$(key "${2%@0.0.0}").deprecated" ;;
  "trust list")
    if [[ " $* " == *" --json "* ]]; then # npm would hold the 2FA URL back for the JSON
      echo "2FA URL hidden by --json" >> "$STUB_LOG"
      exit 1
    fi
    visible "$3" || not_found
    if listed NPM_TRUSTED "$3" || [[ -e "$(key "$3").trusted" ]]; then
      printf 'type: github\nid: 1\nfile: release.yml\nrepository: asamarts/alint\npermissions: publish\n'
    else
      echo "No trust configurations found for package ($3)"
    fi ;;
  "trust github")
    visible "$3" || not_found
    if listed NPM_TRUSTED "$3" || [[ -e "$(key "$3").trusted" ]]; then
      echo "npm error a trust configuration already exists for $3" >&2
      exit 1
    fi
    touch "$(key "$3").trusted" ;;
  "publish "*)
    pkg=$(node -p 'require("./package.json").name')
    echo "${NPM_VISIBLE_AFTER:-0}" >"$(key "$pkg").published"
    [[ "${NPM_VISIBLE_AFTER:-0}" -gt 0 ]] || : >"$(key "$pkg").published"
    echo "published $(node -p 'const p = require("./package.json"); p.name + "@" + p.version + " " + p.os + "/" + p.cpu')" >> "$STUB_LOG" ;;
esac
exit 0
STUB
chmod +x "$sandbox/bin/npm"

ON_PTY='import pty, sys; sys.exit(pty.spawn(sys.argv[1:]) >> 8)'
pass=0
fail=0
mapfile -t ALL < <(node -p 'require("./npm/platforms.json").packages.map((p) => p.name).join("\n")')
N=${#ALL[@]}

run() {
  : >"$sandbox/log"
  rm -rf "$sandbox/state"
  mkdir "$sandbox/state"
  rc=0
  env PATH="$sandbox/bin:$PATH" STUB_LOG="$sandbox/log" STUB_STATE="$sandbox/state" \
    NPM_BOOTSTRAP_POLL_SECONDS=0 "$@" python3 -c "$ON_PTY" bash "$SCRIPT" ${DRY:+--dry-run} \
    </dev/null >"$sandbox/out" 2>&1 || rc=$?
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
check "fresh: every command that needs 2FA runs on a terminal, where npm shows its 2FA URL" \
  bash -c '! grep -qE "^(EOTP|2FA URL hidden)" <<<"$0"' "$log"
check "fresh: publishes every placeholder before any deprecate, and deprecates all before any trust" \
  bash -c 'grep -E "^(published|npm deprecate|npm trust github)" <<<"$0" | cut -d" " -f1-3 | sed -E "s/ @.*//" | uniq | tr "\n" " " | grep -qx "published npm deprecate npm trust github "' "$log"

DRY='' run NPM_EXISTING="${ALL[*]}" NPM_TRUSTED="${ALL[*]}"
check "done: re-running changes nothing" bash -c '[[ $0 -eq 0 ]] && ! grep -qE "^(published|npm deprecate|npm trust github)" <<<"$1"' "$rc" "$log"

DRY='' run NPM_EXISTING="${ALL[*]}" NPM_TRUSTED="${ALL[0]}"
check "existing but untrusted: adds only the missing Trusted Publishers" \
  bash -c '[[ $0 -eq 0 ]] && ! grep -q "^published" <<<"$1" && [[ $(grep -c "^npm trust github" <<<"$1") -eq $2 ]]' "$rc" "$log" "$((N - 1))"

DRY='' run NPM_VISIBLE_AFTER=3
check "slow registry: waits until each placeholder is served, then deprecates and trusts it" \
  bash -c '[[ $0 -eq 0 ]] && [[ $(grep -c "^npm deprecate " <<<"$1") -eq $2 && $(grep -c "^npm trust github " <<<"$1") -eq $2 ]]' \
  "$rc" "$log" "$N"
check "slow registry: polls each new package until it is served" \
  test "$(count "^npm view ${ALL[0]} name ")" -eq 4
check "slow registry: says it is waiting" grep -q "waiting for the registry" "$sandbox/out"

DRY='' run NPM_VISIBLE_AFTER=1000 NPM_BOOTSTRAP_POLL_TRIES=3
check "registry never serves it: stops after the poll limit with a re-run hint, deprecating nothing" \
  bash -c '[[ $0 -ne 0 ]] && ! grep -q "^npm deprecate" <<<"$1" && grep -q "re-run this script later" "$2"' \
  "$rc" "$log" "$sandbox/out"

DRY='' run NPM_EXISTING="${ALL[*]}" NPM_UNDEPRECATED="${ALL[0]}" NPM_TRUSTED="${ALL[*]}"
check "earlier run stopped before deprecating: deprecates that placeholder, publishes nothing" \
  bash -c '[[ $0 -eq 0 ]] && ! grep -qE "^(published|npm trust github)" <<<"$1" && [[ $(grep -c "^npm deprecate $2@0\.0\.0 " <<<"$1") -eq 1 && $(grep -c "^npm deprecate" <<<"$1") -eq 1 ]]' \
  "$rc" "$log" "${ALL[0]}"

DRY='' run NPM_EXISTING="${ALL[0]} ${ALL[1]}" NPM_UNDEPRECATED="${ALL[1]}" NPM_TRUSTED="${ALL[0]}" NPM_VISIBLE_AFTER=2
check "resume (first package done, second published only): finishes the rest and leaves the first alone" \
  bash -c '[[ $0 -eq 0 ]] && [[ $(grep -c "^published" <<<"$1") -eq $(($2 - 2)) && $(grep -c "^npm deprecate" <<<"$1") -eq $(($2 - 1)) && $(grep -c "^npm trust github" <<<"$1") -eq $(($2 - 1)) ]] && ! grep -qE "^npm (deprecate|trust github) $3( |@)" <<<"$1"' \
  "$rc" "$log" "$N" "${ALL[0]}"

DRY='' run NPM_EXISTING="${ALL[*]}" NPM_NO_PLACEHOLDER="${ALL[*]}" NPM_TRUSTED="${ALL[*]}"
check "package without a 0.0.0 placeholder: deprecates nothing" \
  bash -c '[[ $0 -eq 0 ]] && ! grep -q "^npm deprecate" <<<"$1"' "$rc" "$log"

DRY=1 run
check "--dry-run: prints the changes but makes none" \
  bash -c '[[ $0 -eq 0 ]] && ! grep -qE "^(published|npm deprecate|npm trust github)" <<<"$1" && grep -q "^+ npm trust github" "$2"' \
  "$rc" "$log" "$sandbox/out"
check "--dry-run: prints a deprecate for each placeholder it would publish, without waiting" \
  bash -c '[[ $(grep -c "^+ npm deprecate " "$0") -eq $1 && $(grep -c "^npm view .* name " <<<"$2") -eq $1 ]]' \
  "$sandbox/out" "$N" "$log"

DRY='' run NPM_LOGGED_OUT=1
check "logged out: stops before any change" bash -c '[[ $0 -ne 0 ]] && ! grep -q "^published" <<<"$1"' "$rc" "$log"
DRY='' run NPM_NO_TRUST=1
check "npm without 'npm trust': stops before any change" bash -c '[[ $0 -ne 0 ]] && ! grep -q "^published" <<<"$1"' "$rc" "$log"

echo "[test-npm-bootstrap] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
