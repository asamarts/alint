#!/usr/bin/env bash
# Pin the outcome handling of ci/scripts/det-perf-gate.sh:
#   - a comparison that ran: PASS / advisory regression exit 0, an enforced
#     regression exits 1, and every outcome writes the Ir table to the job
#     summary;
#   - a gate that could NOT run (unreadable gungraun version, runner install,
#     baseline build/bench, PR bench with no "Gungraun result:") fails the job
#     as a labelled tooling failure instead of exiting 0 ("NOT gating" let a
#     regression merge unseen during the v0.18 audit PR);
#   - no distinct base is a clean skip, and the PR head is always restored.
# Behavioural: the script runs in a scratch git repo against stubbed `cargo`,
# `gungraun-runner` and `valgrind` that record argv and act on env knobs. The
# real Valgrind run is exercised by the `perf-gate` CI job itself.
#
# Stub bodies are single-quoted on purpose: they expand when the stub runs.
# shellcheck disable=SC2016
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
pass=0
fail=0
ok() { echo "  ok: $1"; pass=$((pass + 1)); }
bad() {
  echo "  FAIL: $1" >&2
  sed 's/^/      | /' "$sandbox/out" "$sandbox/summary" "$sandbox/log" >&2 2>/dev/null || true
  fail=$((fail + 1))
}

mkdir -p "$sandbox/bin"
cat > "$sandbox/bin/cargo" <<'STUB'
#!/usr/bin/env bash
echo "cargo $*" >> "$STUB_LOG"
case "$1" in
  install) exit "${STUB_INSTALL_RC:-0}" ;;
  build) exit "${STUB_BUILD_RC:-0}" ;;
  bench)
    case "${*: -1}" in
      --save-baseline=base) exit "${STUB_BASE_RC:-0}" ;;
      --baseline=base) cat "${STUB_PR_OUTPUT:?}"; exit "${STUB_PR_RC:-0}" ;;
    esac ;;
esac
echo "unexpected cargo $*" >&2
exit 99
STUB
cat > "$sandbox/bin/gungraun-runner" <<'STUB'
#!/usr/bin/env bash
echo "gungraun-runner ${STUB_RUNNER_VERSION:-0.20.0}"
STUB
printf '#!/usr/bin/env bash\nexit 0\n' > "$sandbox/bin/valgrind"
chmod +x "$sandbox/bin/"*

# gungraun's comparison output (as det_check / det_engine print it), one bench
# that regressed, one that improved and one new in this PR.
cat > "$sandbox/gungraun-ok.txt" <<'OUT'
det_check::check_grp::check s2_10k:(materialize("s2", S2, 10_000)) -> crates/alint-be...
  Instructions:                     1836071838|2554444779           (-28.1225%) [-1.39126x]
  L1 Hits:                          2400000000|3300000000           (-27.2727%) [-1.37500x]
det_engine::engine::engine_run n1k:(fixture(1_000))
  Instructions:                         400000|393021               (+1.77574%) [+1.01776x]
det_check::check_grp::check s5_1k:(materialize("s5", S5, 1_000)) -> crates/alint-ben...
  Instructions:                       51194776|N/A                  (*********)
Gungraun result: Ok. 3 without regressions; 0 regressed; 3 benchmarks finished in 120s
OUT
printf 'error: could not compile `alint` (bin "alint")\n' > "$sandbox/build-error.txt"
# The same comparison as gungraun prints it in CI, ANSI-coloured (format taken
# from a hosted perf-gate run): the table must still parse.
e=$'\e'
{
  printf '%s\n' "${e}[32mdet_check::check_grp::check${e}[0m ${e}[36ms2_10k${e}[0m:${e}[1;34m(materialize(\"s2\", S2, 10_000)) -> crates/alint-be...${e}[0m"
  printf '%s\n' "${e}[90m  ${e}[0mInstructions:                 ${e}[1m1840489612${e}[0m|1839321150           (${e}[1;91m+0.06353%${e}[0m) [${e}[1;91m+1.00064x${e}[0m]"
  printf '%s\n' "Gungraun result: ${e}[1;32mOk${e}[0m. 1 without regressions; 0 regressed; 1 benchmarks finished in 1s"
} > "$sandbox/gungraun-color.txt"

repo="$sandbox/repo"
mkdir -p "$repo/ci/scripts"
cp "$REPO_ROOT/ci/scripts/det-perf-gate.sh" "$repo/ci/scripts/"
git -C "$repo" init -q -b main
git -C "$repo" config user.email test@example.com
git -C "$repo" config user.name Test
git -C "$repo" config commit.gpgsign false
printf '[[package]]\nname = "gungraun"\nversion = "0.20.0"\n' > "$repo/Cargo.lock"
git -C "$repo" add -A
git -C "$repo" commit -q -m base
base_sha=$(git -C "$repo" rev-parse HEAD)
echo pr > "$repo/pr.txt"
git -C "$repo" add -A
git -C "$repo" commit -q -m pr
head_sha=$(git -C "$repo" rev-parse HEAD)

# run_gate [VAR=value ...]: run the gate from the repo with a clean summary/log.
run_gate() {
  : > "$sandbox/summary"
  : > "$sandbox/log"
  git -C "$repo" checkout -q "$head_sha"
  set +e
  (cd "$repo" && env PATH="$sandbox/bin:$PATH" STUB_LOG="$sandbox/log" \
    GITHUB_STEP_SUMMARY="$sandbox/summary" PR_BASE_SHA="$base_sha" \
    STUB_PR_OUTPUT="$sandbox/gungraun-ok.txt" "$@" \
    bash ci/scripts/det-perf-gate.sh) > "$sandbox/out" 2>&1
  rc=$?
  set -e
}
restored() { [[ "$(git -C "$repo" rev-parse HEAD)" == "$head_sha" ]]; }
has() { grep -qF -- "$2" "$sandbox/$1"; }

