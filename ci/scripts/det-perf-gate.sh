#!/usr/bin/env bash
# Deterministic perf gate (load-immune).
#
# Compares the PR's instruction / cache / branch counts vs the merge-base, IN
# THIS RUN, with the same toolchain + Valgrind on both sides. Because the metrics
# are deterministic (Valgrind simulation, not wall-clock), the shared runner's
# co-tenant load — which chronically contaminates the wall-clock `bench-scale`
# (see docs/benchmarks/investigations/2026-06-v0.12-perf-validation/) — does NOT
# affect this gate. No committed baseline (the raw gungraun output is ~18 MB);
# the comparison is always against a freshly-built base, so there is zero
# baseline drift.
#
# gungraun's per-bench `soft_limits` (Ir +2%, EstimatedCycles +5%) ARE the gate:
# a breach makes `cargo bench` exit non-zero. Branch mispredicts (Bcm/Bim) are
# collected + printed but NOT gated — they false-positive on benign branch-pattern
# shifts. Advisory while DET_PERF_ADVISORY=1 (warn, don't fail); 0 to enforce.
# Design:
# docs/design/deterministic-perf-gating.md.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)" || exit 1

# One markdown report per run on the job page ($GITHUB_STEP_SUMMARY), so the
# per-bench deltas are visible without opening the log. No-op outside Actions.
summary() { printf '%s\n' "$@" >> "${GITHUB_STEP_SUMMARY:-/dev/null}"; }

# A gate that cannot run is blind, and a blind gate that exits 0 looks like a
# pass: while the PR that became v0.18.0 was open, every run logged "NOT gating"
# here and a +13.5% instruction regression in S2 merged unnoticed. So a tooling
# failure fails the job, loudly and labelled as tooling, never as a perf signal.
# The job is not in `summary.needs` and main has no required checks, so a red
# gate flags the breakage without blocking a merge.
tooling_failure() {
  echo "::error title=Deterministic perf gate::$1 (tooling failure, not a perf signal); the gate did not run"
  summary "### Deterministic perf gate: did not run" "" "Tooling failure: $1. See the job log."
  exit 1
}

# Derive the runner version from Cargo.lock so it can never drift behind the
# `gungraun` library the benches link. A hardcoded pin (was 0.19.1) silently
# rotted when the workspace bumped gungraun to 0.19.3: the older runner refuses
# to drive a newer library, the bench exits non-zero, and this script used to
# mis-report that TOOLING failure as an "Ir regression" on every post-bump PR.
# See docs/benchmarks/investigations/2026-07-v0.14-s2-harness-artifact/.
GUNGRAUN_VERSION="$(awk '/^name = "gungraun"$/{f=1; next} f && /^version = /{gsub(/"/,"",$3); print $3; exit}' Cargo.lock)"
if [[ -z "$GUNGRAUN_VERSION" ]]; then
  tooling_failure "could not read the gungraun version from Cargo.lock"
fi
BENCHES=(--bench det_engine --bench det_check)
ADVISORY="${DET_PERF_ADVISORY:-1}"

HEAD_SHA="$(git rev-parse HEAD)"
BASE_SHA="${PR_BASE_SHA:-}"
if [[ -z "$BASE_SHA" ]]; then
  BASE_SHA="$(git merge-base HEAD origin/main 2>/dev/null || true)"
fi
if [[ -z "$BASE_SHA" || "$BASE_SHA" == "$HEAD_SHA" ]]; then
  echo "==> no distinct base to compare against (push-to-main / first commit) — skipping"
  exit 0
fi

echo "==> ensuring valgrind + gungraun-runner v${GUNGRAUN_VERSION}"
# A fresh hosted image's package lists can be stale, so update before install.
if ! command -v valgrind >/dev/null 2>&1; then
  if ! { sudo apt-get update -qq && sudo apt-get install -y -qq valgrind; }; then
    tooling_failure "could not install valgrind"
  fi
