#!/usr/bin/env bash
# Pin the advisory policy of ci/scripts/mutants.sh against a stubbed `cargo`:
#   - a missed or timed-out mutant, an empty diff and a budget stop pass and are
#     reported (job summary, one warning per missed mutant's line);
#   - a usage error, failing baseline tests or a diff that does not match the
#     tree fail, since then nothing was tested;
#   - the pinned cargo-mutants is installed when the runner has another version;
#   - only a diff file or a crate name reaches cargo mutants.
#
# Stub bodies are single-quoted on purpose: they expand when the stub runs.
# shellcheck disable=SC2016
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
pass=0
fail=0
ok() { echo "  ok: $1"; pass=$((pass + 1)); }
bad() { echo "  FAIL: $1" >&2; sed 's/^/      | /' "$sandbox/out" "$sandbox/log" "$sandbox/summary" >&2; fail=$((fail + 1)); }

root="$sandbox/repo"
mkdir -p "$root/ci/scripts" "$sandbox/bin"
cp ci/scripts/mutants.sh "$root/ci/scripts/"
pinned=$(sed -n 's/^CARGO_MUTANTS_VERSION="\(.*\)"$/\1/p' ci/scripts/mutants.sh)
[[ -n "$pinned" ]] || { echo "could not read CARGO_MUTANTS_VERSION from mutants.sh" >&2; exit 1; }
printf 'diff --git a/x.rs b/x.rs\n' > "$sandbox/pr.diff"

