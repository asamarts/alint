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
if [[ ! "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "invalid version '$NEW': expected X.Y.Z without a leading 'v'" >&2
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
#    The workspace dependencies use Cargo caret requirements. Before 1.0, a
#    new minor is a new compatibility line (`0.18` does not accept `0.19`), and
#    a new major is always a new compatibility line. Advance the internal-crate
#    floors in those cases before asking Cargo to resolve the bumped workspace.
sed -i "s|^version = \"$CUR\"$|version = \"$NEW\"|" Cargo.toml

IFS=. read -r CUR_MAJOR CUR_MINOR _ <<<"$CUR"
IFS=. read -r NEW_MAJOR NEW_MINOR _ <<<"$NEW"
if [[ "$CUR_MAJOR" != "$NEW_MAJOR" || ( "$CUR_MAJOR" == "0" && "$CUR_MINOR" != "$NEW_MINOR" ) ]]; then
  NEW_FLOOR="${NEW_MAJOR}.${NEW_MINOR}.0"
  sed -i -E \
    "s#^(alint-[a-z-]+ = \\{ path = \"[^\"]+\", version = \")[0-9]+\\.[0-9]+\\.[0-9]+(\" \\})\$#\\1${NEW_FLOOR}\\2#" \
    Cargo.toml
fi

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

# 1c. `facts.json` embeds the workspace version and is generated source. Refresh
#     it here so the release commit cannot carry a stale discovery contract.
echo "==> refreshing facts.json (xtask gen-facts)"
cargo run -q -p xtask -- gen-facts

# 1d. Draft v2 schema `$id` (#570): when `schemas/v2/config.json`'s body
#     changed since the previous `v*.*.*` tag (ignoring `$id`), advance
#     `xtask/src/gen_schema_v2.rs::RELEASE`, its unit test, the two docs
#     modelines, and regenerate `schemas/v2/config.json`.
if [[ -f schemas/v2/config.json && -f xtask/src/gen_schema_v2.rs ]]; then
  NEW_TAG="v${NEW}"
  V2_SCHEMA_STATE=$(python3 - "$NEW_TAG" <<'PY'
import json
import re
import subprocess
import sys
from pathlib import Path

new_tag = sys.argv[1]
semver_re = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")

def parse(tag):
    m = semver_re.match(tag)
    return tuple(int(x) for x in m.groups()) if m else None

def canonical_body(obj):
    body = dict(obj)
    body.pop("$id", None)
    return json.dumps(body, sort_keys=True, separators=(",", ":"))

new_ver = parse(new_tag)
if new_ver is None:
    print(f"invalid target tag {new_tag!r}", file=sys.stderr)
    sys.exit(2)

cur = json.loads(Path("schemas/v2/config.json").read_text(encoding="utf-8"))
if not isinstance(cur, dict):
    print("schemas/v2/config.json top-level value is not an object", file=sys.stderr)
    sys.exit(2)

tags = subprocess.run(
    ["git", "tag", "-l", "v*.*.*"],
    capture_output=True,
    text=True,
    check=True,
).stdout.splitlines()
prior = sorted(
    ((parse(t.strip()), t.strip()) for t in tags if parse(t.strip()) and parse(t.strip()) < new_ver),
    reverse=True,
)
for _, tag in prior:
    subprocess.run(
        ["git", "cat-file", "-e", f"{tag}^{{commit}}"],
        capture_output=True,
        text=True,
        check=True,
    )
    ls = subprocess.run(
        ["git", "ls-tree", "-r", "--name-only", tag, "--", "schemas/v2/config.json"],
        capture_output=True,
        text=True,
        check=True,
    )
    if "schemas/v2/config.json" not in ls.stdout.splitlines():
        continue
    show = subprocess.run(
        ["git", "show", f"{tag}:schemas/v2/config.json"],
        capture_output=True,
        text=True,
        check=True,
    )
    prev = json.loads(show.stdout)
    if not isinstance(prev, dict):
        print(f"{tag}:schemas/v2/config.json top-level value is not an object", file=sys.stderr)
        sys.exit(2)
    print("changed" if canonical_body(cur) != canonical_body(prev) else "unchanged")
    sys.exit(0)
print("changed")
PY
  )
  if [[ "$V2_SCHEMA_STATE" == "changed" ]]; then
    OLD_V2_REL=$(sed -nE 's/^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?const[[:space:]]+RELEASE:[[:space:]]*&str[[:space:]]*=[[:space:]]*"(v[0-9]+\.[0-9]+\.[0-9]+)";/\3/p' xtask/src/gen_schema_v2.rs | head -n1)
    if [[ -z "$OLD_V2_REL" ]]; then
      echo "could not read const RELEASE from xtask/src/gen_schema_v2.rs" >&2
      exit 2
    fi
    if [[ "$OLD_V2_REL" != "$NEW_TAG" ]]; then
      echo "==> refreshing draft v2 schema \$id: $OLD_V2_REL -> $NEW_TAG"
      OLD_V2_ESC="${OLD_V2_REL//./\\.}"
      sed -i -E "s|(const[[:space:]]+RELEASE:[[:space:]]*&str[[:space:]]*=[[:space:]]*)\"${OLD_V2_ESC}\";|\\1\"${NEW_TAG}\";|" xtask/src/gen_schema_v2.rs
      if [[ -f xtask/src/gen_schema_v2_tests.rs ]]; then
        sed -i -E "s|(https://[^\"]+/)${OLD_V2_ESC}(/schemas/v2/config\\.json)|\\1${NEW_TAG}\\2|g" xtask/src/gen_schema_v2_tests.rs
      fi
      for doc in docs/site/configuration/version-2.md docs/development/CONFIG-AUTHORING.md; do
        if [[ -f "$doc" ]]; then
          sed -i -E "s|(https://raw\\.githubusercontent\\.com/asamarts/alint/)${OLD_V2_ESC}(/schemas/v2/config\\.json)|\\1${NEW_TAG}\\2|g" "$doc"
        fi
      done
      cargo run -q -p xtask -- gen-schema
    fi
  elif [[ "$V2_SCHEMA_STATE" != "unchanged" ]]; then
    echo "unexpected v2 schema state: $V2_SCHEMA_STATE" >&2
    exit 2
  fi
fi

# 2. Install snippets across user-facing files, plus action.yml's private
#    ALINT_BAKED_VERSION. Keeping the baked value on the release commit lets a
#    commit-SHA Action pin select the matching binary without a network lookup;
#    check-release-version.sh fails a tag whose value was not updated here.
#    The version appears as `vX.Y.Z` (GHA ref, pre-commit rev, docker tag with
#    'v') and `:X.Y.Z` (docker tag, bare semver). One sed handles both
#    forms because the regex's `(v|:)` alternation plus a word boundary
#    on the trailing side covers them. SECURITY.md's advisory list names
#    the fixing release as a bare `X.Y.Z`, which neither this sed nor
#    check-version-pins.sh touches.
SNIPPET_FILES=(
  action.yml
  README.md
  SECURITY.md
  docs/site/integrations/docker.md
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

# 2b. npm shim version and its exact-version platform packages
#     (optionalDependencies). release.yml's stager also stamps both from
#     the tag at publish time (belt-and-suspenders); the bump here makes
#     the committed values at HEAD match what users see post-publish, and
#     check-version-pins.sh enforces it.
if [[ -f npm/package.json ]]; then
  sed -i -E "s/^([[:space:]]*\"version\":[[:space:]]+)\"${CUR_ESCAPED}\"/\1\"${NEW}\"/" npm/package.json
  sed -i -E "s/^([[:space:]]*\"@asamarts\/alint-[a-z0-9-]+\":[[:space:]]+)\"${CUR_ESCAPED}\"/\1\"${NEW}\"/" npm/package.json
fi

# 2c. Zed extension version. Unlike VS Code/JetBrains (which release.yml
#     version-stamps from the tag at publish, so their committed value may
#     lag intentionally), the Zed registry builds the extension from source
#     at the committed version — so the committed value is what ships and
#     must track the workspace version. check-version-pins.sh enforces it.
for zf in editors/zed/extension.toml editors/zed/Cargo.toml; do
  [[ -f "$zf" ]] && sed -i -E "s/^(version = )\"${CUR_ESCAPED}\"/\1\"${NEW}\"/" "$zf"
done
# The extension's own lockfile entry follows its Cargo.toml; the editors job
# builds with --locked, so a stale entry fails it. Set it to NEW whatever it
# held, since this entry lagged the manifest before the gate existed.
if [[ -f editors/zed/Cargo.lock ]]; then
  sed -i -E "/^name = \"alint-zed\"\$/{n;s/^version = \"[^\"]*\"\$/version = \"${NEW}\"/}" editors/zed/Cargo.lock
fi

# 2d. npm README version example (@asamarts/alint@X.Y.Z + "alint vX.Y.Z").
#     Illustrative, and not in the SCOPE files (its @-prefixed pin wouldn't match the
#     SCOPE regex anyway), so keep it current here or it rots (it had gone 10 minors stale).
if [[ -f npm/README.md ]]; then
  sed -i -E "s#(@asamarts/alint@|alint v)${CUR_ESCAPED}#\1${NEW}#g" npm/README.md
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
echo "  7. after publishing the tag, update ci/action-doc-pin.env and the GitHub Action snippets"
echo
echo "==> ok to commit + release"
