#!/usr/bin/env bash
# Pin the outcome handling of ci/scripts/reference-perf-gate.sh and the
# measurement checks of ci/scripts/syscall-counts.py:
#   - a clean comparison passes and reports both tables; each binary reaches
#     det_check through ALINT_DET_BIN, and target/release/alint keeps HEAD's
#     build;
#   - an instruction breach (gungraun "Regressed") and a syscall family over the
#     allowance each exit 1 with a labelled verdict; a count within the
#     allowance passes;
#   - the `probe` family's allowance grows by 3 for each directory in the tree
#     (outside `.git`) and 5 for each one above it, and no other family's does
#     (ADR-0045);
#   - a gate that cannot measure is a labelled tooling failure, never a pass: a
#     missing reference tag, a det_check run with no result line, a non-zero
#     exit without a regression verdict, a missing S1-S3 cell, a cell with no
#     reference measurement, a traced run that did not complete, an empty trace;
#   - the reference worktree is gone afterwards;
#   - syscall-counts.py skips a spawned child's calls and resumed halves,
#     counts a stat of `.git`, `.jj`, `.gitignore` or `.ignore` as a probe, and
#     refuses logs that are not measurements.
# Behavioural: the gate runs in a scratch git repo against stubbed `cargo`,
# `strace`, `gungraun-runner` and `valgrind`. The real Valgrind and strace runs
# are exercised by the `perf-gate-reference` CI job itself.
#
# Stub bodies are single-quoted on purpose: they expand when the stub runs.
# shellcheck disable=SC2016
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
COUNTER="$REPO_ROOT/ci/scripts/syscall-counts.py"

sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
pass=0
fail=0
ok() { echo "  ok: $1"; pass=$((pass + 1)); }
bad() {
  echo "  FAIL: $1" >&2
  sed 's/^/      | /' "$sandbox/out" "$sandbox/summary" >&2 2>/dev/null || true
  fail=$((fail + 1))
}

echo "==> syscall-counts.py: the traced process and its threads only"
cat > "$sandbox/trace.txt" <<'TRACE'
100 execve("/bin/alint", ["alint", "check"], 0x7ffc /* 3 vars */) = 0
100 openat(AT_FDCWD, ".alint.yml", O_RDONLY|O_CLOEXEC) = 3
100 read(3, "version: 1\n", 8192) = 11
101 newfstatat(AT_FDCWD, "a.rs", {st_mode=S_IFREG|0644, st_size=10, ...}, 0) = 0
101 read(4,  <unfinished ...>
100 statx(AT_FDCWD, "b.rs", AT_STATX_SYNC_AS_STAT, STATX_ALL, {...}) = 0
101 <... read resumed>"fn a() {}\n", 8192) = 10
100 getdents64(5, 0x5581 /* 4 entries */, 32768) = 112
101 statx(AT_FDCWD, "/w/src/.gitignore", AT_STATX_SYNC_AS_STAT, STATX_ALL, 0x7f) = -1 ENOENT (No such file or directory)
100 newfstatat(AT_FDCWD, ".git", {st_mode=S_IFDIR|0755, ...}, 0) = 0
101 statx(AT_FDCWD, "/w/.jj",  <unfinished ...>
101 <... statx resumed>0x7f) = -1 ENOENT (No such file or directory)
100 statx(AT_FDCWD, "/w/src/.ignore", AT_STATX_SYNC_AS_STAT, STATX_ALL, 0x7f) = -1 ENOENT (No such file or directory)
100 statx(AT_FDCWD, "/w/a.gitignore", AT_STATX_SYNC_AS_STAT, STATX_ALL, 0x7f) = 0
100 statx(AT_FDCWD, "/w/.git/info/exclude", AT_STATX_SYNC_AS_STAT, STATX_ALL, 0x7f) = 0
102 execve("/usr/bin/git", ["git", "rev-parse"], 0x7ffc /* 3 vars */) = 0
102 read(3, "ref: refs/heads/main\n", 4096) = 21
102 openat(AT_FDCWD, ".git/HEAD", O_RDONLY) = 3
102 newfstatat(AT_FDCWD, ".git", {st_mode=S_IFDIR|0755, ...}, 0) = 0
101 +++ exited with 0 +++
TRACE
counts="$(python3 "$COUNTER" "$sandbox/trace.txt")"
if [[ "$counts" == $'read 2\nstat 4\nopen 1\ndents 1\nprobe 4' ]]; then
  ok "threads count, the git child and resumed halves do not; probes by name"
