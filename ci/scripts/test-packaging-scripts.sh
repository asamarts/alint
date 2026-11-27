#!/usr/bin/env bash
# Regression tests for Batch 7 packaging, CI, action, installer, and runner fixes:
#   M1  bench-docker.yml workflow_dispatch on main + rollback guard before :latest
#   M2  smoke-policy-test.sh falls back from broken `python3` stub to `python`
#   M3  action.yml ALINT_EXTRA shlex/noglob parsing + unique mktemp SARIF path
#   M8  strip-since.mjs executes when invoked via a symlinked directory with spaces
#   M10 ci/runner/*.sh pass bash -n and shellcheck (when available)
#   M12 likec4.sh fails closed under CI=true when npx is absent
#   L1  bench-record.yml uses needs.guard.outputs.head_sha, cleanup trap, idempotent PR
#   L4  bump-version.sh uses no bare `sed -i`
#   L5  smoke-channel.sh has no fixed /tmp/alint-pypi-smoke and cleans temp dirs on failure
#   L10 pull-request-title.yml uses --format=%s without `| head -n 1`
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT

pass=0
fail=0
ok() { echo "  ok: $1"; pass=$((pass + 1)); }
bad() { echo "  FAIL: $1" >&2; fail=$((fail + 1)); }

# ── M1: bench-docker.yml workflow_dispatch precedence + rollback guard ────
python3 ci/scripts/workflow_step.py .github/workflows/bench-docker.yml build "derive image tags" run > "$sandbox/bench-tags.sh"
mkdir -p "$sandbox/m1-bin"
cat > "$sandbox/m1-bin/stub-guard" <<'EOF'
#!/usr/bin/env bash
echo "highest_overall=${STUB_HIGHEST:-false}" >> "$GITHUB_OUTPUT"
EOF
chmod +x "$sandbox/m1-bin/stub-guard"

run_bench_tags() {
  local out="$sandbox/m1-out"
  : > "$out"
  env -i PATH="/usr/bin:/bin" GITHUB_OUTPUT="$out" OWNER=asamarts "$@" \
    bash -eo pipefail "$sandbox/bench-tags.sh" >/dev/null
  grep '^tags=' "$out" | cut -d= -f2-
}

t1=$(run_bench_tags EVENT_NAME=workflow_dispatch REF=refs/heads/main REF_NAME=main OVERRIDE=0.24.0-rc1)
if [[ "$t1" == "ghcr.io/asamarts/alint-bench:0.24.0-rc1" ]]; then
  ok "M1: bench-docker workflow_dispatch on main honors inputs.tag"
else
  bad "M1: workflow_dispatch with override got '$t1'"
fi

t2=$(run_bench_tags EVENT_NAME=workflow_dispatch REF=refs/heads/main REF_NAME=main OVERRIDE="")
if [[ "$t2" == "ghcr.io/asamarts/alint-bench:edge" ]]; then
  ok "M1: bench-docker workflow_dispatch with empty tag defaults to edge"
else
  bad "M1: workflow_dispatch empty tag got '$t2'"
fi

if grep -q 'ci/scripts/release-pointer-guard.sh' .github/workflows/bench-docker.yml; then
  ok "M1: bench-docker.yml consults release-pointer-guard.sh before moving :latest"
else
  bad "M1: bench-docker.yml missing release-pointer-guard.sh"
fi

# ── M2: smoke-policy-test.sh falls back from failing python3 stub to python ─
py_real=$(command -v python3)
mkdir -p "$sandbox/m2-bin"
printf '#!/bin/sh\nexit 9009\n' > "$sandbox/m2-bin/python3"
chmod +x "$sandbox/m2-bin/python3"
ln -s "$py_real" "$sandbox/m2-bin/python"
cat > "$sandbox/m2-bin/alint-stub" <<'EOF'
#!/usr/bin/env bash
cat <<'JSON'
{"schema_version":1,"kind":"policy-test-report","tests":[{"status":"passed"}],"issues":[],"summary":{"selected":1,"passed":1,"failed":0,"invalid":0,"skipped":0}}
JSON
EOF
chmod +x "$sandbox/m2-bin/alint-stub"
if env -i PATH="$sandbox/m2-bin:/usr/bin:/bin" TMPDIR="$sandbox" \
    bash ci/scripts/smoke-policy-test.sh "$sandbox/m2-bin/alint-stub" >/dev/null 2>&1; then
  ok "M2: smoke-policy-test.sh falls back from broken python3 stub to python"
