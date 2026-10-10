#!/usr/bin/env bash
# Pin the release rollback guard (ci/scripts/release-pointer-guard.sh) and its
# use by every step of release.yml that moves a FLOATING pointer: the major tag
# (v0), the GitHub "Latest" badge, Docker :latest / :X.Y, the Homebrew formula
# and npm's `latest` dist-tag. A backport release or a re-run of an old tag's job must never move
# any of them backwards.
#
# Part 1 is a table test of the comparison. Part 2 extracts each guarded step's
# real run: script from release.yml (ci/scripts/workflow_step.py), runs it with
# recording stubs for "highest" and "not highest", and asserts what moved; it
# also checks that each step's guard env var is wired to the output of a step
# that actually runs the helper on the release tag.
#
# Stub bodies are single-quoted on purpose: they expand when the stub runs.
# shellcheck disable=SC2016
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"
WF=.github/workflows/release.yml
GUARD="$REPO_ROOT/ci/scripts/release-pointer-guard.sh"
EXTRACT="$REPO_ROOT/ci/scripts/workflow_step.py"

sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
pass=0
fail=0
ok() { echo "  ok: $1"; pass=$((pass + 1)); }
bad() { echo "  FAIL: $1" >&2; fail=$((fail + 1)); }

# ── Part 1: the comparison ────────────────────────────────────────────
# case <label> <tag> <published tags> <want overall> <want in-minor>
case_() {
  local label=$1 tag=$2 tags=$3 want_o=$4 want_m=$5 out
  if ! out=$(RELEASE_TAGS="$tags" bash "$GUARD" "$tag" 2>&1); then
    bad "$label: guard exited non-zero: $out"; return
  fi
  if grep -qx "highest_overall=$want_o" <<< "$out" && grep -qx "highest_in_minor=$want_m" <<< "$out"; then
    ok "$label"
  else
    bad "$label: want overall=$want_o in_minor=$want_m, got: $(grep highest_ <<< "$out" | tr '\n' ' ')"
  fi
}
case_ "newer release"                   v0.18.0 "v0.16.0 v0.17.0"           true  true
case_ "equal to highest (re-run)"       v0.17.0 "v0.16.0 v0.17.0"           true  true
case_ "first ever release"              v0.1.0  ""                          true  true
case_ "backport, newest of its minor"   v0.16.2 "v0.16.0 v0.16.1 v0.17.0"   false true
case_ "re-run of an old tag"            v0.16.1 "v0.16.1 v0.16.2 v0.17.0"   false false
case_ "older patch, same minor"         v0.17.0 "v0.17.0 v0.17.1"           false false
case_ "numeric, not lexical (0.10>0.9)" v0.10.0 "v0.9.0 v0.9.9"             true  true
case_ "numeric, not lexical (0.9<0.10)" v0.9.5  "v0.9.0 v0.10.0"            false true
case_ "older major"                     v0.99.0 "v0.98.0 v1.0.0"            false true
case_ "pre-release tag never moves"     v0.18.0-rc.1 "v0.17.0"              false false
case_ "pre-releases in the list ignored" v0.17.0 "v0.17.0 v0.18.0-rc.1 v0" true  true

# Fail closed: without RELEASE_TAGS the guard lists remote tags; if that
# fails it must exit non-zero rather than default to "highest".
mkdir -p "$sandbox/failbin"
printf '#!/usr/bin/env bash\nexit 128\n' > "$sandbox/failbin/git"
printf '#!/usr/bin/env bash\nexit 0\n' > "$sandbox/failbin/sleep"
chmod +x "$sandbox/failbin/git" "$sandbox/failbin/sleep"
if env -u RELEASE_TAGS PATH="$sandbox/failbin:$PATH" bash "$GUARD" v9.9.9 > "$sandbox/out" 2>&1; then
  bad "guard must fail closed when the remote tags cannot be listed"
else
  ok "guard fails closed when the remote tags cannot be listed"
fi

