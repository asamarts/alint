#!/usr/bin/env bash
# Decide whether a release tag may move the FLOATING release pointers.
#
# Immutable publishes (crates.io, npm, PyPI, the GitHub Release's own assets,
# Docker :vX.Y.Z / :X.Y.Z) are always safe. The moving pointers are not: a
# backport release (v0.16.2 after v0.17.0) or a `gh run rerun` of an old tag's
# job must never drag them BACKWARDS. release.yml therefore moves
#   - the major tag (v0), Docker :latest, the Homebrew formula and the GitHub
#     "Latest" release badge only when highest_overall=true, and
#   - Docker :X.Y only when highest_in_minor=true,
# and otherwise skips with a ::notice::.
#
# Only final release tags (vX.Y.Z) are compared; anything else (a pre-release
# such as v1.0.0-rc.1, the v0 pointer itself) is ignored, and a pre-release TAG
# never moves a pointer. A tag equal to the highest (a re-run of the newest
# release) counts as highest. Comparison is numeric per component.
#
# Usage: release-pointer-guard.sh <tag>
# Tags come from `git ls-remote --tags origin` (every published tag, no full
# clone needed); RELEASE_TAGS (newline/space separated) overrides it for tests.
# Prints `highest_overall=…` / `highest_in_minor=…` and appends them to
# $GITHUB_OUTPUT when set. Fails closed (non-zero) if the tags cannot be listed.
set -euo pipefail

TAG="${1:?usage: release-pointer-guard.sh <tag>}"
re='^v([0-9]+)\.([0-9]+)\.([0-9]+)$'

if [[ -n "${RELEASE_TAGS+set}" ]]; then
  tags="$RELEASE_TAGS"
else
  tags=""
  for attempt in 1 2 3; do
    if tags="$(git ls-remote --tags --refs origin 'v*')"; then
      break
    fi
    if [[ "$attempt" == 3 ]]; then
      echo "::error::release-pointer-guard: cannot list the remote tags; refusing to guess" >&2
      exit 1
    fi
    sleep 5
  done
  tags="$(awk '{ sub("^refs/tags/", "", $2); print $2 }' <<< "$tags")"
fi

overall=true
in_minor=true
if [[ ! "$TAG" =~ $re ]]; then
  overall=false
  in_minor=false
  echo "release-pointer-guard: ${TAG} is not a final vX.Y.Z release; no floating pointer moves"
else
  maj=$((10#${BASH_REMATCH[1]})) min=$((10#${BASH_REMATCH[2]})) pat=$((10#${BASH_REMATCH[3]}))
  for t in $tags; do
    [[ "$t" =~ $re ]] || continue
    a=$((10#${BASH_REMATCH[1]})) b=$((10#${BASH_REMATCH[2]})) c=$((10#${BASH_REMATCH[3]}))
    if (( a > maj || (a == maj && b > min) || (a == maj && b == min && c > pat) )); then
      overall=false
      echo "release-pointer-guard: ${t} is newer than ${TAG}"
      if (( a == maj && b == min )); then
        in_minor=false
      fi
    fi
  done
fi

echo "highest_overall=${overall}"
echo "highest_in_minor=${in_minor}"
if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    echo "highest_overall=${overall}"
    echo "highest_in_minor=${in_minor}"
  } >> "$GITHUB_OUTPUT"
fi
