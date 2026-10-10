#!/usr/bin/env bash
# RustSec advisory gate via cargo-audit. BLOCKING on vulnerabilities.
#
# One of two blocking advisory gates over the same RustSec database (the other
# is the `advisories` pass in ci/scripts/deny.sh). Both honour ONE waiver list:
# the [advisories] `ignore` array in deny.toml, which cargo-deny reads natively
# and this script turns into `cargo audit --ignore <id>` flags, so a waiver can
# never be granted in one gate and forgotten in the other.
#
# cargo audit exits non-zero only for vulnerabilities; informational advisories
# (unmaintained / unsound) and yanked crates are printed as warnings and do not
# fail it, matching deny.sh's policy.
#
# Usage:
#   ci/scripts/audit.sh                  # run the gate
#   ci/scripts/audit.sh --print-waivers  # print the waived advisory ids and exit
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

# deny.toml [advisories].ignore -> one advisory id per line. Entries must be
# advisory ids (a bare "RUSTSEC-..." string or { id = "...", reason = "..." });
# a crate-scoped waiver cannot be expressed to cargo audit, so it is rejected
# rather than silently honoured by one gate only.
waivers="$(python3 - <<'PY'
import re
import sys
try:
    import tomllib
except ModuleNotFoundError:
    sys.exit('[audit] python3 >= 3.11 (tomllib) is required to read deny.toml waivers')
with open('deny.toml', 'rb') as f:
    cfg = tomllib.load(f)
bad = []
for entry in cfg.get('advisories', {}).get('ignore', []):
    ident = entry if isinstance(entry, str) else entry.get('id') if isinstance(entry, dict) else None
    if not isinstance(ident, str) or not re.fullmatch(r'(RUSTSEC-\d{4}-\d{4}|GHSA(-[0-9a-z]{4}){3})', ident):
        bad.append(entry)
        continue
    if isinstance(entry, dict) and not str(entry.get('reason', '')).strip():
        bad.append(entry)
        continue
    print(ident)
if bad:
    sys.exit(f'[audit] deny.toml [advisories].ignore entries must be '
             f'{{ id = "RUSTSEC-YYYY-NNNN", reason = "..." }}; rejected: {bad}')
PY
)"

if [[ "${1:-}" == "--print-waivers" ]]; then
  [[ -n "$waivers" ]] && printf '%s\n' "$waivers"
  exit 0
fi

# cargo-audit may not be on the runner. It is on the self-hosted CI runner
# but NOT on a fresh GitHub-hosted ubuntu-latest (the fork-PR lane — see
# docs/design/v0.14/ci-fork-pr-isolation.md). Install on demand, pinned with
# --locked, so the gate is portable across both. Swatinem/rust-cache persists
# ~/.cargo/bin so the install only pays once per cache window. (Mirrors
# deny.sh.)
if ! command -v cargo-audit >/dev/null 2>&1; then
    echo "==> cargo-audit not found; installing (cargo install --locked)"
    # Clear RUSTFLAGS for the tool build only: CI sets `-D warnings` for
    # *alint's* code, but applying it while compiling a third-party tool
    # would fail the install on any upstream warning.
    RUSTFLAGS='' cargo install cargo-audit --locked
fi

ignore_args=()
while IFS= read -r id; do
  [[ -n "$id" ]] && ignore_args+=(--ignore "$id")
done <<< "$waivers"

echo "==> Running cargo audit (blocking on vulnerabilities; $(( ${#ignore_args[@]} / 2 )) waiver(s) from deny.toml)"
cargo audit "${ignore_args[@]}"