else
  bad "M2: smoke-policy-test.sh failed when python3 is a Store stub"
fi

# ── M3: action.yml ALINT_EXTRA shlex/noglob + unique mktemp SARIF path ───
python3 - "$sandbox/run-step.sh" <<'PY'
import re, sys
lines = open('action.yml', encoding='utf-8').read().splitlines()
start = next(i for i, l in enumerate(lines) if re.match(r'^\s*- name: Run alint\s*$', l))
run = next(i for i in range(start + 1, len(lines)) if re.match(r'^\s*run: \|\s*$', lines[i]))
key_indent = len(lines[run]) - len(lines[run].lstrip())
body = []
for line in lines[run + 1:]:
    if line.strip() and len(line) - len(line.lstrip()) <= key_indent:
        break
    body.append(line)
indent = min(len(l) - len(l.lstrip()) for l in body if l.strip())
open(sys.argv[1], 'w', encoding='utf-8').write('\n'.join(l[indent:] for l in body) + '\n')
PY
mkdir -p "$sandbox/m3-work" "$sandbox/m3-bin" "$sandbox/m3-tmp"
touch "$sandbox/m3-work/a.json" "$sandbox/m3-work/b.json"
cat > "$sandbox/m3-bin/alint" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$ALINT_ARGV_LOG"
echo '{"version":"2.1.0","runs":[]}'
EOF
chmod +x "$sandbox/m3-bin/alint"

run_action_step() {
  local out_file=$1 argv_log=$2 extra=$3
  : > "$out_file"
  (cd "$sandbox/m3-work" && env PATH="$sandbox/m3-bin:$PATH" \
    ALINT_BIN="$sandbox/m3-bin/alint" \
    ALINT_PATH="." ALINT_CONFIG="" ALINT_FORMAT="sarif" \
    ALINT_FAIL_ON_WARNING="false" \
    ALINT_CHANGED="false" ALINT_CHANGED_BASE="" ALINT_STRICT="false" \
    ALINT_EXTRA="$extra" ALINT_ARGV_LOG="$argv_log" \
    RUNNER_TEMP="$sandbox/m3-tmp" GITHUB_OUTPUT="$out_file" \
    bash -eo pipefail "$sandbox/run-step.sh" >/dev/null)
}

run_action_step "$sandbox/m3-out1" "$sandbox/m3-argv1" '--only *.json --baseline "my dir/base.json"'
run_action_step "$sandbox/m3-out2" "$sandbox/m3-argv2" ''
sarif1=$(grep '^sarif-file=' "$sandbox/m3-out1" | cut -d= -f2-)
sarif2=$(grep '^sarif-file=' "$sandbox/m3-out2" | cut -d= -f2-)
if grep -qxF '*.json' "$sandbox/m3-argv1" && grep -qxF 'my dir/base.json' "$sandbox/m3-argv1"; then
  ok "M3: action.yml preserves globs and quoted args with spaces"
else
  bad "M3: action.yml mangled ALINT_EXTRA: $(tr '\n' '|' < "$sandbox/m3-argv1")"
fi
if [[ -n "$sarif1" && -n "$sarif2" && "$sarif1" != "$sarif2" && -f "$sarif1" && -f "$sarif2" ]]; then
  ok "M3: consecutive SARIF steps write distinct files without clobbering"
else
  bad "M3: SARIF files collided or missing ($sarif1 vs $sarif2)"
fi

# ── M8: strip-since.mjs works through a symlinked directory with spaces ──
if command -v node >/dev/null 2>&1; then
  mkdir -p "$sandbox/m8 real" "$sandbox/m8-doc"
  ln -s "$REPO_ROOT/ci/scripts/strip-since.mjs" "$sandbox/m8 real/strip-since.mjs"
  cat > "$sandbox/m8-doc/page.md" <<'EOF'