# The stub writes the report files cargo-mutants would, from STUB_* variables.
cat > "$sandbox/bin/cargo" <<'STUB'
#!/usr/bin/env bash
echo "cargo $*" >> "$STUB_LOG"
case "$1 $2" in
  "mutants --version") echo "cargo-mutants ${STUB_VERSION:?}"; exit 0 ;;
  "mutants "*)
    out=
    while [[ $# -gt 0 ]]; do [[ "$1" == --output ]] && out=$2; shift; done
    mkdir -p "$out/mutants.out"
    [[ -n "${STUB_CAUGHT:-}" ]] && printf '%b' "$STUB_CAUGHT" > "$out/mutants.out/caught.txt"
    [[ -n "${STUB_MISSED:-}" ]] && printf '%b' "$STUB_MISSED" > "$out/mutants.out/missed.txt"
    [[ -n "${STUB_TIMEOUT:-}" ]] && printf '%b' "$STUB_TIMEOUT" > "$out/mutants.out/timeout.txt"
    [[ -n "${STUB_SLEEP:-}" ]] && sleep "$STUB_SLEEP"
    exit "${STUB_RC:-0}" ;;
esac
exit 0
STUB
chmod +x "$sandbox/bin/cargo"

# run [VAR=value...] -- <args...> -> $rc
run() {
  local vars=()
  while [[ "$1" != -- ]]; do vars+=("$1"); shift; done
  shift
  : > "$sandbox/log"
  : > "$sandbox/summary"
  rc=0
  env PATH="$sandbox/bin:$PATH" STUB_LOG="$sandbox/log" STUB_VERSION="$pinned" \
    GITHUB_STEP_SUMMARY="$sandbox/summary" "${vars[@]}" \
    bash "$root/ci/scripts/mutants.sh" "$@" > "$sandbox/out" 2>&1 || rc=$?
}
logged() { grep -qE -- "$1" "$sandbox/log"; }
said() { grep -qF -- "$1" "$sandbox/out"; }
summarised() { grep -qF -- "$1" "$sandbox/summary"; }

missed='crates/a/src/lib.rs:12:5: replace f -> u8 with 0\ncrates/a/src/lib.rs:30:9: replace % with / in g\n'

run STUB_RC=2 STUB_CAUGHT='c1\nc2\nc3\n' STUB_MISSED="$missed" -- --in-diff "$sandbox/pr.diff"
if [[ "$rc" -eq 0 ]] && summarised '3 caught, 2 missed, 0 timed out' &&
   summarised '- `crates/a/src/lib.rs:12:5: replace f -> u8 with 0`' &&
   said '::warning file=crates/a/src/lib.rs,line=12,col=5,title=Missed mutant::replace f -> u8 with 0' &&
   said '::warning file=crates/a/src/lib.rs,line=30,col=9,title=Missed mutant::replace %25 with / in g'; then
  ok "missed mutants pass, listed in the summary and as warnings on their lines"
else
  bad "missed mutants must pass and be reported"
fi
if logged "^cargo mutants --workspace --in-diff $sandbox/pr.diff --in-place --no-shuffle --output target/mutants$" &&
   ! logged '^cargo install'; then
  ok "--in-diff reaches cargo mutants over the whole workspace; the pinned version is not reinstalled"
else
  bad "expected 'cargo mutants --workspace --in-diff <file> --in-place --no-shuffle --output target/mutants'"
fi

run STUB_RC=3 STUB_CAUGHT='c1\n' STUB_TIMEOUT='crates/a/src/lib.rs:7:1: replace loop body\n' -- --package alint-output
if [[ "$rc" -eq 0 ]] && summarised '1 caught, 0 missed, 1 timed out' && summarised 'Timed out' &&
   logged '^cargo mutants --package alint-output --in-place'; then
  ok "timed-out mutants pass and are reported; --package reaches cargo mutants"
else
  bad "a timed-out mutant must pass and be reported"
fi

run STUB_RC=0 -- --in-diff "$sandbox/pr.diff"
if [[ "$rc" -eq 0 ]] && summarised 'No mutants in the lines this change touches.'; then
  ok "an empty diff (or one without Rust) passes and says so"
else
  bad "an empty diff must pass"
fi

for code in 1 4 5 6 70; do
  run STUB_RC="$code" -- --in-diff "$sandbox/pr.diff"
  if [[ "$rc" -eq "$code" ]] && said 'nothing was tested'; then
    ok "cargo-mutants exit $code fails the job"
  else
    bad "cargo-mutants exit $code must fail the job with that code"
  fi
done

run MUTANTS_BUDGET_SECONDS=1 STUB_SLEEP=30 STUB_CAUGHT='c1\n' -- --in-diff "$sandbox/pr.diff"
if [[ "$rc" -eq 0 ]] && summarised 'stopped at the 1-second budget'; then
  ok "the budget stops the run with a partial report"
else
  bad "a budget stop must pass with a partial report"
fi

run STUB_RC=137 -- --in-diff "$sandbox/pr.diff"
if [[ "$rc" -eq 137 ]] && said 'nothing was tested'; then
  ok "a killed cargo-mutants before the budget fails the job"
else
  bad "a cargo-mutants killed before the budget must fail the job"
fi

run STUB_VERSION=0.0.1 -- --in-diff "$sandbox/pr.diff"
if logged "^cargo install cargo-mutants --locked --version ${pinned//./\\.}$"; then
  ok "a different cargo-mutants version gets the pinned ${pinned} installed"
else
  bad "must install cargo-mutants ${pinned} when the runner has another version"
fi

for args in "--package alint;true" "--package" "--in-diff $sandbox/missing.diff" "--workspace x" "--in-diff $sandbox/pr.diff extra"; do
  # shellcheck disable=SC2086
  run -- $args
  if [[ "$rc" -ne 0 ]] && ! logged '^cargo mutants --'; then
    ok "rejected before running: $args"
  else
    bad "must reject '$args' without running cargo mutants"
  fi
done

run MUTANTS_BUDGET_SECONDS=0 -- --in-diff "$sandbox/pr.diff"
if [[ "$rc" -ne 0 ]] && ! logged '^cargo mutants --'; then ok "a zero budget is rejected"; else bad "a zero budget must be rejected"; fi

echo "[test-mutants] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
