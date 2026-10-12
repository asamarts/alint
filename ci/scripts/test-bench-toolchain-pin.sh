#!/usr/bin/env bash
# Pin the bench toolchain contract of .github/workflows/bench-record.yml:
#   - the bench job sets BENCH_RUST, an exact x.y.z (never `stable`);
#   - it installs exactly ${{ env.BENCH_RUST }} via one dtolnay/rust-toolchain step;
#   - the very next step runs ci/scripts/pin-release-toolchain.sh with
#     RELEASE_RUST = ${{ env.BENCH_RUST }}, unconditionally, before rust-cache
#     or any cargo step, because rust-toolchain.toml (`stable`) otherwise wins
#     inside the checkout and the bench builds with whatever stable the runner
#     user has (rustc is part of the bench fingerprint);
#   - nothing else in the job sets RUSTUP_TOOLCHAIN or another toolchain.
# pin-release-toolchain.sh's behaviour is covered by test-release-toolchain-pin.sh.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

python3 - <<'PY'
import re
import sys
from pathlib import Path

sys.path.insert(0, 'ci/scripts')
import workflow_step as ws  # noqa: E402

WF = '.github/workflows/bench-record.yml'
lines = Path(WF).read_text(encoding='utf-8').splitlines()
body = ws.job_lines(lines, 'bench')
failures = []

pin = None
for line in body:
    m = re.match(r'^      BENCH_RUST: "(?P<v>[^"]*)"$', line)
    if m:
        pin = m.group('v')
if not pin or not re.fullmatch(r'\d+\.\d+\.\d+', pin):
    failures.append(f'bench job: env BENCH_RUST must be an exact "x.y.z" (got {pin!r})')

for line in body:
    if re.match(r'^\s+RUSTUP_TOOLCHAIN:', line):
        failures.append(f'bench job: {line.strip()!r} bypasses the BENCH_RUST pin')
    if re.match(r'^\s+toolchain:', line) and line.split(':', 1)[1].strip() != '${{ env.BENCH_RUST }}':
        failures.append(f'bench job: {line.strip()!r}; install ${{{{ env.BENCH_RUST }}}}')

steps = ws.steps(body)
uses = [ws.scalar(s, 'uses') or '' for s in steps]
runs = []
for s in steps:
    try:
        runs.append(ws.run_script(s))
    except SystemExit:
        runs.append('')

tc = [i for i, u in enumerate(uses) if u.startswith('dtolnay/rust-toolchain@')]
pins = [i for i, r in enumerate(runs) if r.strip() == 'ci/scripts/pin-release-toolchain.sh']
if len(tc) != 1:
    failures.append(f'bench job: needs exactly one dtolnay/rust-toolchain step (found {len(tc)})')
elif len(pins) != 1 or pins[0] != tc[0] + 1:
    failures.append('bench job: ci/scripts/pin-release-toolchain.sh must run right after dtolnay/rust-toolchain')
else:
    step = steps[pins[0]]
    if ws.env_map(step).get('RELEASE_RUST') != '${{ env.BENCH_RUST }}':
        failures.append('bench job: the pin step must pass RELEASE_RUST: ${{ env.BENCH_RUST }}')
    if ws.scalar(step, 'if') or ws.scalar(step, 'continue-on-error'):
        failures.append('bench job: the pin step must be unconditional and blocking')
    rust = re.compile(r'\bcargo\b|\bxtask\b')
    for i, (u, r) in enumerate(zip(uses, runs)):
        if i < pins[0] and (u.startswith('Swatinem/rust-cache@') or rust.search(r)):
            failures.append(f'bench job: step {i} uses Rust before the toolchain is pinned')
    if not any(rust.search(r) for r in runs[pins[0] + 1:]):
        failures.append('bench job: no cargo step after the pin (parser drift?)')

for f in failures:
    print(f'[bench-toolchain-pin] {f}', file=sys.stderr)
if failures:
    sys.exit(1)
print(f'[bench-toolchain-pin] OK - the bench job builds on BENCH_RUST={pin}')
PY
