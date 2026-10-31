#!/usr/bin/env bash
# Negative controls for check-shadow-feature-tree.sh. In copies of the
# workspace's manifests, inject each way `shadow-ir` could reach a shipped or
# benchmarked build and require the check to fail naming that build; the
# unmodified copy must pass. A control whose edit does not apply fails too,
# so a manifest change cannot quietly disarm it.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CHECK="$REPO_ROOT/ci/scripts/check-shadow-feature-tree.sh"
TMP_ROOT=$(mktemp -d)
trap 'rm -rf "$TMP_ROOT"' EXIT

# The manifests, lockfile, and toolchain file as they are in the working tree,
# plus every Rust source as an empty file: cargo needs each target's file to
# exist, not its contents.
pristine="$TMP_ROOT/pristine"
mkdir -p "$pristine"
git -C "$REPO_ROOT" ls-files -z -- '*.toml' Cargo.lock \
  | tar -C "$REPO_ROOT" --null -T - -cf - | tar -C "$pristine" -xf -
while IFS= read -r -d '' source; do
  mkdir -p "$pristine/$(dirname "$source")"
  : >"$pristine/$source"
done < <(git -C "$REPO_ROOT" ls-files -z -- '*.rs')

failures=0
fail() {
  echo "FAIL: $*" >&2
  failures=$((failures + 1))
}

# run_check <copy>: sets $rc and $out.
run_check() {
  set +e
  out=$(SHADOW_TREE_ROOT="$1" bash "$CHECK" 2>&1)
  rc=$?
  set -e
}

run_check "$pristine"
if [[ $rc -ne 0 ]]; then
  fail "the unmodified copy fails the check (rc=$rc):"
  printf '%s\n' "$out" >&2
fi

# leak <name> <manifest> <expected build label> <edit...>: copy the pristine
# tree, apply the edit (a python3 snippet over the manifest's text `s`), and
# require the check to fail naming the build.
leak() {
  local name=$1 manifest=$2 label=$3 edit=$4 copy
  copy="$TMP_ROOT/$name"
  cp -R "$pristine" "$copy"
  if ! python3 - "$copy/$manifest" "$edit" <<'PY'; then
import sys
path, edit = sys.argv[1], sys.argv[2]
s = open(path).read()
before = s
exec(edit)
if s == before:
    sys.exit(1)
open(path, "w").write(s)
PY
    fail "$name: the edit does not apply to $manifest"
    return
  fi
  run_check "$copy"
  if [[ $rc -eq 0 ]]; then
    fail "$name: the check passed with shadow-ir leaked into $label"
    printf '%s\n' "$out" >&2
  elif ! grep -qF "::error title=shadow-ir feature tree::${label} enables shadow-ir" <<<"$out"; then
    fail "$name: the check failed without naming $label (rc=$rc):"
    printf '%s\n' "$out" >&2
  else
    echo "ok: $name fails the check for $label"
  fi
}

# Through the binary package's own default features.
leak default-feature crates/alint/Cargo.toml "the release build (-p alint)" '
assert "[features]" not in s
s += "\n[features]\ndefault = [\"leak\"]\nleak = [\"alint-rules/shadow-ir\"]\n"'

# Through a dependency on a platform other than the host (needs --target all).
leak other-target crates/alint/Cargo.toml "the release build (-p alint)" '
s += "\n[target.\x27cfg(target_os = \"freebsd\")\x27.dependencies]\nalint-rules = { workspace = true, features = [\"shadow-ir\"] }\n"'

# Through a package only bench-record builds next to the binary.
leak xtask xtask/Cargo.toml "the bench-record build (-p alint -p xtask)" '
s = s.replace("alint-rules.workspace = true", "alint-rules = { workspace = true, features = [\"shadow-ir\"] }", 1)'

# Through a bench-only feature.
leak bench-feature crates/alint-bench/Cargo.toml "the bench build (-p alint-bench, all features)" '
s = s.replace("fs-benches = []", "fs-benches = [\"alint-rules/shadow-ir\"]", 1)'

# Through a dev-dependency, which `cargo bench` compiles.
leak bench-dev-dependency crates/alint-bench/Cargo.toml "the bench build (-p alint-bench, all features)" '
s = s.replace("[dev-dependencies]\n", "[dev-dependencies]\nalint-rules = { workspace = true, features = [\"shadow-ir\"] }\n", 1)'

if [[ $failures -ne 0 ]]; then
  echo "[shadow-feature-tree] $failures control(s) failed" >&2
  exit 1
fi
echo "[shadow-feature-tree] OK: the unmodified tree passes and every injected leak fails"