else
  bad "syscall counts: $counts"
fi
grep -vE '\.(git|jj|gitignore|ignore)"' "$sandbox/trace.txt" > "$sandbox/no-probes.txt"
counts="$(python3 "$COUNTER" "$sandbox/no-probes.txt")"
if [[ "$counts" == *$'\nprobe 0' ]]; then
  ok "a trace with no probes is still a measurement"
else
  bad "no-probe counts: $counts"
fi
set_list="$(python3 "$COUNTER" --syscalls)"
if [[ "$set_list" == execve,* ]] && [[ "$set_list" == *'?statx'* ]] && [[ "$set_list" == *'?preadv2'* ]]; then
  ok "the strace set traces execve and every counted call, arch-tolerantly"
else
  bad "trace set: $set_list"
fi
: > "$sandbox/empty.txt"
grep -v getdents64 "$sandbox/trace.txt" > "$sandbox/no-dents.txt"
grep -v '^100 execve' "$sandbox/trace.txt" > "$sandbox/no-execve.txt"
for log in empty no-dents no-execve; do
  if python3 "$COUNTER" "$sandbox/$log.txt" >/dev/null 2>&1; then
    bad "a $log trace was counted"
  else
    ok "a $log trace is not a measurement"
  fi
done

mkdir -p "$sandbox/bin"
# cargo: `build` writes a binary that reports the tree that built it; `bench`
# records which binary it measured, materializes the trees, and replays the
# gungraun output the test chose.
cat > "$sandbox/bin/cargo" <<'STUB'
#!/usr/bin/env bash
case "$1" in
  install) exit 0 ;;
  build)
    mkdir -p target/release
    printf '#!/usr/bin/env bash\necho "%s"\nexit "${STUB_ALINT_RC:-0}"\n' "$PWD" > target/release/alint
    chmod +x target/release/alint
    exit 0 ;;
  bench)
    echo "bench ALINT_DET_BIN=${ALINT_DET_BIN:-unset}" >> "$STUB_LOG"
    for s in s1 s2 s3; do
      mkdir -p "target/det-trees/$s-10000/a/b" "target/det-trees/$s-10000/c" \
        "target/det-trees/$s-10000/.git/objects"
      echo 'version: 1' > "target/det-trees/$s-10000/.alint.yml"
    done
    case " $* " in
      *" --save-baseline=reference "*) exit "${STUB_REF_RC:-0}" ;;
      *" --baseline=reference "*) cat "${STUB_HEAD_OUTPUT:?}"; exit "${STUB_HEAD_RC:-0}" ;;
    esac ;;
esac
echo "unexpected cargo $*" >&2
exit 99
STUB
# strace: write a trace for whichever binary it runs (HEAD's gets
# STUB_HEAD_EXTRA_STATS more stat calls and STUB_HEAD_EXTRA_PROBES more probes)
# and exit with STUB_STRACE_RC.
cat > "$sandbox/bin/strace" <<'STUB'
#!/usr/bin/env bash
out=""; bin=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    -o) out="$2"; shift 2 ;;
    -e) shift 2 ;;
    -*) shift ;;
    *) bin="$1"; break ;;
  esac
done
if [[ -n "${STUB_EMPTY_TRACE:-}" ]]; then : > "$out"; exit 0; fi
extra=0
probes=0
if [[ "$bin" != */reference/* ]]; then
  extra="${STUB_HEAD_EXTRA_STATS:-0}"
  probes="${STUB_HEAD_EXTRA_PROBES:-0}"
fi
{
  echo '7 execve("alint", ["alint"], 0x1 /* 1 vars */) = 0'
  for _ in $(seq 1 $((100 + extra))); do echo '7 newfstatat(AT_FDCWD, "f", {...}, 0) = 0'; done
  for _ in $(seq 1 $((10 + probes))); do echo '7 statx(AT_FDCWD, "d/.gitignore", 0, 0, 0x1) = -1 ENOENT'; done
  for _ in $(seq 1 50); do echo '7 read(3, "x", 8192) = 1'; done
  for _ in $(seq 1 20); do echo '7 openat(AT_FDCWD, "f", O_RDONLY) = 3'; done
  echo '7 getdents64(3, 0x1 /* 2 entries */, 32768) = 48'
} > "$out"
exit "${STUB_STRACE_RC:-0}"
STUB
cat > "$sandbox/bin/gungraun-runner" <<'STUB'
#!/usr/bin/env bash
echo "gungraun-runner 0.20.0"
STUB
printf '#!/usr/bin/env bash\nexit 0\n' > "$sandbox/bin/valgrind"
chmod +x "$sandbox/bin/"*

