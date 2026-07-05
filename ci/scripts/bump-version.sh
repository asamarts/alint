#!/usr/bin/env bash
# Bump the workspace version + every user-facing install-snippet
# version pin to match. Single source of truth: the value in
# `[workspace.package].version` of Cargo.toml. After bumping
# locally, run `ci/scripts/check-version-pins.sh` (or just
# `bash ci/scripts/preflight.sh`) to verify no site was missed.
#
# Out of scope (must be updated manually if relevant):
#   - alint.org repo: src/pages/index.astro JSON-LD softwareVersion
#   - alint.org repo: src/pages/roadmap.astro "Latest release" line
#   - alint.org repo: src/content/docs/docs/index.mdx version badge
#     (the only hand-written page in the synced docs subtree)
#   - docs/design/ROADMAP.md "Latest release: vX.Y.Z" line — the
#     canonical source for `xtask gen-public-roadmap`; the prose
#     after the version describes what shipped, so it can't be
#     mechanically sed-replaced (must be hand-written)
#   - CHANGELOG.md body for the new entry (a stub is inserted; you
#     fill in what changed)
#   - docs/benchmarks/HISTORY.md (per-release perf rows — added
#     by the bench-record workflow, not by this script)
#   - examples/*/README.md (case-study captures at a fixed point;
#     deliberately not auto-bumped)
#
# Usage:
#   bash ci/scripts/bump-version.sh 0.9.21

set -euo pipefail

NEW="${1:-}"
if [[ -z "$NEW" ]]; then
  echo "usage: $0 <new-version, e.g. 0.9.21>" >&2
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

