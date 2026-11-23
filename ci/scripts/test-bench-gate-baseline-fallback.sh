#!/usr/bin/env bash
# Pin bench-record.yml's `bench-gate report` baseline fallback (#621):
#   - when PREV_TAG has committed macro results, use PREV_TAG directly with no
#     fallback notice;
#   - when PREV_TAG is a packaging-only patch with no results.json, walk older
#     release tags in reverse semver order (ignoring prerelease tags) and fall
#     back to the newest older release tag with committed results.json (emitting
#     a ::notice);
#   - when neither PREV_TAG nor any older tag has committed results.json (or
#     PREV_TAG is empty), skip --baseline with a ::warning;
#   - verify the PR-body baseline description accurately reports direct match,
#     fallback, missing-results (never misreporting PREV_TAG when skipped), and
#     no-previous-release cases.
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
python3 "$EXTRACT" "$WF" bench open_pr run > "$sandbox/open_pr.sh"

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

# v0.1.0 (has results), v0.2.0 (has results), v0.2.1-rc.1 (prerelease with results),
# v0.2.1 (packaging-only, no results), v0.3.0 (current)
mkdir -p "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.1.0"
printf '{"v":"0.1.0"}\n' > "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.1.0/results.json"
g add -A && g commit -q -m "v0.1.0" && g tag v0.1.0

mkdir -p "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.2.0"
printf '{"v":"0.2.0"}\n' > "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.2.0/results.json"
g add -A && g commit -q -m "v0.2.0" && g tag v0.2.0

mkdir -p "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.2.1-rc.1"
printf '{"v":"0.2.1-rc.1"}\n' > "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.2.1-rc.1/results.json"
g add -A && g commit -q -m "v0.2.1-rc.1" && g tag v0.2.1-rc.1

g commit --allow-empty -q -m "v0.2.1 packaging-only" && g tag v0.2.1

mkdir -p "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.3.0"
printf '{"v":"0.3.0"}\n' > "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.3.0/results.json"
g add -A && g commit -q -m "v0.3.0" && g tag v0.3.0

# Extract the gate_baseline_desc assignment block from open_pr.sh so we test
# the real workflow shell code verbatim.
awk '/^gate_baseline=/,/^ab_md=/ { if (!/^ab_md=/) print }' "$sandbox/open_pr.sh" > "$sandbox/desc.sh"
printf 'printf "%%s" "$gate_baseline_desc"\n' >> "$sandbox/desc.sh"

run_step() {
  local label=$1 prev=$2
  rm -rf "$sandbox/tmp"
  mkdir -p "$sandbox/tmp"
  : > "$sandbox/summary.md"
  out="$(cd "$repo" && env OUT_LABEL="$label" PREV_TAG="$prev" \
    RUNNER_TEMP="$sandbox/tmp" GITHUB_STEP_SUMMARY="$sandbox/summary.md" \
    bash "$sandbox/bench_gate.sh" 2>&1)"
  baseline_recorded="$(cat "$sandbox/tmp/bench-gate-baseline.txt")"
  baseline_desc="$(env PREV_TAG="$prev" RUNNER_TEMP="$sandbox/tmp" bash "$sandbox/desc.sh")"
}

echo "[bench-gate-baseline-fallback] fallback resolution"

# Case 1: PREV_TAG is packaging-only (v0.2.1) -> skips v0.2.1-rc.1 and falls back to v0.2.0
run_step v0.3.0 v0.2.1
if [[ "$out" == *"--baseline docs/benchmarks/macro/results/linux-x86_64/v0.2.0/results.json"* &&
      "$out" == *"::notice title=bench-gate::previous release 'v0.2.1' has no committed results; falling back to 'v0.2.0'"* &&
      "$baseline_recorded" == "v0.2.0" &&
      "$baseline_desc" == '`v0.2.0` (fallback from `v0.2.1`)' ]]; then
  ok "packaging-only PREV_TAG v0.2.1 skips prerelease v0.2.1-rc.1 and falls back to v0.2.0"
else
  bad "packaging-only fallback failed: baseline='${baseline_recorded}' desc='${baseline_desc}' out='${out}'"
fi

# Case 2: PREV_TAG has results (v0.2.0) -> uses v0.2.0 directly, no fallback notice
run_step v0.3.0 v0.2.0
if [[ "$out" == *"--baseline docs/benchmarks/macro/results/linux-x86_64/v0.2.0/results.json"* &&
      "$out" != *"falling back"* &&
      "$baseline_recorded" == "v0.2.0" &&
      "$baseline_desc" == '`v0.2.0`' ]]; then
  ok "PREV_TAG v0.2.0 with results is used directly"
else
  bad "direct PREV_TAG failed: baseline='${baseline_recorded}' desc='${baseline_desc}' out='${out}'"
fi

# Case 3: multiple consecutive packaging-only patches (v0.2.1, v0.2.2) -> still falls back to v0.2.0
g commit --allow-empty -q -m "v0.2.2 packaging-only" && g tag v0.2.2
run_step v0.3.0 v0.2.2
if [[ "$out" == *"--baseline docs/benchmarks/macro/results/linux-x86_64/v0.2.0/results.json"* &&
      "$baseline_recorded" == "v0.2.0" &&
      "$baseline_desc" == '`v0.2.0` (fallback from `v0.2.2`)' ]]; then
  ok "multiple packaging-only patches (v0.2.2 -> v0.2.1 -> v0.2.0) fall back to v0.2.0"
else
  bad "multi-hop fallback failed: baseline='${baseline_recorded}' desc='${baseline_desc}' out='${out}'"
fi

# Case 4: no prior committed results exist (PREV_TAG non-empty) -> warns, omits --baseline, desc says none
rm -f "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.1.0/results.json" \
      "$repo/docs/benchmarks/macro/results/linux-x86_64/v0.2.0/results.json"
run_step v0.3.0 v0.2.2
if [[ "$out" != *"--baseline "* &&
      "$out" == *"::warning title=bench-gate::no committed results"* &&
      -z "$baseline_recorded" &&
      "$baseline_desc" == '`none` (no committed results for `v0.2.2` or older)' ]]; then
  ok "no prior results omits --baseline, warns, and reports none in PR body"
else
  bad "no-prior-results case failed: baseline='${baseline_recorded}' desc='${baseline_desc}' out='${out}'"
fi

# Case 5: PREV_TAG empty (first release ever) -> warns, omits --baseline, desc says none
run_step v0.3.0 ""
if [[ "$out" != *"--baseline "* &&
      -z "$baseline_recorded" &&
      "$baseline_desc" == '`none`' ]]; then
  ok "empty PREV_TAG omits --baseline and reports none in PR body"
else
  bad "empty-PREV_TAG case failed: baseline='${baseline_recorded}' desc='${baseline_desc}' out='${out}'"
fi

echo "[bench-gate-baseline-fallback] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
