#!/usr/bin/env bash
# Keep every workflow's GITHUB_TOKEN authority explicit and reject common
# repository-write/review-approval drift. This is a source-policy gate; GitHub
# remains responsible for enforcing the declared permissions at runtime.
set -euo pipefail

REPO_ROOT="${WORKFLOW_PERMISSIONS_REPO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$REPO_ROOT"

python3 - <<'PY'
from pathlib import Path
import re
import sys

WORKFLOW_DIR = Path('.github/workflows')
PERMISSION_KEYS = {
    'actions', 'artifact-metadata', 'attestations', 'checks', 'code-quality',
    'contents', 'deployments', 'discussions', 'id-token', 'issues', 'models',
    'packages', 'pages', 'pull-requests', 'security-events', 'statuses',
    'vulnerability-alerts',
}

EXPECTED_TOP = {
    'action-selftest.yml': {'contents': 'read'},
    'bench-docker.yml': {},
    'bench-record.yml': {},
    'ci.yml': {'contents': 'read'},
    'coverage.yml': {'contents': 'read'},
    'cross-platform.yml': {'contents': 'read'},
    'docs-bundle.yml': {'contents': 'write'},
    'editors-e2e.yml': {'contents': 'read'},
    'homebrew-smoke.yml': {'contents': 'read'},
    'issue-26-e2e.yml': {'contents': 'read'},
    'kani.yml': {'contents': 'read'},
    'mutants.yml': {'contents': 'read'},
    'post-publish-smoke.yml': {'contents': 'read'},
    'release.yml': {},
}

EXPECTED_JOB_OVERRIDES = {
    ('bench-docker.yml', 'build'): {
        'contents': 'read',
        'packages': 'write',
    },
    ('bench-record.yml', 'guard'): {'contents': 'read'},
    ('bench-record.yml', 'bench'): {
        'contents': 'write',
        'pull-requests': 'write',
    },
    ('release.yml', 'preflight'): {'contents': 'read'},
    ('release.yml', 'build'): {'contents': 'read'},
    ('release.yml', 'supply-chain'): {'contents': 'read'},
    ('release.yml', 'release'): {
        'actions': 'write',
        'attestations': 'write',
        'contents': 'write',
        'id-token': 'write',
    },
    ('release.yml', 'docker'): {
        'attestations': 'write',
        'contents': 'read',
        'id-token': 'write',
        'packages': 'write',
    },
    ('release.yml', 'publish-crates'): {
        'contents': 'read',
        'id-token': 'write',
    },
    ('release.yml', 'publish-npm'): {
        'contents': 'read',
        'id-token': 'write',
    },
    ('release.yml', 'publish-pypi'): {
        'contents': 'read',
        'id-token': 'write',
    },
    ('release.yml', 'homebrew'): {'contents': 'read'},
    ('release.yml', 'publish-vscode'): {'contents': 'read'},
    ('release.yml', 'publish-jetbrains'): {'contents': 'read'},
    ('release.yml', 'post-publish-smoke'): {'actions': 'write'},
}


def fail(message: str) -> None:
    print(f'[workflow-permissions] {message}', file=sys.stderr)
    raise SystemExit(1)


def parse_permissions(lines: list[str], index: int, indent: int) -> dict[str, str]:
    match = re.match(rf'^ {{{indent}}}permissions:\s*(.*?)\s*$', lines[index])
    if not match:
        fail(f'internal parser mismatch at line {index + 1}')
    inline = match.group(1)
    if inline:
        if inline == '{}':
            return {}
        fail(f'permissions must be a mapping or {{}} at line {index + 1}')

    result: dict[str, str] = {}
    for line in lines[index + 1:]:
        if not line.strip() or line.lstrip().startswith('#'):
            continue
        current_indent = len(line) - len(line.lstrip(' '))
        if current_indent <= indent:
            break
        item = re.match(
            rf'^ {{{indent + 2}}}([a-z-]+):\s*(read|write|none)\s*(?:#.*)?$',
            line,
        )
        if not item:
            fail(f'unsupported permissions entry: {line.strip()}')
        key, value = item.groups()
        if key not in PERMISSION_KEYS:
            fail(f'unknown permission key {key!r}')
        if key in result:
            fail(f'duplicate permission key {key!r}')
        result[key] = value
    return result