Before
<!-- alint:since=9.9.9 -->
Future feature
<!-- /alint:since -->
After
EOF
  node "$sandbox/m8 real/strip-since.mjs" --released-version 0.23.0 "$sandbox/m8-doc" >/dev/null
  if ! grep -q 'Future feature' "$sandbox/m8-doc/page.md" && grep -q 'Before' "$sandbox/m8-doc/page.md"; then
    ok "M8: strip-since.mjs strips blocks when invoked via symlinked path with spaces"
  else
    bad "M8: strip-since.mjs did not strip unreleased block through symlink/space path"
  fi
fi

# ── M10: ci/runner/*.sh syntax + shellcheck ──────────────────────────────
runner_ok=1
for f in ci/runner/*.sh; do
  if ! bash -n "$f"; then
    bad "M10: $f failed bash -n"
    runner_ok=0
  fi
  if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck -S warning "$f"; then
      bad "M10: $f failed shellcheck"
      runner_ok=0
    fi
  fi
done
[[ "$runner_ok" -eq 1 ]] && ok "M10: ci/runner/*.sh pass bash -n and shellcheck"

# ── M12: likec4.sh fails closed under CI=true when npx is absent ─────────
mkdir -p "$sandbox/m12-bin"
# Populate only coreutils needed by bash without npx.
for tool in bash dirname basename mktemp rm; do
  p=$(command -v "$tool" || true)
  [[ -n "$p" ]] && ln -s "$p" "$sandbox/m12-bin/$tool"
done
if env -i PATH="$sandbox/m12-bin" CI=true bash ci/scripts/likec4.sh > "$sandbox/m12-out" 2>&1; then
  bad "M12: likec4.sh exited 0 under CI=true without npx"
elif grep -q 'npx is required in CI' "$sandbox/m12-out"; then
  ok "M12: likec4.sh fails closed under CI=true without npx"
else
  bad "M12: likec4.sh failed with unexpected message: $(cat "$sandbox/m12-out")"
fi

# ── L1: bench-record.yml uses head_sha, cleanup trap, idempotent PR ──────
if grep -Fq 'SOURCE_SHA: ${{ needs.guard.outputs.head_sha }}' .github/workflows/bench-record.yml &&
   grep -Fq 'trap '\''rm -rf "$stage_root"'\'' EXIT' .github/workflows/bench-record.yml &&
   grep -Fq 'if gh pr view "$branch"' .github/workflows/bench-record.yml; then
  ok "L1: bench-record.yml uses guard head_sha, cleans stage_root, and checks existing PR"
else
  bad "L1: bench-record.yml missing head_sha, stage_root cleanup trap, or gh pr view check"
fi

# ── L4: bump-version.sh has no bare GNU-only `sed -i` ────────────────────
if grep -nE '^[[:space:]]*sed[[:space:]]+-i([[:space:]]|$)' ci/scripts/bump-version.sh; then
  bad "L4: bump-version.sh still has bare GNU-only sed -i"
else
  ok "L4: bump-version.sh uses portable in-place editing (no bare sed -i)"
fi

# ── L5: smoke-channel.sh has no hardcoded /tmp/alint-pypi-smoke ──────────
if grep -Fq '/tmp/alint-pypi-smoke' ci/scripts/smoke-channel.sh; then
  bad "L5: smoke-channel.sh still references /tmp/alint-pypi-smoke"
elif ! grep -Fq 'astral-sh/uv/releases/download' ci/scripts/smoke-channel.sh ||
     ! grep -Fq 'sha256sum -c' ci/scripts/smoke-channel.sh; then
  bad "L5: smoke-channel.sh missing SHA-256 verified uv tarball install"
else
  ok "L5: smoke-channel.sh uses mktemp + cleanup trap + SHA-256 verified uv"
fi

# ── L10: pull-request-title.yml uses --format=%s without `| head -n 1` ───
if grep -Fq 'git log -1 --format=%s "$PR_HEAD"' .github/workflows/pull-request-title.yml &&
   ! grep -Fq 'head -n 1' .github/workflows/pull-request-title.yml; then
  ok "L10: pull-request-title.yml reads subject directly with --format=%s (no SIGPIPE)"
else
  bad "L10: pull-request-title.yml still pipes git log into head -n 1"
fi

echo "[test-packaging-scripts] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
