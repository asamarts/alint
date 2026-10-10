#!/usr/bin/env bash
# Pin the re-runnability contract of .github/workflows/release.yml.
#
# Recovery from a partial release is always `gh run rerun <id> --failed`, never
# a re-tag (RELEASING.md "Recovering a partial release"). That only works if
# every publishing step tolerates "this version is already out there".
#
# BEHAVIOURAL: each publishing step's real `run:` script is extracted from
# release.yml (ci/scripts/workflow_step.py) and executed under bash -eo
# pipefail (the Actions `shell: bash` flags) against stubbed gh / npm / npx /
# node / curl / git / gradlew that record their argv. A deleted `exit 0`, a
# flipped comparison or a flag moved behind a `#` comment changes what the
# stubs record, so it fails here; grepping the YAML text could not see that.
#
# Stub bodies are single-quoted on purpose: they expand when the stub runs.
# shellcheck disable=SC2016
set -euo pipefail

REPO_ROOT="${RELEASE_IDEMPOTENCY_REPO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$REPO_ROOT"
WF=.github/workflows/release.yml
EXTRACT="$REPO_ROOT/ci/scripts/workflow_step.py"

sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
pass=0
fail=0

# ── Stubs ─────────────────────────────────────────────────────────────
# Every stub appends "<tool> <argv>" to $STUB_LOG; behaviour is driven by env.
mkdir -p "$sandbox/bin"
stub() { printf '#!/usr/bin/env bash\necho "%s $*" >> "$STUB_LOG"\n%s\n' "$1" "$2" > "$sandbox/bin/$1"; chmod +x "$sandbox/bin/$1"; }
{
stub npm 'if [[ "$1" == view ]]; then printf "%s\n" "${NPM_VIEW_OUT:-}"; [[ -n "${NPM_VIEW_OUT:-}" ]] || exit 1; fi; exit 0'
stub node 'exit 0'
stub npx 'exit 0'
stub curl 'if [[ "${CURL_FAIL:-0}" == 1 ]]; then exit 22; fi; printf "%s\n" "${CURL_OUT:-}"'
stub git 'if [[ "$1 $2 $3" == "diff --cached --quiet" ]]; then exit "${GIT_DIFF_RC:-0}"; fi; exit 0'
stub gh '
if [[ "$1 $2" == "release view" ]]; then
  case " $* " in
    *" --json assets "*) ls -1 "${GH_ASSET_DIR:?}" | grep -v "^${GH_DROP_ASSET:-<none>}\$" | sort; exit 0 ;;
    *" --json isDraft "*) echo "${GH_IS_DRAFT:-false}"; exit 0 ;;
  esac
  [[ "${GH_RELEASE_EXISTS:-0}" == 1 ]] && exit 0 || exit 1
fi
exit 0'
}

# run_step <job> <step name> <workdir> [VAR=value...]: run the extracted step
# script with the stubs first on PATH; sets $rc and $log.
run_step() {
  local job=$1 name=$2 dir=$3
  shift 3
  python3 "$EXTRACT" "$WF" "$job" "$name" run > "$sandbox/step.sh"
  : > "$sandbox/log"
  rc=0
  (cd "$dir" && env PATH="$sandbox/bin:$PATH" STUB_LOG="$sandbox/log" "$@" \
     bash --noprofile --norc -eo pipefail "$sandbox/step.sh") > "$sandbox/out" 2>&1 || rc=$?
  log=$(cat "$sandbox/log")
}

ok() { echo "  ok: $1"; pass=$((pass + 1)); }
bad() {
  echo "  FAIL: $1" >&2
  sed 's/^/      out| /' "$sandbox/out" >&2
  printf '%s\n' "$log" | sed 's/^/      log| /' >&2
  fail=$((fail + 1))
}
# expect <label> <rc-want: 0|nonzero> <grep -E that must match log or ''> <grep -E that must NOT match or ''>
expect() {
  local label=$1 want_rc=$2 must=$3 mustnot=$4
  if [[ "$want_rc" == 0 && "$rc" -ne 0 ]] || [[ "$want_rc" == nonzero && "$rc" -eq 0 ]]; then
    bad "$label (exit $rc, want $want_rc)"; return
  fi
  if [[ -n "$must" ]] && ! grep -qE -- "$must" <<< "$log"; then bad "$label (no call matching /$must/)"; return; fi
  if [[ -n "$mustnot" ]] && grep -qE -- "$mustnot" <<< "$log"; then bad "$label (unexpected call matching /$mustnot/)"; return; fi
  ok "$label"
}

# ── 1. GitHub Release: create, or refresh an existing one ─────────────
REL_STEP='create (or, on re-run, refresh) the GitHub Release'
rel="$sandbox/rel"
mkdir -p "$rel/release-artifacts"
for f in install.sh SHA256SUMS SHA256SUMS.cosign.bundle THIRD-PARTY-LICENSES.html alint.cdx.json \
         alint-v1.2.3-x86_64-unknown-linux-musl.tar.gz alint-v1.2.3-x86_64-unknown-linux-musl.tar.gz.sha256; do
  : > "$rel/release-artifacts/$f"
done
REL_ENV=(TAG=v1.2.3 REPO=o/r GH_ASSET_DIR="$rel/release-artifacts")
run_step release "$REL_STEP" "$rel" "${REL_ENV[@]}" GH_RELEASE_EXISTS=0
expect "release: first run creates the Release" 0 '^gh release create v1\.2\.3 ' '^gh release upload'
run_step release "$REL_STEP" "$rel" "${REL_ENV[@]}" GH_RELEASE_EXISTS=1
expect "release: re-run refreshes assets with --clobber, never re-creates" 0 \
  '^gh release upload v1\.2\.3 .*--clobber( |$)' '^gh release create'
