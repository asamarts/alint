#!/usr/bin/env bash
# Pin the release toolchain contract of .github/workflows/release.yml:
#   - one workflow-level RELEASE_RUST, an exact x.y.z (never `stable`);
#   - every release job that runs cargo installs exactly ${{ env.RELEASE_RUST }}
#     (no floating `stable`, no other version) and then runs
#     ci/scripts/pin-release-toolchain.sh BEFORE rust-cache or any cargo step,
#     because dtolnay/rust-toolchain only sets the rustup default, which
#     rust-toolchain.toml (`stable`) overrides inside the checkout;
#   - nothing else in release.yml sets RUSTUP_TOOLCHAIN / RELEASE_RUST;
#   - pin-release-toolchain.sh exports RUSTUP_TOOLCHAIN and fails unless rustc
#     reports the pinned version (behavioural, stubbed rustc), and, when the
#     pinned toolchain is installed locally, really overrides
#     rust-toolchain.toml in this checkout;
#   - msrv.sh keeps using `cargo +<msrv>`, which beats RUSTUP_TOOLCHAIN.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"
fail=0

python3 - <<'PY' || fail=1
import re
import sys
from pathlib import Path

sys.path.insert(0, 'ci/scripts')
import workflow_step as ws  # noqa: E402

WF = Path('.github/workflows/release.yml')
text = WF.read_text(encoding='utf-8')
lines = text.splitlines()
failures = []

m = re.search(r'^env:\n(?:  .*\n)*?  RELEASE_RUST: "(?P<v>[^"]*)"$', text, re.MULTILINE)
pin = m.group('v') if m else None
if not pin or not re.fullmatch(r'\d+\.\d+\.\d+', pin):
    failures.append(f'release.yml: workflow-level env RELEASE_RUST must be an exact "x.y.z" (got {pin!r})')

# Only the workflow-level definition may set these.
for n, line in enumerate(lines, 1):
    if re.match(r'^\s+(RELEASE_RUST|RUSTUP_TOOLCHAIN):', line) and not line.startswith('  RELEASE_RUST:'):
        failures.append(f'release.yml:{n}: {line.strip()!r} overrides the pinned release toolchain')
    if re.match(r'^\s+toolchain:', line) and line.split(':', 1)[1].strip() != '${{ env.RELEASE_RUST }}':
        failures.append(f'release.yml:{n}: {line.strip()!r}; every release job must install '
                        '${{ env.RELEASE_RUST }}')

CARGO_SCRIPTS = re.compile(
    r'ci/scripts/(fmt|clippy|test|build|docs|dogfood|demo-drift|examples-validate|deny|audit|'
    r'msrv|release-binary|supply-chain-artifacts|publish-crates)\.sh|\bcargo\b|\bcross\b')
jobs = re.findall(r'^  ([A-Za-z0-9_-]+):\s*$', text[text.index('\njobs:\n'):], re.MULTILINE)
checked = 0
for job in jobs:
    steps = ws.steps(ws.job_lines(lines, job))
    uses = [ws.scalar(s, 'uses') or '' for s in steps]
    runs = []
    for s in steps:
        try:
            runs.append(ws.run_script(s))
        except SystemExit:
            runs.append('')
    needs_rust = any(CARGO_SCRIPTS.search(r) for r in runs) or any('rust-toolchain' in u for u in uses)
    if not needs_rust:
        continue
    checked += 1
    tc = [i for i, u in enumerate(uses) if u.startswith('dtolnay/rust-toolchain@')]
    pins = [i for i, r in enumerate(runs) if r.strip() == 'ci/scripts/pin-release-toolchain.sh']
    if len(tc) != 1:
        failures.append(f'{job}: needs exactly one dtolnay/rust-toolchain step (found {len(tc)})')
        continue
    if len(pins) != 1 or pins[0] != tc[0] + 1:
        failures.append(f'{job}: ci/scripts/pin-release-toolchain.sh must run right after dtolnay/rust-toolchain')
        continue
    for i, (u, r) in enumerate(zip(uses, runs)):
        if i < pins[0] and (u.startswith('Swatinem/rust-cache@') or CARGO_SCRIPTS.search(r)):
            failures.append(f'{job}: step {i} uses Rust before the release toolchain is pinned')
    if ws.scalar(steps[pins[0]], 'if') or ws.scalar(steps[pins[0]], 'continue-on-error'):
        failures.append(f'{job}: the pin step must be unconditional and blocking')
