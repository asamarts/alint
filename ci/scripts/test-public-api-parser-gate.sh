#!/usr/bin/env bash
# Regression tests for public-api-parser-gate.sh, on hand-made trees.
# - The scan: a clean tree passes; a link to any parser crate (a listed one, or
#   a documented crate named like one) from any page of the three crates fails
#   and names the page; a link from a source page or another crate's page does
#   not count; a missing crate is an error.
# - The source checks: every spelling of `#[doc(hidden)]` is removed; a
#   `doc(hidden)` left in a `cfg_attr`, a `cfg` predicate on `doc`, and an
#   exported macro naming a parser crate fail; comments and private macros do
#   not count.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
GATE="$REPO_ROOT/ci/scripts/public-api-parser-gate.sh"
sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
pass=0
fail=0

# A documentation tree of the three crates, each with an index page and a type
# page that links only to serde and std.
make_tree() {
  local doc=$1 crate
  for crate in alint_core alint_dsl alint_testkit; do
    mkdir -p "$doc/$crate/yaml"
    printf '<a href="../serde/index.html">serde</a>\n' >"$doc/$crate/index.html"
    printf '<a href="../../serde/trait.Serialize.html">Serialize</a>
<a href="https://doc.rust-lang.org/nightly/alloc/string/struct.String.html">String</a>\n' \
      >"$doc/$crate/yaml/struct.Value.html"
  done
  mkdir -p "$doc/src/alint_core" "$doc/ayaml"
  # Source pages and the parser's own pages may name the parser.
  printf '<a href="../../ayaml/struct.Value.html">Value</a>\n' >"$doc/src/alint_core/yaml.rs.html"
  printf '<a href="../ayaml/struct.Value.html">Value</a>\n' >"$doc/ayaml/index.html"
}

expect() {
  local name=$1 want_rc=$2 want_text=$3 doc=$4 rc=0 out
  out=$(bash "$GATE" --scan "$doc" 2>&1) || rc=$?
  if [[ $rc -eq $want_rc && $out == *"$want_text"* ]]; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    echo "FAIL: $name: rc=$rc (want $want_rc), output:" >&2
    sed 's/^/  /' <<<"$out" >&2
  fi
}

doc="$sandbox/clean"
make_tree "$doc"
expect "a clean tree passes" 0 "no public item" "$doc"