# ── Part 2: every floating-pointer step honours the guard ─────────────
mkdir -p "$sandbox/bin"
stub() { printf '#!/usr/bin/env bash\necho "%s $*" >> "$STUB_LOG"\n%s\n' "$1" "$2" > "$sandbox/bin/$1"; chmod +x "$sandbox/bin/$1"; }
stub git 'if [[ "$1 $2 $3" == "diff --cached --quiet" ]]; then exit 1; fi; exit 0'
stub gh '
if [[ "$1 $2" == "release view" ]]; then
  case " $* " in
    *" --json assets "*) ls -1 "${GH_ASSET_DIR:?}" | sort; exit 0 ;;
    *" --json isDraft "*) echo false; exit 0 ;;
  esac
  exit 1
fi
exit 0'

# run_step <job> <step> <workdir> [VAR=value...]: sets $rc; stubs log to $sandbox/log
run_step() {
  local job=$1 name=$2 dir=$3
  shift 3
  python3 "$EXTRACT" "$WF" "$job" "$name" run > "$sandbox/step.sh"
  : > "$sandbox/log"
  : > "$sandbox/gh_output"
  rc=0
  (cd "$dir" && env PATH="$sandbox/bin:$PATH" STUB_LOG="$sandbox/log" \
     GITHUB_OUTPUT="$sandbox/gh_output" GITHUB_REF_NAME=v0.16.2 GITHUB_SHA=deadbeef "$@" \
     bash --noprofile --norc -eo pipefail "$sandbox/step.sh") > "$sandbox/out" 2>&1 || rc=$?
}
# Predicates over the last run_step.
logged() { grep -qE -- "$1" "$sandbox/log"; }
silent() { ! grep -q . "$sandbox/log"; }
noticed() { grep -q '::notice::' "$sandbox/out"; }
docker_tags() { [[ "$(sed -n 's/^tags=//p' "$sandbox/gh_output" | tr ',' '\n' | sed 's/.*://' | sort | tr '\n' ' ')" == "$1 " ]]; }
# check <label> <predicate> [args...]: the step must succeed AND the predicate hold.
check() {
  local label=$1
  shift
  if [[ "$rc" -eq 0 ]] && "$@"; then ok "$label"; else
    bad "$label (exit $rc)"; sed 's/^/      | /' "$sandbox/out" "$sandbox/log" >&2
  fi
}
backport_skipped() { silent && noticed; }

# major tag (v0)
MAJ='move the major tag (e.g. v0) to this release'
run_step release "$MAJ" "$sandbox" HIGHEST_OVERALL=true
check "major tag: highest release moves v0" logged '^git push -f origin refs/tags/v0$'
run_step release "$MAJ" "$sandbox" HIGHEST_OVERALL=false
check "major tag: backport leaves v0 alone (notice)" backport_skipped

# Docker :X.Y and :latest
DOCK='derive image tags'
run_step docker "$DOCK" "$sandbox" TAG=v0.16.2 OWNER=Owner HIGHEST_OVERALL=true HIGHEST_IN_MINOR=true
check "docker: highest release tags :0.16 + :latest" docker_tags "0.16 0.16.2 latest v0.16.2"
run_step docker "$DOCK" "$sandbox" TAG=v0.16.2 OWNER=Owner HIGHEST_OVERALL=false HIGHEST_IN_MINOR=true
check "docker: backport moves :0.16 but not :latest" docker_tags "0.16 0.16.2 v0.16.2"
run_step docker "$DOCK" "$sandbox" TAG=v0.16.2 OWNER=Owner HIGHEST_OVERALL=false HIGHEST_IN_MINOR=false
check "docker: old-tag re-run moves neither floating tag" docker_tags "0.16.2 v0.16.2"

# Homebrew formula
mkdir -p "$sandbox/tap"
run_step homebrew 'commit + push' "$sandbox" TAG=v0.16.2 HIGHEST_OVERALL=true
check "homebrew: highest release pushes the formula" logged '^git push origin main$'
run_step homebrew 'commit + push' "$sandbox" TAG=v0.16.2 HIGHEST_OVERALL=false
check "homebrew: backport leaves the tap alone (notice)" backport_skipped

# GitHub "Latest" badge (install.sh / Action default version)
mkdir -p "$sandbox/rel/release-artifacts"
for f in install.sh SHA256SUMS SHA256SUMS.cosign.bundle THIRD-PARTY-LICENSES.html alint.cdx.json \
         alint-v0.16.2-x.tar.gz alint-v0.16.2-x.tar.gz.sha256; do : > "$sandbox/rel/release-artifacts/$f"; done
