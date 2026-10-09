#!/usr/bin/env bash
# Pin the re-runnability contract of .github/workflows/release.yml.
#
# Recovery from a partial release is always `gh run rerun <id> --failed`, never
# a re-tag (RELEASING.md "Recovering a partial release"). That only works if
# every publishing step tolerates "this version is already out there". Each
# assertion below names the step it pins, so a regression (e.g. a bare
# `gh release create` with no existence guard) fails here with a pointer.
set -euo pipefail

REPO_ROOT="${RELEASE_IDEMPOTENCY_REPO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$REPO_ROOT"

python3 - <<'PY'
from pathlib import Path
import re
import sys

WORKFLOW = Path('.github/workflows/release.yml')
lines = WORKFLOW.read_text(encoding='utf-8').splitlines()
failures = []


def fail(message):
    failures.append(message)


def job_blocks():
    jobs_index = lines.index('jobs:')
    starts = [
        (m.group(1), i)
        for i in range(jobs_index + 1, len(lines))
        if (m := re.match(r'^  ([A-Za-z0-9_-]+):\s*$', lines[i]))
    ]
    blocks = {}
    for pos, (name, start) in enumerate(starts):
        end = starts[pos + 1][1] if pos + 1 < len(starts) else len(lines)
        # Comments are documentation, not behaviour: strip them so a comment
        # mentioning a flag cannot satisfy an assertion.
        code = [l for l in lines[start:end] if not l.lstrip().startswith('#')]
        blocks[name] = '\n'.join(code)
    return blocks


jobs = job_blocks()


def need(job, pattern, why):
    if job not in jobs:
        fail(f'job {job!r} missing from {WORKFLOW}')
        return
    if not re.search(pattern, jobs[job], re.MULTILINE):
        fail(f'{job}: {why} (pattern {pattern!r} not found)')


def forbid(job, pattern, why):
    if job in jobs and re.search(pattern, jobs[job], re.MULTILINE):
        fail(f'{job}: {why} (forbidden pattern {pattern!r} present)')


# 1. GitHub Release creation is guarded: an existing Release is refreshed with
#    `gh release upload --clobber` and the asset set is verified afterwards.
need('release', r'gh release view "\$TAG"[^\n]*>/dev/null',
     '`gh release create` must be guarded by a `gh release view` existence check')
need('release', r'gh release upload "\$TAG"[^\n]*--clobber',
     'an existing Release must be refreshed with `gh release upload --clobber`')
need('release', r"--json assets --jq '\.assets\[\]\.name'",
     'the published asset set must be verified after create/upload')
create_idx = jobs.get('release', '').find('gh release create')
view_idx = jobs.get('release', '').find('gh release view "$TAG"')
if create_idx != -1 and (view_idx == -1 or view_idx > create_idx):
    fail('release: the `gh release view` guard must precede `gh release create`')

# 1b. The docs-bundle dispatch has a manual fallback, so it must not fail the
#     job (that would skip every `needs: release` publisher).
need('release', r'if ! gh workflow run docs-bundle\.yml',
     'the docs-bundle dispatch must be non-fatal (if ! ... ::warning::)')

# 2. Every downstream publisher tolerates an already-published version.
need('publish-vscode', r'vsce publish\b[^\n]*--skip-duplicate',
     '`vsce publish` must pass --skip-duplicate')
need('publish-vscode', r'ovsx publish\b[^\n]*--skip-duplicate',
     '`ovsx publish` must pass --skip-duplicate')
need('publish-npm', r'npm view "@asamarts/alint@\$\{ver\}" version',
     '`npm publish` must be guarded by an `npm view pkg@ver` existence check')
need('publish-pypi', r'^\s+skip-existing:\s*true\s*$',
     'the PyPI publish must set skip-existing: true')
need('publish-jetbrains', r'plugins\.jetbrains\.com/plugins/list\?pluginId=org\.alint\.lsp',
     '`publishPlugin` must be guarded by a Marketplace version lookup')
need('homebrew', r'git diff --cached --quiet',
     'the tap bump must no-op when the formula is unchanged')
publish_crates = Path('ci/scripts/publish-crates.sh').read_text(encoding='utf-8')
if 'already published; skipping' not in publish_crates:
    fail('publish-crates.sh: must skip crates already on crates.io')

if failures:
    for message in failures:
        print(f'[release-idempotency] {message}', file=sys.stderr)
    sys.exit(1)
print('[release-idempotency] OK - release.yml publish steps are re-run safe')
PY