run_step release "$REL_STEP" "$rel" "${REL_ENV[@]}" GH_RELEASE_EXISTS=1 GH_DROP_ASSET=SHA256SUMS
expect "release: a drifted asset set fails the step" nonzero '' ''

# ── 2. npm: skip a version the registry already serves ────────────────
NPM_STEP='stamp package version from tag + publish'
mkdir -p "$sandbox/npmw/npm"
echo '{"name":"@asamarts/alint","version":"0.0.0"}' > "$sandbox/npmw/npm/package.json"
run_step publish-npm "$NPM_STEP" "$sandbox/npmw" TAG=v1.2.3 NPM_VIEW_OUT=1.2.3
expect "npm: already-published version is skipped (exit 0, no publish)" 0 '^npm view @asamarts/alint@1\.2\.3 version' '^npm publish'
run_step publish-npm "$NPM_STEP" "$sandbox/npmw" TAG=v1.2.3 NPM_VIEW_OUT=
expect "npm: unpublished version is published" 0 '^npm publish --access public --provenance$' ''
run_step publish-npm "$NPM_STEP" "$sandbox/npmw" TAG=v1.2.3 NPM_VIEW_OUT=1.2.2
expect "npm: an older registry version does not suppress the publish" 0 '^npm publish ' ''

# ── 3. VS Code / Open VSX: --skip-duplicate is a real argument ────────
mkdir -p "$sandbox/vsc/editors/vscode"
echo '{"name":"alint","version":"0.0.0"}' > "$sandbox/vsc/editors/vscode/package.json"
run_step publish-vscode 'stamp version from tag + publish to both registries' "$sandbox/vsc" TAG=v1.2.3
expect "vscode: vsce publish passes --skip-duplicate" 0 '^npx .*vsce publish( .*)? --skip-duplicate( |$)' ''
expect "vscode: ovsx publish passes --skip-duplicate" 0 '^npx .*ovsx publish( .*)? --skip-duplicate( |$)' ''

# ── 4. JetBrains: skip a version the Marketplace feed already lists ───
JB_STEP='stamp version from tag + publish'
mkdir -p "$sandbox/jb/editors/jetbrains"
# shellcheck disable=SC2016
printf '#!/usr/bin/env bash\necho "gradlew $*" >> "$STUB_LOG"\n' > "$sandbox/jb/editors/jetbrains/gradlew"
chmod +x "$sandbox/jb/editors/jetbrains/gradlew"
run_step publish-jetbrains "$JB_STEP" "$sandbox/jb" TAG=v1.2.3 CURL_OUT='<plugin><version>1.2.3</version></plugin>'
expect "jetbrains: listed version is skipped" 0 '^curl .*pluginId=org\.alint\.lsp' '^gradlew'
run_step publish-jetbrains "$JB_STEP" "$sandbox/jb" TAG=v1.2.3 CURL_OUT='<plugin><version>1.2.2</version></plugin>'
expect "jetbrains: unlisted version is published" 0 '^gradlew -PpluginVersion=1\.2\.3 publishPlugin$' ''
run_step publish-jetbrains "$JB_STEP" "$sandbox/jb" TAG=v1.2.3 CURL_FAIL=1
expect "jetbrains: a feed lookup failure falls through to the publish" 0 '^gradlew .*publishPlugin' ''

# ── 5. Homebrew: an unchanged formula is a no-op, not a push ──────────
mkdir -p "$sandbox/hb/tap"
run_step homebrew 'commit + push' "$sandbox/hb" TAG=v1.2.3 GIT_DIFF_RC=0
expect "homebrew: unchanged formula pushes nothing" 0 '^git diff --cached --quiet' '^git (commit|push)'
run_step homebrew 'commit + push' "$sandbox/hb" TAG=v1.2.3 GIT_DIFF_RC=1
expect "homebrew: changed formula is committed and pushed" 0 '^git push origin main$' ''

# ── Non-run guards (plain config / helper script) ─────────────────────
python3 - "$WF" <<'PY' || fail=$((fail + 1))
from pathlib import Path
import re
import sys

failures = []
text = Path(sys.argv[1]).read_text(encoding='utf-8')
# Strip full-line and trailing comments so a commented-out key cannot count.
code = '\n'.join(re.sub(r'\s+#.*$', '', l) for l in text.splitlines() if not l.lstrip().startswith('#'))
m = re.search(r'^  publish-pypi:\n(?P<body>.*?)(?=^  [A-Za-z0-9_-]+:\n|\Z)', code, re.MULTILINE | re.DOTALL)
if not m or not re.search(r'^\s+skip-existing:\s*true\s*$', m.group('body'), re.MULTILINE):
    failures.append('publish-pypi: the PyPI publish must set skip-existing: true')
if not re.search(r'if ! gh workflow run docs-bundle\.yml', code):
    failures.append('release: the docs-bundle dispatch must be non-fatal (if ! ... ::warning::)')
if 'already published; skipping' not in Path('ci/scripts/publish-crates.sh').read_text(encoding='utf-8'):
    failures.append('publish-crates.sh: must skip crates already on crates.io')
for f in failures:
    print(f'  FAIL: {f}', file=sys.stderr)
sys.exit(1 if failures else 0)
PY

echo "[release-idempotency] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