REL='create (or, on re-run, refresh) the GitHub Release'
REL_ENV=(TAG=v0.16.2 REPO=o/r GH_ASSET_DIR="$sandbox/rel/release-artifacts")
run_step release "$REL" "$sandbox/rel" "${REL_ENV[@]}" HIGHEST_OVERALL=true
check "release: highest release is marked Latest" logged '^gh release create v0\.16\.2 .*--latest=true( |$)'
run_step release "$REL" "$sandbox/rel" "${REL_ENV[@]}" HIGHEST_OVERALL=false
check "release: backport is not marked Latest" logged '^gh release create v0\.16\.2 .*--latest=false( |$)'

# npm `latest` dist-tag
NPM_STEP='stamp package version from tag + publish'
mkdir -p "$sandbox/npmw/npm"
echo '{"name":"@asamarts/alint","version":"0.0.0"}' > "$sandbox/npmw/npm/package.json"
printf '#!/usr/bin/env bash\necho "npm $*" >> "$STUB_LOG"\n[[ "$1" == view ]] && exit 1\nexit 0\n' > "$sandbox/bin/npm"
printf '#!/usr/bin/env bash\nexit 0\n' > "$sandbox/bin/node"
chmod +x "$sandbox/bin/npm" "$sandbox/bin/node"
run_step publish-npm "$NPM_STEP" "$sandbox/npmw" TAG=v0.16.2 HIGHEST_OVERALL=true
check "npm: highest release moves the latest dist-tag" logged '^npm publish .*--tag latest$'
run_step publish-npm "$NPM_STEP" "$sandbox/npmw" TAG=v0.16.2 HIGHEST_OVERALL=false
check "npm: backport publishes under release-0.16, not latest" logged '^npm publish .*--tag release-0\.16$'

# Wiring: each guarded step's env var must come from the output of a step in
# the SAME job that runs the helper on the release tag.
python3 - "$WF" "$EXTRACT" <<'PY' || fail=$((fail + 1))
import re
import subprocess
import sys

wf, extract = sys.argv[1], sys.argv[2]


def step(job, name, what):
    return subprocess.run([sys.executable, extract, wf, job, name, what],
                          check=True, capture_output=True, text=True).stdout


GUARDED = [
    ('release', 'create (or, on re-run, refresh) the GitHub Release', {'HIGHEST_OVERALL': 'highest_overall'}),
    ('release', 'move the major tag (e.g. v0) to this release', {'HIGHEST_OVERALL': 'highest_overall'}),
    ('docker', 'derive image tags', {'HIGHEST_OVERALL': 'highest_overall', 'HIGHEST_IN_MINOR': 'highest_in_minor'}),
    ('homebrew', 'commit + push', {'HIGHEST_OVERALL': 'highest_overall'}),
    ('publish-npm', 'stamp package version from tag + publish', {'HIGHEST_OVERALL': 'highest_overall'}),
]
failures = []
for job, name, wants in GUARDED:
    env = dict(l.split('\t', 1) for l in step(job, name, 'env').splitlines() if '\t' in l)
    for var, output in wants.items():
        m = re.fullmatch(r'\$\{\{\s*steps\.([\w-]+)\.outputs\.' + output + r'\s*\}\}', env.get(var, ''))
        if not m:
            failures.append(f'{job} / {name}: {var} must be ${{{{ steps.<guard>.outputs.{output} }}}}')
            continue
        guard_id = m.group(1)
        g_env = dict(l.split('\t', 1) for l in step(job, guard_id, 'env').splitlines() if '\t' in l)
        if step(job, guard_id, 'run').strip() != 'ci/scripts/release-pointer-guard.sh "$TAG"' \
                or g_env.get('TAG') != '${{ github.ref_name }}' or step(job, guard_id, 'if').strip():
            failures.append(f'{job}: step {guard_id!r} must unconditionally run '
                            'ci/scripts/release-pointer-guard.sh "$TAG" with TAG=${{ github.ref_name }}')
for f in failures:
    print(f'  FAIL: {f}', file=sys.stderr)
sys.exit(1 if failures else 0)
PY

echo "[release-pointer-guard] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
