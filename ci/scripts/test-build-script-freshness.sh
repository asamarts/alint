#!/usr/bin/env bash
# Pin crates/alint/build.rs's rerun tracking (docs/design/build-dev-loop.md
# section 1).
#
# BEHAVIOURAL: a two-file crate whose build script is a verbatim copy of
# alint's is built with real cargo inside real git checkouts. Each case reads
# cargo's own verbose output to tell whether the build script ran and whether
# the binary stayed fresh, then runs the binary to check that the SHA it baked
# matches the checkout's HEAD. Checking the emitted paths would not catch the
# original defect: `../../.git/HEAD` is a plausible-looking path that does not
# exist in a linked worktree, and cargo treats a missing path as changed on
# every build.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BUILD_RS="$REPO_ROOT/crates/alint/build.rs"

if ! command -v cargo >/dev/null 2>&1; then
  echo "[build-script-freshness] cargo not found" >&2
  exit 1
fi

TMP_ROOT=$(mktemp -d)
trap 'rm -rf "$TMP_ROOT"' EXIT

# Isolate git from the caller: no repository variables inherited from a hook,
# no user or system config, fixed identities, and no discovery of a repository
# above the fixture (the no-git case relies on that).
while IFS= read -r var; do unset "$var"; done < <(git rev-parse --local-env-vars)
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=fixture GIT_AUTHOR_EMAIL=fixture@example.invalid
export GIT_COMMITTER_NAME=fixture GIT_COMMITTER_EMAIL=fixture@example.invalid
export GIT_CEILING_DIRECTORIES="$TMP_ROOT"
export CARGO_TERM_COLOR=never
unset CARGO_TARGET_DIR CARGO_BUILD_TARGET_DIR

pass=0
fail=0

ok() {
  echo "  ok: $1"
  pass=$((pass + 1))
}

bad() {
  echo "  FAIL: $1" >&2
  fail=$((fail + 1))
}

# The crate sits two levels down, like crates/alint, so a GIT_DIR relative to
# the repository root does not resolve from the build script's directory.
make_fixture() {
  local root=$1
  mkdir -p "$root/crates/fixture/src"
  cat >"$root/Cargo.toml" <<'EOF'
[workspace]
members = ["crates/fixture"]
resolver = "2"
EOF
  cat >"$root/crates/fixture/Cargo.toml" <<'EOF'
[package]
name = "fixture"
version = "0.1.0"
edition = "2024"
publish = false
EOF
  cat >"$root/crates/fixture/src/main.rs" <<'EOF'
fn main() {
    println!("{} {}", env!("ALINT_GIT_SHA"), env!("ALINT_BUILD_DATE"));
}
EOF
  cp "$BUILD_RS" "$root/crates/fixture/build.rs"
  printf 'Cargo.lock\n' >"$root/.gitignore"
}

# script_ran: whether cargo -v output on stdin shows the build script run.
# cargo prints "Running `<path>`" for it: `<hash dir>/build-script-build`
# (cargo 1.97, for one) or `<hash dir>/out/build_script_build` (the build-dir
# layout of cargo 1.100, the default Rust of the ubuntu-26.04 runner image
# from 20261116). Compiling it shows `--crate-name build_script_build` and arguments
# after it, so the name is never right before the backtick there.
# shellcheck disable=SC2016  # the backticks are cargo's, literal
script_ran() {
  grep -qE 'Running `[^`]*/build[-_]script[-_]build`'
}

# Both layouts' lines, whichever cargo runs this harness: a regex that misses
# one reads every build as fresh, and every expect_fresh case passes.
# shellcheck disable=SC2016  # the backticks are cargo's, literal
for line in \
  '     Running `/t/debug/build/fixture-0123456789abcdef/build-script-build`' \
  '     Running `/t/debug/build/fixture/0123456789abcdef/out/build_script_build`'; do
  if script_ran <<<"$line"; then ok "a run line is read as a run: ${line##*/}"; else
    bad "a run line is not read as a run: $line"
  fi
done
# shellcheck disable=SC2016  # the backticks are cargo's, literal
compile='     Running `rustc --crate-name build_script_build --edition=2024 build.rs -C opt-level=0`'
if script_ran <<<"$compile"; then bad "compiling the script is read as a run"; else
  ok "compiling the script is not read as a run"
fi

# build <checkout> [env assignments...]: one verbose cargo build with the
# checkout's own target dir. Sets RAN (the build script executed) and FRESH
# (cargo reported the binary up to date).
build() {
  local dir=$1
  shift
  local log
  log=$(cd "$dir" && env "$@" CARGO_TARGET_DIR="$dir.target" cargo build -v 2>&1) || {
    echo "$log" >&2
    bad "cargo build failed in ${dir#"$TMP_ROOT"/}"
    RAN=error FRESH=error
    return 0
  }
  if script_ran <<<"$log"; then RAN=yes; else RAN=no; fi
  if grep -qE '^ *Fresh fixture v' <<<"$log"; then FRESH=yes; else FRESH=no; fi
}

