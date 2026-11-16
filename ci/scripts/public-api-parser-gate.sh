#!/usr/bin/env bash
# The public API of alint-core, alint-dsl and alint-testkit names no YAML parser
# crate (ADR-0038): a parser type in a public signature would make a change of
# parser a breaking change for library users. alint_core::yaml holds alint's own
# YAML types, which wrap the parser's privately.
#
# The gate documents the three crates, with all their features and with their
# dependencies, so rustdoc links every external type to its crate's pages, and
# fails on any page of the three crates that links to a parser crate's pages,
# relatively or on docs.rs (prose may link elsewhere, such as to an issue): one
# of PARSERS, or any documented crate whose name contains `yaml` or `saphyr`. It
# documents a copy of the sources with `#[doc(hidden)]` removed, so a hidden
# public item is checked as well, including one that names a parser type
# imported with `use`.
#
# rustdoc cannot see everything, so the copy's sources (every .rs file under
# each crate's directory) are checked first, and the gate fails on what rustdoc
# would miss:
#   - a `hidden` left in a `doc(...)` list (`#[doc(alias = "x", hidden)]`, or
#     one in a `cfg_attr`), and an attribute a macro builds: any `$` in a
#     `#[...]` or `#![...]`, or in brackets after a `$` variable, which can hide
#     an item (`#[doc $($t)*]`, `#[path = $p]`, `#[allow($l)]`);
#   - a `cfg` or `cfg_attr` predicate on `doc`, at any depth (`cfg(not(doc))`
#     hides an item from rustdoc);
#   - any other `cfg` or `cfg_attr`, on an item, field, variant, trait item or
#     statement, unless its predicate holds in the documented build whenever it
#     holds in a build a user makes (`all` and `any` of `feature = ".."`,
#     `test`, `doctest` and `docsrs`), or PLATFORM_ALLOWLIST names it: only the
#     host target is documented, in a debug build, with every feature, so code
#     under `cfg(windows)`, `cfg(not(debug_assertions))` or
#     `cfg(panic = "abort")` is API rustdoc never sees. `cfg_select!` fails too.
#     Test-only code is skipped: an item under `cfg(test)` (or `doctest`, or an
#     `all` with one of them) and what it holds, a file only such code loads,
#     and, unless the library reaches outside `src/`, the files outside it;
#   - an `include!` or `#[path]` that reaches a file outside the crate's
#     directory, or one this check does not read; a `mod` a macro names; and,
#     outside test-only code, the word `include` in a `use` or in a macro's
#     input or body, where it can rename include! (`use core::include as inc;`);
#   - a crate root without `#![deny(unnameable_types)]`, or an `allow` of it: a
#     public type in a private module never gets a page, nor do its fields;
#   - a `#[macro_export]` macro (`r#` names too) whose body names a parser crate
#     anywhere, even as a bare word (`use ::ayaml as parser;`): rustdoc shows an
#     exported macro's matchers, not what it expands to.
# Comments, string literals and character literals are removed first, as Rust
# reads them, so neither can hide code from these checks or fake it.
#
# One shape stays outside the gate by design: `ref_cast`'s derived
# `RefCastCustom`, which alint_core::yaml uses to view the parser's values as
# its own, adds an `impl ref_cast::__private<N>::RefCastCustom<ayaml::...>` per
# type. That trait is `#[doc(hidden)]` in a `__private` module of ref_cast, so
# rustdoc never shows the impl, and by Rust convention such an item is outside
# semver: no user can name the trait, and a change of parser does not break one.
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
#                                                (crates/<crate>), in place; DIR in a
#                                                Git work tree is refused
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
# The cfgs the gate accepts although rustdoc does not see what they hide, as
# "<file>|<cfg>|<what it is on>", as the check prints them: a cfg in a
# function's body by the function (`in fn name`), one on a function by its name
# (`pub fn name`), one on a module's inner attribute by `the module`, and any
# other by what it is on, up to a `{`, `;`, `=` or `,` outside brackets. An
# entry accepts every cfg with that key, and one that matches no cfg in its file
# fails, so the list holds exactly the cfgs in the code. Each says why what it
# hides cannot name a parser type.
PLATFORM_ALLOWLIST=(
  # Statements in a function's body that call the platform's file-system or
  # process API (file modes, symlinks, process groups, a path's bytes) or keep
  # Windows' system variables: they declare no impl or other item, and name no
  # parser type.
  "crates/alint-core/src/engine/fix.rs|cfg(not(unix))|in fn stage_fixes"
  "crates/alint-core/src/engine/fix.rs|cfg(unix)|in fn stage_fixes"
  "crates/alint-core/src/git.rs|cfg(not(unix))|in fn path_from_git_chunk"
  "crates/alint-core/src/git.rs|cfg(unix)|in fn path_from_git_chunk"
  "crates/alint-core/src/process.rs|cfg(not(unix))|in fn kill_group"
  "crates/alint-core/src/process.rs|cfg(unix)|in fn kill_group"
  "crates/alint-core/src/process.rs|cfg(unix)|in fn run_bounded_in"
  "crates/alint-core/src/relation/cache.rs|cfg(not(windows))|in fn is_interested"
  "crates/alint-core/src/relation/cache.rs|cfg(windows)|in fn is_interested"
  "crates/alint-core/src/relation/cache.rs|cfg(windows)|in fn mark_interest"
  "crates/alint-core/src/scope.rs|cfg(not(unix))|in fn literal_matches"
  "crates/alint-core/src/scope.rs|cfg(unix)|in fn literal_matches"
  "crates/alint-core/src/trace.rs|cfg(not(windows))|in fn normalize_repository_path"
  "crates/alint-core/src/trace.rs|cfg(windows)|in fn normalize_repository_path"
  "crates/alint-testkit/src/policy_runner.rs|cfg(unix)|in fn copy_fixture_dir"
  "crates/alint-testkit/src/policy_runner.rs|cfg(windows)|in fn copy_fixture_dir"
  "crates/alint-testkit/src/policy_runner.rs|cfg(windows)|in fn granted_process_environment"
  "crates/alint-testkit/src/policy_runner.rs|cfg(windows)|in fn runner_git_environment"
  "crates/alint-testkit/src/runner_artifacts.rs|cfg(not(unix))|in fn collect"
  "crates/alint-testkit/src/runner_artifacts.rs|cfg(unix)|in fn collect"
  "crates/alint-testkit/src/treespec/materialize.rs|cfg(not(unix))|in fn create_symlink"
  "crates/alint-testkit/src/treespec/materialize.rs|cfg(unix)|in fn create_symlink"
  "crates/alint-testkit/src/treespec/materialize.rs|cfg(unix)|in fn write_map"
  "crates/alint-testkit/src/treespec/verify.rs|cfg(unix)|in fn verify_exec"
  # Private items with one definition per platform, the same signature on each,
    # and no parser type in it: a path's bytes, a symlink-loop check and a
  # fixture's setuid check.
  "crates/alint-core/src/baseline.rs|cfg(not(any(unix, windows)))|fn normalize_path"
  "crates/alint-core/src/baseline.rs|cfg(unix)|fn normalize_path"
  "crates/alint-core/src/baseline.rs|cfg(windows)|fn normalize_path"
  "crates/alint-core/src/walker.rs|cfg(not(unix))|fn is_eloop"
  "crates/alint-core/src/walker.rs|cfg(unix)|fn is_eloop"
  "crates/alint-testkit/src/confinement.rs|cfg(not(unix))|fn validate_regular_metadata"
  "crates/alint-testkit/src/confinement.rs|cfg(unix)|fn validate_regular_metadata"
  # Debug-build checks: private assertions on the rule registry and the fix
  # merge, the imports they use, and index-build timing traces (a private macro
  # with a no-op twin for release builds). None declares public API or names a
  # parser type.
  "crates/alint-core/src/engine/fix.rs|cfg(debug_assertions)|in fn fix"
  "crates/alint-core/src/registry.rs|cfg(debug_assertions)|fn assert_dispatch_and_content"
  "crates/alint-core/src/registry.rs|cfg(debug_assertions)|fn assert_instance_conformance"
  "crates/alint-core/src/registry.rs|cfg(debug_assertions)|in fn build"
  "crates/alint-core/src/registry.rs|cfg(debug_assertions)|use crate::kind_meta::"
  "crates/alint-core/src/registry.rs|cfg(debug_assertions)|use crate::rule::GitTrackedMode"
  "crates/alint-core/src/walker.rs|cfg(debug_assertions)|in fn all_path_set_arc"
  "crates/alint-core/src/walker.rs|cfg(debug_assertions)|in fn children_of"
  "crates/alint-core/src/walker.rs|cfg(debug_assertions)|in fn file_entry_indices_arc"
  "crates/alint-core/src/walker.rs|cfg(debug_assertions)|in fn file_path_set_arc"
  "crates/alint-core/src/walker.rs|cfg(debug_assertions)|macro_rules! trace_index_build"
  "crates/alint-core/src/walker.rs|cfg(not(debug_assertions))|macro_rules! trace_index_build"
  # Kani proofs, and the path model they share with the unit tests: private
  # modules that only the Kani verifier (`cfg(kani)`) and `cargo test` compile,
  # never a user's build.
  "crates/alint-core/src/located_fix.rs|cfg(kani)|mod kani_proofs"
  "crates/alint-core/src/pathsafe.rs|cfg(any(test, kani))|mod model"
  "crates/alint-core/src/pathsafe.rs|cfg(kani)|mod kani_proofs"
  "crates/alint-core/src/pathsafe.rs|cfg_attr(kani)|pub(super) enum Step"
  # The YAML parser facade (ADR-0041): ayaml, when alint-core's test-only
  # `yaml-reference` feature is off; the differential harness's stand-in, in a
  # build with that feature outside tests, for the defects its tests plant; and
  # lint expectations on the harness's private items, which such a build
  # compiles but uses only in tests. The first is a statement that names ayaml's
  # decode through alint's own types, the stand-in a private module whose one
  # function returns its argument, and a lint level hides no code.
  "crates/alint-core/src/yaml_backend.rs|cfg(not(feature = \"yaml-reference\"))|in fn parser"
  "crates/alint-core/src/yaml_differential.rs|cfg(not(test))|mod planted"
  "crates/alint-core/src/yaml_differential/allowlist.rs|cfg_attr(not(test))|Narrow"
  "crates/alint-core/src/yaml_differential/allowlist.rs|cfg_attr(not(test))|pub(crate) struct Deviation"
  "crates/alint-core/src/yaml_differential/allowlist.rs|cfg_attr(not(test))|pub(crate) struct Location"
)

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