def parse_workflow(path: Path):
    lines = path.read_text(encoding='utf-8').splitlines()
    top_indexes = [i for i, line in enumerate(lines) if line.startswith('permissions:')]
    if len(top_indexes) != 1:
        fail(f'{path}: expected one explicit top-level permissions block, found {len(top_indexes)}')
    top = parse_permissions(lines, top_indexes[0], 0)

    try:
        jobs_index = lines.index('jobs:')
    except ValueError:
        fail(f'{path}: missing jobs mapping')

    starts: list[tuple[str, int]] = []
    for index in range(jobs_index + 1, len(lines)):
        match = re.match(r'^  ([A-Za-z0-9_-]+):\s*$', lines[index])
        if match:
            starts.append((match.group(1), index))
    if not starts:
        fail(f'{path}: no jobs found')

    jobs = {}
    for position, (name, start) in enumerate(starts):
        end = starts[position + 1][1] if position + 1 < len(starts) else len(lines)
        block = lines[start:end]
        permission_indexes = [
            start + offset
            for offset, line in enumerate(block)
            if line.startswith('    permissions:')
        ]
        if len(permission_indexes) > 1:
            fail(f'{path}:{name}: duplicate job permissions block')
        override = (
            parse_permissions(lines, permission_indexes[0], 4)
            if permission_indexes else None
        )
        jobs[name] = {'override': override, 'lines': block}
    return top, jobs


paths = sorted([*WORKFLOW_DIR.glob('*.yml'), *WORKFLOW_DIR.glob('*.yaml')])
actual_names = {path.name for path in paths}
if actual_names != set(EXPECTED_TOP):
    missing = sorted(set(EXPECTED_TOP) - actual_names)
    unexpected = sorted(actual_names - set(EXPECTED_TOP))
    fail(f'workflow inventory drift (missing={missing}, unexpected={unexpected})')

observed_overrides = {}
parsed = {}
for path in paths:
    top, jobs = parse_workflow(path)
    parsed[path.name] = (top, jobs)
    if top != EXPECTED_TOP[path.name]:
        fail(f'{path}: top-level permissions {top} != expected {EXPECTED_TOP[path.name]}')
    for name, job in jobs.items():
        if job['override'] is not None:
            observed_overrides[(path.name, name)] = job['override']
        if not top and job['override'] is None:
            fail(f'{path}:{name}: deny-all workflow requires an explicit job declaration')

if observed_overrides != EXPECTED_JOB_OVERRIDES:
    missing = sorted(set(EXPECTED_JOB_OVERRIDES) - set(observed_overrides))
    unexpected = sorted(set(observed_overrides) - set(EXPECTED_JOB_OVERRIDES))
    changed = sorted(
        key for key in set(observed_overrides) & set(EXPECTED_JOB_OVERRIDES)
        if observed_overrides[key] != EXPECTED_JOB_OVERRIDES[key]
    )
    fail(f'job-permission drift (missing={missing}, unexpected={unexpected}, changed={changed})')


def effective_permissions(workflow: str, job: str) -> dict[str, str]:
    top, jobs = parsed[workflow]
    return jobs[job]['override'] if jobs[job]['override'] is not None else top


ORDINARY_READ_ONLY_WORKFLOWS = {
    'action-selftest.yml',
    'ci.yml',
    'coverage.yml',
    'cross-platform.yml',
    'editors-e2e.yml',
    'issue-26-e2e.yml',
    'mutants.yml',
}

# These workflows formerly inherited the repository's write default. Pin both
# sides of the intended negative contract: they can read source, and no job can
# write repository contents, Actions state, or pull requests. Because GitHub
# sets every omitted scope to `none` when a permissions mapping is present,
# the exact map is stronger than merely checking that no `write` value appears.
for workflow in ORDINARY_READ_ONLY_WORKFLOWS:
    _, jobs = parsed[workflow]
    for job_name in jobs:
        effective = effective_permissions(workflow, job_name)
        if effective != {'contents': 'read'}:
            fail(
                f'{workflow}:{job_name}: ordinary job must have only '
                f'contents: read; got {effective}'
            )


EXTERNAL_WRITE_EXEMPTIONS = {
    # This push targets the separate homebrew-alint repository over SSH with a
    # dedicated deploy key; the current repository token stays contents: read.
    ('release.yml', 'homebrew', 'git push'): (
        'ssh-private-key: ${{ secrets.HOMEBREW_TAP_DEPLOY_KEY }}',
        'git clone git@github.com:asamarts/homebrew-alint.git tap',
    ),
}


def require_write(
    workflow: str,
    job: str,
    permission: str,
    marker: str,
    code: str,
) -> None:
    effective = effective_permissions(workflow, job)
    if effective.get(permission) != 'write':
        exemption = EXTERNAL_WRITE_EXEMPTIONS.get((workflow, job, marker))
        if exemption and all(token in code for token in exemption):
            return
        fail(f'{workflow}:{job}: {marker!r} requires {permission}: write; got {effective}')


