#!/usr/bin/env bash
# Pre-merge validation for the distribution-packaging files that no Rust or
# docs gate covers: install.sh, the npm shim (npm/**), the release Dockerfile
# (+ .dockerignore) and .pre-commit-hooks.yaml. ci.yml's `packaging` job runs
# this whenever detect-changes.sh reports packaging=true; release.yml's
# preflight runs it unconditionally.
#
# Each section is independent; all failures are reported before exiting.
# Tools that a hosted runner always has (shellcheck, node, python3, docker) are
# REQUIRED under CI=true and skipped with a note locally when missing.
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT" || exit 1

failed=0
note() { printf '[packaging] %s\n' "$*"; }
bad() { printf '[packaging] FAIL: %s\n' "$*" >&2; failed=$((failed + 1)); }
have() {
  if command -v "$1" >/dev/null 2>&1; then return 0; fi
  if [[ "${CI:-}" == "true" ]]; then bad "$1 is required in CI"; else note "skip: $1 not installed"; fi
  return 1
}

# ── install.sh ────────────────────────────────────────────────────────
bash -n install.sh || bad "install.sh: bash -n syntax check"
if have shellcheck; then
  shellcheck install.sh || bad "install.sh: shellcheck"
fi

# ── npm shim ──────────────────────────────────────────────────────────
if have node; then
  for f in npm/install.js npm/bin/alint.js; do
    node --check "$f" || bad "$f: node --check"
  done
  node -e 'JSON.parse(require("fs").readFileSync("npm/package.json","utf8"))' \
    || bad "npm/package.json: invalid JSON"
  bash ci/scripts/test-npm-shim.sh || bad "npm shim: os/cpu -> release target table"
fi

# ── .pre-commit-hooks.yaml ────────────────────────────────────────────
if have python3; then
  python3 - <<'PY' || bad ".pre-commit-hooks.yaml: schema check"
import re
import sys

# Plain-text parse (no PyYAML dependency on the runner): a top-level list of
# flat hook mappings, each starting at `- id:`.
text = open('.pre-commit-hooks.yaml', encoding='utf-8').read()
blocks = re.split(r'^- ', text, flags=re.MULTILINE)
if blocks[0].strip() and not all(l.lstrip().startswith('#') for l in blocks[0].splitlines() if l.strip()):
    sys.exit('.pre-commit-hooks.yaml must be a top-level list of hooks')
hooks = []
for block in blocks[1:]:
    hook = {}
    for m in re.finditer(r'^\s*([a-z_]+):[ \t]*(\S[^\n]*)?$', block, re.MULTILINE):
        hook.setdefault(m.group(1), (m.group(2) or '').strip())
    hooks.append(hook)
if not hooks:
    sys.exit('.pre-commit-hooks.yaml must be a non-empty list of hooks')
ids = set()
for hook in hooks:
    for key in ('id', 'name', 'entry', 'language'):
        if not hook.get(key):
            sys.exit(f'hook {hook.get("id")!r}: missing/empty {key!r}')
    if hook['id'] in ids:
        sys.exit(f'duplicate hook id {hook["id"]!r}')
    ids.add(hook['id'])
    if not hook['entry'].startswith('alint '):
        sys.exit(f'hook {hook["id"]!r}: entry must invoke the alint binary')
print(f'[packaging] .pre-commit-hooks.yaml OK ({len(hooks)} hooks)')
PY
fi

# ── Dockerfile ────────────────────────────────────────────────────────
# The release image COPYs pre-staged binaries, so build it against a
# throwaway context with placeholder files: this proves the FROM resolves,
# every COPY source survives .dockerignore, and the instructions parse.
if have docker; then
  ctx="$(mktemp -d)"
  trap 'rm -rf "$ctx"' EXIT
  cp Dockerfile .dockerignore LICENSE-APACHE LICENSE-MIT NOTICE "$ctx/"
  mkdir -p "$ctx/linux-amd64"
  printf '#!/bin/sh\n' > "$ctx/linux-amd64/alint"
  printf '<html></html>\n' > "$ctx/THIRD-PARTY-LICENSES.html"
  if docker build --quiet --platform linux/amd64 -t alint-packaging-check:local "$ctx" >/dev/null; then
    docker image rm -f alint-packaging-check:local >/dev/null 2>&1 || true
    note "Dockerfile builds against a staged context"
  else
    bad "Dockerfile: docker build against a staged context"
  fi
fi

if [[ "$failed" -ne 0 ]]; then
  echo "[packaging] $failed check(s) failed" >&2
  exit 1
fi
note "OK"
