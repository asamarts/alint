#!/usr/bin/env bash
# Pin the supply-chain trust anchors outside Cargo/Actions:
#   - Dependabot covers every shipped dependency manifest (not just cargo +
#     github-actions at /).
#   - The release image's base is digest-pinned.
#   - The JetBrains Gradle wrapper verifies its distribution checksum.
#   - No workflow trusts `ssh-keyscan` (trust-on-first-use) for a host key.
#   - dtolnay/rust-toolchain is pinned to a commit on its `master` branch and
#     every use names its toolchain. The action's `stable` and version
#     branches are rebuilt on every change, so a SHA taken from one of them is
#     orphaned and may stop resolving; only `master` keeps its history, and its
#     action has no default toolchain.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

python3 - <<'PY'
from pathlib import Path
import re
import sys

failures = []

# Plain-text parse (no PyYAML dependency on the runner): each update entry
# starts at `- package-ecosystem:` and runs to the next one.
text = Path('.github/dependabot.yml').read_text(encoding='utf-8')
entries = re.split(r'^\s*- package-ecosystem:', text, flags=re.MULTILINE)[1:]
updates = []
for entry in entries:
    eco = entry.split('\n', 1)[0].strip().strip('"\'')
    d = re.search(r'^\s+directory:\s*["\']?([^"\'\s]+)', entry, re.MULTILINE)
    p = re.search(r'^\s+prefix:\s*["\']?([^"\'\n]+)', entry, re.MULTILINE)
    updates.append({'package-ecosystem': eco,
                    'directory': d.group(1) if d else '',
                    'commit-message': {'prefix': p.group(1).strip() if p else ''}})
covered = {(u['package-ecosystem'], u['directory'].rstrip('/') or '/') for u in updates}
REQUIRED = {
    ('cargo', '/'),
    ('github-actions', '/'),
    ('npm', '/editors/vscode'),
    ('npm', '/npm'),
    ('cargo', '/editors/zed'),
    ('gradle', '/editors/jetbrains'),
    ('docker', '/'),
}
for eco, directory in sorted(REQUIRED - covered):
    failures.append(f'.github/dependabot.yml: no {eco} update entry for {directory}')
for update in updates:
    if not update.get('commit-message', {}).get('prefix', '').startswith('chore('):
        failures.append(f'dependabot {update["package-ecosystem"]} {update["directory"]}: '
                        'commit-message prefix must be a chore(...) conventional type')

for n, line in enumerate(Path('Dockerfile').read_text(encoding='utf-8').splitlines(), 1):
    if re.match(r'^\s*FROM\s', line, re.IGNORECASE) and not re.search(r'@sha256:[0-9a-f]{64}\b', line):
        failures.append(f'Dockerfile:{n}: base image must be digest-pinned (@sha256:...)')

props = Path('editors/jetbrains/gradle/wrapper/gradle-wrapper.properties').read_text(encoding='utf-8')
if not re.search(r'^distributionSha256Sum=[0-9a-f]{64}$', props, re.MULTILINE):
    failures.append('gradle-wrapper.properties: distributionSha256Sum (64 hex) is required')

for wf in sorted(Path('.github/workflows').glob('*.y*ml')):
    for n, line in enumerate(wf.read_text(encoding='utf-8').splitlines(), 1):
        if 'ssh-keyscan' in line and not line.lstrip().startswith('#'):
            failures.append(f'{wf}:{n}: ssh-keyscan trusts the first key it sees; pin known_hosts')
toolchain_uses = 0
for wf in sorted(Path('.github/workflows').glob('*.y*ml')):
    lines = wf.read_text(encoding='utf-8').splitlines()
    for n, line in enumerate(lines):
        m = re.match(r'^(?P<lead>\s*)(?P<dash>- )?uses:\s*dtolnay/rust-toolchain@(?P<ref>.*)$', line)
        if not m:
            continue
        toolchain_uses += 1
        where = f'{wf}:{n + 1}'
        if not re.fullmatch(r'[0-9a-f]{40} # master', m.group('ref').strip()):
            failures.append(f'{where}: pin dtolnay/rust-toolchain to a master commit as '
                            f'`@<sha> # master`, not `@{m.group("ref").strip()}`')
        # The step's other keys sit at the column of `uses`.
        key_col = len(m.group('lead')) + (2 if m.group('dash') else 0)
        step = []
        for later in lines[n + 1:]:
            if later.strip() and len(later) - len(later.lstrip(' ')) < key_col:
                break
            if later.strip() and len(later) - len(later.lstrip(' ')) == key_col and later.lstrip().startswith('- '):
                break
            step.append(later)
        if not any(re.match(r'^\s+toolchain:\s*\S', l) for l in step):
            failures.append(f'{where}: dtolnay/rust-toolchain at master needs a `toolchain:` input')
if toolchain_uses < 20:
    failures.append(f'found {toolchain_uses} dtolnay/rust-toolchain uses, expected at least 20 (parser drift?)')

release = Path('.github/workflows/release.yml').read_text(encoding='utf-8')
if 'github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl' not in release:
    failures.append("release.yml: homebrew job must pin GitHub's published ed25519 host key")

if failures:
    for f in failures:
        print(f'[supply-chain-pins] {f}', file=sys.stderr)
    sys.exit(1)
print(f'[supply-chain-pins] OK - {len(covered)} Dependabot lanes, base image + Gradle wrapper + host keys pinned, '
      f'{toolchain_uses} rust-toolchain uses on master')
PY
