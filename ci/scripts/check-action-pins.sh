#!/usr/bin/env bash
# Every remote Action in the workflows and action.yml is pinned to a full commit
# SHA with a comment naming the release it stands for (`@<sha> # v2.9.3`). The
# SHA is what runs and the comment is what a reviewer reads, so they must agree:
# Dependabot once pinned an untagged commit under a `v2.9.3` comment (#376).
# This resolves each comment against the Action's repository and fails when:
#   - a remote `uses:` is not a 40-hex SHA with a `# <ref>` comment;
#   - the comment names a tag whose commit is not the pinned one. A major or
#     minor tag (`# v7`) also accepts any release in its series (`v7.0.1`),
#     since a moving major tag may have moved on since the pin;
#   - the comment names a branch (`# master`, for an Action that publishes no
#     release tags) whose history does not contain the pinned commit;
#   - the comment names neither, or the repository cannot be read.
# Local (`./`) and `docker://` references are not Actions in a repository.
#
# Needs network access to the Actions' repositories; each git call is tried
# ACTION_PINS_ATTEMPTS times (default 3). ACTION_PINS_REPO_ROOT and
# ACTION_PINS_GIT_BASE override the checkout and the git host, for
# test-check-action-pins.sh.
set -euo pipefail

REPO_ROOT="${ACTION_PINS_REPO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$REPO_ROOT"

python3 - <<'PY'
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

GIT_BASE = os.environ.get('ACTION_PINS_GIT_BASE', 'https://github.com').rstrip('/')
ATTEMPTS = max(1, int(os.environ.get('ACTION_PINS_ATTEMPTS', '3')))
FILES = sorted(Path('.github/workflows').glob('*.yml')) + sorted(Path('.github/workflows').glob('*.yaml'))
if Path('action.yml').is_file():
    FILES.append(Path('action.yml'))

USES = re.compile(r'^\s*(?:-\s+)?uses:\s*(?P<ref>[^\s#]+)\s*(?:#\s*(?P<comment>.*?))?\s*$')
PINNED = re.compile(r'^(?P<repo>[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)(?:/[^@]*)?@(?P<sha>[0-9a-f]{40})$')

errors = []
pins = {}  # (repo, sha, comment) -> [where]
for path in FILES:
    for number, line in enumerate(path.read_text(encoding='utf-8').splitlines(), 1):
        m = USES.match(line)
        if not m:
            continue
        ref, comment = m.group('ref').strip('\'"'), (m.group('comment') or '').strip()
        if ref.startswith('./') or ref.startswith('docker://'):
            continue
        where = f'{path}:{number}'
        p = PINNED.match(ref)
        if not p:
            errors.append(f'{where}: `{ref}` is not pinned to a full commit SHA')
            continue
        if not re.fullmatch(r'[A-Za-z0-9_./-]+', comment):
            errors.append(f'{where}: `{ref}` needs a `# <tag or branch>` comment naming what the SHA is')
            continue
        pins.setdefault((p.group('repo'), p.group('sha'), comment), []).append(where)


def git(*args, cwd=None):
    """Run git, retrying network failures; return stdout or raise."""
    last = None
    for attempt in range(ATTEMPTS):
        if attempt:
            time.sleep(2 * attempt)
        r = subprocess.run(['git', *args], cwd=cwd, capture_output=True, text=True,
                           env={**os.environ, 'GIT_TERMINAL_PROMPT': '0'})
        if r.returncode == 0:
            return r.stdout
        last = r.stderr.strip()
    raise RuntimeError(last or f'git {" ".join(args)} failed')


refs_cache = {}
def remote_refs(repo):
    """{'tags': {name: commit}, 'heads': {name: commit}} with annotated tags peeled."""
    if repo not in refs_cache:
        out = git('ls-remote', '--tags', '--heads', f'{GIT_BASE}/{repo}')
        tags, peeled, heads = {}, {}, {}
        for row in out.splitlines():
            sha, name = row.split('\t', 1)
            if name.startswith('refs/tags/') and name.endswith('^{}'):
                peeled[name[len('refs/tags/'):-3]] = sha
            elif name.startswith('refs/tags/'):
                tags[name[len('refs/tags/'):]] = sha
            elif name.startswith('refs/heads/'):
                heads[name[len('refs/heads/'):]] = sha
        tags.update(peeled)
        refs_cache[repo] = {'tags': tags, 'heads': heads}
    return refs_cache[repo]


def on_branch(repo, branch, sha):
    """Whether `sha` is in `branch`'s history (a blobless fetch of that branch)."""
    with tempfile.TemporaryDirectory() as tmp:
        git('init', '-q', '--bare', tmp)
        git('fetch', '-q', '--filter=blob:none', '--no-tags', f'{GIT_BASE}/{repo}',
            f'refs/heads/{branch}:refs/heads/{branch}', cwd=tmp)
        r = subprocess.run(['git', 'merge-base', '--is-ancestor', sha, f'refs/heads/{branch}'],
                           cwd=tmp, capture_output=True)
        return r.returncode == 0


checked = 0
for (repo, sha, comment), places in sorted(pins.items()):
    where = ', '.join(places)
    try:
        refs = remote_refs(repo)
    except RuntimeError as e:
        errors.append(f'{where}: cannot read {repo}: {e}')
        continue
    tags = refs['tags']
    series = {name: commit for name, commit in tags.items()
              if name == comment or name.startswith(comment + '.')}
    if series:
        if sha in series.values():
            checked += 1
            continue
        named = tags.get(comment)
        hint = f'`{comment}` is {named[:12]}' if named else f'no `{comment}.*` release is {sha[:12]}'
        errors.append(f'{where}: {repo}@{sha[:12]} is not `{comment}` ({hint})')
    elif comment in refs['heads']:
        try:
            if on_branch(repo, comment, sha):
                checked += 1
                continue
        except RuntimeError as e:
            errors.append(f'{where}: cannot fetch {repo} {comment}: {e}')
            continue
        errors.append(f'{where}: {repo}@{sha[:12]} is not in the history of branch `{comment}`')
    else:
        errors.append(f'{where}: {repo} has no tag or branch `{comment}`')

for e in errors:
    print(f'  FAIL: {e}', file=sys.stderr)
if errors:
    print(f'[action-pins] {len(errors)} pin(s) do not match their comment; '
          'pin the commit the comment names (or fix the comment)', file=sys.stderr)
    sys.exit(1)
print(f'[action-pins] OK: {checked} pinned Action reference(s) match their comments '
      f'across {len(FILES)} file(s)')
PY