CUR=$(awk -F'"' '
  /^\[workspace\.package\]/ { f=1 }
  f && /^version =/ { print $2; exit }
' Cargo.toml)

if [[ -z "$CUR" ]]; then
  echo "could not read workspace version from Cargo.toml" >&2
  exit 2
fi

if [[ "$CUR" == "$NEW" ]]; then
  echo "already at $NEW — nothing to do"
  exit 0
fi

echo "==> bump: $CUR -> $NEW"

# 1. Workspace version (single line under [workspace.package]).
#    We anchor on the literal old version to avoid bumping the
#    [workspace.dependencies] internal-crate pins (those are the
#    inter-crate API compat floor — bumped separately when an
#    inter-crate API breaks).
sed -i "s|^version = \"$CUR\"$|version = \"$NEW\"|" Cargo.toml

# 1b. Refresh Cargo.lock workspace-crate version entries to match
#     the new workspace.package.version. Without this explicit
#     refresh, the first `cargo build` in the next preflight does
#     it as a side effect — but if the maintainer doesn't notice
#     and stage the change, the release pushes a stale Cargo.lock
#     that mismatches the bumped Cargo.toml. CI's
#     `cargo build --locked` then fails with "cannot update the
#     lock file because --locked was passed". Caught the hard way
#     on v0.9.22 (release.yml run 25890555488).
#
#     `cargo metadata` resolves the workspace dep graph and writes
#     out an updated Cargo.lock as a side effect, without
#     compiling. `--offline` keeps it from hitting the network.
echo "==> refreshing Cargo.lock (cargo metadata --offline)"
cargo metadata --offline --format-version 1 > /dev/null

# 2. Install snippets across user-facing files. The version
#    appears as `vX.Y.Z` (GHA ref, pre-commit rev, docker tag with
#    'v'), `:X.Y.Z` (docker tag, bare semver), and once in the
#    SECURITY.md sentence "as of the vX.Y.Z release". One sed
#    handles all forms because the regex's `(v|:)` alternation
#    plus a word boundary on the trailing side covers them.
SNIPPET_FILES=(
  README.md
  SECURITY.md
  docs/site/integrations/docker.md
  docs/site/integrations/github-actions.md
  docs/site/integrations/pre-commit.md
  docs/site/getting-started/installation.md
)
CUR_ESCAPED="${CUR//./\\.}"
# `#` as the sed delimiter so the alternation `(v|:)` inside the
# pattern doesn't terminate the substitution. `|` would clash.
for f in "${SNIPPET_FILES[@]}"; do
  if [[ -f "$f" ]]; then
    sed -i -E "s#(v|:)${CUR_ESCAPED}([^0-9.]|\$)#\1${NEW}\2#g" "$f"
  fi
done

# 2a-bis. Docker `<major>.<minor>` channel tag (docker.md). The loop above only
#   rewrites full-version pins (`:0.13.0`); the bare minor-channel tag (`:0.13`)
#   has no patch component, so it never matched and silently rotted for several
#   releases (shipped `:0.10` at v0.13.0). Rewrite it here. Ordering matters: the
#   full-version pass already bumped `:0.13.0`, and the trailing `[^0-9.]|$`
#   guard keeps this pass from re-matching a full pin.
CUR_MINOR="${CUR%.*}"
NEW_MINOR="${NEW%.*}"
if [[ -f docs/site/integrations/docker.md ]]; then
  sed -i -E "s#:${CUR_MINOR//./\\.}([^0-9.]|\$)#:${NEW_MINOR}\1#g" docs/site/integrations/docker.md
fi

# 2b. npm shim version. The package itself ships zero JS behaviour
#     — it downloads the matching binary at install time — but
#     `npm view @asamarts/alint version` reports whatever is in
#     this file at HEAD, so it should track the workspace version.
#     release.yml also rewrites this at publish time (belt-and-
#     suspenders); the bump here makes the committed value at HEAD
#     match what users see post-publish.
if [[ -f npm/package.json ]]; then
  sed -i -E "s/^([[:space:]]*\"version\":[[:space:]]+)\"${CUR_ESCAPED}\"/\1\"${NEW}\"/" npm/package.json
fi

# 3. CHANGELOG stub at top so the release date is captured.
#    The new entry is "TBD" — the author fills in what changed.
#    We do NOT touch any existing `## [...]` entries (history).
if [[ -f CHANGELOG.md ]]; then
  TODAY=$(date -u +%Y-%m-%d)
  # Insert before the FIRST existing `## [` entry. awk preserves
  # earlier lines (header / preamble) verbatim.
  awk -v new="$NEW" -v today="$TODAY" '
    !inserted && /^## \[/ {
      printf("## [%s] - %s (release notes pending)\n\nTBD\n\n", new, today);
      inserted = 1
    }
    { print }
  ' CHANGELOG.md > CHANGELOG.md.tmp && mv CHANGELOG.md.tmp CHANGELOG.md

  # 3b. CHANGELOG link-reference footer. Two operations:
  #     a) Repoint [Unreleased] from v$CUR...HEAD to v$NEW...HEAD
  #        so the compare link at the top of the footer reflects
  #        the new floor.
  #     b) Insert a `[$NEW]: compare/v$CUR...v$NEW` row right after
  #        the [Unreleased] row so GitHub renders the [x.y.z] link
  #        references inside the new release entry.
  #     Both are idempotent: re-running with the same $NEW leaves
  #     the footer untouched (sed match misses; awk insert guards
  #     against an existing $NEW row).
  if grep -q "^\[Unreleased\]:" CHANGELOG.md; then
    sed -i -E "s#^(\[Unreleased\]:[[:space:]]+https://github\.com/asamarts/alint/compare/v)${CUR_ESCAPED}(\.\.\.HEAD)\$#\1${NEW}\2#" CHANGELOG.md
    if ! grep -q "^\[${NEW}\]:" CHANGELOG.md; then
      awk -v new="$NEW" -v cur="$CUR" '
        /^\[Unreleased\]:/ {
          print
          printf("[%s]: https://github.com/asamarts/alint/compare/v%s...v%s\n", new, cur, new)
          next
        }
        { print }
      ' CHANGELOG.md > CHANGELOG.md.tmp && mv CHANGELOG.md.tmp CHANGELOG.md
    fi
  fi
fi

echo
echo "==> manual followups"
echo "  1. fill in CHANGELOG.md [$NEW] body"
echo "  2. update docs/design/ROADMAP.md 'Latest release' line (prose, not just the version)"
echo "  3. alint.org repo (separate): bump src/pages/index.astro JSON-LD softwareVersion"
echo "  4. alint.org repo (separate): bump src/pages/roadmap.astro 'Latest release: vX.Y.Z'"
echo "  5. alint.org repo (separate): bump src/content/docs/docs/index.mdx 'vX.Y.Z latest' badge"
echo "  6. verify (alint.org): bash scripts/check-version-pins.sh"
echo
echo "==> ok to commit + release"
