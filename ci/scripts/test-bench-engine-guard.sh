#!/usr/bin/env bash
# Pin bench-record's engine-change guard (ci/scripts/bench-engine-changed.sh and
# the guard job's `decide` step in .github/workflows/bench-record.yml) against a
# fixture workspace:
#   - a release that only bumps the workspace version, the path dependencies'
#     versions (before or after `path`) and the members' lockfile entries, and
#     changes docs, CI, legal files, manifest comments and test data, reports
#     nothing and skips;
#   - engine source, a build script, a bundled ruleset and a file in a directory
#     the script has never seen each count as a file that builds into the binary;
#   - a build profile or a feature counts as a manifest change;
#   - a third-party package bumped, added, removed or moved to another source
#     counts, by name;
#   - the decide step benches on any of these, skips on none, and benches a tag
#     with no prior release.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPT="$REPO_ROOT/ci/scripts/bench-engine-changed.sh"
EXTRACT="$REPO_ROOT/ci/scripts/workflow_step.py"
WF="$REPO_ROOT/.github/workflows/bench-record.yml"

sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
pass=0
fail=0
ok() { echo "  ok: $1"; pass=$((pass + 1)); }
bad() { echo "  FAIL: $1" >&2; fail=$((fail + 1)); }

repo="$sandbox/repo"
g() {
  git -C "$repo" -c user.name=test -c user.email=test@example.invalid \
    -c commit.gpgsign=false -c tag.gpgsign=false -c core.hooksPath=/dev/null "$@"
}
put() { mkdir -p "$(dirname "$repo/$1")"; printf '%s' "$2" > "$repo/$1"; }
commit() { g add -A; g commit -q -m "$1"; }

lock() { # lock <member version> <third-party entries...>
  printf 'version = 4\n\n[[package]]\nname = "a"\nversion = "%s"\ndependencies = [\n "dep",\n]\n' "$1"
  shift
  local entry
  for entry in "$@"; do
    read -r name version source <<< "$entry"
    printf '\n[[package]]\nname = "%s"\nversion = "%s"\nsource = "%s"\nchecksum = "00"\n' \
      "$name" "$version" "$source"
  done
}
REG="registry+https://github.com/rust-lang/crates.io-index"

git init -q "$repo"
put Cargo.toml '[workspace]
members = ["crates/a"]

[workspace.package]
version = "0.1.0"

[workspace.dependencies]
a = { path = "crates/a", version = "0.1.0" }
b = { version = "0.1.0", path = "crates/b" }
dep = "1.0.0"

[profile.release]
lto = true
'
put crates/a/Cargo.toml '[package]
name = "a"
version.workspace = true

[dependencies]
dep.workspace = true

[features]
default = []
'
put Cargo.lock "$(lock 0.1.0 "dep 1.0.0 $REG")"
put crates/a/src/lib.rs 'pub fn f() {}'
put crates/a/rulesets/r.yml 'rules: []'
put crates/a/README.md 'a'
put crates/a/NOTICE 'notice'
put crates/a/LICENSE-MIT 'mit'
put crates/a/tests/t.rs '#[test] fn t() {}'
put crates/a/tests/data/case.yml 'x: 1'
put crates/a/fixtures/tree/f.txt 'f'
put crates/a/scenarios/s.yml 's: 1'
put crates/a/ruleset-tests/r.yml 'r: 1'
put crates/a/proptest-regressions/p.txt 'cc 00'
put crates/b/benches/b.rs 'fn main() {}'
put docs/guide.md 'guide'
put .github/workflows/ci.yml 'on: push'
commit base
g tag v0.1.0
base=$(g rev-parse HEAD)

# check_case <label> <expected output>: diffs v0.1.0 against the working tree's
# commit, then resets to v0.1.0.
check_case() {
  local label=$1 want=$2 got rc=0
  commit "$label"
  got="$(cd "$repo" && "$SCRIPT" "$base" HEAD 2>&1)" || rc=$?
  if [[ $rc -eq 0 && "$got" == "$want" ]]; then ok "$label"; else
    bad "$label (exit $rc)"
    printf '      want: %s\n      got:  %s\n' "${want//$'\n'/ | }" "${got//$'\n'/ | }" >&2
  fi
  g reset -q --hard "$base"
}

release_bump() {
  sed -i -e 's/^version = "0.1.0"/version = "0.1.1"/' \
    -e 's/a = { path = "crates\/a", version = "0.1.0" }/a = { path = "crates\/a", version = "0.1.1" }/' \
    -e 's/b = { version = "0.1.0", path = "crates\/b" }/b = { version = "0.1.1", path = "crates\/b" }/' \
    -e 's/^\[workspace\]$/[workspace]\n# members are the crates\/ dirs/' "$repo/Cargo.toml"
  put Cargo.lock "$(lock 0.1.1 "dep 1.0.0 $REG")"
  for f in crates/a/README.md crates/a/NOTICE crates/a/LICENSE-MIT crates/a/tests/t.rs \
    crates/a/tests/data/case.yml crates/a/fixtures/tree/f.txt crates/a/scenarios/s.yml \
    crates/a/ruleset-tests/r.yml crates/a/proptest-regressions/p.txt crates/b/benches/b.rs \
    docs/guide.md .github/workflows/ci.yml; do
    echo changed >> "$repo/$f"
  done
}

