#!/usr/bin/env bash
set -euo pipefail

# Drift gate: keep docs/development/release-credentials.md's secrets inventory in
# sync with the secrets the workflows actually use. Two assertions:
#
#   1. every `${{ secrets.X }}` referenced by a workflow has an inventory row --
#      a new secret cannot ship undocumented (how to obtain + rotate it, whether
#      it expires). GITHUB_TOKEN is built-in (never provisioned), so it is exempt.
#
#   2. no secret the inventory marks *deleted* or *retired* is still wired into a
#      workflow -- the inverse of the NPM_TOKEN drift (PR #196): once a secret is
#      retired in the doc, it must be gone from the workflows.
#
# A documented-but-unused ACTIVE secret is allowed on purpose (e.g. the crates.io
# CARGO_REGISTRY_TOKEN token *fallback* the OIDC path replaces), so there is no
# orphan assertion -- only (1) used-implies-documented and (2) retired-implies-unwired.

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"
# Paths are overridable so ci/scripts/test-check-secrets-inventory.sh can aim the
# gate at fixtures instead of the live repo (absolute paths survive the cd above).
DOC="${SECRETS_DOC:-docs/development/release-credentials.md}"
WF_DIR="${SECRETS_WORKFLOWS_DIR:-.github/workflows}"

if [ ! -f "$DOC" ]; then
  echo "[secrets-inventory] FAIL: ${DOC} not found" >&2
  exit 1
fi

# (1) Secrets referenced by a real workflow EXPRESSION (a `${{ ... secrets.NAME`
# on a non-comment line), in either `secrets.NAME` or `secrets['NAME']` form -- not
# a `secrets.X` placeholder in a full-line comment. The first grep finds candidate
# lines; the `-o` grep extracts both syntaxes precisely; sed strips to the name.
mapfile -t USED < <(
  grep -rhnE '\$\{\{[^}]*secrets(\.|\[)' "$WF_DIR" 2>/dev/null \
    | grep -vE '^[0-9]+:[[:space:]]*#' \
    | grep -oE "secrets\.[A-Za-z_][A-Za-z0-9_]*|secrets\[['\"][A-Za-z_][A-Za-z0-9_]*['\"]\]" \
    | sed -E "s/^secrets\.//; s/^secrets\[['\"]//; s/['\"]\]$//" \
    | grep -vx 'GITHUB_TOKEN' \
    | sort -u
)

# Parse the "## Inventory" markdown table. A row's secret name(s) are the
# `code`-spanned UPPER_SNAKE tokens in the FIRST column (one row may list several,
# e.g. the JetBrains cert/key/password). A row is RETIRED if its FIRST column
# (where the `*(deleted YYYY-MM-DD)*` marker lives) says deleted/retired/removed/
# revoked -- scoped to col1 so descriptive prose in a later column (e.g. "replaces
# the retired PAT flow") can't misflag an ACTIVE secret. Init the arrays empty so
# `${#RETIRED[@]}` is safe under `set -u` when nothing is retired.
inv_table="$(awk '/^## Inventory/{f=1;next} f && /^## /{f=0} f' "$DOC")"
declare -A DOCUMENTED=() RETIRED=()
while IFS= read -r row; do
  [[ "$row" == \|* ]] || continue                     # markdown table rows only
  col1="${row#|}"; col1="${col1%%|*}"                 # first data column
  # shellcheck disable=SC2016  # the backticks are literal grep-pattern chars, not command substitution
  names="$(printf '%s' "$col1" | grep -oE '`[A-Z][A-Z0-9_]*`' | tr -d '`' || true)"
  [[ -n "$names" ]] || continue                       # header / separator rows
  is_retired=0
  printf '%s' "$col1" | grep -qiE 'deleted|retired|removed|revoked' && is_retired=1
  while IFS= read -r name; do
    [[ -n "$name" ]] || continue
    DOCUMENTED["$name"]=1
    [[ "$is_retired" -eq 1 ]] && RETIRED["$name"]=1
  done <<< "$names"
done <<< "$inv_table"

if [ "${#DOCUMENTED[@]}" -eq 0 ]; then
  echo "[secrets-inventory] FAIL: parsed no secrets from ${DOC} '## Inventory' table" >&2
  exit 1
fi

rc=0

# Assertion 1: every used secret is documented.
for s in "${USED[@]:-}"; do
  [[ -n "$s" ]] || continue
  if [[ -z "${DOCUMENTED[$s]:-}" ]]; then
    echo "[secrets-inventory] FAIL: a workflow references secrets.${s}, but it has no row in ${DOC} '## Inventory' (add how to obtain + rotate it)." >&2
    rc=1
  fi
done

# Assertion 2: no retired/deleted secret is still wired into a workflow.
# `RETIRED=()` above makes "${!RETIRED[@]}" safe under `set -u` when empty.
for s in "${!RETIRED[@]}"; do
  if printf '%s\n' "${USED[@]:-}" | grep -Fqx "$s"; then
    echo "[secrets-inventory] FAIL: ${s} is marked deleted/retired in ${DOC}, but a workflow still references secrets.${s} (remove the wiring or un-retire the row)." >&2
    rc=1
  fi
done

if [ "$rc" -eq 0 ]; then
  echo "[secrets-inventory] OK -- ${#USED[@]} workflow secret(s) all documented; ${#RETIRED[@]} retired secret(s) unwired."
fi
exit "$rc"
