#!/usr/bin/env bash
# Pin two contracts between ci.yml, release.yml and Cargo.toml:
#
#   1. Preflight parity. release.yml's `preflight` job claims to re-run every
#      correctness gate ci.yml runs (release.yml fires in PARALLEL with ci.yml
#      on a tag push, so a CI-only gate does not block publishing). Every
#      ci/scripts/* program ci.yml invokes must therefore also be invoked by the
#      release preflight, unless it is on the documented exemption list below.
#
#   2. MSRV single source. The MSRV toolchain is derived from
#      [workspace.package].rust-version by ci/scripts/msrv.sh; no workflow may
#      hand-copy a numeric `toolchain:` pin that could drift from what every
#      published crate advertises.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

python3 - <<'PY'
from pathlib import Path
import re
import sys

failures = []

# ci.yml scripts that are deliberately NOT part of the release preflight.
EXEMPT = {
    'ci/scripts/detect-changes.sh': 'PR change routing; a release verifies the whole tree',
    'ci/scripts/summary.sh': 'aggregate status bookkeeping',
    'ci/scripts/audit.sh': 'advisory-only by design (never fails); ci.yml Audit runs on the tag too',
    'ci/scripts/bench-smoke.sh': 'perf smoke, not a correctness gate',
    'ci/scripts/det-perf-gate.sh': 'advisory PR-vs-merge-base perf gate (needs a base)',
    'ci/scripts/supply-chain-artifacts.sh': 'release.yml runs it in its own supply-chain job',
    'ci/scripts/check-secrets-inventory.sh': 'repo-config drift gate; ci.yml runs it unconditionally on the tag',
}

script_re = re.compile(r'ci/scripts/[A-Za-z0-9_.-]+\.(?:sh|py)')


def code(path):
    return '\n'.join(
        l for l in Path(path).read_text(encoding='utf-8').splitlines()
        if not l.lstrip().startswith('#')
    )


def job(text, name):
    m = re.search(rf'^  {re.escape(name)}:\n(?P<body>.*?)(?=^  [A-Za-z0-9_-]+:\n|\Z)',
                  text, re.MULTILINE | re.DOTALL)
    return m.group('body') if m else ''


ci = code('.github/workflows/ci.yml')
release = code('.github/workflows/release.yml')
preflight = job(release, 'preflight')
if not preflight:
    failures.append('release.yml: preflight job not found')

ci_scripts = set(script_re.findall(ci))
pre_scripts = set(script_re.findall(preflight))
for script in sorted(ci_scripts - pre_scripts - set(EXEMPT)):
    failures.append(f'release.yml preflight does not run {script} (ci.yml does); '
                    'add it or document an exemption in this test')
for script in sorted(set(EXEMPT) - ci_scripts):
    failures.append(f'stale exemption: ci.yml no longer runs {script}')

for required in ('ci/scripts/msrv.sh', 'ci/scripts/check-workspace-dep-floors.sh',
                 'ci/scripts/demo-drift.sh'):
    if required not in pre_scripts:
        failures.append(f'release.yml preflight must run {required}')
    if required not in ci_scripts:
        failures.append(f'ci.yml must run {required}')

# No hand-copied numeric toolchain anywhere in the workflows. bench-record.yml
# is the one deliberate exception: rustc is part of the bench fingerprint, so
# that series pins its own toolchain independently of the MSRV.
TOOLCHAIN_PIN_EXEMPT = {'bench-record.yml'}
for wf in sorted(Path('.github/workflows').glob('*.yml')):
    if wf.name in TOOLCHAIN_PIN_EXEMPT:
        continue
    for n, line in enumerate(wf.read_text(encoding='utf-8').splitlines(), 1):
        if re.match(r'^\s+toolchain:\s*["\']?\d', line):
            failures.append(f'{wf}:{n}: numeric toolchain pin {line.strip()!r}; '
                            'derive it from Cargo.toml via ci/scripts/msrv.sh --print')

msrv_job = job(ci, 'msrv')
if '${{ steps.msrv.outputs.toolchain }}' not in msrv_job or 'ci/scripts/msrv.sh --print' not in msrv_job:
    failures.append('ci.yml msrv job must install the toolchain msrv.sh --print derives')

# A gate that cannot fail is no gate: `continue-on-error` (on the job or on
# any step) or an `|| true` / `|| :` swallow would keep the job green while
# the MSRV / preflight check is red.
for where, body in (('ci.yml msrv job', msrv_job), ('release.yml preflight job', preflight)):
    if re.search(r'^\s+continue-on-error:', body, re.MULTILINE):
        failures.append(f'{where}: continue-on-error makes its gates non-blocking')
    for line in body.splitlines():
        if 'ci/scripts/' in line and re.search(r'\|\|\s*(true|:)(\s|$)|;\s*(true|exit 0)(\s|$)', line):
            failures.append(f'{where}: a gate result is swallowed: {line.strip()!r}')

if failures:
    for f in failures:
        print(f'[release-preflight-parity] {f}', file=sys.stderr)
    sys.exit(1)
PY

# msrv.sh must read exactly [workspace.package].rust-version (not another
# table's), normalise x.y to x.y.0, and agree with the live Cargo.toml.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cat > "$tmp/Cargo.toml" <<'TOML'
[package]
rust-version = "1.70"

[workspace.package]
version = "9.9.9"
rust-version = "1.91"

[workspace.dependencies]
rust-version = "1.60"
TOML
got=$(MSRV_REPO_ROOT="$tmp" bash ci/scripts/msrv.sh --print)
if [[ "$got" != "1.91.0" ]]; then
  echo "[release-preflight-parity] msrv.sh fixture: expected 1.91.0, got $got" >&2
  exit 1
fi
printf '[workspace.package]\nversion = "1.0.0"\n' > "$tmp/Cargo.toml"
if MSRV_REPO_ROOT="$tmp" bash ci/scripts/msrv.sh --print >/dev/null 2>&1; then
  echo "[release-preflight-parity] msrv.sh must fail closed without rust-version" >&2
  exit 1
fi
live=$(awk '/^\[/{p=($0=="[workspace.package]");next} p&&/^rust-version/{gsub(/.*= *"|"/,"");print;exit}' Cargo.toml)
if [[ "$(bash ci/scripts/msrv.sh --print)" != "$live" && "$(bash ci/scripts/msrv.sh --print)" != "$live.0" ]]; then
  echo "[release-preflight-parity] msrv.sh disagrees with Cargo.toml rust-version ($live)" >&2
  exit 1
fi

echo "[release-preflight-parity] OK - preflight mirrors ci.yml gates; MSRV derived from Cargo.toml"
