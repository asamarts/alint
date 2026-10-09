#!/usr/bin/env bash
# Pin the supply-chain trust anchors outside Cargo/Actions:
#   - Dependabot covers every shipped dependency manifest (not just cargo +
#     github-actions at /).
#   - The release image's base is digest-pinned.
#   - The JetBrains Gradle wrapper verifies its distribution checksum.
#   - No workflow trusts `ssh-keyscan` (trust-on-first-use) for a host key.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

python3 - <<'PY'
from pathlib import Path
import re
import sys

import yaml

failures = []

dependabot = yaml.safe_load(Path('.github/dependabot.yml').read_text(encoding='utf-8'))
covered = {(u['package-ecosystem'], u['directory'].rstrip('/') or '/') for u in dependabot['updates']}
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
for update in dependabot['updates']:
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
release = Path('.github/workflows/release.yml').read_text(encoding='utf-8')
if 'github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl' not in release:
    failures.append("release.yml: homebrew job must pin GitHub's published ed25519 host key")

if failures:
    for f in failures:
        print(f'[supply-chain-pins] {f}', file=sys.stderr)
    sys.exit(1)
print(f'[supply-chain-pins] OK - {len(covered)} Dependabot lanes, base image + Gradle wrapper + host keys pinned')
PY
