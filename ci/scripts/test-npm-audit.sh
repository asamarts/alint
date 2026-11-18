#!/usr/bin/env bash
# Pin the npm advisory policy of ci/scripts/npm-audit.sh:
#   - an advisory at moderate or above that is not waived FAILS, naming it;
#   - a waived one passes, and a low one needs no waiver;
#   - a waiver lapses once `npm audit fix` can fix its advisory; a fix that
#     changes a major version (perhaps a downgrade) only warns;
#   - a waiver needs a GHSA id and a reason;
#   - the runtime dependencies are audited with no waivers: their failure
#     fails the gate whatever the waiver list says;
#   - `npm run audit` in editors/vscode runs this gate.
# Behavioural: saved reports go through --check-json, and the full run goes
# through a stubbed `npm` on PATH. The real audit needs the network, so CI's
# Editors job is its end-to-end check.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
GATE="$REPO_ROOT/ci/scripts/npm-audit.sh"
sandbox="$(mktemp -d)"
trap 'rm -rf -- "$sandbox"' EXIT
pass=0
fail=0
ok() { echo "  ok: $1"; pass=$((pass + 1)); }
bad() { echo "  FAIL: $1" >&2; sed 's/^/      | /' "$sandbox/out" >&2; fail=$((fail + 1)); }

# A report holding one advisory of `severity` with id `ghsa`, reached through
# one package, as `npm audit --json` writes it; `fix` is the advisory's
# package's `fixAvailable` (false when not given).
report() {
  local ghsa="$1" severity="$2" fix="${3:-false}"
  cat <<JSON
{"auditReportVersion": 2, "vulnerabilities": {
  "picomatch": {"name": "picomatch", "severity": "$severity", "fixAvailable": $fix, "via": [
    {"source": 1, "name": "picomatch", "title": "a ReDoS",
     "url": "https://github.com/advisories/$ghsa", "severity": "$severity", "range": "<=4.0.7"}]},
  "tinyglobby": {"name": "tinyglobby", "severity": "$severity", "via": ["picomatch"]}
}}
JSON
}

waivers() {
  printf '{"waivers": [%s]}\n' "$1" > "$sandbox/waivers.json"
}

expect() {
  local name="$1" want="$2" pattern="$3"
  shift 3
  local status=0
  NPM_AUDIT_WAIVERS="$sandbox/waivers.json" "$@" > "$sandbox/out" 2>&1 || status=$?
  if [[ "$want" == pass && "$status" -ne 0 ]] || [[ "$want" == fail && "$status" -eq 0 ]]; then
    bad "$name: expected $want, exit $status"
  elif [[ -n "$pattern" ]] && ! grep -q -- "$pattern" "$sandbox/out"; then
    bad "$name: output lacks '$pattern'"
  else
    ok "$name"
  fi
}

waived='{"id": "GHSA-fc5q-39xc-vjpf", "reason": "no fixed version; packaging tools only"}'

report GHSA-aaaa-bbbb-cccc high > "$sandbox/unwaived.json"
waivers "$waived"
expect "an unwaived high advisory fails, naming it" fail "GHSA-aaaa-bbbb-cccc" \
  "$GATE" --check-json "$sandbox/unwaived.json"

report GHSA-fc5q-39xc-vjpf high > "$sandbox/waived.json"
expect "a waived advisory passes" pass "waived GHSA-fc5q-39xc-vjpf" \
  "$GATE" --check-json "$sandbox/waived.json"

report GHSA-aaaa-bbbb-cccc moderate > "$sandbox/moderate.json"
expect "a moderate advisory needs a waiver" fail "GHSA-aaaa-bbbb-cccc" \
  "$GATE" --check-json "$sandbox/moderate.json"

report GHSA-aaaa-bbbb-cccc low > "$sandbox/low.json"
expect "a low advisory needs no waiver" pass "ok: 0" \
  "$GATE" --check-json "$sandbox/low.json"

waivers '{"id": "GHSA-fc5q-39xc-vjpf", "reason": " "}'
expect "a waiver without a reason is refused" fail "rejected" \
  "$GATE" --check-json "$sandbox/waived.json"

waivers '{"id": "picomatch", "reason": "a package is not an advisory"}'
expect "a waiver must name a GHSA id" fail "rejected" \
  "$GATE" --check-json "$sandbox/waived.json"

waivers "$waived"
echo '{"error": "no lockfile"}' > "$sandbox/broken.json"
expect "a report that is not an audit report fails" fail "not an" \
  "$GATE" --check-json "$sandbox/broken.json"

report GHSA-fc5q-39xc-vjpf critical true > "$sandbox/fixable.json"
expect "a waiver lapses once npm audit fix can fix its advisory" fail "can fix it now" \
  "$GATE" --check-json "$sandbox/fixable.json"

# What npm reported for the waiver when it was written: a fix that downgrades
# @vscode/vsce to a release from before it depended on picomatch.
report GHSA-fc5q-39xc-vjpf high \
  '{"name": "@vscode/vsce", "version": "3.9.2", "isSemVerMajor": true}' > "$sandbox/major.json"
expect "a fix that changes a major version only warns" pass "a new major version" \
  "$GATE" --check-json "$sandbox/major.json"

waivers "$waived, {\"id\": \"GHSA-dddd-eeee-ffff\", \"reason\": \"fixed since\"}"
expect "a waiver for an advisory no longer reported warns" pass "no longer reported" \
  "$GATE" --check-json "$sandbox/waived.json"

# The full run, with a stub npm: `--omit=dev` is the runtime audit, which the
# STUB_RUNTIME_RC status fails; the full report is STUB_REPORT.
mkdir -p "$sandbox/bin"
cat > "$sandbox/bin/npm" <<'STUB'
#!/usr/bin/env bash
case " $* " in
  *" --omit=dev "*) exit "${STUB_RUNTIME_RC:-0}" ;;
  *" --json "*) cat "$STUB_REPORT"; exit 1 ;;
esac
exit 2
STUB
chmod +x "$sandbox/bin/npm"
waivers "$waived"
expect "the full run passes on waived development advisories" pass "all waived" \
  env PATH="$sandbox/bin:$PATH" STUB_REPORT="$sandbox/waived.json" "$GATE"
expect "a runtime advisory fails whatever is waived" fail "" \
  env PATH="$sandbox/bin:$PATH" STUB_RUNTIME_RC=1 STUB_REPORT="$sandbox/waived.json" "$GATE"
expect "the full run fails on an unwaived development advisory" fail "GHSA-aaaa-bbbb-cccc" \
  env PATH="$sandbox/bin:$PATH" STUB_REPORT="$sandbox/unwaived.json" "$GATE"

# `npm run audit`, which CI and the release run, is this gate.
script=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["scripts"]["audit"])' \
  "$REPO_ROOT/editors/vscode/package.json")
if [[ "$script" == "bash ../../ci/scripts/npm-audit.sh" ]]; then
  ok "npm run audit runs the gate"
else
  echo "$script" > "$sandbox/out"
  bad "npm run audit runs the gate"
fi

# The committed waivers are well formed.
if NPM_AUDIT_WAIVERS="$REPO_ROOT/editors/vscode/npm-audit-waivers.json" \
  "$GATE" --check-json "$sandbox/waived.json" > "$sandbox/out" 2>&1; then
  ok "the committed waiver list loads"
else
  bad "the committed waiver list loads"
fi

echo "[test-npm-audit] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