# An extended regular expression for the rest of an href to a page of the crates
# $1 (a pattern): a link rustdoc makes to another crate's pages, relative, or
# one to docs.rs. Prose may link elsewhere, such as to an issue on GitHub.
link_to() {
  printf '((\\.\\./)+|https://docs\\.rs/([^"/]+/)*)(%s)/[^"]*"' "$1"
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
    if ! grep -qE "href=\"$(link_to ayaml)" "$canary" 2>/dev/null; then
      echo "public-api-parser-gate: ${canary#"$doc"/} does not link to ayaml; the build did" \
        "not document the stripped copy with every feature and its dependencies" >&2
      broken=1
    fi
    while IFS= read -r -d '' page; do
      [[ $page == "$canary" ]] && continue
      local links
      links=$(grep -oE "href=\"$(link_to "$parsers")" "$page" | sort -u || true)
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
# The checks that skip test-only code report at the end, once every file's
# `mod`, `#[path]` and `include!` are known.
read -r -d '' CHECK_SOURCES <<'PERL' || true
BEGIN {
  $status = 0;
  @dirs = split /\n/, $ENV{CRATE_DIRS};
  %allowed = map { $_ => 0 } grep { length } split /\n/, $ENV{ALLOWED};
  %denies = ();
  # Each file read, with its crate directory; what can load a file; and the
  # findings that do not count in test-only code.
  %crate_of = ();
  @loads = ();
  @deferred = ();
}
# The terms of a cfg list, split at its top-level commas.
sub cfg_terms {
  my ($list, $depth, $start, @terms) = ($_[0], 0, 0);
  while ($list =~ /[(),]/g) {
    if ($& eq "(") { $depth++ } elsif ($& eq ")") { $depth-- }
    elsif ($depth == 0) { push @terms, substr($list, $start, $-[0] - $start); $start = $+[0] }
  }
  return grep { length } map { s/\A\s+|\s+\z//gr } @terms, substr($list, $start);
}
# Whether a cfg predicate holds in the documented build whenever it holds in a
# build a user makes: `all` and `any` of `feature = ".."`, as every feature is
# on, and of `test`, `doctest` and `docsrs`, which no user's build sets.
sub cfg_documented {
  my ($p) = @_;
  return 1 if $p =~ /\A(?:test|doctest|docsrs|feature\s*=\s*"\d+")\z/;
  return 0 unless $p =~ /\A(?:all|any)\s*\((.*)\)\z/s;
  return !grep { !cfg_documented($_) } cfg_terms($1);
}
# Whether a cfg predicate holds in a test build only.
sub cfg_test_only {
  my ($p) = @_;
  return 1 if $p =~ /\A(?:test|doctest)\z/;
  return scalar grep { cfg_test_only($_) } cfg_terms($1) if $p =~ /\Aall\s*\((.*)\)\z/s;
  return 0 unless $p =~ /\Aany\s*\((.*)\)\z/s;
  my @terms = cfg_terms($1);
  return @terms && !grep { !cfg_test_only($_) } @terms;
}
my $file = $ARGV;
my ($dir) = grep { index($file, "$_/") == 0 } @dirs;
my $name = substr($file, length($ENV{ROOT}) + 1);
my $in_src = defined $dir && index($file, "$dir/src/") == 0;
$crate_of{$file} = $dir;
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
# A finding that test-only code does not count, reported at the end; a cfg's
# carries its allowlist key.
my $defer = sub {
  my ($at, $message, $key) = @_;
  push @deferred, [$file, $name, $line->($at), $message, $key];
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
# The file a path literal names, relative to the crate directory, or undef when
# it leaves the directory.
my $resolve = sub {
  my ($path) = @_;
  return undef unless defined $dir && defined $path;
  return undef if $path =~ m{^/} || $path =~ /\\/;
  my ($from) = $file =~ m{^(.*)/};
  my @parts = grep { length && $_ ne "." } split m{/}, substr($from, length $dir);
  for my $part (grep { length && $_ ne "." } split m{/}, $path) {
    if ($part eq "..") { return undef unless @parts; pop @parts } else { push @parts, $part }
  }
  return join "/", @parts;
};
# Whether a path literal resolves inside the crate directory, to a .rs file.
my $inside = sub { defined $_[0] && $_[0] =~ /\.rs\z/ && defined $resolve->($_[0]) };

# A copy of the text, to scan forward from a place while a loop matches $text.
my $copy = $text;
# From $_[0]: where what starts there ends (after its first `;` outside
# brackets, or after the block it opens, or where the block it is in ends), and
# where the block it opens starts.
my $scan = sub {
  my ($depth, $open) = (0);
  pos($copy) = $_[0];
  while ($copy =~ /[()\[\]{};]/g) {
    my ($c, $at, $after) = ($&, $-[0], $+[0]);
    if ($c eq ";") { return ($after, $open) if $depth == 0 }
    elsif ($c =~ /[(\[{]/) { $open //= $at if $c eq "{" && $depth == 0; $depth++ }
    elsif (--$depth < 0) { return ($at, $open) }
    elsif ($depth == 0 && $c eq "}" && defined $open) { return ($after, $open) }
  }
  return (length $copy, $open);
};
# From $_[0], where the block it is in ends.
my $enclosing = sub {
  my $depth = 0;
  pos($copy) = $_[0];
  while ($copy =~ /[()\[\]{}]/g) {
    my ($c, $at) = ($&, $-[0]);
    return $at if ($depth += $c =~ /[(\[{]/ ? 1 : -1) < 0;
  }
  return length $copy;
};

# Every attribute, and a `$` in one, or in brackets after a `$` variable that
# can be a `#`: a macro builds the attribute, and can build one that hides.
my @attributes;
while ($text =~ /#\s*(!?)\s*$brackets/g) {
  my ($at, $end, $inner, $args) = ($-[0], $+[0], length $1, $2);
  push @attributes, [$at, $end, $inner];
  $report->($at, 2, "a macro builds an attribute, which can hide an item") if $args =~ /\$/;
}
my $attribute = sub {
  my ($at) = @_;
  my ($found) = grep { $at > $_->[0] && $at < $_->[1] } @attributes;
  return $found;
};
while ($text =~ /\$\w+\s*!?\s*$brackets/g) {
  my ($at, $args) = ($-[0], $1);
  next if $attribute->($at) || $args !~ /\$/;
  $report->($at, 2, "a macro builds an attribute, which can hide an item");
}

# Test-only code: an item under a test-only cfg, from its first attribute to
# its end, and the rest of the block a test-only inner cfg is in.
my @regions;
while ($text =~ /(?:#\s*$brackets\s*)+/g) {
  my ($start, $end, $run) = ($-[0], $+[0], $&);
  next unless grep { cfg_test_only(substr($_, 1, -1) =~ s/\A\s+|\s+\z//gr) }
    $run =~ /#\s*\[\s*cfg\s*$group\s*\]/g;
  pos($copy) = $end;
  next unless $copy =~ /\G(?:pub\b(?:\s*$group)?\s*)?
    (?:(?:const|async|unsafe|safe|default|extern(?:\s*"\d+")?)\s+)*(?:mod|fn|impl|trait)\b/gx;
  push @regions, [$start, ($scan->($end))[0]];
}
while ($text =~ /#\s*!\s*\[\s*cfg\s*$group\s*\]/g) {
  my ($at, $args) = ($-[0], substr($1, 1, -1));
  push @regions, [$at, $enclosing->($at)] if cfg_test_only($args =~ s/\A\s+|\s+\z//gr);
}
my $test_only = sub {
  my ($at) = @_;
  return scalar grep { $at >= $_->[0] && $at < $_->[1] } @regions;
};

# Function bodies: a cfg in one is named by its function.
my @functions;
while ($text =~ /\bfn\s+(?:r#)?(\w+)/g) {
  my ($function, $after) = ($1, $+[0]);
  my ($end, $open) = $scan->($after);
  push @functions, [$open, $end, $function] if defined $open;
}
my $function = sub {
  my ($at, $in) = @_;
  for (@functions) { $in = $_ if $at > $_->[0] && $at < $_->[1] && (!$in || $_->[0] > $in->[0]) }
  return $in && $in->[2];
};
# What an attribute at $_[0] is on, up to a `{`, `;`, `=` or `,` outside
# brackets (a field, a variant), or a function's name, with its literals.
my $head = sub {
  my ($s, $depth, $cut) = (substr($copy, $_[0], 4096), 0);
  my $qualifiers = qr/(?:(?:const|async|unsafe|safe|default|extern(?:\s*"\d+")?)\s+)*/;
  if ($s =~ /\A(?:pub\b\s*$group?\s*)?$qualifiers fn\s+(?:r\#)?\w+/x) {
    $cut = $+[0];
  }
  while (!defined $cut && $s =~ /[(\[<]|[)\]]|(?<![-=])>|[{;,]|(?<![=!<>])=(?![=>])/g) {
    my ($c, $at) = ($&, $-[0]);
    if ($c =~ /[(\[<]/) {
      $depth++;
    } elsif ($c =~ /[)\]>]/ ? --$depth < 0 : $depth == 0) {
      $cut = $at;
      last;
    }
  }
  $s = substr($s, 0, $cut) if defined $cut;
  $s =~ s/"(\d+)"/"$literals[$1]"/g;
  $s =~ s/\s+/ /g;
  $s =~ s/([(\[<]) /$1/g;
  $s =~ s/ ([)\]>])/$1/g;
  $s =~ s/,([)\]>])/$1/g;
  $s =~ s/\A | \z//g;
  return length $s ? $s : "?";
};

while ($text =~ /\bdoc\s*(?=$group)/g) {
  my ($args, $at) = ($1, $-[0]);
  $report->($at, 2, "a doc(hidden) is left") if $args =~ /\bhidden\b/;
  if ($args =~ /\$/ && !$attribute->($at)) {
    $report->($at, 2, "a macro builds a doc attribute, which can hide an item");
  }
}
# A cfg hides what it is on from rustdoc unless its predicate holds in the
# documented build, which is for this target, in a debug build, with every
# feature: anything else is allowlisted, or in test-only code.
while ($text =~ /\b(cfg|cfg_attr)\s*(?=$group)/g) {
  my ($kind, $args, $at, $after) = ($1, substr($2, 1, -1), $-[0], $+[0] + length $2);
  my $condition = ($kind eq "cfg_attr" ? $predicate->($args) : $args) =~ s/\A\s+|\s+\z//gr;
  if ($condition =~ /\bdoc\b/) {
    $report->($at, 2, "a $kind predicate on `doc` hides code from rustdoc");
  }
  if ($args =~ /\$/ && !$attribute->($at)) {
    $report->($at, 2, "a macro builds a $kind, which can hide an item");
  }
  next if $condition =~ /\bdoc\b|\$/ || cfg_documented($condition) || $test_only->($at);
  my $shown = $condition =~ s/\s+/ /gr =~ s/ ?([(),]) ?/$1/gr =~ s/,/, /gr =~ s/ ?= ?/ = /gr;
  $shown = "$kind(" . ($shown =~ s/"(\d+)"/"$literals[$1]"/gr) . ")";
  my $item = $function->($at);
  if (defined $item) {
    $item = "in fn $item";
  } else {
    my $in = $attribute->($at);
    if ($in && $in->[2]) {
      $item = "the module";
    } else {
      pos($copy) = $in ? $in->[1] : $after;
      $copy =~ /\G\s*\]?(?:\s*#\s*!?\s*$brackets)*\s*/g;
      $item = $head->(pos $copy);
    }
  }
  my $key = "$name|$shown|$item";
  $defer->($at, "`$shown` on `$item` hides code from the documentation, which is built for " .
    "this target, in a debug build, with every feature; if what it hides cannot name a " .
    "parser, allowlist \"$key\" in the gate with the reason", $key);
}
while ($text =~ /\bcfg_(?:select|match)\b/g) {
  my $at = $-[0];
  next if $test_only->($at);
  $defer->($at, "cfg_select! picks code by platform, which the check cannot read");
}

# Macro input and bodies, where a macro can rearrange what the checks read.
my $tokens = qr/($group|$brackets|$braces)/;
my @macros;
while ($text =~ /\b(?!(?:if|while|match|return|in|else|break)\b)\w+\s*!\s*(?=$tokens)/g) {
  push @macros, [$+[0], $+[0] + length $1];
}
while ($text =~ /\bmacro_rules\s*!\s*(?:r#)?\w+\s*(?=$tokens)/g) {
  push @macros, [$+[0], $+[0] + length $1];
}
my $in_macro = sub {
  my ($at) = @_;
  return scalar grep { $at >= $_->[0] && $at < $_->[1] } @macros;
};

# What can load a file, for the test-only files: an `include!` or `#[path]`,
# with the name of the file it names, and a `mod name;`, whose files the end
# works out (see there).
my $load = sub {
  my ($at, $kind, $path) = @_;
  push @loads, { file => $file, test => $test_only->($at), kind => $kind,
    escapes => $in_src && $resolve->($path) !~ m{\Asrc/}, names => [$path =~ m{([^/]+)\z}] };
};
while ($text =~ /\binclude\s*!\s*/g) {
  my $at = $-[0];
  my ($path) = substr($text, $+[0]) =~ /\A[\(\[\{]\s*"(\d+)"\s*,?\s*[\)\]\}]/;
  if (defined $path && $inside->($literals[$path])) {
    $load->($at, "include", $literals[$path]);
    next;
  }
  $report->($at, 2, "an include! of a file outside the crate directory, or of a computed path");
}
while ($text =~ /#\s*\[\s*path\s*=\s*"(\d+)"\s*\]|\bcfg_attr\s*(?=$group)/g) {
  my ($index, $at, $args) = ($1, $-[0], $2);
  my @paths = defined $index ? ($index) : (($args // "") =~ /\bpath\s*=\s*"(\d+)"/g);
  for my $path (@paths) {
    if ($inside->($literals[$path])) {
      $load->($at, "path", $literals[$path]);
      next;
    }
    $report->($at, 2, "a #[path] to a file outside the crate directory");
  }
}
# A `mod name;` names a file in the directory of the inline modules it is in,
# unless one of them has a #[path].
my @inline;
while ($text =~ /(?<run>(?:\#\s*$brackets\s*)*)(?:pub\b(?:\s*$group)?\s*)?
    \bmod\s+(?:r\#)?(?<module>\w+)\s*(?=\{)/gx) {
  my ($run, $module, $open) = ($+{run}, $+{module}, $+[0]);
  push @inline, [$open, ($scan->($open))[0], $module, $run =~ /\bpath\b/];
}
while ($text =~ /\bmod\s+(?:r#)?(\w+)\s*;/g) {
  my ($at, $module) = ($-[0], $1);
  my @in = sort { $a->[0] <=> $b->[0] } grep { $at > $_->[0] && $at < $_->[1] } @inline;
  push @loads, { file => $file, test => $test_only->($at), module => $module,
    inline => [map { $_->[2] } @in], anywhere => $in_macro->($at) || scalar grep { $_->[3] } @in };
}
while ($text =~ /\bmod\s+\$/g) {
  $report->($-[0], 2, "a macro declares a module, so the check cannot tell which file it loads");
}
# include! renamed with `use`, or called through a macro given the word, would
# reach a file the checks above never see: outside test-only code, `include`
# fails in a `use` and in a macro's input or body.
while ($text =~ /\buse\b(?!\s*<)[^;]*/g) {
  my ($at, $use) = ($-[0], $&);
  next if $use !~ /\binclude\b/ || $test_only->($at);
  $defer->($at, "a `use` of `include` can rename include!, so the check cannot see what it " .
    "includes");
}
while ($text =~ /\binclude\b(?!\s*!)/g) {
  my $at = $-[0];
  next if $test_only->($at) || !$in_macro->($at);
  $defer->($at, "`include` in a macro's input or body can call include!, so the check cannot " .
    "see what it includes");
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

# An exported macro expands in another crate, so the scan never sees what it
# names; a private one's items are documented where it expands. Its body may
# name a parser crate in no form, a bare word included (`use ::ayaml as p;`).
while ($text =~ /((?:#\s*$brackets\s*)*)macro_rules\s*!\s*(?:r#)?\w+\s*/g) {
  my ($attributes, $at, $end) = ($1, $-[0], $+[0]);
  next unless $attributes =~ /\bmacro_export\b/;
  pos($copy) = $end;
  if ($copy =~ /\G($braces|$group|$brackets)/g && $1 =~ /\b(?:$ENV{PARSERS})\b/) {
    $report->($at, 1, "a macro names a parser crate");
  }
}

END {
  # A name is a file's path, or the end of it, which any file it ends counts as.
  my $names = sub {
    my ($load, $file) = @_;
    return grep { m{\A/} ? $file eq $_ : $file =~ m{/\Q$_\E\z} } @{$load->{names}};
  };
  # The files a `mod name;` can load: name.rs or name/mod.rs in the directory of
  # its file's children (lib.rs's, mod.rs's or a file a #[path] loads: its own
  # directory; another file's: the one named after it; either, when a #[path]
  # can name the file), then its inline modules'. Any such file, when a macro or
  # an inline module's #[path] decides, or an include! can splice the file in.
  for my $load (grep { defined $_->{module} } @loads) {
    my ($from, $stem) = $load->{file} =~ m{\A(.*)/([^/]+)\.rs\z};
    my @names = ("$load->{module}.rs", "$load->{module}/mod.rs");
    my %by = map { $_->{kind} => 1 } grep { $_->{kind} && $names->($_, $load->{file}) } @loads;
    if ($load->{anywhere} || $by{include}) {
      $load->{names} = \@names;
      next;
    }
    my $root = $stem =~ /\A(?:lib|main|mod)\z/;
    my @bases = (($root || $by{path} ? $from : ()), ($root ? () : "$from/$stem"));
    $load->{names} = [map { my $base = join "/", $_, @{$load->{inline}}; map { "$base/$_" } @names }
      @bases];
  }
  # The test-only files, to a fixed point: a file under src/ that something
  # loads, all of it test-only code (in a test-only item or file); and the
  # files outside src/, unless code under src/ outside test-only code loads one
  # from outside src/.
  my %test;
  my $test = sub { $_[0]{test} || $test{$_[0]{file}} };
  for (my $more = 1; $more;) {
    $more = 0;
    my $escapes = grep { $_->{escapes} && !$test->($_) } @loads;
    for my $file (sort keys %crate_of) {
      next if $test{$file} || !defined $crate_of{$file};
      my $relative = substr($file, length($crate_of{$file}) + 1);
      next if $relative eq "src/lib.rs";
      my @by = grep { $names->($_, $file) } @loads;
      my $only = $relative =~ m{\Asrc/} ? @by && !grep { !$test->($_) } @by : !$escapes;
      ($test{$file}, $more) = (1, 1) if $only;
    }
  }
  for (sort { $a->[1] cmp $b->[1] || $a->[2] <=> $b->[2] } @deferred) {
    my ($file, $name, $line, $message, $key) = @$_;
    next if $test{$file};
    if (defined $key && exists $allowed{$key}) {
      $allowed{$key}++;
      next;
    }
    printf STDERR "public-api-parser-gate: %s:%d: %s\n", $name, $line, $message;
    $status |= 2;
  }
  my %names = map { substr($_, length($ENV{ROOT}) + 1) => 1 } keys %crate_of;
  for my $key (sort keys %allowed) {
    my ($name) = split /\|/, $key;
    next if !$names{$name} || $allowed{$key};
    print STDERR "public-api-parser-gate: $name: the allowlist entry \"$key\" matches no cfg " .
      "outside test-only code; remove it\n";
    $status |= 2;
  }
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
    # It removes `#[doc(hidden)]` from the files under DIR, so DIR must be a
    # copy: one in a Git work tree, such as the repository itself, is refused.
    if [[ $(git -C "$2" rev-parse --is-inside-work-tree 2>/dev/null) == true ]]; then
      echo "public-api-parser-gate: --check-sources rewrites the sources under $2;" \
        "pass a copy outside any Git work tree" >&2
      exit 2
    fi
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