WRITE_INDICATORS = (
    (re.compile(r'\bgit\s+push\b'), 'contents', 'git push'),
    (re.compile(r'\bgh\s+release\s+(?:create|upload|edit|delete)\b'), 'contents', 'gh release mutation'),
    (re.compile(r'\bgh\s+pr\s+create\b'), 'pull-requests', 'gh pr create'),
    (re.compile(r'\bgh\s+workflow\s+run\b'), 'actions', 'workflow dispatch'),
    (re.compile(r'^\s*push:\s*true\s*$', re.MULTILINE), 'packages', 'registry push'),
    (re.compile(r'actions/attest-build-provenance@'), 'attestations', 'GitHub attestation'),
)

for workflow, (_, jobs) in parsed.items():
    for job_name, job in jobs.items():
        code = '\n'.join(
            line for line in job['lines']
            if not line.lstrip().startswith('#')
        )
        for pattern, permission, marker in WRITE_INDICATORS:
            if pattern.search(code):
                require_write(workflow, job_name, permission, marker, code)

# Review approval is intentionally prohibited while GitHub's combined
# create/approve setting must remain enabled for bench-record's `gh pr create`.
approval_patterns = (
    re.compile(r'\bgh\s+pr\s+review\b[^\n]*--approve\b', re.IGNORECASE),
    re.compile(r'\b(?:event|state)\s*[:=]\s*["\']?APPROVE\b'),
    re.compile(r'\b(?:add|submit)PullRequestReview\b'),
    re.compile(r'/pulls/[^\s"\']+/reviews\b'),
)
raw_mutation_patterns = (
    re.compile(r'\bgh\s+api\b[^\n]*(?:--method|-X)\s+(?:POST|PUT|PATCH|DELETE)\b', re.IGNORECASE),
    re.compile(r'\bgh\s+api\b[^\n]*(?:^|\s)(?:-f|-F|--field|--raw-field|--input)(?:\s|=)', re.IGNORECASE),
    re.compile(r'\bgh\s+api\s+graphql\b[^\n]*\bmutation\b', re.IGNORECASE),
    re.compile(r'\bcurl\b[^\n]*(?:-X|--request)\s+(?:POST|PUT|PATCH|DELETE)\b[^\n]*api\.github\.com', re.IGNORECASE),
    re.compile(r'\bcurl\b[^\n]*(?:-d|--data(?:-ascii|-binary|-raw|-urlencode)?|--json)(?:\s|=)[^\n]*api\.github\.com', re.IGNORECASE),
    re.compile(r'\bcurl\b[^\n]*api\.github\.com[^\n]*(?:-d|--data(?:-ascii|-binary|-raw|-urlencode)?|--json)(?:\s|=)', re.IGNORECASE),
)
SOURCE_SUFFIXES = {
    '.bash', '.js', '.json', '.mjs', '.py', '.rs', '.sh', '.toml', '.ts',
    '.tsx', '.yaml', '.yml',
}
# Cover workflow files and the repository programs they can invoke. Host-side
# runner lifecycle utilities under ci/runner are an operator boundary: they do
# not execute inside a workflow and use a separate registration credential.
SOURCE_ROOTS = (Path('.github'), Path('ci/scripts'), Path('scripts'), Path('xtask'))
approval_sources = set(paths)
for root in SOURCE_ROOTS:
    if not root.exists():
        continue
    approval_sources.update(
        path for path in root.rglob('*')
        if path.is_file() and path.suffix in SOURCE_SUFFIXES
    )
approval_sources.add(Path('install.sh'))
approval_sources = sorted(path for path in approval_sources if path.exists())
for source in approval_sources:
    if source.name.startswith('test-workflow-permissions'):
        continue
    text = '\n'.join(
        line for line in source.read_text(encoding='utf-8').splitlines()
        if not line.lstrip().startswith('#')
    )
    # A shell backslash continuation is one command at runtime. Scan both the
    # source form and that logical form so moving a prohibited flag to the next
    # line cannot evade the policy.
    scan_forms = (text, re.sub(r'\\\s*\n\s*', ' ', text))
    for pattern in approval_patterns:
        if any(pattern.search(form) for form in scan_forms):
            fail(f'{source}: pull-request approval operation is prohibited')
    for pattern in raw_mutation_patterns:
        if any(pattern.search(form) for form in scan_forms):
            fail(f'{source}: raw GitHub API mutation requires an explicit policy mapping')

# Self-test the two policy engines so a future refactor cannot make the gate
# vacuously green while still passing the current repository.
if not WRITE_INDICATORS[0][0].search('git push origin main'):
    fail('internal write-indicator fixture was not detected')
if not approval_patterns[0].search('gh pr review 7 --approve'):
    fail('internal approval-operation fixture was not detected')
if not raw_mutation_patterns[0].search('gh api repos/o/r/issues/1 --method PATCH'):
    fail('internal raw-mutation fixture was not detected')

print(
    f'[workflow-permissions] OK — {len(paths)} workflows, '
    f'{sum(len(jobs) for _, jobs in parsed.values())} jobs, '
    f'{len(approval_sources)} runtime source files, explicit least privilege'
)
PY
