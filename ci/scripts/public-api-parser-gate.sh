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
# rustdoc cannot see everything, so the copy's sources (every .rs file under
# each crate's directory) are checked first, and the gate fails on what rustdoc
# would miss:
#   - a `hidden` left in a `doc(...)` list (`#[doc(alias = "x", hidden)]`, or
#     one in a `cfg_attr`), and an attribute a macro builds (`#[doc($a)]`,
#     `#[$a]`, a `$` in a `cfg`), which can hide an item;
#   - a `cfg` or `cfg_attr` predicate on `doc`, at any depth (`cfg(not(doc))`
#     hides an item from rustdoc);
#   - a `cfg` or `cfg_attr` on a public item or an `impl` that names a platform
#     or profile (windows, unix, target_*, debug_assertions) or a `not(feature)`,
#     since only the host target is documented, in a debug build, with every
#     feature: such an item is not in the documentation (PLATFORM_ALLOWLIST);
#   - an `include!` or `#[path]` that reaches a file outside the crate's
#     directory, or one this check does not read;
#   - a crate root without `#![deny(unnameable_types)]`, or an `allow` of it: a
#     public type in a private module never gets a page, nor do its fields;
#   - a `#[macro_export]` macro whose body names a parser crate (rustdoc shows an
#     exported macro's matchers, not what it expands to).
# Comments, string literals and character literals are removed first, as Rust
# reads them, so neither can hide code from these checks or fake it.
#
# The gate checks itself on every run. Before the copy is checked, a canary is
# added to each crate: a hidden public type alias of an ayaml type, behind a
# feature of its own. The scan must find each canary, linked to ayaml's pages,
# and the parser's documentation must be there, or the run is an error (exit
# 2): a build that dropped `--all-features`, the removal of `doc(hidden)`, the
# dependencies' documentation, or that documented anything but the copy, would
# otherwise pass while seeing nothing.
#
# Usage:
#   public-api-parser-gate.sh                    build the documentation, then scan it
#   public-api-parser-gate.sh --scan DIR         scan a documentation directory the gate
#                                                built (with its canaries)
#   public-api-parser-gate.sh --check-sources DIR
#                                                strip and check a source copy in DIR
#                                                (crates/<crate>), in place
#   public-api-parser-gate.sh --parsers          print PARSERS
# Exit status: 0 clean, 1 a public item names a parser crate, 2 the gate cannot
# tell (3 when both).
#
# The build keeps its target directory under target/public-api-gate between runs.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PARSERS='ayaml|saphyr|saphyr_parser|yaml_serde|yaml_rust|yaml_rust2|serde_yaml|serde_yaml_ng|serde_yml|serde_norway|serde_saphyr|marked_yaml|libyaml_rs|libyaml_safer|unsafe_libyaml|noyalib'
CRATES=(alint_core alint_dsl alint_testkit)
SOURCES=(crates/alint-core crates/alint-dsl crates/alint-testkit)
CANARY=PublicApiGateCanary
CANARY_FEATURE=public-api-gate-canary
# Public items under a platform or profile cfg that the gate accepts, as
# "<file>|<item>" (the item as the check prints it), each with the reason it
# cannot name a parser type. None today.
PLATFORM_ALLOWLIST=()

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
  local doc=$1 crate page found=0 broken=0 parsers canary
  for crate in "${CRATES[@]}"; do
    if [[ ! -f "$doc/$crate/index.html" ]]; then
      echo "public-api-parser-gate: no documentation for $crate under $doc" >&2
      return 2
    fi
  done
  # The canaries link to ayaml's pages, which a build without the dependencies'
  # documentation does not have.
  if [[ ! -f "$doc/ayaml/index.html" ]]; then
    echo "public-api-parser-gate: no documentation for ayaml under $doc; the dependencies" \
      "were not documented" >&2
    broken=1
  fi
  parsers=$(parser_pattern "$doc")
  for crate in "${CRATES[@]}"; do
    canary="$doc/$crate/type.$CANARY.html"
    if ! grep -qE "href=\"[^\"]*/ayaml/[^\"]*\"" "$canary" 2>/dev/null; then
      echo "public-api-parser-gate: ${canary#"$doc"/} does not link to ayaml; the build did" \
        "not document the stripped copy with every feature and its dependencies" >&2
      broken=1
    fi
    while IFS= read -r -d '' page; do
      [[ $page == "$canary" ]] && continue
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
  fi
  if ((found || broken)); then
    return $((found | broken << 1))
  fi
  echo "public-api-parser-gate: no public item of ${CRATES[*]} names a YAML parser crate"
}