if checked < 4:
    failures.append(f'expected at least 4 Rust-building release jobs, found {checked} (parser drift?)')

if not re.search(r'cargo "\+\$\{msrv\}"', Path('ci/scripts/msrv.sh').read_text(encoding='utf-8')):
    failures.append('msrv.sh must build with `cargo "+${msrv}"` (beats RUSTUP_TOOLCHAIN in the preflight)')
if 'rustc --version' not in Path('ci/scripts/release-binary.sh').read_text(encoding='utf-8'):
    failures.append('release-binary.sh must log `rustc --version` (proof of the shipped toolchain)')

for f in failures:
    print(f'[release-toolchain-pin] {f}', file=sys.stderr)
if failures:
    sys.exit(1)
print(f'[release-toolchain-pin] OK - {checked} release jobs build on RELEASE_RUST={pin}')
PY

# ── pin-release-toolchain.sh, behaviourally ──────────────────────────
sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
mkdir -p "$sandbox/bin"
# shellcheck disable=SC2016  # expands when the stub runs
printf '#!/usr/bin/env bash\necho "rustc ${STUB_RUSTC:-$RUSTUP_TOOLCHAIN} (stub 2026-01-01)"\n' > "$sandbox/bin/rustc"
printf '#!/usr/bin/env bash\necho "cargo stub"\n' > "$sandbox/bin/cargo"
chmod +x "$sandbox/bin/rustc" "$sandbox/bin/cargo"
pin_run() { # pin_run VAR=value... -> exit status
  : > "$sandbox/env"
  env PATH="$sandbox/bin:$PATH" GITHUB_ENV="$sandbox/env" "$@" \
    bash ci/scripts/pin-release-toolchain.sh > "$sandbox/out" 2>&1
}
if pin_run RELEASE_RUST=1.2.3 && grep -qx 'RUSTUP_TOOLCHAIN=1.2.3' "$sandbox/env"; then
  echo "  ok: pin exports RUSTUP_TOOLCHAIN for later steps"
else
  echo "  FAIL: pin must export RUSTUP_TOOLCHAIN=<RELEASE_RUST> via GITHUB_ENV" >&2; fail=1
fi
if pin_run RELEASE_RUST=1.2.3 STUB_RUSTC=1.99.0; then
  echo "  FAIL: pin must fail when rustc is not the pinned version" >&2; fail=1
else
  echo "  ok: pin fails when rustc is not the pinned version"
fi
for bad in stable 1.98 ''; do
  if pin_run RELEASE_RUST="$bad"; then
    echo "  FAIL: pin accepted RELEASE_RUST='$bad'" >&2; fail=1
  else
    echo "  ok: pin rejects RELEASE_RUST='$bad'"
  fi
done

# Real rustup, when the pinned toolchain is installed (developer machines): the
# exported RUSTUP_TOOLCHAIN must beat rust-toolchain.toml in this checkout.
pinned=$(sed -n 's/^  RELEASE_RUST: "\(.*\)"$/\1/p' .github/workflows/release.yml)
if command -v rustup >/dev/null 2>&1 && rustup run "$pinned" rustc --version >/dev/null 2>&1; then
  if env -u RUSTUP_TOOLCHAIN RELEASE_RUST="$pinned" GITHUB_ENV=/dev/null \
       bash ci/scripts/pin-release-toolchain.sh > "$sandbox/out" 2>&1; then
    echo "  ok: real rustup: $(tail -n 1 "$sandbox/out")"
  else
    echo "  FAIL: real rustup did not honour RUSTUP_TOOLCHAIN=$pinned over rust-toolchain.toml" >&2
    cat "$sandbox/out" >&2; fail=1
  fi
else
  echo "  note: toolchain $pinned not installed locally; skipped the real-rustup check"
fi

[[ "$fail" -eq 0 ]] && echo "[release-toolchain-pin] OK"
exit "$fail"