baked_sha() {
  "$1.target/debug/fixture" | cut -d' ' -f1
}

head_sha() {
  git -C "$1" rev-parse --short=12 HEAD
}

commit() {
  echo "$2" >>"$1/notes.txt"
  git -C "$1" add notes.txt
  git -C "$1" commit -q -m "$2"
}

expect_fresh() {
  local name=$1 dir=$2
  shift 2
  build "$dir" "$@"
  if [[ "$RAN" == no && "$FRESH" == yes ]]; then
    ok "$name"
  else
    bad "$name (build script ran=$RAN, binary fresh=$FRESH; expected no rerun)"
  fi
}

# expect_rerun <name> <dir> <expected sha> [env...]
expect_rerun() {
  local name=$1 dir=$2 want=$3
  shift 3
  build "$dir" "$@"
  local got
  got=$(baked_sha "$dir" 2>/dev/null || echo missing)
  if [[ "$RAN" == yes && "$got" == "$want" ]]; then
    ok "$name"
  else
    bad "$name (build script ran=$RAN, baked $got, expected a rerun baking $want)"
  fi
}

MAIN="$TMP_ROOT/main"
WT="$TMP_ROOT/worktree"
NOGIT="$TMP_ROOT/nogit"

make_fixture "$MAIN"
git -C "$MAIN" init -q -b main
git -C "$MAIN" add -A
git -C "$MAIN" commit -q -m initial

echo "==> main checkout"
expect_rerun "first build bakes HEAD" "$MAIN" "$(head_sha "$MAIN")"
expect_fresh "unchanged rebuild is fresh" "$MAIN"

echo "==> linked worktree"
git -C "$MAIN" worktree add -q -b other "$WT"
expect_rerun "first worktree build bakes its HEAD" "$WT" "$(head_sha "$WT")"
expect_fresh "unchanged worktree rebuild is fresh" "$WT"

echo "==> commits on other branches"
commit "$WT" "on other"
expect_fresh "a commit in another worktree does not rerun the main checkout" "$MAIN"
expect_rerun "a commit on the worktree's branch reruns it" "$WT" "$(head_sha "$WT")"
expect_fresh "worktree rebuild after that is fresh" "$WT"
commit "$MAIN" "on main"
expect_rerun "a commit on the checked-out branch reruns" "$MAIN" "$(head_sha "$MAIN")"
expect_fresh "rebuild after the commit is fresh" "$MAIN"
expect_fresh "the main checkout's commit does not rerun the worktree" "$WT"

echo "==> packed refs"
git -C "$MAIN" pack-refs --all
[[ ! -e "$MAIN/.git/refs/heads/main" ]] || bad "pack-refs left a loose refs/heads/main"
# Packing deletes the watched loose ref, so one rerun here is expected.
build "$MAIN"
expect_fresh "rebuild with the branch packed is fresh" "$MAIN"
commit "$MAIN" "on packed main"
expect_rerun "a commit on a packed branch reruns" "$MAIN" "$(head_sha "$MAIN")"
expect_fresh "rebuild after it is fresh" "$MAIN"

echo "==> detached HEAD"
git -C "$MAIN" checkout -q --detach HEAD~1
expect_rerun "detaching to another commit reruns" "$MAIN" "$(head_sha "$MAIN")"
expect_fresh "rebuild while detached is fresh" "$MAIN"
commit "$MAIN" "detached"
expect_rerun "a commit while detached reruns" "$MAIN" "$(head_sha "$MAIN")"
git -C "$MAIN" checkout -q main

echo "==> inherited repository variables"
# A linked worktree's git hooks run with an absolute GIT_DIR. A hook there
# that builds another checkout must bake that checkout's commit, not the
# worktree's; a relative GIT_DIR must not make git fail from the crate dir.
commit "$MAIN" "before inherited GIT_DIR build"
expect_rerun "another checkout's absolute GIT_DIR still bakes this HEAD" "$MAIN" "$(head_sha "$MAIN")" \
  GIT_DIR="$(git -C "$WT" rev-parse --absolute-git-dir)"
commit "$MAIN" "before relative GIT_DIR build"
expect_rerun "a root-relative GIT_DIR still bakes HEAD" "$MAIN" "$(head_sha "$MAIN")" GIT_DIR=.git

echo "==> no git"
mkdir -p "$NOGIT"
cp -R "$MAIN/Cargo.toml" "$MAIN/crates" "$NOGIT/"
expect_rerun "a build outside git bakes unknown" "$NOGIT" unknown
expect_fresh "unchanged rebuild outside git is fresh" "$NOGIT"

echo "[build-script-freshness] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