echo "[bench-engine-guard] what counts"
release_bump
check_case "a version bump with docs, CI, legal and test-data changes reports nothing" ""

echo changed >> "$repo/crates/a/src/lib.rs"
check_case "engine source counts" "1 files under crates/ that build into the binary changed"
put crates/a/build.rs 'fn main() {}'
check_case "a build script counts" "1 files under crates/ that build into the binary changed"
echo "  - id: r" >> "$repo/crates/a/rulesets/r.yml"
check_case "a bundled ruleset counts" "1 files under crates/ that build into the binary changed"
put crates/a/assets/table.bin 'x'
put crates/a/assets/other.bin 'y'
check_case "files in a directory the script does not know count" \
  "2 files under crates/ that build into the binary changed"

sed -i 's/^lto = true$/lto = "thin"/' "$repo/Cargo.toml"
check_case "a build profile counts" "2 manifest lines changed besides the workspace version"
sed -i 's/^default = \[\]$/default = ["fast"]\nfast = []/' "$repo/crates/a/Cargo.toml"
check_case "a feature counts" "3 manifest lines changed besides the workspace version"

put Cargo.lock "$(lock 0.1.0 "dep 1.0.1 $REG")"
check_case "a third-party bump counts by name" "third-party dependencies changed: dep"
put Cargo.lock "$(lock 0.1.0 "dep 1.0.0 $REG" "zz 2.0.0 $REG" "extra 0.3.0 $REG")"
check_case "added third-party packages count by name" "third-party dependencies changed: extra zz"
put Cargo.lock "$(lock 0.1.0)"
check_case "a removed third-party package counts" "third-party dependencies changed: dep"
put Cargo.lock "$(lock 0.1.0 "dep 1.0.0 git+https://example.invalid/dep#00")"
check_case "a third-party package from another source counts" "third-party dependencies changed: dep"

release_bump
echo changed >> "$repo/crates/a/src/lib.rs"
sed -i 's/^lto = true$/lto = "thin"/' "$repo/Cargo.toml"
put Cargo.lock "$(lock 0.1.1 "dep 1.0.1 $REG")"
check_case "every kind is reported, the version bump still left out" \
  "1 files under crates/ that build into the binary changed
2 manifest lines changed besides the workspace version
third-party dependencies changed: dep"

rc=0
(cd "$repo" && "$SCRIPT" "$base" no-such-ref > /dev/null 2>&1) || rc=$?
if [[ $rc -eq 2 ]]; then ok "an unknown commit exits 2"; else bad "an unknown commit exits 2 (exit $rc)"; fi

echo "[bench-engine-guard] the decide step"
release_bump
commit "v0.1.1: version bump only"
g tag v0.1.1
echo changed >> "$repo/crates/a/src/lib.rs"
commit "v0.1.2: engine change"
g tag v0.1.2
mkdir -p "$repo/ci/scripts"
cp "$SCRIPT" "$repo/ci/scripts/"
python3 "$EXTRACT" "$WF" guard decide run > "$sandbox/decide.sh"

# decide <tag>: runs the step for a tag push; sets $rc, $out and $gh_output
decide() {
  : > "$sandbox/gh_output"
  rc=0
  out="$(cd "$repo" && env EVENT=push TAG="$1" INPUT_REF= GITHUB_OUTPUT="$sandbox/gh_output" \
    bash "$sandbox/decide.sh" 2>&1)" || rc=$?
  gh_output="$(cat "$sandbox/gh_output")"
}
expect_decide() {
  local label=$1 bench=$2 prev=$3 notice=$4
  if [[ $rc -eq 0 && "$gh_output" == *"should_bench=${bench}"* && "$gh_output" == *"prev=${prev}"* &&
    "$out" == *"$notice"* ]]; then ok "$label"; else
    bad "$label (exit $rc)"
    printf '      output: %s\n      GITHUB_OUTPUT: %s\n' "${out//$'\n'/ | }" "${gh_output//$'\n'/ | }" >&2
  fi
}
decide v0.1.1
expect_decide "a version-bump-only tag skips" false v0.1.0 \
  "::notice::nothing that builds into the binary changed v0.1.0..v0.1.1 -- skipping bench"
decide v0.1.2
expect_decide "a tag with an engine change benches" true v0.1.1 \
  "::notice::v0.1.1..v0.1.2: 1 files under crates/ that build into the binary changed -- benching."
decide v0.1.0
expect_decide "a tag with no prior release benches" true "" "::notice::no prior release tag -- benching."

echo "[bench-engine-guard] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
