#!/usr/bin/env bash
# Advisory mutation testing with cargo-mutants (owner decision in
# docs/design/v0.21/pre-release-audit.md, section 7, item 3).
#
# Usage:
#   ci/scripts/mutants.sh --in-diff <diff-file>   # the mutants in the lines a diff changes
#   ci/scripts/mutants.sh --package <crate>       # every mutant in one workspace crate
#
# Advisory: a missed or timed-out mutant is reported, never failed. The report goes to
# $GITHUB_STEP_SUMMARY (when set), as a warning on each missed mutant's line, and to
# target/mutants/mutants.out. cargo-mutants' exit codes (upstream src/exit_code.rs):
#   0  every mutant caught, or none to test (an empty diff, or no Rust in it)  reported
#   2  some mutants missed                                                     reported
#   3  some mutants timed out (often a mutant that loops forever)              reported
#   1  usage error, 4 the unmutated tests already fail, 5 or 6 the diff does not
#      match the tree or does not parse, 70 internal error                      fail: nothing was tested
# MUTANTS_BUDGET_SECONDS (default 5400, 90 minutes) stops the run with the results
# so far, so a large change ends as a partial report instead of a cancelled job.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

CARGO_MUTANTS_VERSION="27.1.0"
BUDGET_SECONDS="${MUTANTS_BUDGET_SECONDS:-5400}"
OUTPUT=target/mutants
REPORT="$OUTPUT/mutants.out"

usage() {
  echo "usage: ci/scripts/mutants.sh --in-diff <diff-file> | --package <crate>" >&2
  exit 1
}

[[ $# -eq 2 ]] || usage
case "$1" in
  --in-diff)
    [[ -f "$2" ]] || { echo "[mutants] diff file '$2' does not exist" >&2; exit 1; }
    # --workspace: the root manifest's default-members is the CLI crate alone,
    # and without it cargo-mutants generates mutants there only, so a change to
    # any other crate would report "no mutants".
    filter=(--workspace --in-diff "$2")
    scope="the lines this change touches"
    ;;
  --package)
    # A crate name only: the value comes from a workflow_dispatch input.
    [[ "$2" =~ ^[a-z0-9][a-z0-9_-]*$ ]] || { echo "[mutants] '$2' is not a crate name" >&2; exit 1; }
    filter=(--package "$2")
    scope="the crate \`$2\`"
    ;;
  *) usage ;;
esac
[[ "$BUDGET_SECONDS" =~ ^[1-9][0-9]*$ ]] || { echo "[mutants] MUTANTS_BUDGET_SECONDS must be a positive integer" >&2; exit 1; }

if [[ "$(cargo mutants --version 2>/dev/null | awk '{ print $NF }' || true)" != "${CARGO_MUTANTS_VERSION}" ]]; then
  echo "==> cargo-mutants ${CARGO_MUTANTS_VERSION} not found; installing (cargo install --locked)"
  # Clear RUSTFLAGS for the tool build only, as deny.sh and audit.sh do.
  RUSTFLAGS='' cargo install cargo-mutants --locked --version "${CARGO_MUTANTS_VERSION}"
fi

# --in-place builds in this checkout, so the restored target/ cache serves the
# baseline build; CI's checkout is disposable. No `-D warnings`: a mutant often
# leaves an unused value, and failing its build would hide whether tests catch it.
rm -rf "$OUTPUT"
mkdir -p "$OUTPUT"
echo "==> cargo mutants ${filter[*]} (budget ${BUDGET_SECONDS} s)"
rc=0
started=$SECONDS
RUSTFLAGS='' timeout --signal=INT --kill-after=60 "$BUDGET_SECONDS" \
  cargo mutants "${filter[@]}" --in-place --no-shuffle --output "$OUTPUT" || rc=$?
elapsed=$((SECONDS - started))

count() { [[ -s "$REPORT/$1" ]] && wc -l < "$REPORT/$1" | tr -d ' ' || echo 0; }
caught=$(count caught.txt)
missed=$(count missed.txt)
timeouts=$(count timeout.txt)
unviable=$(count unviable.txt)

# A budget stop is decided by the clock, not the exit code: timeout reports 124,
# but on a GitHub-hosted runner the stopped command has also come back as 137.
if [[ "$rc" == 0 || "$rc" == 2 || "$rc" == 3 ]]; then
  status="complete"
elif (( elapsed >= BUDGET_SECONDS )); then
  status="stopped at the ${BUDGET_SECONDS}-second budget; the counts cover the mutants tested by then"
else
  echo "[mutants] cargo-mutants exited $rc: nothing was tested (see its output above)" >&2
  exit "$rc"
fi

summary="${GITHUB_STEP_SUMMARY:-/dev/null}"
{
  echo "### cargo-mutants (advisory)"
  echo
  if (( caught + missed + timeouts + unviable == 0 )); then
    echo "No mutants in ${scope}."
  else
    echo "Mutants in ${scope}: ${caught} caught, ${missed} missed, ${timeouts} timed out, ${unviable} did not build (${status})."
    if (( missed > 0 )); then
      echo
      echo "Each missed mutant is a change to the code that no test noticed:"
      echo
      # shellcheck disable=SC2016 # literal backticks: each line becomes inline code
      sed 's/^/- `/; s/$/`/' "$REPORT/missed.txt"
    fi
    if (( timeouts > 0 )); then
      echo
      echo "Timed out (usually a mutant that makes a loop run forever, which counts as caught):"
      echo
      # shellcheck disable=SC2016 # literal backticks: each line becomes inline code
      sed 's/^/- `/; s/$/`/' "$REPORT/timeout.txt"
    fi
  fi
} >> "$summary"

# One warning per missed mutant on its line (GitHub shows up to 50 per job).
if (( missed > 0 )); then
  head -n 50 "$REPORT/missed.txt" | while IFS= read -r line; do
    if [[ "$line" =~ ^([^:]+):([0-9]+):([0-9]+):[[:space:]]*(.*)$ ]]; then
      message=${BASH_REMATCH[4]//%/%25}
      echo "::warning file=${BASH_REMATCH[1]},line=${BASH_REMATCH[2]},col=${BASH_REMATCH[3]},title=Missed mutant::${message}"
    fi
  done
fi
echo "[mutants] ${caught} caught, ${missed} missed, ${timeouts} timed out, ${unviable} unviable (${status})"