echo "==> pass: exit 0, PASS, Ir table in the job summary"
run_gate
if [[ $rc -eq 0 ]] && has out 'deterministic perf gate: PASS' \
  && has summary '### Deterministic perf gate: pass' \
  && has summary '| `det_check::check s2_10k` | 2554444779 | 1836071838 | -28.1225% |' \
  && has summary '| `det_engine::engine_run n1k` | 393021 | 400000 | +1.77574% |' \
  && has summary '| `det_check::check s5_1k` | N/A | 51194776 | new |' \
  && has log 'bench -p alint-bench --bench det_engine --bench det_check -- --save-baseline=base' \
  && has log 'bench -p alint-bench --bench det_engine --bench det_check -- --baseline=base' \
  && restored; then
  ok "a clean comparison passes and reports every bench"
else
  bad "pass case (rc=$rc)"
fi

echo "==> ANSI-coloured gungraun output still yields the table"
run_gate STUB_PR_OUTPUT="$sandbox/gungraun-color.txt"
if [[ $rc -eq 0 ]] && has summary '| `det_check::check s2_10k` | 1839321150 | 1840489612 | +0.06353% |'; then
  ok "coloured output parses"
else
  bad "coloured output (rc=$rc)"
fi

echo "==> advisory regression: exit 0 with a warning and the table"
run_gate STUB_PR_RC=1
if [[ $rc -eq 0 ]] && has out '::warning title=Deterministic perf gate::Ir/branch regression' \
  && has summary '### Deterministic perf gate: regression (advisory, not failing)' \
  && has summary '| `det_check::check s2_10k` |' && restored; then
  ok "an advisory regression warns but does not fail"
else
  bad "advisory regression (rc=$rc)"
fi

echo "==> enforced regression: exit 1"
run_gate STUB_PR_RC=1 DET_PERF_ADVISORY=0
if [[ $rc -eq 1 ]] && has out '::error title=Deterministic perf gate::Ir regression' \
  && has summary '### Deterministic perf gate: regression' && restored; then
  ok "an enforced regression fails"
else
  bad "enforced regression (rc=$rc)"
fi

echo "==> PR bench never compared: tooling failure, exit 1"
run_gate STUB_PR_RC=101 STUB_PR_OUTPUT="$sandbox/build-error.txt"
if [[ $rc -eq 1 ]] && has out 'the PR build or bench did not complete (tooling failure' \
  && has summary '### Deterministic perf gate: did not run' && restored; then
  ok "a PR bench that produced no comparison fails as tooling"
else
  bad "PR tooling failure (rc=$rc)"
fi

echo "==> baseline bench fails: tooling failure, exit 1, PR head restored"
run_gate STUB_BASE_RC=1
if [[ $rc -eq 1 ]] && has out "baseline build or bench failed at the merge base ${base_sha:0:12}" \
  && ! has log ' -- --baseline=base' && restored; then
  ok "a failed baseline fails as tooling and restores the PR head"
else
  bad "baseline tooling failure (rc=$rc)"
fi

echo "==> baseline build fails: tooling failure, exit 1"
run_gate STUB_BUILD_RC=1
if [[ $rc -eq 1 ]] && has out 'baseline build or bench failed' && ! has log 'bench -p' && restored; then
  ok "a failed baseline build fails as tooling"
else
  bad "baseline build failure (rc=$rc)"
fi

echo "==> runner version drift: reinstall pinned version"
run_gate STUB_RUNNER_VERSION=0.19.3
if [[ $rc -eq 0 ]] && has log 'install gungraun-runner --version 0.20.0 --locked'; then
  ok "a drifted gungraun-runner is reinstalled at the Cargo.lock version"
else
  bad "runner drift (rc=$rc)"
fi

echo "==> runner install fails: tooling failure, exit 1"
run_gate STUB_RUNNER_VERSION=0.19.3 STUB_INSTALL_RC=1
if [[ $rc -eq 1 ]] && has out 'could not install gungraun-runner v0.20.0' && ! has log 'build'; then
  ok "a failed runner install fails as tooling before building"
else
  bad "runner install failure (rc=$rc)"
fi

echo "==> no distinct base: clean skip"
run_gate PR_BASE_SHA="$head_sha"
if [[ $rc -eq 0 ]] && has out 'no distinct base' && [[ ! -s "$sandbox/log" ]]; then
  ok "no distinct base skips without building"
else
  bad "no distinct base (rc=$rc)"
fi

echo "==> no gungraun in Cargo.lock: tooling failure, exit 1"
git -C "$repo" checkout -q "$head_sha"
cp "$repo/Cargo.lock" "$sandbox/Cargo.lock.keep"
printf '[[package]]\nname = "other"\nversion = "1.0.0"\n' > "$repo/Cargo.lock"
git -C "$repo" commit -q -am "drop gungraun"
head_sha=$(git -C "$repo" rev-parse HEAD)
run_gate
if [[ $rc -eq 1 ]] && has out 'could not read the gungraun version from Cargo.lock' \
  && [[ ! -s "$sandbox/log" ]]; then
  ok "an unreadable gungraun version fails as tooling"
else
  bad "missing gungraun version (rc=$rc)"
fi

echo "[det-perf-gate] $pass passed, $fail failed"
[[ $fail -eq 0 ]]
