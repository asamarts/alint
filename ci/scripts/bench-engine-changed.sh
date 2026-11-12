#!/usr/bin/env bash
# bench-record's engine-change guard: what, between two commits, can change the
# alint binary the bench builds. Prints one line per kind of change and nothing
# when there is none:
#   - files under crates/ that build into the binary: everything except docs,
#     legal files and test data, so bundled rulesets, schemas and build-script
#     inputs count along with the Rust source;
#   - manifests: Cargo.toml and crates/*/Cargo.toml (features, build profiles);
#   - third-party dependencies: Cargo.lock's packages that have a `source`.
# The workspace version changes on every release, so the root manifest's
# `version = "..."` line and the workspace members' lockfile entries (they have
# no `source`) are left out. Test data is listed by what it is not, so a new
# directory of inputs counts until it is added here. rustc is pinned by the
# bench job's BENCH_RUST, so the toolchain file is not an input.
#
# Usage: bench-engine-changed.sh <from> <to>, inside the repository.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <from> <to>" >&2
  exit 2
fi
from=$1
to=$2
for ref in "$from" "$to"; do
  git rev-parse --verify --quiet "${ref}^{commit}" > /dev/null ||
    { echo "$0: ${ref} is not a commit" >&2; exit 2; }
done

files="$(git diff --name-only "$from" "$to" -- crates/ |
  grep -vE '(^|/)(tests|ruleset-tests|benches|fixtures|scenarios|proptest-regressions)/|\.md$|/(NOTICE|LICENSE[^/]*|Cargo\.toml)$' ||
  true)"
if [[ -n "$files" ]]; then
  echo "$(wc -l <<< "$files") files under crates/ that build into the binary changed"
fi

# A commit's manifests, less comments, blank lines, the workspace version and the
# version of each path dependency (cargo builds a path dependency from its path,
# whatever version it names).
manifests() {
  local path
  git ls-tree -r --name-only "$1" -- Cargo.toml crates | grep -E '^(crates/[^/]+/)?Cargo\.toml$' |
    while IFS= read -r path; do
      git show "$1:$path" | sed -E \
        -e '/^[[:space:]]*(#|$)/d' \
        -e '/^version = "/d' \
        -e '/path = "/s/,[[:space:]]*version = "[^"]*"//' \
        -e '/path = "/s/version = "[^"]*",[[:space:]]*//' \
        -e "s|^|${path}: |"
    done
}
manifest="$(diff <(manifests "$from") <(manifests "$to") | grep -c '^[<>]' || true)"
if [[ "$manifest" -gt 0 ]]; then
  echo "${manifest} manifest lines changed besides the workspace version"
fi

# name, version and source of each third-party package in a commit's Cargo.lock
third_party() {
  git show "$1:Cargo.lock" | awk '
    /^\[\[package\]\]$/ { if (source != "") print name, version, source; name = version = source = "" }
    /^name = /    { name = $3 }
    /^version = / { version = $3 }
    /^source = /  { source = $3 }
    END { if (source != "") print name, version, source }' | LC_ALL=C sort
}
from_deps="$(third_party "$from")"
to_deps="$(third_party "$to")"
deps="$(LC_ALL=C comm -3 <(echo "$from_deps") <(echo "$to_deps") |
  awk 'NF { gsub(/"/, "", $1); print $1 }' | LC_ALL=C sort -u | paste -sd ' ' -)"
if [[ -n "$deps" ]]; then
  echo "third-party dependencies changed: ${deps}"
fi