mapfile -t parsers < <(bash "$GATE" --parsers)
((${#parsers[@]} >= 16)) || { echo "FAIL: --parsers lists ${#parsers[@]} crates" >&2; exit 1; }
index=0
for parser in "${parsers[@]}"; do
  crates=(alint_core alint_dsl alint_testkit)
  crate=${crates[$((index % 3))]}
  index=$((index + 1))
  doc="$sandbox/$parser-$crate"
  make_tree "$doc"
  printf '<a href="../../%s/struct.Mapping.html">Mapping</a>\n' "$parser" \
    >>"$doc/$crate/yaml/struct.Value.html"
  expect "a $crate page linking $parser fails" 1 "$crate/yaml/struct.Value.html" "$doc"
done

# A documented crate whose name contains `yaml` or `saphyr` counts as a parser.
doc="$sandbox/named-like-a-parser"
make_tree "$doc"
mkdir -p "$doc/yaml_peg"
printf '<a href="../../yaml_peg/struct.Node.html">Node</a>\n' >>"$doc/alint_dsl/index.html"
expect "a link to a crate named like a parser fails" 1 "alint_dsl/index.html" "$doc"

# alint's own yaml module is not a parser crate.
doc="$sandbox/own-yaml-module"
make_tree "$doc"
printf '<a href="../../alint_core/yaml/struct.Mapping.html">Mapping</a>\n' \
  >>"$doc/alint_dsl/index.html"
expect "a link to alint_core::yaml passes" 0 "no public item" "$doc"

# An absolute link to the parser's published documentation counts too.
doc="$sandbox/absolute"
make_tree "$doc"
printf '<a href="https://docs.rs/ayaml/0.1.2/ayaml/struct.Error.html">Error</a>\n' \
  >>"$doc/alint_core/index.html"
expect "an absolute link to a parser crate fails" 1 "alint_core/index.html" "$doc"

# A crate whose name only starts like a parser's is not one.
doc="$sandbox/lookalike"
make_tree "$doc"
printf '<a href="../../ayamlish/struct.Value.html">Value</a>\n' >>"$doc/alint_dsl/index.html"
expect "a lookalike crate name passes" 0 "no public item" "$doc"

doc="$sandbox/missing"
make_tree "$doc"
rm -rf "$doc/alint_testkit"
expect "a missing crate is an error" 2 "no documentation for alint_testkit" "$doc"

# Source trees for --check-sources: one lib.rs per crate.
make_sources() {
  local root=$1 body=$2 crate
  for crate in alint-core alint-dsl alint-testkit; do
    mkdir -p "$root/crates/$crate/src"
    printf 'pub fn ok() {}\n' >"$root/crates/$crate/src/lib.rs"
  done
  printf '%s\n' "$body" >"$root/crates/alint-core/src/extra.rs"
}

expect_sources() {
  local name=$1 want_rc=$2 want_text=$3 body=$4 root rc=0 out
  root="$sandbox/sources-$((pass + fail))"
  make_sources "$root" "$body"
  out=$(bash "$GATE" --check-sources "$root" 2>&1) || rc=$?
  if [[ $rc -eq $want_rc && $out == *"$want_text"* ]]; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    echo "FAIL: $name: rc=$rc (want $want_rc), output:" >&2
    sed 's/^/  /' <<<"$out" >&2
  fi
  last_sources="$root/crates/alint-core/src/extra.rs"
}

expect_sources "every spelling of doc(hidden) is removed" 0 "" '#[doc(hidden)]
pub fn a() {}
#[doc (hidden)]
pub fn b() {}
#[doc(
    hidden
)]
pub fn c() {}
mod inner {
    #![doc(hidden)]
}
// A comment may say doc(hidden), and so may /* doc(hidden) */ a block.
#[cfg(doctest)]
pub struct Sealed;
#[cfg_attr(docsrs, doc(cfg(feature = "strategies")))]
pub fn d() {}
macro_rules! private { () => { ayaml::Value::Null } }
#[macro_export]
macro_rules! exported { () => { 1 } }'
if perl -0ne 'exit(/#!?\s*\[\s*doc\s*\(\s*hidden\s*\)\s*\]/ ? 0 : 1)' "$last_sources"; then
  fail=$((fail + 1))
  echo "FAIL: a #[doc(hidden)] survived --check-sources" >&2
else
  pass=$((pass + 1))
fi

expect_sources "a doc(hidden) in a cfg_attr fails" 2 "a doc(hidden) is left" \
  '#[cfg_attr(test, doc(hidden))]
pub fn a() {}'
expect_sources "cfg(not(doc)) fails" 2 "predicate on \`doc\`" '#[cfg(not(doc))]
pub fn a() {}'
expect_sources "cfg(any(test, doc)) fails" 2 "predicate on \`doc\`" '#[cfg(any(test, doc))]
pub fn a() {}'
expect_sources "cfg_attr on doc fails" 2 "predicate on \`doc\`" '#[cfg_attr(doc, allow(dead_code))]
pub fn a() {}'
expect_sources "an exported macro naming a parser fails" 1 "a macro names a parser crate" \
  '#[macro_export]
macro_rules! leak { () => { ::ayaml::Value::Null } }'
expect_sources "an exported macro with other attributes naming a parser fails" 1 \
  "a macro names a parser crate" '#[macro_export]
#[doc = "Builds a value."]
macro_rules! leak ( () => { saphyr::Yaml::Null } );'

rc=0
out=$(bash "$GATE" --check-sources "$sandbox/no-such-tree" 2>&1) || rc=$?
if [[ $rc -eq 2 && $out == *"no sources at"* ]]; then
  pass=$((pass + 1))
else
  fail=$((fail + 1))
  echo "FAIL: a missing source tree is an error: rc=$rc, output: $out" >&2
fi

rc=0
out=$(bash "$GATE" --scan "$sandbox/clean" extra 2>&1) || rc=$?
if [[ $rc -eq 2 && $out == *usage* ]]; then
  pass=$((pass + 1))
else
  fail=$((fail + 1))
  echo "FAIL: an extra argument is an error: rc=$rc, output: $out" >&2
fi

echo "test-public-api-parser-gate: $pass passed, $fail failed"
((fail == 0))
