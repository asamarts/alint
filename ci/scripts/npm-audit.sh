#!/usr/bin/env bash
# npm advisory gate for the VS Code extension (editors/vscode). BLOCKING.
#
#   - Runtime dependencies, which the extension ships, may have no advisory at
#     moderate or above. No waiver applies to them.
#   - Development dependencies (the packaging, publishing and test tools) may
#     have none either, except an advisory listed in
#     editors/vscode/npm-audit-waivers.json with a reason. A waiver is for an
#     advisory no released version fixes yet; drop it once one does.
#
# `npm run audit` in editors/vscode runs this, in CI and in the release
# workflow.
#
# Usage:
#   ci/scripts/npm-audit.sh                    # audit editors/vscode
#   ci/scripts/npm-audit.sh --check-json FILE  # judge a saved `npm audit --json`
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
EXTENSION="$REPO_ROOT/editors/vscode"
WAIVERS="${NPM_AUDIT_WAIVERS:-$EXTENSION/npm-audit-waivers.json}"

# Every advisory at moderate or above in a full `npm audit --json` report ($1)
# must be waived in $WAIVERS; each waiver needs a GHSA id and a reason.
judge() {
  python3 - "$1" "$WAIVERS" <<'PY'
import json
import re
import sys

report_path, waivers_path = sys.argv[1], sys.argv[2]
with open(waivers_path) as f:
    waivers = json.load(f).get('waivers', [])
waived = {}
for entry in waivers:
    ident = entry.get('id') if isinstance(entry, dict) else None
    if not isinstance(ident, str) or not re.fullmatch(r'GHSA(-[0-9a-z]{4}){3}', ident) \
            or not str(entry.get('reason', '')).strip():
        sys.exit(f'[npm-audit] each waiver must be {{"id": "GHSA-...", "reason": "..."}}; '
                 f'rejected: {entry}')
    waived[ident] = entry['reason']

with open(report_path) as f:
    report = json.load(f)
if 'vulnerabilities' not in report:
    sys.exit(f'[npm-audit] not an `npm audit --json` report: {report_path}')

blocking = {'moderate', 'high', 'critical'}
found = {}
for vulnerability in report['vulnerabilities'].values():
    for via in vulnerability.get('via', []):
        # A string names the dependency the advisory came through; the
        # advisory itself is an object.
        if not isinstance(via, dict) or via.get('severity') not in blocking:
            continue
        ident = str(via.get('url', '')).rstrip('/').rsplit('/', 1)[-1]
        found.setdefault(ident, f"{via.get('name')} ({via.get('severity')}): {via.get('title')}")

unwaived = sorted(ident for ident in found if ident not in waived)
for ident in sorted(found):
    if ident in waived:
        print(f'[npm-audit] waived {ident}, {found[ident]}: {waived[ident]}')
for ident in sorted(set(waived) - set(found)):
    print(f'[npm-audit] warning: {ident} is waived but no longer reported; remove its waiver')
if unwaived:
    for ident in unwaived:
        print(f'[npm-audit] {ident}, {found[ident]}', file=sys.stderr)
    sys.exit(f'[npm-audit] {len(unwaived)} advisory(ies) not waived in {waivers_path}')
print(f'[npm-audit] ok: {len(found)} advisory(ies), all waived')
PY
}

if [[ "${1:-}" == "--check-json" ]]; then
  judge "${2:?--check-json needs a file}"
  exit 0
fi

cd "$EXTENSION"
echo "==> npm audit of the runtime dependencies (no waivers)"
npm audit --omit=dev --audit-level=moderate

report="$(mktemp)"
trap 'rm -f "$report"' EXIT
echo "==> npm audit of every dependency (waivers: ${WAIVERS#"$REPO_ROOT"/})"
# npm audit exits 1 when it reports anything; the judge decides.
npm audit --json > "$report" || true
judge "$report"
