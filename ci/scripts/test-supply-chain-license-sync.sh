#!/usr/bin/env bash
set -euo pipefail

# Assert about.toml's `accepted` license set stays in sync with deny.toml's
# license policy -- the authoritative source. cargo-about (see
# ci/scripts/supply-chain-artifacts.sh) FAILS release-time bundle generation on
# any dependency whose license is not in `accepted`. So if a maintainer widens
# deny.toml's allow-list for a new dependency but forgets about.toml, the next
# release's supply-chain job breaks. This harness catches that drift pre-merge
# (and in the release preflight, which also runs shell-tests) with no compile.
#
# Invariant:  deny.allow  ⊆  about.accepted  ⊆  deny.allow ∪ deny.exceptions
# where deny.exceptions is the set of licenses granted per-crate via
# deny.toml's [[licenses.exceptions]] (e.g. MPL-2.0 for option-ext). Those are
# scoped in deny.toml but must be accepted globally in about.toml so the bundle
# can render that crate's text. The exception set is PARSED from deny.toml (not
# hard-coded), so dropping the exception there without dropping it from about.toml
# is caught rather than silently masked.

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

# Print the quoted SPDX ids inside `<key> = [ ... ]`, optionally scoped to a
# `[<section>]` TOML header, skipping comments. Output is sorted + unique.
extract() {
  local file="$1" key="$2" section="${3:-}"
  awk -v key="$key" -v section="$section" '
    section == "" { insec = 1 }
    section != "" && $0 ~ "^\\[" section "\\]" { insec = 1; next }
    section != "" && /^\[/ { insec = 0 }
    { line = $0; sub(/#.*/, "", line) }                    # strip comments
    insec && line ~ "^[[:space:]]*" key "[[:space:]]*=[[:space:]]*\\[" { infield = 1 }
    infield {
      s = line
      while (match(s, /"[^"]*"/)) {
        print substr(s, RSTART + 1, RLENGTH - 2)
        s = substr(s, RSTART + RLENGTH)
      }
      if (index(line, "]")) infield = 0
    }
  ' "$file" | sort -u
}

# Print every license id granted by a deny.toml [[licenses.exceptions]] block
# (the `allow = [...]` inside each array-of-tables entry), sorted + unique.
extract_exceptions() {
  awk '
    /^\[\[licenses\.exceptions\]\]/ { inexc = 1; next }
    inexc && /^\[/ { inexc = 0 }
    { line = $0; sub(/#.*/, "", line) }
    inexc && line ~ "^[[:space:]]*allow[[:space:]]*=[[:space:]]*\\[" { infield = 1 }
    inexc && infield {
      s = line
      while (match(s, /"[^"]*"/)) {
        print substr(s, RSTART + 1, RLENGTH - 2)
        s = substr(s, RSTART + RLENGTH)
      }
      if (index(line, "]")) infield = 0
    }
  ' deny.toml | sort -u
}

DENY_ALLOW="$(extract deny.toml allow licenses)"
DENY_EXCEPTIONS="$(extract_exceptions)"
ABOUT_ACCEPTED="$(extract about.toml accepted)"
# Everything about.toml is allowed to accept: the global allow-list plus the
# per-crate exceptions deny.toml grants.
ABOUT_PERMITTED="$(printf '%s\n%s\n' "$DENY_ALLOW" "$DENY_EXCEPTIONS" | sort -u | sed '/^$/d')"

if [ -z "$DENY_ALLOW" ]; then
  echo "[license-sync] FAIL: parsed no licenses from deny.toml [licenses].allow" >&2
  exit 1
fi
if [ -z "$ABOUT_ACCEPTED" ]; then
  echo "[license-sync] FAIL: parsed no licenses from about.toml accepted" >&2
  exit 1
fi

MISSING="$(comm -23 <(printf '%s\n' "$DENY_ALLOW") <(printf '%s\n' "$ABOUT_ACCEPTED"))"
EXTRA="$(comm -13 <(printf '%s\n' "$ABOUT_PERMITTED") <(printf '%s\n' "$ABOUT_ACCEPTED"))"

rc=0
if [ -n "$MISSING" ]; then
  echo "[license-sync] FAIL: deny.toml allows licenses about.toml does not accept" >&2
  echo "  (release bundle generation would fail on a dependency using one of these):" >&2
  printf '%s\n' "$MISSING" | sed 's/^/    - /' >&2
  echo "  -> add them to about.toml's accepted list." >&2
  rc=1
fi
if [ -n "$EXTRA" ]; then
  echo "[license-sync] FAIL: about.toml accepts licenses deny.toml neither allows nor" >&2
  echo "  grants as a per-crate exception (policy drift):" >&2
  printf '%s\n' "$EXTRA" | sed 's/^/    - /' >&2
  echo "  -> remove them from about.toml, or widen deny.toml if genuinely permitted." >&2
  rc=1
fi

if [ "$rc" -eq 0 ]; then
  n="$(printf '%s\n' "$DENY_ALLOW" | grep -c .)"
  e="$(printf '%s\n' "$DENY_EXCEPTIONS" | grep -c . || true)"
  echo "[license-sync] OK -- about.toml accepts all ${n} deny-allowed licenses + ${e} per-crate exception(s)"
fi
exit "$rc"