# Add the canary to each crate of the copy under $1: a hidden public alias of an
# ayaml type, behind a feature of its own.
add_canaries() {
  local root=$1 dir
  for dir in "${SOURCES[@]}"; do
    CANARY_FEATURE=$CANARY_FEATURE perl -0pi -e '
      s/^\[features\][^\S\n]*\n/[features]\n$ENV{CANARY_FEATURE} = []\n/m
        or $_ .= "\n[features]\n$ENV{CANARY_FEATURE} = []\n";
    ' "$root/$dir/Cargo.toml"
    printf '\n#[cfg(feature = "%s")]\n#[doc(hidden)]\npub type %s = ayaml::Number;\n' \
      "$CANARY_FEATURE" "$CANARY" >>"$root/$dir/src/lib.rs"
  done
}

# The source checks, as a Perl program over one file at a time (-0). CRATE_DIRS
# lists the crate directories, ROOT the copy's root, and ALLOWED the allowlist.
read -r -d '' CHECK_SOURCES <<'PERL' || true
BEGIN {
  $status = 0;
  @dirs = split /\n/, $ENV{CRATE_DIRS};
  %allowed = map { $_ => 1 } grep { length } split /\n/, $ENV{ALLOWED};
  %denies = ();
}
my $file = $ARGV;
my ($dir) = grep { index($file, "$_/") == 0 } @dirs;
my $name = substr($file, length($ENV{ROOT}) + 1);
my @literals;
my $text = $_;
my $lines = sub { "\n" x ($_[0] =~ tr/\n//) };
my $char = qr/(?:[^'\\\n\x80-\xff]|[\xc0-\xf7][\x80-\xbf]{1,3})/;
# Comments become a space and a string literal "N", its index in @literals, in
# one pass from the left, so a quote in a comment or a comment in a string is
# read as Rust reads it. Each keeps its newlines, so line numbers hold.
$text =~ s{
    (?<block>/\*(?:[^/*]++|/(?!\*)|\*(?!/)|(?&block))*\*/)
  | (?<line>//[^\n]*)
  | (?<![\w])[bc]?r(?<hashes>\#*)"(?<raw>.*?)"\k<hashes>
  | (?<![\w])[bc]?"(?<string>(?:[^"\\]++|\\.)*+)"
  | (?<![\w])b?'(?:$char|\\(?:u\{[0-9a-fA-F_]*\}|x[0-9a-fA-F]{2}|.))'
}{
  my $whole = $&;
  if (defined $+{block} || defined $+{line}) {
    ' ' . $lines->($whole);
  } elsif (defined $+{raw} || defined $+{string}) {
    push @literals, $+{raw} // $+{string};
    '"' . $#literals . '"' . $lines->($whole);
  } else {
    "' '";
  }
}gsxe;

my $group = qr/(\((?:[^()]++|(?-1))*\))/;
my $brackets = qr/(\[(?:[^\[\]]++|(?-1))*\])/;
my $braces = qr/(\{(?:[^{}]++|(?-1))*\})/;
my $line = sub { 1 + (substr($text, 0, $_[0]) =~ tr/\n//) };
my $report = sub {
  my ($at, $code, $message) = @_;
  printf STDERR "public-api-parser-gate: %s:%d: %s\n", $name, $line->($at), $message;
  $status |= $code;
};
# The arguments of a cfg_attr up to its first top-level comma: its predicate.
my $predicate = sub {
  my ($args, $depth) = ($_[0], 0);
  for my $i (0 .. length($args) - 1) {
    my $c = substr($args, $i, 1);
    $depth++ if $c eq "(";
    $depth-- if $c eq ")";
    return substr($args, 0, $i) if $c eq "," && $depth == 0;
  }
  return $args;
};
# Whether a path literal resolves inside the crate directory, to a .rs file.
my $inside = sub {
  my ($path) = @_;
  return 0 unless defined $dir && defined $path;
  return 0 if $path =~ m{^/} || $path =~ /\\/ || $path !~ /\.rs\z/;
  my ($from) = $file =~ m{^(.*)/};
  my @parts = grep { length && $_ ne "." } split m{/}, substr($from, length $dir);
  for my $part (grep { length && $_ ne "." } split m{/}, $path) {
    if ($part eq "..") { return 0 unless @parts; pop @parts } else { push @parts, $part }
  }
  return 1;
};

while ($text =~ /\bdoc\s*(?=$group)/g) {
  my ($args, $at) = ($1, $-[0]);
  $report->($at, 2, "a doc(hidden) is left") if $args =~ /\bhidden\b/;
  $report->($at, 2, "a macro builds a doc attribute, which can hide an item") if $args =~ /\$/;
}
while ($text =~ /#\s*!?\s*\[\s*\$/g) {
  $report->($-[0], 2, "a macro builds an attribute, which can hide an item");
}
while ($text =~ /\b(cfg|cfg_attr)\s*(?=$group)/g) {
  my ($kind, $args, $at) = ($1, substr($2, 1, -1), $-[0]);
  my $condition = $kind eq "cfg_attr" ? $predicate->($args) : $args;
  if ($condition =~ /\bdoc\b/) {
    $report->($at, 2, "a $kind predicate on `doc` hides code from rustdoc");
  }
  $report->($at, 2, "a macro builds a $kind, which can hide an item") if $args =~ /\$/;
}

# A cfg or cfg_attr on a public item or an impl, whose conditions (a cfg_attr's
# predicate and the cfgs it applies) name a platform, a profile or a missing
# feature.
my $gated = sub {
  my ($conditions) = @_;
  return 1 if $conditions =~ /\b(?:windows|unix|target_\w+|debug_assertions)\b/;
  while ($conditions =~ /\bnot\s*$group/g) { return 1 if $1 =~ /\bfeature\b/ }
  return 0;
};
while ($text =~ /#\s*(!?)\s*\[\s*(cfg|cfg_attr)\s*$group\s*\]/g) {
  my ($inner, $kind, $args, $at, $end) = ($1, $2, substr($3, 1, -1), $-[0], $+[0]);
  my $conditions = $args;
  if ($kind eq "cfg_attr") {
    $conditions = $predicate->($args);
    while ($args =~ /\bcfg\s*$group/g) { $conditions .= " $1" }
  }
  next unless $gated->($conditions);
  my $rest = substr($text, $end);
  $rest =~ s/\A(?:\s*#\s*$brackets)*\s*//;
  my $item;
  if ($inner) {
    $item = "the module" if $rest =~ /\bpub\b(?!\s*\()/;
  } elsif ($rest =~ /\A(pub\b(?!\s*\()|(?:unsafe\s+)?impl\b)[^{;=(]*/) {
    ($item = $&) =~ s/\s+/ /g;
    $item =~ s/ \z//;
  }
  next unless defined $item;
  next if $allowed{"$name|$item"};
  $report->($at, 2, "`$item` exists only under a platform, profile or feature cfg, so it " .
    "is not documented; allowlist it in the gate with a reason if it cannot name a parser");
}

while ($text =~ /\binclude\s*!\s*/g) {
  my $at = $-[0];
  my ($path) = substr($text, $+[0]) =~ /\A[\(\[\{]\s*"(\d+)"\s*,?\s*[\)\]\}]/;
  next if defined $path && $inside->($literals[$path]);
  $report->($at, 2, "an include! of a file outside the crate directory, or of a computed path");
}
while ($text =~ /#\s*\[\s*path\s*=\s*"(\d+)"\s*\]|\bcfg_attr\s*(?=$group)/g) {
  my ($index, $at, $args) = ($1, $-[0], $2);
  my @paths = defined $index ? ($index) : (($args // "") =~ /\bpath\s*=\s*"(\d+)"/g);
  for my $path (@paths) {
    next if $inside->($literals[$path]);
    $report->($at, 2, "a #[path] to a file outside the crate directory");
  }
}

for my $level (qw(allow expect warn deny forbid)) {
  while ($text =~ /\b$level\s*(?=$group)/g) {
    my ($args, $at) = ($1, $-[0]);
    next unless $args =~ /\bunnameable_types\b/;
    if ($level =~ /^(?:allow|expect|warn)$/) {
      $report->($at, 2, "an `$level` of unnameable_types: a public type in a private module " .
        "is not documented");
    }
  }
}
if (defined $dir && $file eq "$dir/src/lib.rs"
    && $text =~ /#\s*!\s*\[\s*(?:deny|forbid)\s*\([^()]*\bunnameable_types\b[^()]*\)\s*\]/) {
  $denies{$dir} = 1;
}

# An exported macro expands in another crate; the scan covers the items a
# private one expands to.
while ($text =~ /#\s*\[\s*macro_export\b[^\]]*\]\s*(?:#\s*$brackets\s*)*macro_rules\s*!\s*\w+\s*/g) {
  my $at = $-[0];
  my ($body) = substr($text, $+[0]) =~ /\A($braces|$group|$brackets)/;
  if (defined $body && $body =~ /\b(?:$ENV{PARSERS})\s*::/) {
    $report->($at, 1, "a macro names a parser crate");
  }
}

END {
  for my $dir (@dirs) {
    next if $denies{$dir};
    printf STDERR "public-api-parser-gate: %s/src/lib.rs: no #![deny(unnameable_types)]; a " .
      "public type in a private module is not documented\n", substr($dir, length($ENV{ROOT}) + 1);
    $status |= 2;
  }
  exit $status;
}
PERL

# Remove `#[doc(hidden)]` and `#![doc(hidden)]`, in any spacing and over any
# lines, from the sources under $1, then fail on what rustdoc would still miss.
check_sources() {
  local root dir files=() dirs=() allowed
  root=$(cd "$1" 2>/dev/null && pwd) || {
    echo "public-api-parser-gate: no sources at $1" >&2
    return 2
  }
  for dir in "${SOURCES[@]}"; do
    if [[ ! -d "$root/$dir/src" ]]; then
      echo "public-api-parser-gate: no sources at $root/$dir/src" >&2
      return 2
    fi
    dirs+=("$root/$dir")
    while IFS= read -r -d '' file; do files+=("$file"); done \
      < <(find "$root/$dir" -name '*.rs' -print0)
  done
  perl -0pi -e 's/#!?\s*\[\s*doc\s*\(\s*hidden\s*\)\s*\]//g' "${files[@]}"
  allowed=$(printf '%s\n' "${PLATFORM_ALLOWLIST[@]}")
  PARSERS="$PARSERS" ROOT="$root" ALLOWED="$allowed" \
    CRATE_DIRS="$(printf '%s\n' "${dirs[@]}")" perl -0ne "$CHECK_SOURCES" "${files[@]}"
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
doc="$work/target/doc"
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
add_canaries "$copy"
check_sources "$copy"

echo "==> Documenting alint-core, alint-dsl and alint-testkit with their dependencies"
# A fresh copy of the pages the scan reads: the three crates' and the parsers'.
# cargo writes again what is missing.
if [[ -d $doc ]]; then
  parsers=$(parser_pattern "$doc")
  for dir in "$doc"/*/; do
    name=$(basename "$dir")
    if [[ $name =~ ^($parsers)$ || " ${CRATES[*]} " == *" $name "* ]]; then
      rm -rf "$dir"
    fi
  done
fi
cargo doc -q --manifest-path "$copy/Cargo.toml" --target-dir "$work/target" --all-features \
  -p alint-core -p alint-dsl -p alint-testkit
scan "$doc"
