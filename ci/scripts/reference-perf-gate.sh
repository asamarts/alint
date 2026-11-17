#!/usr/bin/env bash
# Relation-layer perf gate: HEAD against the frozen v0.18.2 reference
# (ADR-0023 decision V21-6, ADR-0019 DEC-13), on the deterministic scenarios
# S1-S3.
#
#   1. Instructions. The reference's release binary and HEAD's are each run by
#      HEAD's det_check harness (same generated trees, toolchain, Valgrind and
#      gungraun runner; only the alint binary differs, passed as ALINT_DET_BIN
#      so nothing in target/ is overwritten). Each of the six S1-S3 cells must
#      be present with a reference measurement, and none may exceed +3% Ir.
#      Nothing is committed as a baseline, so neither the host nor the
#      toolchain can drift it.
#   2. Syscalls. read/stat-family counts for the alint process (spawned
#      children excluded, see syscall-counts.py) on the 10k trees. A family
#      more than 32 calls over the reference fails: fixed per-run probes differ
#      by a few calls, a per-file regression by thousands. The `probe` family
#      (stats of `.git`, `.jj`, `.gitignore` and `.ignore`) may also grow by 3
#      for each directory in the tree and 5 for each one above it (ADR-0045,
#      see probe_allowance). A run strace could not trace, or one alint did not
#      complete, is a tooling failure.
#
# S4 is not compared: it extends bundled rulesets that arrived in v0.19, so the
# reference binary cannot load it. A breach exits 1; so does a gate that cannot
# run, labelled as a tooling failure. Run by ci.yml's `perf-gate-reference` job.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)" || exit 1

REFERENCE="${PERF_REFERENCE:-v0.18.2}"
IR_LIMIT="${PERF_REFERENCE_IR_LIMIT:-3}"
SYSCALL_ALLOWANCE="${PERF_REFERENCE_SYSCALL_ALLOWANCE:-32}"
FILTER='det_check::check_grp::*'
CELLS=(s1_1k s1_10k s2_1k s2_10k s3_1k s3_10k)
SCENARIOS=(s1 s2 s3)

summary() { printf '%s\n' "$@" >> "${GITHUB_STEP_SUMMARY:-/dev/null}"; }
# The `probe` family's allowance for a walk of the tree `$1` (ADR-0045): the
# fixed allowance, 3 for each directory in the tree outside `.git` (the walk
# checks its `.ignore`, `.gitignore` and `.git` before `ignore` reads them), and
# 5 for each directory above it, which `ignore` reads too and, under
# `require_git` (ADR-0028), also probes for `.git` and `.jj`.
probe_allowance() {
  local tree in_tree above
  tree="$(cd "$1" && pwd -P)" || return 1
  in_tree="$(find "$tree" -name .git -prune -o -type d -print | wc -l)"
  above="$(tr -cd / <<< "$tree" | wc -c)"
  echo $((SYSCALL_ALLOWANCE + 3 * in_tree + 5 * above))
}
tooling_failure() {
  echo "::error title=Reference perf gate::$1 (tooling failure, not a perf signal); the gate did not run"
  summary "### Reference perf gate (${REFERENCE}): did not run" "" "Tooling failure: $1. See the job log."
  exit 1
}

# Scratch lives under target/ (on disk, and outside any RAM-backed /tmp).
work="target/reference-perf-gate"
ref_tree="$work/reference"
cleanup() {
  git worktree remove --force "$ref_tree" >/dev/null 2>&1 || true
  git worktree prune >/dev/null 2>&1 || true
}
trap cleanup EXIT
cleanup
rm -rf "$work"
mkdir -p "$work" || tooling_failure "could not create $work"

GUNGRAUN_VERSION="$(awk '/^name = "gungraun"$/{f=1; next} f && /^version = /{gsub(/"/,"",$3); print $3; exit}' Cargo.lock)"
[[ -n "$GUNGRAUN_VERSION" ]] || tooling_failure "could not read the gungraun version from Cargo.lock"
for tool in valgrind strace; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    { sudo apt-get update -qq && sudo apt-get install -y -qq "$tool"; } \
      || tooling_failure "could not install $tool"
  fi
done
if [[ "$(gungraun-runner --version 2>/dev/null | awk '{print $NF}')" != "$GUNGRAUN_VERSION" ]]; then
  cargo install gungraun-runner --version "$GUNGRAUN_VERSION" --locked \
    || tooling_failure "could not install gungraun-runner v${GUNGRAUN_VERSION}"
fi

# One compiler for both sides: the reference checkout's rust-toolchain.toml
# must not pick a different one.
if [[ -z "${RUSTUP_TOOLCHAIN:-}" ]] && command -v rustup >/dev/null 2>&1; then
  RUSTUP_TOOLCHAIN="$(rustup show active-toolchain 2>/dev/null | awk '{print $1; exit}')"
  export RUSTUP_TOOLCHAIN
fi

echo "==> [1/4] build the ${REFERENCE} release binary"
git rev-parse --verify -q "${REFERENCE}^{commit}" >/dev/null \
  || tooling_failure "${REFERENCE} is not in this checkout (fetch tags)"
git worktree add -q --detach "$ref_tree" "${REFERENCE}^{commit}" \
  || tooling_failure "could not check out ${REFERENCE}"
# Frozen source meets a moving toolchain: a new warn-by-default lint must not
# fail it under CI's `-D warnings`. Lint levels do not change codegen.
(cd "$ref_tree" && RUSTFLAGS="--cap-lints=warn" cargo build --release -p alint --locked) \
  || tooling_failure "the ${REFERENCE} release build failed"
ref_bin="$PWD/$ref_tree/target/release/alint"

