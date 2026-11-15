#!/usr/bin/env bash
# The public API of alint-core, alint-dsl and alint-testkit names no YAML parser
# crate (ADR-0038): a parser type in a public signature would make a change of
# parser a breaking change for library users. alint_core::yaml holds alint's own
# YAML types, which wrap the parser's privately.
#
# The gate documents the three crates, with all their features and with their
# dependencies, so rustdoc links every external type to its crate's pages, and
# fails on any page of the three crates that links to a parser crate: one of
# PARSERS, or any documented crate whose name contains `yaml` or `saphyr`. It
# documents a copy of the sources with `#[doc(hidden)]` removed, so a hidden
# public item is checked as well, including one that names a parser type
# imported with `use`.
#
# rustdoc cannot see everything, so the copy is checked first, and the gate
# fails on what it would miss: a `doc(hidden)` that is left (in a `cfg_attr`),
# a `cfg` or `cfg_attr` predicate on `doc` (`cfg(not(doc))` hides an item from
# rustdoc), and a `#[macro_export]` macro whose body names a parser crate
# (rustdoc shows an exported macro's matchers, not what it expands to).
#
# Usage:
#   public-api-parser-gate.sh                    build the documentation, then scan it
#   public-api-parser-gate.sh --scan DIR         scan an existing documentation directory
#   public-api-parser-gate.sh --check-sources DIR
#                                                strip and check a source copy in DIR
#                                                (crates/<crate>/src), in place
#   public-api-parser-gate.sh --parsers          print PARSERS
#
# The build keeps its target directory under target/public-api-gate between runs.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PARSERS='ayaml|saphyr|saphyr_parser|yaml_serde|yaml_rust|yaml_rust2|serde_yaml|serde_yaml_ng|serde_yml|serde_norway|serde_saphyr|marked_yaml|libyaml_rs|libyaml_safer|unsafe_libyaml|noyalib'
CRATES=(alint_core alint_dsl alint_testkit)
SOURCES=(crates/alint-core/src crates/alint-dsl/src crates/alint-testkit/src)

# PARSERS, and every crate documented under $1 whose name contains `yaml` or
# `saphyr`, other than alint's own.
parser_pattern() {
  local doc=$1 dir name pattern=$PARSERS
  for dir in "$doc"/*/; do
    name=$(basename "$dir")
    if [[ $name =~ yaml|saphyr && $name != alint_* ]]; then
      pattern+="|$name"
    fi
  done
  printf '%s' "$pattern"
}

scan() {
  local doc=$1 crate page found=0 parsers
  for crate in "${CRATES[@]}"; do
    if [[ ! -f "$doc/$crate/index.html" ]]; then
      echo "public-api-parser-gate: no documentation for $crate under $doc" >&2
      return 2
    fi
  done
  parsers=$(parser_pattern "$doc")
  for crate in "${CRATES[@]}"; do
    while IFS= read -r -d '' page; do
      local links
      links=$(grep -oE "href=\"[^\"]*/($parsers)/[^\"]*\"" "$page" | sort -u || true)
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

# Remove `#[doc(hidden)]` and `#![doc(hidden)]`, in any spacing and over any
# lines, from the sources under $1, then fail on what rustdoc would still miss.
check_sources() {
  local root=$1 dir files=()
  for dir in "${SOURCES[@]}"; do
    if [[ ! -d "$root/$dir" ]]; then
      echo "public-api-parser-gate: no sources at $root/$dir" >&2
      return 2
    fi
    while IFS= read -r -d '' file; do files+=("$file"); done \
      < <(find "$root/$dir" -name '*.rs' -print0)
  done
  ((${#files[@]})) || return 0
  perl -0pi -e 's/#!?\s*\[\s*doc\s*\(\s*hidden\s*\)\s*\]//g' "${files[@]}"
  PARSERS="$PARSERS" perl -0ne '
    BEGIN { $status = 0 }
    my $text = $_;
    # Comments may mention anything.
    $text =~ s{/\*.*?\*/}{}gs;
    $text =~ s{//[^\n]*}{}g;
    my $group = qr/(\((?:[^()]++|(?-1))*\))/;
    my $line = sub { 1 + (substr($text, 0, $_[0]) =~ tr/\n//) };
    while ($text =~ /doc\s*\(\s*hidden/g) {
      printf STDERR "public-api-parser-gate: %s:%d: a doc(hidden) is left\n",
        $ARGV, $line->($-[0]);
      $status |= 2;
    }
    while ($text =~ /\b(cfg|cfg_attr)\s*$group/g) {
      my ($kind, $args, $at) = ($1, substr($2, 1, -1), $-[0]);
      my $predicate = $args;
      if ($kind eq "cfg_attr") {
        my ($depth, $end) = (0, length $args);
        for my $i (0 .. length($args) - 1) {
          my $c = substr($args, $i, 1);
          $depth++ if $c eq "(";
          $depth-- if $c eq ")";
          if ($c eq "," && $depth == 0) { $end = $i; last }
        }
        $predicate = substr($args, 0, $end);
      }
      if ($predicate =~ /\bdoc\b/) {
        printf STDERR "public-api-parser-gate: %s:%d: a %s predicate on `doc` hides " .
          "code from rustdoc\n", $ARGV, $line->($at), $kind;
        $status |= 2;
      }
    }
    # An exported macro expands in another crate; the scan covers the items a
    # private one expands to.
    my $attribute = qr/#\s*\[(?:[^\[\]]++|\[(?:[^\[\]]++)*\])*\]\s*/;
    while ($text =~ /#\s*\[\s*macro_export\b[^\]]*\]\s*(?:$attribute)*macro_rules!\s*\w+\s*(\{(?:[^{}]++|(?-1))*\}|$group)/g) {
      my ($body, $at) = ($1, $-[0]);
      if ($body =~ /\b(?:$ENV{PARSERS})\s*::/) {
        printf STDERR "public-api-parser-gate: %s:%d: a macro names a parser crate\n",
          $ARGV, $line->($at);
        $status |= 1;
      }
    }
    END { exit $status }
  ' "${files[@]}"
}

case ${1-} in
  --scan)
    [[ $# -eq 2 ]] || { echo "usage: $0 [--scan DIR | --check-sources DIR | --parsers]" >&2; exit 2; }
    scan "$2"
    exit
    ;;
  --check-sources)
    [[ $# -eq 2 ]] || { echo "usage: $0 [--scan DIR | --check-sources DIR | --parsers]" >&2; exit 2; }
    check_sources "$2"
    exit
    ;;
  --parsers)
    [[ $# -eq 1 ]] || { echo "usage: $0 [--scan DIR | --check-sources DIR | --parsers]" >&2; exit 2; }
    tr '|' '\n' <<<"$PARSERS"
    exit
    ;;
esac
[[ $# -eq 0 ]] || { echo "usage: $0 [--scan DIR | --check-sources DIR | --parsers]" >&2; exit 2; }

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
check_sources "$copy"

echo "==> Documenting alint-core, alint-dsl and alint-testkit with their dependencies"
rm -rf "$work/target/doc/alint_core" "$work/target/doc/alint_dsl" "$work/target/doc/alint_testkit"
cargo doc -q --manifest-path "$copy/Cargo.toml" --target-dir "$work/target" --all-features \
  -p alint-core -p alint-dsl -p alint-testkit
scan "$work/target/doc"