cell() { # name new old delta
  printf 'det_check::check_grp::check %s:(materialize("s", S, 1)) -> crates/alint-be...\n' "$1"
  printf '  Instructions:                      %s|%s            (%s) [x]\n' "$2" "$3" "$4"
}
{
  cell s1_1k 48983783 52691903 -7.03736%
  cell s1_10k 396279941 431761845 -8.21793%
  cell s2_1k 189117197 194618264 -2.82659%
  cell s2_10k 1810427475 1872799790 -3.33043%
  cell s3_1k 2115991900 2119721372 -0.17594%
  cell s3_10k 22374765762 22383599179 -0.03946%
  echo 'Gungraun result: Ok. 6 without regressions; 0 regressed; 3 filtered; 6 benchmarks finished in 400s'
} > "$sandbox/gungraun-ok.txt"
{
  sed '/s2_10k/,+1d; /^Gungraun/d' "$sandbox/gungraun-ok.txt"
  cell s2_10k 1947714782 1872799790 +4.00014%
  echo 'Gungraun result: Regressed. 5 without regressions; 1 regressed; 3 filtered; 6 benchmarks finished in 400s'
} > "$sandbox/gungraun-regressed.txt"
sed '/s3_10k/,+1d' "$sandbox/gungraun-ok.txt" > "$sandbox/gungraun-missing-cell.txt"
sed 's/22374765762|22383599179 /22374765762|N\/A /' "$sandbox/gungraun-ok.txt" > "$sandbox/gungraun-no-reference.txt"
printf 'error: could not compile `alint-bench`\n' > "$sandbox/bench-error.txt"

repo="$sandbox/repo"
mkdir -p "$repo/ci/scripts"
cp "$REPO_ROOT/ci/scripts/reference-perf-gate.sh" "$COUNTER" "$repo/ci/scripts/"
printf 'target/\n' > "$repo/.gitignore"
git -C "$repo" init -q -b main
git -C "$repo" config user.email test@example.com
git -C "$repo" config user.name Test
git -C "$repo" config commit.gpgsign false
printf '[[package]]\nname = "gungraun"\nversion = "0.20.0"\n' > "$repo/Cargo.lock"
git -C "$repo" add -A
git -C "$repo" commit -q -m reference
git -C "$repo" tag v0.18.2
echo head > "$repo/head.txt"
git -C "$repo" add -A
git -C "$repo" commit -q -m head

run_gate() {
  : > "$sandbox/summary"
  : > "$sandbox/log"
  set +e
  (cd "$repo" && env PATH="$sandbox/bin:$PATH" GITHUB_STEP_SUMMARY="$sandbox/summary" \
    STUB_LOG="$sandbox/log" STUB_HEAD_OUTPUT="$sandbox/gungraun-ok.txt" "$@" \
    bash ci/scripts/reference-perf-gate.sh) > "$sandbox/out" 2>&1
  rc=$?
  set -e
}
has() { grep -qF -- "$2" "$sandbox/$1"; }
# HEAD's build is untouched in target/release and the reference worktree gone.
clean() {
  [[ "$(bash "$repo/target/release/alint")" == "$repo" ]] \
    && [[ "$(git -C "$repo" worktree list | wc -l)" -eq 1 ]]
}
tooling() { [[ $rc -eq 1 ]] && has out "$1" && has out '(tooling failure' && has summary 'did not run'; }

echo "==> pass: exit 0, both tables, each binary measured through ALINT_DET_BIN"
run_gate
if [[ $rc -eq 0 ]] && has out 'reference perf gate: PASS' \
  && has summary '### Reference perf gate (v0.18.2): pass' \
  && has summary '| `s1_10k` | 431761845 | 396279941 | -8.21793% |' \
  && has summary '| s2_10k | stat | 100 | 100 | 0 |' \
  && has log 'bench ALINT_DET_BIN=/' && grep -q 'reference/target/release/alint$' "$sandbox/log" \
  && grep -q 'reference-perf-gate/alint-head$' "$sandbox/log" && clean; then
  ok "a clean comparison passes without overwriting target/release/alint"
else
  bad "pass case (rc=$rc)"
fi