echo "==> [2/4] build HEAD's release binary"
cargo build --release -p alint --locked || tooling_failure "HEAD's release build failed"
head_bin="$PWD/$work/alint-head"
cp target/release/alint "$head_bin" || tooling_failure "could not copy HEAD's binary"

echo "==> [3/4] instructions: ${REFERENCE} then HEAD through HEAD's det_check"
# A persistent target/ keeps an earlier run's `reference` baseline, which
# --save-baseline would compare against with det_check's own limits.
rm -rf target/gungraun/alint-bench/det_check/check_grp
ALINT_DET_BIN="$ref_bin" cargo bench -p alint-bench --bench det_check --locked -- \
  --save-baseline=reference "$FILTER" \
  || tooling_failure "the det_check run of the ${REFERENCE} binary failed"
bench_log="$work/bench.log"
ALINT_DET_BIN="$head_bin" cargo bench -p alint-bench --bench det_check --locked -- \
  --baseline=reference --callgrind-limits="ir=${IR_LIMIT}%" "$FILTER" 2>&1 | tee "$bench_log"
bench_rc="${PIPESTATUS[0]}"
result_line="$(sed 's/\x1b\[[0-9;]*m//g' "$bench_log" | grep '^Gungraun result:' | tail -1)"
[[ -n "$result_line" ]] || tooling_failure "the HEAD det_check run did not complete"
regressed=false
if [[ "$bench_rc" -ne 0 ]]; then
  [[ "$result_line" == "Gungraun result: Regressed"* ]] \
    || tooling_failure "the HEAD det_check run exited ${bench_rc} without a regression verdict"
  regressed=true
fi

ir_rows="$(sed 's/\x1b\[[0-9;]*m//g' "$bench_log" | awk '
  /^det_check::/ { split($2, id, ":"); name = id[1]; next }
  name != "" && $1 == "Instructions:" {
    split($2, v, "|"); delta = $3; gsub(/[()]/, "", delta)
    printf "| `%s` | %s | %s | %s |\n", name, v[2], v[1], delta; name = ""
  }')"
for cell in "${CELLS[@]}"; do
  row="$(grep -F "| \`${cell}\` |" <<< "$ir_rows")"
  [[ -n "$row" ]] || tooling_failure "cell ${cell} is missing from the det_check output"
  reference_count="$(awk -F'|' '{gsub(/ /, "", $3); print $3}' <<< "$row")"
  [[ "$reference_count" =~ ^[0-9]+$ ]] \
    || tooling_failure "cell ${cell} has no ${REFERENCE} measurement (${reference_count})"
done

echo "==> [4/4] syscalls on the 10k trees"
trace_set="$(python3 ci/scripts/syscall-counts.py --syscalls)"
syscall_rows=""
syscall_breach=false
for scenario in "${SCENARIOS[@]}"; do
  tree="target/det-trees/${scenario}-10000"
  [[ -f "$tree/.alint.yml" ]] || tooling_failure "det_check left no tree at $tree"
  for side in reference head; do
    bin="$ref_bin"
    [[ "$side" == head ]] && bin="$head_bin"
    trace="$PWD/$work/$scenario-$side.trace"
    # strace exits with the traced program's status: alint's 0 (clean) or 1
    # (violations) are runs that completed; anything else is not a measurement.
    (cd "$tree" && strace -f -qq -o "$trace" -e trace="$trace_set" \
      "$bin" check --format json --config .alint.yml >/dev/null 2>"$trace.stderr")
    rc=$?
    if [[ "$rc" -gt 1 ]]; then
      tooling_failure "the traced ${side} run on ${scenario} exited ${rc}: $(head -c 300 "$trace.stderr")"
    fi
    python3 ci/scripts/syscall-counts.py "$trace" > "$work/$scenario-$side.counts" \
      || tooling_failure "could not count syscalls for ${scenario} (${side})"
  done
  probes_allowed="$(probe_allowance "$tree")" \
    || tooling_failure "could not count the directories of ${scenario}"
  while read -r family ref_count; do
    head_count="$(awk -v f="$family" '$1 == f {print $2}' "$work/$scenario-head.counts")"
    extra=$((head_count - ref_count))
    allowance="$SYSCALL_ALLOWANCE"
    [[ "$family" == probe ]] && allowance="$probes_allowed"
    mark=""
    if (( extra > allowance )); then
      mark=" (over)"
      syscall_breach=true
    fi
    syscall_rows+="| ${scenario}_10k | ${family} | ${ref_count} | ${head_count} | ${extra}${mark} | ${allowance} |"$'\n'
  done < "$work/$scenario-reference.counts"
done

verdict="pass"
[[ "$regressed" == true ]] && verdict="instruction regression"
if [[ "$syscall_breach" == true ]]; then
  [[ "$verdict" == pass ]] && verdict="syscall regression" || verdict="$verdict and syscall regression"
fi
summary "### Reference perf gate (${REFERENCE}): ${verdict}" "" \
  "S1-S3, HEAD against the ${REFERENCE} binary, both measured by HEAD's harness." "" \
  "| Bench | ${REFERENCE} | HEAD | Delta (limit +${IR_LIMIT}%) |" "|---|---:|---:|---:|" "$ir_rows" "" \
  "| Tree | Family | ${REFERENCE} | HEAD | Extra | Allowance |" "|---|---|---:|---:|---:|---:|" \
  "$syscall_rows"
printf '%s\n' "$syscall_rows"
if [[ "$verdict" != pass ]]; then
  echo "::error title=Reference perf gate::${verdict} against ${REFERENCE} (Ir limit +${IR_LIMIT}%, syscall allowance ${SYSCALL_ALLOWANCE}, probes per directory as in ADR-0045)"
  exit 1
fi
echo "==> reference perf gate: PASS"
