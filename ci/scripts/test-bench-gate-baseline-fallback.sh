#!/usr/bin/env bash
# Pin bench-record.yml's `bench-gate report` baseline fallback (#621):
#   - when PREV_TAG has committed macro results, use PREV_TAG directly with no
#     fallback notice;
#   - when PREV_TAG is a packaging-only patch with no results.json, walk older
#     release tags in reverse semver order and fall back to the newest older tag
#     that has committed results.json (emitting a ::notice);
#   - when neither PREV_TAG nor any older tag has committed results.json (or
#     PREV_TAG is empty), skip --baseline with a ::warning.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
EXTRACT="$REPO_ROOT/ci/scripts/workflow_step.py"
WF="$REPO_ROOT/.github/workflows/bench-record.yml"

sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT

pass=0
fail=0
ok() { echo "  ok: $1"; pass=$((pass + 1)); }
bad() { echo "  FAIL: $1" >&2; fail=$((fail + 1)); }

python3 "$EXTRACT" "$WF" bench bench_gate run > "$sandbox/bench_gate.sh"

repo="$sandbox/repo"
g() {
  git -C "$repo" -c user.name=test -c user.email=test@example.invalid \
    -c commit.gpgsign=false -c tag.gpgsign=false -c core.hooksPath=/dev/null "$@"
}

git init -q "$repo"
mkdir -p "$repo/target/release" "$repo/docs/benchmarks/macro/results/linux-x86_64"
cat > "$repo/target/release/xtask" <<'SH'
#!/usr/bin/env bash
printf 'ARGS:%s\n' "$*"
SH
chmod +x "$repo/target/release/xtask"
printf 'version = "v0.1.0"\n' > "$repo/docs/benchmarks/ANCHOR"

# v0.1.0 (has results), v0.2.0 (has results), v0.2.1 (packaging-only, no results), v0.3.0 (current)
mkdir -p "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.1.0"
printf '{"v":"0.1.0"}\n' > "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.1.0/results.json"
g add -A && g commit -q -m "v0.1.0" && g tag v0.1.0

mkdir -p "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.2.0"
printf '{"v":"0.2.0"}\n' > "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.2.0/results.json"
g add -A && g commit -q -m "v0.2.0" && g tag v0.2.0

g commit --allow-empty -q -m "v0.2.1 packaging-only" && g tag v0.2.1

mkdir -p "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.3.0"
printf '{"v":"0.3.0"}\n' > "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.3.0/results.json"
g add -A && g commit -q -m "v0.3.0" && g tag v0.3.0

run_step() {
  local label=$1 prev=$2
  rm -rf "$sandbox/tmp"
  mkdir -p "$sandbox/tmp"
  : > "$sandbox/summary.md"
  out="$(cd "$repo" && env OUT_LABEL="$label" PREV_TAG="$prev" \
    RUNNER_TEMP="$sandbox/tmp" GITHUB_STEP_SUMMARY="$sandbox/summary.md" \
    bash "$sandbox/bench_gate.sh" 2>&1)"
  baseline_recorded="$(cat "$sandbox/tmp/bench-gate-baseline.txt")"
}

echo "[bench-gate-baseline-fallback] fallback resolution"

# Case 1: PREV_TAG is packaging-only (v0.2.1) -> falls back to v0.2.0
run_step v0.3.0 v0.2.1
if [[ "$out" == *"--baseline docs/benchmarks/macro/results/linux-x86_64/v0.2.0/results.json"* &&
      "$out" == *"::notice title=bench-gate::previous release 'v0.2.1' has no committed results; falling back to 'v0.2.0'"* &&
      "$baseline_recorded" == "v0.2.0" ]]; then
  ok "packaging-only PREV_TAG v0.2.1 falls back to v0.2.0 with notice"
else
  bad "packaging-only fallback failed: baseline='${baseline_recorded}' out='${out}'"
fi

# Case 2: PREV_TAG has results (v0.2.0) -> uses v0.2.0 directly, no fallback notice
run_step v0.3.0 v0.2.0
if [[ "$out" == *"--baseline docs/benchmarks/macro/results/linux-x86_64/v0.2.0/results.json"* &&
      "$out" != *"falling back"* &&
      "$baseline_recorded" == "v0.2.0" ]]; then
  ok "PREV_TAG v0.2.0 with results is used directly"
else
  bad "direct PREV_TAG failed: baseline='${baseline_recorded}' out='${out}'"
fi

# Case 3: multiple consecutive packaging-only patches (v0.2.1, v0.2.2) -> still falls back to v0.2.0
g commit --allow-empty -q -m "v0.2.2 packaging-only" && g tag v0.2.2
run_step v0.3.0 v0.2.2
if [[ "$out" == *"--baseline docs/benchmarks/macro/results/linux-x86_64/v0.2.0/results.json"* &&
      "$baseline_recorded" == "v0.2.0" ]]; then
  ok "multiple packaging-only patches (v0.2.2 -> v0.2.1 -> v0.2.0) fall back to v0.2.0"
else
  bad "multi-hop fallback failed: baseline='${baseline_recorded}' out='${out}'"
fi

# Case 4: no prior committed results exist -> warns and omits --baseline
rm -f "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.1.0/results.json" \
      "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.2.0/results.json"
run_step v0.3.0 v0.2.2
if [[ "$out" != *"--baseline "* &&
      "$out" == *"::warning title=bench-gate::no committed results"* &&
      -z "$baseline_recorded" ]]; then
  ok "no prior results omits --baseline and emits warning"
else
  bad "no-prior-results case failed: baseline='${baseline_recorded}' out='${out}'"
fi

echo "[bench-gate-baseline-fallback] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