echo "==> instruction breach: exit 1"
run_gate STUB_HEAD_RC=3 STUB_HEAD_OUTPUT="$sandbox/gungraun-regressed.txt"
if [[ $rc -eq 1 ]] && has summary '### Reference perf gate (v0.18.2): instruction regression' \
  && has out '::error title=Reference perf gate::instruction regression' && clean; then
  ok "an S1-S3 cell over +3% fails"
else
  bad "instruction breach (rc=$rc)"
fi

echo "==> syscall growth over the allowance: exit 1; within it: pass"
run_gate STUB_HEAD_EXTRA_STATS=40
if [[ $rc -eq 1 ]] && has summary 'syscall regression' && has summary '| s1_10k | stat | 100 | 140 | 40 (over) |'; then
  ok "40 extra stat calls fail"
else
  bad "syscall breach (rc=$rc)"
fi
run_gate STUB_HEAD_EXTRA_STATS=32
if [[ $rc -eq 0 ]] && has summary '| s3_10k | stat | 100 | 132 | 32 | 32 |'; then
  ok "32 extra stat calls are within the allowance"
else
  bad "syscall allowance (rc=$rc)"
fi

echo "==> probes: 32, 3 per directory in the tree (not .git) and 5 per one above it"
# The stub tree holds the root, a, a/b and c (and .git/objects, not counted).
tree_path="$(python3 -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$repo/target/det-trees/s1-10000")"
above="$(python3 -c 'import sys; print(len(sys.argv[1].strip("/").split("/")))' "$tree_path")"
probe_allowance=$((32 + 3 * 4 + 5 * above))
run_gate STUB_HEAD_EXTRA_PROBES="$probe_allowance"
if [[ $rc -eq 0 ]] \
  && has summary "| s2_10k | probe | 10 | $((10 + probe_allowance)) | ${probe_allowance} | ${probe_allowance} |"; then
  ok "probes at the per-directory allowance pass"
else
  bad "probe allowance (rc=$rc)"
fi
run_gate STUB_HEAD_EXTRA_PROBES="$((probe_allowance + 1))"
if [[ $rc -eq 1 ]] && has summary "| s1_10k | probe | 10 | $((11 + probe_allowance)) | $((probe_allowance + 1)) (over) |"; then
  ok "one probe over the allowance fails"
else
  bad "probe breach (rc=$rc)"
fi
run_gate STUB_HEAD_EXTRA_STATS=33 STUB_HEAD_EXTRA_PROBES=0
if [[ $rc -eq 1 ]] && has summary '| s1_10k | stat | 100 | 133 | 33 (over) | 32 |'; then
  ok "the per-directory allowance is the probe family's alone"
else
  bad "stat allowance with directories (rc=$rc)"
fi

echo "==> tooling failures: exit 1, labelled"
run_gate PERF_REFERENCE=v9.9.9
if tooling 'v9.9.9 is not in this checkout (fetch tags)'; then ok "a missing reference tag"; else bad "missing reference (rc=$rc)"; fi
run_gate STUB_HEAD_RC=101 STUB_HEAD_OUTPUT="$sandbox/bench-error.txt"
if tooling 'the HEAD det_check run did not complete' && clean; then ok "a HEAD bench with no result line"; else bad "HEAD bench (rc=$rc)"; fi
run_gate STUB_HEAD_RC=1
if tooling 'exited 1 without a regression verdict'; then ok "a non-zero exit without a Regressed verdict"; else bad "non-regression exit (rc=$rc)"; fi
run_gate STUB_REF_RC=1
if tooling 'det_check run of the v0.18.2 binary failed' && clean; then ok "a failed reference bench"; else bad "reference bench (rc=$rc)"; fi
run_gate STUB_HEAD_OUTPUT="$sandbox/gungraun-missing-cell.txt"
if tooling 'cell s3_10k is missing'; then ok "a missing S1-S3 cell"; else bad "missing cell (rc=$rc)"; fi
run_gate STUB_HEAD_OUTPUT="$sandbox/gungraun-no-reference.txt"
if tooling 'cell s3_10k has no v0.18.2 measurement'; then ok "a cell with no reference measurement"; else bad "N/A reference (rc=$rc)"; fi
run_gate STUB_STRACE_RC=2
if tooling 'exited 2'; then ok "a traced run that did not complete"; else bad "strace rc (rc=$rc)"; fi
run_gate STUB_EMPTY_TRACE=1
if tooling 'could not count syscalls for s1 (reference)'; then ok "an empty trace"; else bad "empty trace (rc=$rc)"; fi

echo ""
echo "reference-perf-gate: ${pass} passed, ${fail} failed"
[[ $fail -eq 0 ]]
