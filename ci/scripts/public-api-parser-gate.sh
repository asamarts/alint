#!/usr/bin/env bash
# The public API of alint-core, alint-dsl and alint-testkit names no YAML parser
# crate (ADR-0038): a parser type in a public signature would make a change of
# parser a breaking change for library users. alint_core::yaml holds alint's own
# YAML types, which wrap the parser's privately.
#
# The gate documents the three crates with their dependencies, so rustdoc links
# every external type to its crate's pages, and fails on any page of the three
# crates that links to a parser crate. It documents a copy of the sources with
# `#[doc(hidden)]` removed, so a hidden public item is checked as well, including
# one that names a parser type imported with `use`.
#
# Usage:
#   public-api-parser-gate.sh             build the documentation, then scan it
#   public-api-parser-gate.sh --scan DIR  scan an existing documentation directory
#
# The build keeps its target directory under target/public-api-gate between runs.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PARSERS='ayaml|saphyr|saphyr_parser|yaml_serde|yaml_rust2|serde_yaml|libyaml_safer|unsafe_libyaml'
CRATES=(alint_core alint_dsl alint_testkit)

scan() {
  local doc=$1 crate page found=0
  for crate in "${CRATES[@]}"; do
    if [[ ! -f "$doc/$crate/index.html" ]]; then
      echo "public-api-parser-gate: no documentation for $crate under $doc" >&2
      return 2
    fi
    while IFS= read -r -d '' page; do
      local links
      links=$(grep -oE "href=\"[^\"]*/($PARSERS)/[^\"]*\"" "$page" | sort -u || true)
      if [[ -n "$links" ]]; then
        found=1
        echo "public-api-parser-gate: ${page#"$doc"/} names a parser crate:" >&2
        sed 's/^/  /' <<<"$links" >&2
      fi
    done < <(find "$doc/$crate" -name '*.html' -print0)
  done
  if ((found)); then
    echo "public-api-parser-gate: a public item names a YAML parser crate; use the" \
      "alint_core::yaml types instead (ADR-0038)" >&2
    return 1
  fi
  echo "public-api-parser-gate: no public item of ${CRATES[*]} names a YAML parser crate"
}

if [[ ${1-} == --scan ]]; then
  [[ $# -eq 2 ]] || { echo "usage: $0 [--scan DIR]" >&2; exit 2; }
  scan "$2"
  exit
fi
[[ $# -eq 0 ]] || { echo "usage: $0 [--scan DIR]" >&2; exit 2; }

work="$REPO_ROOT/target/public-api-gate"
copy="$work/source"
rm -rf "$copy"
mkdir -p "$copy"
trap 'rm -rf "$copy"' EXIT
# The tracked and new files, as they are in the working tree.
(
  cd "$REPO_ROOT"
  git ls-files -z --cached --others --exclude-standard |
    while IFS= read -r -d '' file; do
      [[ -f $file ]] && printf '%s\0' "$file"
    done |
    tar --null -T - -cf -
) | tar -xf - -C "$copy"
for crate in alint-core alint-dsl alint-testkit; do
  find "$copy/crates/$crate/src" -name '*.rs' -exec sed -i 's/#\[doc(hidden)\]//g' {} +
done
if grep -rq 'doc(hidden' "$copy/crates/alint-core/src" "$copy/crates/alint-dsl/src" \
  "$copy/crates/alint-testkit/src"; then
  echo "public-api-parser-gate: a doc(hidden) spelling other than #[doc(hidden)] is left:" >&2
  grep -rn 'doc(hidden' "$copy/crates/alint-core/src" "$copy/crates/alint-dsl/src" \
    "$copy/crates/alint-testkit/src" >&2
  exit 2
fi

echo "==> Documenting alint-core, alint-dsl and alint-testkit with their dependencies"
rm -rf "$work/target/doc/alint_core" "$work/target/doc/alint_dsl" "$work/target/doc/alint_testkit"
cargo doc -q --manifest-path "$copy/Cargo.toml" --target-dir "$work/target" \
  -p alint-core -p alint-dsl -p alint-testkit
scan "$work/target/doc"