fi
# Install/upgrade to the EXACT version Cargo.lock resolves. The old
# `command -v … || install` guard skipped the install whenever *any*
# gungraun-runner was already present, so a stale one from a prior run would
# persist and mismatch the library. Compare versions and (re)install on drift.
have_gungraun="$(gungraun-runner --version 2>/dev/null | awk '{print $NF}')"
if [[ "$have_gungraun" != "$GUNGRAUN_VERSION" ]]; then
  cargo install gungraun-runner --version "$GUNGRAUN_VERSION" --locked \
    || tooling_failure "could not install gungraun-runner v${GUNGRAUN_VERSION}"
fi

# Build the release binary (det_check measures it) + run the deterministic
# benches with the given gungraun baseline arg. Returns the bench exit code.
run_benches() {
  cargo build --release -p alint || return 2
  cargo bench -p alint-bench "${BENCHES[@]}" -- "$1"
}

# Markdown rows `| bench | base | PR | delta |` from gungraun's output: each
# bench header line (`det_check::check_grp::check s2_10k:(...) -> ...`) is
# followed by `Instructions: <new>|<old> (<delta>) [...]`.
ir_rows() {
  sed 's/\x1b\[[0-9;]*m//g' "$1" | awk '
    /^det_[a-z_]+::/ {
      n = split($1, part, "::"); split($2, id, ":")
      name = part[1] "::" part[n] " " id[1]; next
    }
    name != "" && $1 == "Instructions:" {
      split($2, v, "|"); delta = $3; gsub(/[()]/, "", delta)
      if (v[2] == "" || v[2] == "N/A") delta = "new"
      printf "| `%s` | %s | %s | %s |\n", name, v[2], v[1], delta
      name = ""
    }'
}

report() { # $1 = verdict line, $2 = gungraun log
  summary "### Deterministic perf gate: $1" "" \
    "Valgrind instruction counts (Ir), this PR vs merge base \`${BASE_SHA:0:12}\`." "" \
    "| Bench | Base | PR | Delta |" "|---|---:|---:|---:|"
  summary "$(ir_rows "$2")"
}

echo "==> [1/2] baseline: build + bench merge-base ${BASE_SHA:0:12}"
git checkout -q "$BASE_SHA" || tooling_failure "could not check out the merge base ${BASE_SHA:0:12}"
if ! run_benches --save-baseline=base; then
  git checkout -q "$HEAD_SHA"
  tooling_failure "the baseline build or bench failed at the merge base ${BASE_SHA:0:12}"
fi

echo "==> [2/2] PR: build + bench ${HEAD_SHA:0:12}, compare vs base"
git checkout -q "$HEAD_SHA"
bench_log="$(mktemp)"
trap 'rm -f "$bench_log"' EXIT
run_benches --baseline=base 2>&1 | tee "$bench_log"
rc="${PIPESTATUS[0]}"
git checkout -q "$HEAD_SHA" 2>/dev/null || true # ensure tree restored

if [[ "$rc" -ne 0 ]]; then
  # Distinguish a real soft-limit breach from a bench that never produced a
  # comparison (build / valgrind / gungraun-version tooling failure). gungraun
  # prints a "Gungraun result:" summary line ONLY when the comparison actually
  # ran; without it, the non-zero exit is a broken harness, not a perf signal —
  # reporting it as a regression is the bug this guard closes.
  if ! grep -q 'Gungraun result:' "$bench_log"; then
    tooling_failure "the PR build or bench did not complete"
  fi
  if [[ "$ADVISORY" == "1" ]]; then
    report "regression (advisory, not failing)" "$bench_log"
    echo "::warning title=Deterministic perf gate::Ir/branch regression vs base (ADVISORY — not failing). Review the deltas above; set DET_PERF_ADVISORY=0 to enforce."
    exit 0
  fi
  report "regression" "$bench_log"
  echo "::error title=Deterministic perf gate::Ir regression > +2% (or branch > +50%) vs base"
  exit 1
fi
report "pass" "$bench_log"
echo "==> deterministic perf gate: PASS"
