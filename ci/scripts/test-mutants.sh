#!/usr/bin/env bash
# Pin the advisory policy of ci/scripts/mutants.sh against a stubbed `cargo`:
#   - a missed or timed-out mutant, an empty diff and a budget stop pass and are
#     reported (job summary, a warning on the first 10 missed mutants' lines, with
#     their text escaped for the workflow command and the markdown);
#   - a usage error, failing baseline tests or a diff that does not match the
#     tree fail, since then nothing was tested;
#   - the pinned cargo-mutants is installed when the runner has another version;
#   - only a diff file or a crate name reaches cargo mutants, and every mutant is tested
#     by the workspace's tests with alint-core's `yaml-reference` feature.
#   - cargo mutants runs with each process's address space capped and no core files,
#     and, where sudo and systemd-run work, in a transient scope whose MemoryMax is
#     the cap and whose OOMPolicy is `continue`, as the invoking user with the
#     script's environment; stub `sudo` and `systemd-run` stand in for the real ones.
#   - Where this user's systemd can make a scope, a real one with the script's
#     properties (and a small MemoryMax) keeps a parent running when the kernel
#     kills its child at the cap, which the default OOMPolicy does not.
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
    echo "limits $(ulimit -v) $(ulimit -c)" >> "$STUB_LOG"
    echo "env HOME=${HOME-} CARGO_HOME=${CARGO_HOME-} RUSTUP_HOME=${RUSTUP_HOME-} RUSTFLAGS=${RUSTFLAGS-unset}" >> "$STUB_LOG"
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
# sudo fails unless STUB_SUDO=ok (no passwordless sudo), and resets the
# environment as the real one does.
cat > "$sandbox/bin/sudo" <<'STUB'
#!/usr/bin/env bash
echo "sudo $*" >> "$STUB_LOG"
[[ "${STUB_SUDO:-}" == ok ]] || exit 1
[[ "$1" == -n ]] && shift
exec env -i STUB_LOG="$STUB_LOG" STUB_SCOPE="${STUB_SCOPE:-}" PATH="$STUB_BIN:/usr/bin:/bin" "$@"
STUB
# systemd-run fails with STUB_SCOPE=fail (no systemd), and otherwise runs the
# command after `--`, as a scope does.
cat > "$sandbox/bin/systemd-run" <<'STUB'
#!/usr/bin/env bash
echo "systemd-run $*" >> "$STUB_LOG"
[[ "${STUB_SCOPE:-}" != fail ]] || exit 1
while [[ $# -gt 0 && "$1" != -- ]]; do shift; done
shift
exec "$@"
STUB
chmod +x "$sandbox/bin/cargo" "$sandbox/bin/sudo" "$sandbox/bin/systemd-run"

# run [VAR=value...] -- <args...> -> $rc
run() {
  local vars=()
  while [[ "$1" != -- ]]; do vars+=("$1"); shift; done
  shift
  : > "$sandbox/log"
  : > "$sandbox/summary"
  rc=0
  env PATH="$sandbox/bin:$PATH" STUB_BIN="$sandbox/bin" STUB_LOG="$sandbox/log" \
    STUB_VERSION="$pinned" GITHUB_STEP_SUMMARY="$sandbox/summary" "${vars[@]}" \
    bash "$root/ci/scripts/mutants.sh" "$@" > "$sandbox/out" 2>&1 || rc=$?
}
logged() { grep -qE -- "$1" "$sandbox/log"; }
said() { grep -qF -- "$1" "$sandbox/out"; }
summarised() { grep -qF -- "$1" "$sandbox/summary"; }

missed='crates/a/src/lib.rs:12:5: replace f -> u8 with 0\ncrates/a/src/lib.rs:30:9: replace % with / in g\n'

run STUB_RC=2 STUB_CAUGHT='c1\nc2\nc3\n' STUB_MISSED="$missed" -- --in-diff "$sandbox/pr.diff"
if [[ "$rc" -eq 0 ]] && summarised '3 caught, 2 missed, 0 timed out' &&
   summarised '- `` crates/a/src/lib.rs:12:5: replace f -> u8 with 0 ``' &&
   said '::warning file=crates/a/src/lib.rs,line=12,col=5,title=Missed mutant::replace f -> u8 with 0' &&
   said '::warning file=crates/a/src/lib.rs,line=30,col=9,title=Missed mutant::replace %25 with / in g'; then
  ok "missed mutants pass, listed in the summary and as warnings on their lines"
else
  bad "missed mutants must pass and be reported"
fi
tested_with="--test-workspace=true --features alint-core/yaml-reference"
if logged "^cargo mutants --workspace --in-diff $sandbox/pr.diff $tested_with --in-place --no-shuffle --output target/mutants$" &&
   ! logged '^cargo install'; then
  ok "--in-diff reaches cargo mutants over the whole workspace, tested by its tests; the pinned version is not reinstalled"
else
  bad "expected 'cargo mutants --workspace --in-diff <file> $tested_with --in-place --no-shuffle --output target/mutants'"
fi

run STUB_RC=3 STUB_CAUGHT='c1\n' STUB_TIMEOUT='crates/a/src/lib.rs:7:1: replace `loop` body\n' -- --package alint-output
if [[ "$rc" -eq 0 ]] && summarised '1 caught, 0 missed, 1 timed out' && summarised 'Timed out' &&
   summarised '- `` crates/a/src/lib.rs:7:1: replace `loop` body ``' &&
   logged "^cargo mutants --package alint-output $tested_with --in-place"; then
  ok "timed-out mutants pass and are reported; --package reaches cargo mutants, tested by the workspace's tests"
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

run MUTANTS_BUDGET_SECONDS=1 STUB_SLEEP=30 -- --in-diff "$sandbox/pr.diff"
if [[ "$rc" -eq 0 ]] && summarised 'finished: the run stopped at the 1-second budget' &&
   ! summarised 'No mutants in'; then
  ok "a budget stop before any mutant finished says so, not 'no mutants'"
else
  bad "a budget stop with nothing finished must not read as 'no mutants'"
fi

many=''
for n in $(seq 1 12); do many+="crates/a/src/lib.rs:$n:1: replace f$n\n"; done
run STUB_RC=2 STUB_MISSED="$many" -- --in-diff "$sandbox/pr.diff"
if [[ "$rc" -eq 0 ]] && [[ "$(grep -c '^::warning ' "$sandbox/out")" -eq 10 ]] &&
   summarised 'replace f12 ``'; then
  ok "12 missed mutants: 10 warnings (GitHub's per-step limit), all 12 in the summary"
else
  bad "expected 10 warnings and 12 summary entries"
fi

run STUB_RC=2 STUB_MISSED='crates/a,title=x/src/lib.rs:3:1: replace `a` with %\r b\n' -- --in-diff "$sandbox/pr.diff"
if [[ "$rc" -eq 0 ]] &&
   said '::warning file=crates/a%2Ctitle=x/src/lib.rs,line=3,col=1,title=Missed mutant::replace `a` with %25%0D b' &&
   summarised '- `` crates/a,title=x/src/lib.rs:3:1: replace `a` with %'; then
  ok "a comma in the path and a backtick in the text cannot inject a property or end the code span"
else
  bad "the warning's file property and the summary entry must be escaped"
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

run -- --in-diff "$sandbox/pr.diff"
if logged '^limits 12582912 0$'; then
  ok "cargo mutants runs with a 12 GiB address-space cap and no core files"
else
  bad "cargo mutants must run under 'ulimit -v 12582912' and 'ulimit -c 0'"
fi

run MUTANTS_MEMORY_LIMIT_KIB=1048576 -- --in-diff "$sandbox/pr.diff"
if logged '^limits 1048576 0$'; then ok "MUTANTS_MEMORY_LIMIT_KIB sets the cap"; else bad "MUTANTS_MEMORY_LIMIT_KIB must set the cap"; fi

# Without passwordless sudo, or without systemd, the per-process cap is the only layer.
for stubs in "STUB_SUDO=no" "STUB_SUDO=ok STUB_SCOPE=fail"; do
  # shellcheck disable=SC2086
  run $stubs -- --in-diff "$sandbox/pr.diff"
  if [[ "$rc" -eq 0 ]] && logged '^sudo -n systemd-run --scope .* -- true$' &&
     ! logged '^(sudo -n )?systemd-run .* -- env -i' && logged '^limits 12582912 0$' &&
     logged '^cargo mutants --workspace --in-diff' && said 'without a memory scope'; then
    ok "with $stubs, cargo mutants runs outside a scope, each process capped"
  else
    bad "with $stubs, cargo mutants must run outside a scope, each process capped"
  fi
done

# With both, the whole run is in one scope capped at the limit, with no swap, as the
# invoking user, and with the script's environment, which sudo would have reset.
uid=$(id -u)
gid=$(id -g)
scope_args="--scope --quiet --collect -p MemoryMax=12582912K -p MemorySwapMax=0 -p OOMPolicy=continue"
scope_args+=" --uid=$uid --gid=$gid"
run STUB_SUDO=ok HOME="$sandbox/home" CARGO_HOME="$sandbox/cargo" RUSTUP_HOME="$sandbox/rustup" \
  STUB_RC=0 -- --in-diff "$sandbox/pr.diff"
if [[ "$rc" -eq 0 ]] && logged "^sudo -n systemd-run $scope_args -- true$" &&
   logged "^systemd-run $scope_args -- env -i .* timeout --signal=INT --kill-after=60 5400 cargo mutants --workspace --in-diff $sandbox/pr.diff $tested_with --in-place --no-shuffle --output target/mutants$" &&
   logged '^limits 12582912 0$' &&
   logged "^env HOME=$sandbox/home CARGO_HOME=$sandbox/cargo RUSTUP_HOME=$sandbox/rustup RUSTFLAGS=$" &&
   said 'in a transient scope with MemoryMax=12582912K, no swap, OOMPolicy=continue' &&
   summarised 'No mutants in the lines this change touches.'; then
  ok "with sudo and systemd, cargo mutants runs in a 12 GiB scope as the user, with its environment"
else
  bad "with sudo and systemd, cargo mutants must run in a MemoryMax scope with the script's environment"
fi
# The scope's properties, as mutants.sh passes them, for the real scope below.
mapfile -t properties < <(grep -m 1 '^systemd-run --scope .* -- true$' "$sandbox/log" |
  grep -oE -- '-p [^ ]+' | sed 's/^-p //')

run STUB_SUDO=ok MUTANTS_MEMORY_LIMIT_KIB=1048576 STUB_RC=2 STUB_MISSED="$missed" -- --package alint-output
if [[ "$rc" -eq 0 ]] && logged '^systemd-run --scope .* -p MemoryMax=1048576K -p MemorySwapMax=0 .* -- env -i ' &&
   logged '^limits 1048576 0$' && summarised '0 caught, 2 missed' &&
   said '::warning file=crates/a/src/lib.rs,line=12,col=5,title=Missed mutant::'; then
  ok "MUTANTS_MEMORY_LIMIT_KIB sets the scope's MemoryMax; a run in a scope is reported as usual"
else
  bad "MUTANTS_MEMORY_LIMIT_KIB must set the scope's MemoryMax, and the run must be reported"
fi

for limit in 0 12G -1; do
  run MUTANTS_MEMORY_LIMIT_KIB="$limit" -- --in-diff "$sandbox/pr.diff"
  if [[ "$rc" -ne 0 ]] && ! logged '^cargo mutants --'; then
    ok "a memory cap of '$limit' is rejected"
  else
    bad "a memory cap of '$limit' must be rejected"
  fi
done

run MUTANTS_BUDGET_SECONDS=0 -- --in-diff "$sandbox/pr.diff"
if [[ "$rc" -ne 0 ]] && ! logged '^cargo mutants --'; then ok "a zero budget is rejected"; else bad "a zero budget must be rejected"; fi

# A real scope with the script's properties, where this user's systemd can make
# one: the kernel kills a child that outgrows MemoryMax (64 MiB here), and its
# parent, standing in for cargo-mutants, must run on. A control scope with the
# default OOMPolicy shows that systemd would stop the parent; where it does not
# (an older systemd, or no memory controller for the user), this proves nothing
# and is skipped.
# real_scope <systemd-run property args...> <command...>
# (In braces, so that bash's notice of a scope systemd stopped goes to the file.)
real_scope() {
  { timeout 120 env XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}" \
    systemd-run --user --scope --quiet --collect "$@"; } > "$sandbox/out" 2>&1
}
# The parent waits after the kill, long enough for systemd to stop the scope if
# its policy says so.
outgrow='perl -e "\$x = q(a) x (512 * 1024 * 1024)"; echo "child $?"; sleep 3; echo "parent alive"'
scope=()
for property in "${properties[@]}"; do
  [[ "$property" == MemoryMax=* ]] && property=MemoryMax=64M
  scope+=(-p "$property")
done
if [[ " ${properties[*]} " != *" MemoryMax="* ]]; then
  bad "could not read the scope's properties from the stub systemd-run's arguments"
elif ! real_scope "${scope[@]}" -- true; then
  echo "  skip: this user's systemd cannot make such a scope, so the OOM policy is not run for real"
else
  real_scope "${scope[@]/OOMPolicy=continue/OOMPolicy=stop}" -- bash -c "$outgrow" || true
  if grep -q '^child 0$' "$sandbox/out"; then
    echo "  skip: a child over the scope's MemoryMax is not killed here (no memory controller)"
  elif grep -q '^parent alive$' "$sandbox/out"; then
    echo "  skip: this systemd does not stop a scope on an OOM kill, so continue proves nothing here"
  else
    real_scope "${scope[@]}" -- bash -c "$outgrow" || true
    if grep -q '^child 137$' "$sandbox/out" && grep -q '^parent alive$' "$sandbox/out"; then
      ok "in a real scope with the script's properties, an OOM-killed child leaves its parent running"
    else
      bad "in a real scope with the script's properties, an OOM-killed child must leave its parent running"
    fi
  fi
fi

echo "[test-mutants] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
