#!/usr/bin/env python3
"""Extract one step of a GitHub Actions workflow, for behavioural shell tests.

The release-pipeline harnesses run a step's real `run:` script against stubbed
tools (gh / npm / npx / git / curl / ...) instead of grepping its text, so a
deleted `exit 0`, a flipped comparison or a flag hidden behind a `#` comment
changes what the stubs record and fails the test.

Dependency-free (no PyYAML on the runner): it understands the block-style
layout these workflows use (`jobs:` -> `  <job>:` -> `    steps:` -> `- name:`
items with a `run: |` block scalar and an `env:` mapping).

Usage:
  workflow_step.py <workflow.yml> <job> <step name or id> run   # print the script
  workflow_step.py <workflow.yml> <job> <step name or id> env   # KEY<TAB>value lines
  workflow_step.py <workflow.yml> <job> <step name or id> if    # the step's if:
"""
import re
import sys
from pathlib import Path


def indent_of(line):
    return len(line) - len(line.lstrip(' '))


def block(lines, start, parent_indent):
    """Lines after `start` that are blank or indented deeper than parent_indent."""
    out = []
    for line in lines[start + 1:]:
        if line.strip() and indent_of(line) <= parent_indent:
            break
        out.append(line)
    while out and not out[-1].strip():
        out.pop()
    return out


def job_lines(lines, job):
    for i, line in enumerate(lines):
        if line.rstrip() == f'  {job}:':
            return block(lines, i, 2)
    raise SystemExit(f'workflow_step: job {job!r} not found')


def steps(job_body):
    for i, line in enumerate(job_body):
        if line.rstrip() == '    steps:':
            body = block(job_body, i, 4)
            break
    else:
        raise SystemExit('workflow_step: job has no steps:')
    items, cur = [], None
    for line in body:
        if re.match(r'^      - ', line):
            cur = ['        ' + line[8:]]
            items.append(cur)
        elif cur is not None:
            cur.append(line)
    return items


def scalar(step, key):
    for line in step:
        m = re.match(rf'^        {key}:\s*(.*?)\s*$', line)
        if m:
            return m.group(1).strip('"\'')
    return None


def find_step(lines, job, name):
    matches = [s for s in steps(job_lines(lines, job))
               if scalar(s, 'name') == name or scalar(s, 'id') == name]
    if len(matches) != 1:
        raise SystemExit(f'workflow_step: {len(matches)} steps named {name!r} in job {job!r}')
    return matches[0]


def run_script(step):
    for i, line in enumerate(step):
        m = re.match(r'^        run:\s*(.*)$', line)
        if not m:
            continue
        rest = m.group(1).strip()
        if rest and rest[0] not in '|>':
            return rest + '\n'
        body = block(step, i, 8)
        width = min((indent_of(l) for l in body if l.strip()), default=0)
        return '\n'.join(l[width:] for l in body) + '\n'
    raise SystemExit('workflow_step: step has no run:')


def env_map(step):
    for i, line in enumerate(step):
        if line.rstrip() == '        env:':
            out = {}
            for l in block(step, i, 8):
                m = re.match(r'^          ([A-Za-z_][A-Za-z0-9_]*):\s*(.*?)\s*$', l)
                if m:
                    out[m.group(1)] = m.group(2)
            return out
    return {}


def main(argv):
    if len(argv) != 5 or argv[4] not in ('run', 'env', 'if'):
        raise SystemExit(__doc__)
    _, path, job, name, what = argv
    step = find_step(Path(path).read_text(encoding='utf-8').splitlines(), job, name)
    if what == 'run':
        sys.stdout.write(run_script(step))
    elif what == 'env':
        for k, v in env_map(step).items():
            print(f'{k}\t{v}')
    else:
        print(scalar(step, 'if') or '')


if __name__ == '__main__':
    main(sys.argv)
