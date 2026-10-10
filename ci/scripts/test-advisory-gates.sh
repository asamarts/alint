#!/usr/bin/env bash
# Pin the RustSec advisory policy of ci/scripts/deny.sh + ci/scripts/audit.sh:
#   - a vulnerability FAILS both gates (no `|| warn` swallow);
#   - unmaintained / unsound advisories are demoted to warnings (deny.sh
#     passes -W for exactly those two lints; cargo audit only fails on
#     vulnerabilities), yanked crates warn (deny.toml);
#   - waivers live in ONE place, deny.toml [advisories].ignore, which audit.sh
#     turns into `cargo audit --ignore <id>` (ids with a reason only);
#   - deny.sh runs the cargo-deny version its deny.toml schema is written for.
# Behavioural: both scripts run from a scratch repo root against a stubbed
# `cargo` that records argv and fails the advisory check on demand. The real
# end-to-end check (a crate with RUSTSEC-2021-0003 fails both, the waiver
# clears both) needs network + the tools, so it is a manual pre-change check.
#
# Stub bodies are single-quoted on purpose: they expand when the stub runs.
# shellcheck disable=SC2016
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
pass=0
fail=0
ok() { echo "  ok: $1"; pass=$((pass + 1)); }
bad() { echo "  FAIL: $1" >&2; sed 's/^/      | /' "$sandbox/out" "$sandbox/log" >&2; fail=$((fail + 1)); }

root="$sandbox/repo"
mkdir -p "$root/ci/scripts" "$sandbox/bin"
cp ci/scripts/deny.sh ci/scripts/audit.sh "$root/ci/scripts/"
pinned=$(sed -n 's/^CARGO_DENY_VERSION="\(.*\)"$/\1/p' ci/scripts/deny.sh)

cat > "$sandbox/bin/cargo" <<'STUB'
#!/usr/bin/env bash
echo "cargo $*" >> "$STUB_LOG"
case "$1 $2" in
  "deny --version") echo "cargo-deny ${STUB_DENY_VERSION:?}"; exit 0 ;;
  "deny check")
    for a in "$@"; do [[ "$a" == advisories ]] && exit "${STUB_ADVISORIES_RC:-0}"; done
    exit 0 ;;
  "audit "*|"audit") exit "${STUB_ADVISORIES_RC:-0}" ;;
esac
exit 0
STUB
printf '#!/usr/bin/env bash\nexit 0\n' > "$sandbox/bin/cargo-audit"
chmod +x "$sandbox/bin/cargo" "$sandbox/bin/cargo-audit"

# run <script> [VAR=value...] -> $rc
run() {
  local script=$1
  shift
  : > "$sandbox/log"
  rc=0
  env PATH="$sandbox/bin:$PATH" STUB_LOG="$sandbox/log" STUB_DENY_VERSION="$pinned" "$@" \
    bash "$root/ci/scripts/$script" > "$sandbox/out" 2>&1 || rc=$?
}
logged() { grep -qE -- "$1" "$sandbox/log"; }

cp deny.toml "$root/deny.toml"

# ── deny.sh ───────────────────────────────────────────────────────────
run deny.sh STUB_ADVISORIES_RC=1
if [[ "$rc" -ne 0 ]]; then ok "deny.sh: a vulnerability fails the gate"; else bad "deny.sh: a vulnerability must fail the gate"; fi
run deny.sh STUB_ADVISORIES_RC=0
if [[ "$rc" -eq 0 ]] && logged '^cargo deny check licenses bans sources$' &&
   logged '^cargo deny check -W unmaintained -W unsound advisories$'; then
  ok "deny.sh: clean graph passes; advisories demote exactly unmaintained + unsound"
else
  bad "deny.sh: expected 'cargo deny check -W unmaintained -W unsound advisories' (and the blocking license pass)"
fi
if logged '^cargo deny check .*(-[AW] *(vulnerability|yanked|notice)|--allow|-A )'; then
  bad "deny.sh: must not demote vulnerabilities (or anything beyond unmaintained/unsound)"
else
  ok "deny.sh: vulnerabilities are never demoted"
fi
run deny.sh STUB_DENY_VERSION=0.0.1
if logged "^cargo install cargo-deny --locked --version ${pinned//./\\.}$"; then
  ok "deny.sh: a different cargo-deny version gets the pinned ${pinned} installed"
else
  bad "deny.sh: must install cargo-deny ${pinned} when the runner has another version"
fi

# ── audit.sh ──────────────────────────────────────────────────────────
run audit.sh STUB_ADVISORIES_RC=1
if [[ "$rc" -ne 0 ]]; then ok "audit.sh: a vulnerability fails the gate"; else bad "audit.sh: a vulnerability must fail the gate"; fi
run audit.sh STUB_ADVISORIES_RC=0
if [[ "$rc" -eq 0 ]] && logged '^cargo audit$'; then ok "audit.sh: clean graph passes (no waivers)"; else bad "audit.sh: clean run"; fi

# The waiver list is read from deny.toml, not duplicated in audit.sh.
python3 - "$root/deny.toml" <<'PY'
import re, sys
p = sys.argv[1]
s = open(p, encoding='utf-8').read()
s = re.sub(r'^ignore = \[\]$', 'ignore = [\n    { id = "RUSTSEC-2021-0003", reason = "fixture" },\n    { id = "RUSTSEC-2024-0375", reason = "fixture" },\n]', s, flags=re.M)
open(p, 'w', encoding='utf-8').write(s)
PY
run audit.sh STUB_ADVISORIES_RC=0
if logged '^cargo audit --ignore RUSTSEC-2021-0003 --ignore RUSTSEC-2024-0375$'; then
  ok "audit.sh: honours deny.toml's waivers as --ignore flags"
else
  bad "audit.sh: deny.toml [advisories].ignore must become cargo audit --ignore flags"
fi
sed -i 's/reason = "fixture" },$/reason = "" },/' "$root/deny.toml"
run audit.sh STUB_ADVISORIES_RC=0
if [[ "$rc" -ne 0 ]] && ! logged '^cargo audit'; then ok "audit.sh: a waiver without a reason is rejected"; else bad "audit.sh: reasonless waiver accepted"; fi
cp deny.toml "$root/deny.toml"
sed -i 's/^ignore = \[\]$/ignore = [{ crate = "smallvec", reason = "x" }]/' "$root/deny.toml"
run audit.sh STUB_ADVISORIES_RC=0
if [[ "$rc" -ne 0 ]]; then ok "audit.sh: a crate-scoped waiver (invisible to cargo audit) is rejected"; else bad "audit.sh: crate-scoped waiver accepted"; fi

# ── deny.toml policy ──────────────────────────────────────────────────
python3 - <<'PY' || fail=$((fail + 1))
import sys
import tomllib
cfg = tomllib.load(open('deny.toml', 'rb')).get('advisories', {})
errors = []
if cfg.get('yanked') != 'warn':
    errors.append('[advisories] yanked must be "warn"')
if 'ignore' not in cfg:
    errors.append('[advisories] must carry the (possibly empty) `ignore` waiver list')
for key in ('unmaintained', 'unsound'):
    if cfg.get(key, 'all') == 'none':
        errors.append(f'[advisories] {key} = "none" hides it; deny.sh reports it as a warning')
for e in errors:
    print(f'  FAIL: deny.toml: {e}', file=sys.stderr)
sys.exit(1 if errors else 0)
PY

echo "[advisory-gates] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
