#!/usr/bin/env bash
# Regression tests for public-api-parser-gate.sh, on hand-made trees.
# - The scan: a clean tree with its canaries passes; a link to any parser crate
#   (a listed one, or a documented crate named like one) from any page of the
#   three crates fails and names the page, and so does a link to a parser
#   crate's source, which a parser item re-exported in any form has; a link from
#   a source page or another crate's page, one in prose to elsewhere (an issue
#   on GitHub), and a source link to alint's own source or another crate's do
#   not count; a missing crate, a missing or unlinked canary, and missing parser
#   documentation are errors.
# - The source checks: every spelling of `#[doc(hidden)]` is removed, in every
#   .rs file under a crate's directory; a `hidden` left in a doc list, a `$` in
#   an attribute, a `cfg` predicate on `doc` at any depth, any `cfg` other than
#   one on features, `test`, `doctest` or `docsrs` (on an item, field, variant,
#   trait item, module or statement; unless allowlisted or in test-only code),
#   `cfg_select!`, an `include!` or `#[path]` that leaves the crate, a `use` of
#   `include` or the word in a macro, a crate without
#   `#![deny(unnameable_types)]` or with an `allow` of it, and an exported macro
#   naming a parser crate in any form fail; comments, string and character
#   literals and private macros do not count, and cannot hide code. A `pub use`
#   or `pub extern crate` of a parser crate fails in every form, and a narrower
#   one passes. An allowlisted cfg may hide no declaration (an impl, a function,
#   a `use`, a macro call, a derive) but the item its key names and the kinds
#   its entry excepts, and an exception nothing needs fails. Test-only code is
#   an item under `cfg(test)`, a file only such code loads, and the files
#   outside src/ while the library loads none; and the repository's own sources
#   pass, so the allowlist holds exactly their cfgs and declarations.
#
# Rust bodies are single-quoted on purpose: `$a` in them is a macro variable.
# shellcheck disable=SC2016
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
GATE="$REPO_ROOT/ci/scripts/public-api-parser-gate.sh"
sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
pass=0
fail=0

# A documentation tree of the three crates, each with an index page, a type page
# that links only to serde and std, and the gate's canary, linked to ayaml.
make_tree() {
  local doc=$1 crate
  for crate in alint_core alint_dsl alint_testkit; do
    mkdir -p "$doc/$crate/yaml"
    printf '<a href="../serde/index.html">serde</a>\n' >"$doc/$crate/index.html"
    # Its own source, and a blanket impl's in another crate, as rustdoc links
    # them.
    printf '<a href="../../serde/trait.Serialize.html">Serialize</a>
<a href="https://doc.rust-lang.org/nightly/alloc/string/struct.String.html">String</a>
<a class="src" href="../../src/%s/yaml.rs.html#121">Source</a>
<a class="src rightside" href="../../src/crossbeam_epoch/atomic.rs.html#194">Source</a>\n' \
      "$crate" >"$doc/$crate/yaml/struct.Value.html"
    printf '<a href="../ayaml/number/struct.Number.html">Number</a>\n' \
      >"$doc/$crate/type.PublicApiGateCanary.html"
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
  doc="$sandbox/$parser-$crate-source"
  make_tree "$doc"
  printf '<a class="src" href="../src/%s/lib.rs.html#9">Source</a>\n' "$parser" \
    >"$doc/$crate/struct.Reexported.html"
  expect "a $crate page linking $parser's source fails" 1 "$crate/struct.Reexported.html" "$doc"
done

# rustdoc copies an item another crate re-exports onto the re-exporting crate's
# pages, whose links to the parser are only its source links, and links the
# crate's other mentions of the item to that copy (the review of #530). Each
# form, as rustdoc 1.100 writes its page: `pub use ayaml::ErrorKind as
# ParserErrorKind;` in alint_core::yaml, a plain `pub use ayaml::LimitKind;`, a
# `#[doc(inline)]` one, a glob `pub use ayaml::*;` in a public module, and a
# re-export in a public module.
# reexport NAME PAGE SOURCE: the page, under the tree, with the source link.
reexport() {
  local name=$1 page=$2 source=$3
  doc="$sandbox/reexport-$((pass + fail))"
  make_tree "$doc"
  mkdir -p "$(dirname "$doc/$page")"
  cat >"$doc/$page" <<HTML
<h1>Enum <span class="enum">Item</span></h1><span class="sub-heading">
<a class="src" href="$source">Source</a> </span>
<section class="impl"><a class="src rightside" href="$source">Source</a>
<h3 class="code-header">impl <a class="trait"
href="https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html">Clone</a> for
<a class="enum" href="enum.Item.html">Item</a></h3></section>
<section class="impl"><a class="src rightside"
href="${source%%src/*}src/crossbeam_epoch/atomic.rs.html#194">Source</a></section>
HTML
  expect "$name fails" 1 "$page names a parser crate" "$doc"
}
reexport "an aliased re-export of a parser item" alint_core/yaml/enum.ParserErrorKind.html \
  ../../src/ayaml/error.rs.html#18
reexport "a plain re-export of a parser item" alint_core/enum.LimitKind.html \
  ../src/ayaml/limits.rs.html#46
reexport "a doc(inline) re-export of a parser item" alint_dsl/struct.ParserSpan.html \
  ../src/ayaml/span.rs.html#100
reexport "a glob re-export of a parser crate" alint_testkit/parser/fn.from_str.html \
  ../../src/ayaml/lib.rs.html#692
reexport "a re-export in a public module" alint_core/parser/struct.Marker.html \
  ../../src/ayaml/span.rs.html#15
doc="$sandbox/lookalike-source"
make_tree "$doc"
printf '<a class="src" href="../src/ayamlish/lib.rs.html#9">Source</a>\n' \
  >"$doc/alint_dsl/struct.Reexported.html"
expect "a lookalike crate's source passes" 0 "no public item" "$doc"

# A documented crate whose name contains `yaml` or `saphyr` counts as a parser.
doc="$sandbox/named-like-a-parser"
make_tree "$doc"
mkdir -p "$doc/yaml_peg"
printf '<a href="../../yaml_peg/struct.Node.html">Node</a>\n' >>"$doc/alint_dsl/index.html"
expect "a link to a crate named like a parser fails" 1 "alint_dsl/index.html" "$doc"

# Prose may link to a parser's repository or issues; only rustdoc's links to a
# crate's pages count.
doc="$sandbox/prose-link"
make_tree "$doc"
printf '<a href="https://github.com/asamarts/ayaml/issues/132">asamarts/ayaml#132</a>\n' \
  >>"$doc/alint_core/yaml/struct.Value.html"
expect "a prose link to a parser's issue passes" 0 "no public item" "$doc"

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

# The canaries check the build: each must be there and link to ayaml, whose own
# documentation must be there too.
doc="$sandbox/no-canary"
make_tree "$doc"
rm "$doc/alint_dsl/type.PublicApiGateCanary.html"
expect "a missing canary is an error" 2 "alint_dsl/type.PublicApiGateCanary.html does not link" \
  "$doc"
doc="$sandbox/unlinked-canary"
make_tree "$doc"
printf 'pub type PublicApiGateCanary = Number;\n' >"$doc/alint_core/type.PublicApiGateCanary.html"
expect "a canary without a link (no dependencies documented) is an error" 2 \
  "alint_core/type.PublicApiGateCanary.html does not link" "$doc"
doc="$sandbox/no-parser-docs"
make_tree "$doc"
rm -rf "$doc/ayaml"
expect "missing parser documentation is an error" 2 "no documentation for ayaml" "$doc"
# A canary is not a finding, and does not excuse one.
doc="$sandbox/canary-and-leak"
make_tree "$doc"
printf '<a href="../ayaml/struct.Marker.html">Marker</a>\n' >>"$doc/alint_testkit/index.html"
expect "a leak next to the canaries fails" 1 "alint_testkit/index.html names a parser" "$doc"
rc=0
out=$(bash "$GATE" --scan "$doc" 2>&1) || rc=$?
if [[ $out != *"type.PublicApiGateCanary.html names"* ]]; then
  pass=$((pass + 1))
else
  fail=$((fail + 1))
  echo "FAIL: a canary was reported as a finding: $out" >&2
fi

# Source trees for --check-sources: one lib.rs per crate, which denies
# unnameable_types, and the body under test in alint-core's src/extra.rs.
make_sources() {
  local root=$1 body=$2 crate
  for crate in alint-core alint-dsl alint-testkit; do
    mkdir -p "$root/crates/$crate/src"
    printf '//! A crate.\n#![deny(unnameable_types)]\npub fn ok() {}\n' \
      >"$root/crates/$crate/src/lib.rs"
  done
  printf '%s\n' "$body" >"$root/crates/alint-core/src/extra.rs"
}

# expect_sources NAME RC TEXT BODY [PATH CONTENT]...: the body in extra.rs and
# each further file at its path.
expect_sources() {
  local name=$1 want_rc=$2 want_text=$3 body=$4 root rc=0 out
  root="$sandbox/sources-$((pass + fail))"
  make_sources "$root" "$body"
  shift 4
  while (($# >= 2)); do
    mkdir -p "$(dirname "$root/$1")"
    printf '%s\n' "$2" >"$root/$1"
    shift 2
  done
  out=$(bash "${GATE_UNDER_TEST:-$GATE}" --check-sources "$root" 2>&1) || rc=$?
  if [[ $rc -eq $want_rc && $out == *"$want_text"* ]]; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    echo "FAIL: $name: rc=$rc (want $want_rc), output:" >&2
    sed 's/^/  /' <<<"$out" >&2
  fi
  last_root=$root
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
#[doc(alias = "hidden")]
pub fn named() {}
#[cfg(doctest)]
pub struct Sealed;
#[cfg_attr(docsrs, doc(cfg(feature = "strategies")))]
pub fn d() {}
#[path = "inner/more.rs"]
mod more;
#[cfg_attr(feature = "x", path = "inner/more.rs")]
mod also_more;
macro_rules! private { () => { ayaml::Value::Null } }
#[macro_export]
macro_rules! exported { () => { "ayaml::Value" } }
#[macro_export]
macro_rules! commented { () => { /* ayaml::Value */ 1 } }' \
  crates/alint-core/tests/helper.rs '#[doc(hidden)]
pub fn helper() {}'
if perl -0ne 'exit(/#!?\s*\[\s*doc\s*\(\s*hidden\s*\)\s*\]/ ? 0 : 1)' \
  "$last_root/crates/alint-core/src/extra.rs" "$last_root/crates/alint-core/tests/helper.rs"; then
  fail=$((fail + 1))
  echo "FAIL: a #[doc(hidden)] survived --check-sources" >&2
else
  pass=$((pass + 1))
fi

expect_sources "a doc(hidden) in a cfg_attr fails" 2 "a doc(hidden) is left" \
  '#[cfg_attr(test, doc(hidden))]
pub fn a() {}'
expect_sources "hidden after another doc option fails" 2 "a doc(hidden) is left" \
  '#[doc(alias = "x", hidden)]
pub fn a() -> ayaml::Value { todo!() }'
expect_sources "a doc attribute a macro builds fails" 2 "a macro builds an attribute" \
  'macro_rules! m { ($a:meta) => { #[doc($a)] pub fn f() -> ayaml::Value { todo!() } } }
m!(hidden);'
expect_sources "an attribute a macro builds fails" 2 "a macro builds an attribute" \
  'macro_rules! m { ($a:meta) => { #[$a] pub fn f() -> ayaml::Value { todo!() } } }'
expect_sources "a cfg a macro builds fails" 2 "a macro builds an attribute" \
  'macro_rules! m { ($p:meta) => { #[cfg($p)] pub fn f() -> ayaml::Value { todo!() } } }'
# Any `$` in an attribute, or in brackets after a `$` variable: a macro builds
# it (the review of #523's shapes, which the doc and cfg checks missed).
expect_sources "a doc attribute a macro builds from its tokens fails" 2 \
  "a macro builds an attribute" \
  'macro_rules! m { ($($t:tt)*) => { #[doc $($t)*] pub fn f() -> ayaml::Value { todo!() } } }
m!((hidden));'
expect_sources "a cfg a macro builds from its tokens fails" 2 "a macro builds an attribute" \
  'macro_rules! m { ($($t:tt)*) => { #[cfg $($t)*] pub fn f() -> ayaml::Value { todo!() } } }
m!((not(doc)));'
expect_sources "a #[path] a macro builds fails" 2 "a macro builds an attribute" \
  'macro_rules! m { ($p:literal) => { #[path = $p] pub mod x; } }
m!("../../../outside.rs");'
expect_sources "an allow a macro builds fails" 2 "a macro builds an attribute" \
  'macro_rules! m { ($l:ident) => { mod hidden { #[allow($l)] pub struct U(pub ayaml::Marker); } } }
m!(unnameable_types);'
expect_sources "an attribute a macro builds after a # it is given fails" 2 \
  "a macro builds an attribute" \
  'macro_rules! m { ($h:tt $($t:tt)*) => { $h [doc $($t)*] pub fn f() -> ayaml::Value { todo!() } } }
m!(# (hidden));'
expect_sources "a module a macro names fails" 2 "a macro declares a module" \
  'macro_rules! m { ($n:ident) => { pub mod $n; } }
m!(leak);'
expect_sources "cfg(not(doc)) fails" 2 "predicate on \`doc\`" '#[cfg(not(doc))]
pub fn a() {}'
expect_sources "cfg(any(test, doc)) fails" 2 "predicate on \`doc\`" '#[cfg(any(test, doc))]
pub fn a() {}'
expect_sources "cfg_attr on doc fails" 2 "predicate on \`doc\`" '#[cfg_attr(doc, allow(dead_code))]
pub fn a() {}'
expect_sources "a cfg on doc inside a cfg_attr fails" 2 "a cfg predicate on \`doc\`" \
  '#[cfg_attr(all(), cfg(not(doc)))]
pub fn a() -> ayaml::Value { todo!() }'

# Code rustdoc never sees on the documented target, profile and features: any
# cfg but one on features, test, doctest or docsrs fails, on whatever it is on,
# and names its allowlist key.
expect_sources "a public fn under cfg(windows) fails" 2 "\`cfg(windows)\` on \`pub fn a\`" \
  '#[cfg(windows)]
pub fn a() -> ayaml::Value { todo!() }'
expect_sources "a public item under cfg(not(debug_assertions)) fails" 2 \
  "\`cfg(not(debug_assertions))\` on \`pub struct S(pub ayaml::Value)\`" \
  '#[cfg(not(debug_assertions))]
#[derive(Debug)]
pub struct S(pub ayaml::Value);'
expect_sources "an impl under a target cfg fails" 2 \
  "\`cfg(target_os = \"macos\")\` on \`impl From<ayaml::Value> for Value\`" \
  '#[cfg(target_os = "macos")]
impl From<ayaml::Value> for Value { fn from(_: ayaml::Value) -> Self { todo!() } }'
expect_sources "a public item under cfg_attr(unix, cfg(...)) fails" 2 "\`cfg(unix)\` on \`pub fn a\`" \
  '#[cfg_attr(all(), cfg(unix))]
pub fn a() {}'
expect_sources "a public item under cfg(not(feature)) fails" 2 \
  "\`cfg(not(feature = \"strategies\"))\` on \`pub fn a\`" \
  '#[cfg(not(feature = "strategies"))]
pub fn a() -> ayaml::Value { todo!() }'
expect_sources "a module under #![cfg(unix)] with a public item fails" 2 \
  "\`cfg(unix)\` on \`the module\`" '#![cfg(unix)]
pub fn a() {}'
# The shapes the review of #523 found passing: none is a public item or an impl
# under a cfg the old check knew, and each is API on another target.
expect_sources "a module under cfg(windows) holding impls fails" 2 \
  "extra.rs:1: \`cfg(windows)\` on \`mod m\`" '#[cfg(windows)]
mod m;' crates/alint-core/src/extra/m.rs 'impl crate::yaml::Value {
    pub fn f(&self) -> &ayaml::Value { todo!() }
}
impl From<ayaml::Value> for crate::yaml::Mapping {
    fn from(_: ayaml::Value) -> Self { todo!() }
}'
expect_sources "an impl in a const block under cfg(windows) fails" 2 \
  "\`cfg(windows)\` on \`const _: ()\`" '#[cfg(windows)]
const _: () = {
    impl From<ayaml::Mapping> for crate::yaml::Value {
        fn from(_: ayaml::Mapping) -> Self { todo!() }
    }
};'
expect_sources "a variant under cfg(windows) fails" 2 "\`cfg(windows)\` on \`Leak(ayaml::Value)\`" \
  'pub enum E {
    Plain,
    #[cfg(windows)]
    Leak(ayaml::Value),
}'
expect_sources "a trait item under cfg(windows) fails" 2 "\`cfg(windows)\` on \`fn f\`" \
  'pub trait T {
    #[cfg(windows)]
    fn f(&self) -> ayaml::Value;
}'
expect_sources "a public fn under cfg(panic = \"abort\") fails" 2 \
  "\`cfg(panic = \"abort\")\` on \`pub fn f\`" '#[cfg(panic = "abort")]
pub fn f() -> ayaml::Value { todo!() }'
expect_sources "a function is named by its name and qualifiers" 2 \
  "\`cfg(unix)\` on \`pub(crate) unsafe extern \"C\" fn r#g\`" '#[cfg(unix)]
pub(crate) unsafe extern "C" fn r#g<T>(x: T) -> T { x }'
expect_sources "a field under a target cfg fails" 2 \
  "\`cfg(target_os = \"macos\")\` on \`pub leak: ayaml::Value\`" 'pub struct S {
    pub plain: u8,
    #[cfg(target_os = "macos")]
    pub leak: ayaml::Value,
}'
expect_sources "a statement under cfg(windows) fails, named by its function" 2 \
  "\`cfg(windows)\` on \`in fn f\`" 'pub fn f() {
    #[cfg(windows)]
    {
        impl From<ayaml::Value> for crate::yaml::Value {
            fn from(_: ayaml::Value) -> Self { todo!() }
        }
    }
}'
expect_sources "a cfg a macro puts after a # it is given fails" 2 "\`cfg(windows)\` on \`pub fn f\`" \
  'macro_rules! m { ($h:tt) => { $h [cfg(windows)] pub fn f() -> ayaml::Value { todo!() } } }
m!(#);'
expect_sources "a raw r#cfg fails" 2 "\`cfg(windows)\` on \`pub fn f\`" \
  '#[r#cfg(windows)]
pub fn f() -> ayaml::Value { todo!() }'
expect_sources "cfg_select! fails" 2 "cfg_select! picks code by platform" 'cfg_select! {
    windows => { pub fn leak() -> ayaml::Value { todo!() } }
    _ => {}
}'
expect_sources "cfgs on features, test, doctest and docsrs pass" 0 "" '#[cfg(feature = "x")]
pub fn a() -> ayaml::Value { todo!() }
#[cfg(all(feature = "a", any(test, docsrs, doctest)))]
pub fn b() {}
#[cfg_attr(feature = "x", derive(Debug))]
pub struct S;
#[cfg_attr(docsrs, doc(cfg(feature = "x")))]
pub fn c() {}'

# Test-only code is skipped: an item under a test-only cfg and what it holds,
# up to its end only.
expect_sources "cfgs in a test module pass" 0 "" '#[cfg(test)]
mod tests {
    #[cfg(windows)]
    fn t() -> ayaml::Value { todo!() }
    #[test]
    fn u() {
        #[cfg(unix)]
        let _ = 1;
    }
}
#[cfg(all(test, unix))]
mod unix_tests {
    #[cfg(target_os = "linux")]
    fn t() {}
}
#[doc = "A helper."]
#[cfg(test)]
pub(crate) fn helper() {
    #[cfg(windows)]
    let _ = 1;
}'
expect_sources "a test-only region ends with its item" 2 \
  "extra.rs:3: \`cfg(windows)\` on \`pub fn leak\`" '#[cfg(test)]
fn helper() {}
#[cfg(windows)]
pub fn leak() -> ayaml::Value { todo!() }'
expect_sources "a test-only field does not cover the next one" 2 \
  "\`cfg(windows)\` on \`pub leak: ayaml::Value\`" 'pub struct S {
    #[cfg(test)]
    a: u8,
    #[cfg(windows)]
    pub leak: ayaml::Value,
}'
# A file only test-only code loads is test-only, and so are the files it loads;
# one that a library mod, #[path] or include! may load is not.
expect_sources "a test file a cfg(test) #[path] module loads passes" 0 "" '#[cfg(test)]
#[path = "extra_tests.rs"]
mod tests;' crates/alint-core/src/extra_tests.rs '#[cfg(windows)]
pub fn x() -> ayaml::Value { todo!() }'
expect_sources "a test file the library also loads fails" 2 \
  "extra_tests.rs:1: \`cfg(windows)\` on \`pub fn x\`" '#[cfg(test)]
#[path = "extra_tests.rs"]
mod tests;
#[path = "extra_tests.rs"]
pub mod leak;' crates/alint-core/src/extra_tests.rs '#[cfg(windows)]
pub fn x() -> ayaml::Value { todo!() }'
expect_sources "a module a test module loads passes" 0 "" '#[cfg(test)]
mod tests;
mod deeper;' crates/alint-core/src/extra/tests.rs 'mod deeper;' \
  crates/alint-core/src/extra/tests/deeper.rs '#[cfg(windows)]
pub fn x() -> ayaml::Value { todo!() }' crates/alint-core/src/extra/deeper.rs 'pub fn y() {}'
expect_sources "a module a test module and a #[path] load fails" 2 \
  "tests/deeper.rs:1: \`cfg(windows)\`" '#[cfg(test)]
mod tests;
#[path = "extra/tests/deeper.rs"]
pub mod leak;' crates/alint-core/src/extra/tests.rs 'mod deeper;' \
  crates/alint-core/src/extra/tests/deeper.rs '#[cfg(windows)]
pub fn x() -> ayaml::Value { todo!() }'
expect_sources "a module an inline module of the library loads fails" 2 \
  "extra/inner/deeper.rs:1: \`cfg(windows)\`" '#[cfg(test)]
mod tests;
pub mod inner {
    pub mod deeper;
}' crates/alint-core/src/extra/tests.rs '#[cfg(test)]
#[path = "inner/deeper.rs"]
mod deeper;' crates/alint-core/src/extra/inner/deeper.rs '#[cfg(windows)]
pub fn x() -> ayaml::Value { todo!() }'
# The files outside src/ (integration tests, benches) are test-only, unless the
# library loads one.
expect_sources "an integration test's cfgs pass" 0 "" 'pub fn f() {}' \
  crates/alint-core/tests/it.rs '#[cfg(unix)]
#[test]
fn t() {}'
expect_sources "an integration test's cfgs fail when the library includes a file outside src/" \
  2 "tests/it.rs:1: \`cfg(unix)\`" 'include!("../tests/shared.rs");' \
  crates/alint-core/tests/shared.rs 'pub fn shared() {}' crates/alint-core/tests/it.rs '#[cfg(unix)]
#[test]
fn t() {}'
expect_sources "an include! outside src/ in test-only code keeps the tests test-only" 0 "" \
  '#[cfg(test)]
mod tests {
    include!("../tests/shared.rs");
}' crates/alint-core/tests/shared.rs 'fn shared() {}' crates/alint-core/tests/it.rs '#[cfg(unix)]
#[test]
fn t() {}'

# The allowlist accepts the cfgs it names, in a copy of the gate, and an entry
# that matches no cfg in its file fails.
GATE_UNDER_TEST="$sandbox/allowing-gate.sh"
sed 's/^PLATFORM_ALLOWLIST=($/&\
  "crates\/alint-core\/src\/extra.rs|cfg(windows)|pub fn a"\
  "crates\/alint-core\/src\/extra.rs|cfg(unix)|in fn f"/' "$GATE" >"$GATE_UNDER_TEST"
if cmp -s "$GATE" "$GATE_UNDER_TEST"; then
  fail=$((fail + 1))
  echo "FAIL: could not add an allowlist entry to a copy of the gate" >&2
fi
expect_sources "allowlisted cfgs pass" 0 "" '#[cfg(windows)]
pub fn a() {}
pub fn f() {
    #[cfg(unix)]
    let _ = 1;
    #[cfg(unix)]
    let _ = 2;
}'
expect_sources "the allowlist names one item only" 2 "\`cfg(windows)\` on \`pub fn b\`" \
  '#[cfg(windows)]
pub fn b() {}
pub fn f() {
    #[cfg(unix)]
    let _ = 1;
}'
expect_sources "an allowlist entry for another predicate does not count" 2 \
  "\`cfg(not(windows))\` on \`pub fn a\`" '#[cfg(not(windows))]
pub fn a() {}
pub fn f() {
    #[cfg(unix)]
    let _ = 1;
}'
expect_sources "an allowlist entry that matches no cfg fails" 2 \
  "the allowlist entry \"crates/alint-core/src/extra.rs|cfg(unix)|in fn f\" matches no cfg" \
  '#[cfg(windows)]
pub fn a() {}'

# An entry names where its cfg is, not what the cfg hides (the review of #530:
# an impl in the facade's `fn parser`, under its allowlisted cfg, was API that
# the documentation, built with every feature, never showed). What it hides may
# declare nothing beyond the item the key names: an impl, a function, a `use`
# or a macro call, which can expand to an impl even in an expression, fails.
allowed_a='#[cfg(windows)]
pub fn a() {}'
expect_sources "an impl under an allowlisted cfg in a function fails" 2 \
  "extra.rs:5: \`cfg(unix)\` hides \`impl crate::yaml::Value\`, which declares" "$allowed_a
pub fn f() {
    #[cfg(unix)]
    impl crate::yaml::Value {
        pub fn parser_value(&self) -> &ayaml::Value { todo!() }
    }
}"
expect_sources "a public fn under an allowlisted cfg in a function fails" 2 \
  "\`cfg(unix)\` hides \`pub fn leak\`" "$allowed_a
pub fn f() {
    #[cfg(unix)]
    pub fn leak() -> ayaml::Value { todo!() }
}"
expect_sources "a pub use under an allowlisted cfg in a function fails" 2 \
  "\`cfg(unix)\` hides \`pub use crate::yaml::Value as Leak\`" "$allowed_a
pub fn f() {
    #[cfg(unix)]
    pub use crate::yaml::Value as Leak;
}"
expect_sources "an impl in a block under an allowlisted cfg fails" 2 \
  "\`cfg(unix)\` hides \`impl From<ayaml::Value> for crate::yaml::Value\`" "$allowed_a
pub fn f() {
    #[cfg(unix)]
    {
        let _ = 1;
        impl From<ayaml::Value> for crate::yaml::Value {
            fn from(_: ayaml::Value) -> Self { todo!() }
        }
    }
}"
expect_sources "a macro call in an expression under an allowlisted cfg fails" 2 \
  "\`cfg(unix)\` hides \`declare!(...)\`, which declares" "$allowed_a
pub fn f() {
    #[cfg(unix)]
    let _ = 1 + declare!(ayaml::Value);
}"
expect_sources "an impl in an allowlisted item fails" 2 \
  "\`cfg(windows)\` hides \`impl crate::yaml::Value\`" '#[cfg(windows)]
pub fn a() {
    impl crate::yaml::Value {
        pub fn leak(&self) -> &ayaml::Value { todo!() }
    }
}
pub fn f() {
    #[cfg(unix)]
    let _ = 1;
}'
unset GATE_UNDER_TEST

# A fourth field excepts kinds of declaration, each of which something must
# need; a cfg_attr may apply no attribute but a lint level, doc or cfg.
GATE_UNDER_TEST="$sandbox/excepting-gate.sh"
sed 's/^PLATFORM_ALLOWLIST=($/&\
  "crates\/alint-core\/src\/extra.rs|cfg(unix)|in fn g|use, debug_assert!"\
  "crates\/alint-core\/src\/extra.rs|cfg_attr(windows)|pub struct S"/' "$GATE" >"$GATE_UNDER_TEST"
if cmp -s "$GATE" "$GATE_UNDER_TEST"; then
  fail=$((fail + 1))
  echo "FAIL: could not add an allowlist entry with exceptions to a copy of the gate" >&2
fi
allowed_s='#[cfg_attr(windows, allow(dead_code), doc = "A struct.")]
pub struct S;'
expect_sources "an entry's exceptions let what its cfg hides declare those kinds" 0 "" \
  "$allowed_s
pub fn g() {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;
    #[cfg(unix)]
    debug_assert!(true);
}"
expect_sources "an exception excepts its kind only" 2 \
  "\`cfg(unix)\` hides \`impl crate::yaml::Value\`" "$allowed_s
pub fn g() {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;
    #[cfg(unix)]
    {
        debug_assert!(true);
        impl crate::yaml::Value {}
    }
}"
expect_sources "an exception that nothing hides fails" 2 \
  "excepts \`debug_assert!\`, which nothing it matches hides; remove it" "$allowed_s
pub fn g() {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;
}"
expect_sources "a derive under an allowlisted cfg_attr fails" 2 \
  "\`cfg_attr(windows)\` hides \`#[derive(Debug)]\`" '#[cfg_attr(windows, derive(Debug))]
pub struct S;
pub fn g() {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;
    #[cfg(unix)]
    debug_assert!(true);
}'
unset GATE_UNDER_TEST

# Files outside src/ are stripped and checked too; only code outside the crate
# directory, or at a computed path, is out of reach.
expect_sources "a file included from outside src/ is stripped" 0 "" \
  'include!("../extra/leak.rs");' crates/alint-core/extra/leak.rs '#[doc(hidden)]
pub fn leak() -> ayaml::Value { todo!() }'
if grep -q 'doc(hidden)' "$last_root/crates/alint-core/extra/leak.rs"; then
  fail=$((fail + 1))
  echo "FAIL: the doc(hidden) in an included file outside src/ survived" >&2
else
  pass=$((pass + 1))
fi
expect_sources "an included file outside src/ is checked" 2 "a doc(hidden) is left" \
  'include!("../extra/leak.rs");' crates/alint-core/extra/leak.rs '#[doc(alias = "x", hidden)]
pub fn leak() -> ayaml::Value { todo!() }'
expect_sources "an include! outside the crate fails" 2 "an include! of a file outside" \
  'include!("../../../shared/leak.rs");'
expect_sources "an include! of a computed path fails" 2 "an include! of a file outside" \
  'include!(concat!(env!("OUT_DIR"), "/leak.rs"));'
expect_sources "a #[path] outside the crate fails" 2 "a #[path] to a file outside" \
  '#[path = "../../../shared/leak.rs"]
pub mod leak;'
expect_sources "a #[path] in a cfg_attr outside the crate fails" 2 "a #[path] to a file outside" \
  '#[cfg_attr(all(), path = "/abs/leak.rs")]
pub mod leak;'
# include! renamed, or called through a macro given the word, reaches a file the
# checks never read (the review of #523); a field or a local named `include`,
# and the word in test-only code, do not count.
expect_sources "a use renaming include fails" 2 "a \`use\` of \`include\` can rename include!" \
  'use core::include as inc;
inc!("../../outside.rs");'
expect_sources "a use of include in braces fails" 2 "a \`use\` of \`include\`" \
  'pub use ::std::{include as splice};'
expect_sources "include passed to a macro fails" 2 "\`include\` in a macro's input or body" \
  'macro_rules! call { ($m:ident) => { $m!("../../outside.rs"); } }
call!(include);'
expect_sources "include in a macro's body fails" 2 "\`include\` in a macro's input or body" \
  'macro_rules! call { ($b:tt) => { include $b ("../../outside.rs"); } }
call!(!);'
expect_sources "a field, a local and include_str! named include pass" 0 "" \
  'pub struct Paths { pub include: Vec<String> }
pub fn count(paths: &Paths) -> usize {
    let include = &paths.include;
    include.len() + include_str!("extra.rs").len()
}
#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        let include = 1;
        assert_eq!(include, 1);
    }
}'
expect_sources "include in a test file's macro passes" 0 "" '#[cfg(test)]
#[path = "extra_tests.rs"]
mod tests;' crates/alint-core/src/extra_tests.rs 'fn t() {
    let include = 1;
    assert_eq!(include, 1);
}'

# A public type in a private module has no page; the lint makes it nameable.
expect_sources "an allow of unnameable_types fails" 2 "an \`allow\` of unnameable_types" \
  'mod hidden {
    #[allow(unnameable_types)]
    pub struct U { pub inner: ayaml::Marker }
}
pub fn f() -> hidden::U { todo!() }'
root="$sandbox/no-deny"
make_sources "$root" 'pub fn f() {}'
printf '//! A crate without the lint.\npub fn ok() {}\n' >"$root/crates/alint-dsl/src/lib.rs"
rc=0
out=$(bash "$GATE" --check-sources "$root" 2>&1) || rc=$?
if [[ $rc -eq 2 && $out == *"crates/alint-dsl/src/lib.rs: no #![deny(unnameable_types)]"* &&
  $out != *alint-core* ]]; then
  pass=$((pass + 1))
else
  fail=$((fail + 1))
  echo "FAIL: a crate without #![deny(unnameable_types)] must fail: rc=$rc, output: $out" >&2
fi

expect_sources "an exported macro naming a parser fails" 1 "a macro names a parser crate" \
  '#[macro_export]
macro_rules! leak { () => { ::ayaml::Value::Null } }'
expect_sources "an exported macro with other attributes naming a parser fails" 1 \
  "a macro names a parser crate" '#[macro_export]
#[doc = "Builds a value."]
macro_rules! leak ( () => { saphyr::Yaml::Null } );'
expect_sources "an exported macro in brackets naming a parser fails" 1 \
  "a macro names a parser crate" '#[macro_export]
macro_rules! leak [ () => { ::ayaml::Value::Null } ];'
# The review of #523: a parser crate renamed in the body, and a raw name.
expect_sources "an exported macro naming a parser as a bare word fails" 1 \
  "a macro names a parser crate" '#[macro_export]
macro_rules! leak { () => {{ use ::ayaml as parser; parser::Value::Null }} }'
expect_sources "an exported macro with a raw name fails" 1 "a macro names a parser crate" \
  '#[macro_export]
macro_rules! r#leak { () => { ::ayaml::Value::Null } }'
expect_sources "a macro exported through a cfg_attr fails" 1 "a macro names a parser crate" \
  '#[cfg_attr(feature = "x", macro_export)]
macro_rules! leak { () => { ::ayaml::Value::Null } }'
expect_sources "a private macro naming a parser as a bare word passes" 0 "" \
  'macro_rules! private { () => {{ use ::ayaml as parser; parser::Value::Null }} }'
# A public re-export of a parser crate or its item fails in every form (the
# review of #530: rustdoc shows such an item as alint's own); `pub(crate)` and
# narrower, a private `use`, a path that only contains a parser's name, and
# test-only code pass.
expect_sources "an aliased re-export of a parser item fails" 1 \
  "\`pub use ayaml::ErrorKind as ParserErrorKind\` re-exports a parser crate or its item" \
  'pub use ayaml::ErrorKind as ParserErrorKind;'
expect_sources "a plain re-export of a parser item fails" 1 \
  "\`pub use ayaml::LimitKind\` re-exports" 'pub use ayaml::LimitKind;'
expect_sources "a doc(inline) re-export of a parser item fails" 1 \
  "extra.rs:2: \`pub use ayaml::Span as ParserSpan\` re-exports" '#[doc(inline)]
pub use ayaml::Span as ParserSpan;'
expect_sources "a glob re-export of a parser crate fails" 1 "\`pub use ayaml::*\` re-exports" \
  'pub use ayaml::*;'
expect_sources "a re-export in a public module fails" 1 \
  "extra.rs:2: \`pub use ayaml::Marker\` re-exports" 'pub mod parser {
    pub use ayaml::Marker;
}'
expect_sources "a re-export of a parser crate itself fails" 1 \
  "\`pub use ::ayaml as parser\` re-exports" 'pub use ::ayaml as parser;'
expect_sources "a pub extern crate of a parser fails" 1 "\`pub extern crate saphyr\` re-exports" \
  'pub extern crate saphyr;'
expect_sources "a parser item in a use group fails" 1 \
  "\`pub use {crate::yaml::Value, ::serde_yaml::Mapping}\` re-exports" \
  'pub use {crate::yaml::Value, ::serde_yaml::Mapping};'
expect_sources "narrower re-exports, private uses and lookalikes pass" 0 "" \
  'pub(crate) use ayaml::Value;
pub(super) use ayaml::Marker;
pub (in crate::x) use saphyr::Yaml;
use ayaml::Mapping;
pub use crate::yaml::ayaml_from_str;
#[cfg(test)]
mod tests {
    pub use ayaml::Number;
}'
# Literals are read as Rust reads them: a comment marker inside one is text, and
# each finding keeps its line.
expect_sources "a // in a string does not hide the rest of the line" 1 \
  "extra.rs:1: a macro names a parser crate" '#[macro_export]
macro_rules! leak { () => { ("http://x", ::ayaml::Value::Null) } }'
expect_sources "a /* and */ in strings do not hide the code between them" 1 \
  "extra.rs:2: a macro names a parser crate" 'pub const A: &str = "target/*";
#[macro_export]
macro_rules! leak { () => { ::ayaml::Value::Null } }
pub const B: &str = "**/*.rs";'
read -r -d '' literals <<'RUST' || true
pub const A: &str = r#"a "quoted" /* word
over two lines"#;
pub const B: &[u8] = b"an \" escaped /* quote";
pub const C: char = '"';
pub fn f<'a>(s: &'a str) -> &'a str { s }
#[macro_export]
macro_rules! leak { () => { (/* "a /* nested */ comment */ ::ayaml::Value::Null, '\'', 'é') } }
pub const D: &str = "*/";
RUST
expect_sources "raw strings, escapes, lifetimes and character literals are read whole" 1 \
  "extra.rs:6: a macro names a parser crate" "$literals"
expect_sources "a quote in a comment does not start a string" 1 \
  "extra.rs:2: a macro names a parser crate" '// a " quote
#[macro_export]
macro_rules! leak { () => { ::ayaml::Value::Null } } // "'

# The repository's own sources pass, so PLATFORM_ALLOWLIST holds exactly the
# cfgs outside test-only code: each entry matches one, and each needs one.
root="$sandbox/repository"
mkdir -p "$root"
(cd "$REPO_ROOT" && find crates/alint-core crates/alint-dsl crates/alint-testkit -name '*.rs' \
  -print0 | tar --null -T - -cf -) | tar -xf - -C "$root"
rc=0
out=$(bash "$GATE" --check-sources "$root" 2>&1) || rc=$?
if [[ $rc -eq 0 ]]; then
  pass=$((pass + 1))
else
  fail=$((fail + 1))
  echo "FAIL: the repository's sources must pass --check-sources: rc=$rc, output:" >&2
  sed 's/^/  /' <<<"$out" >&2
fi

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

# --check-sources rewrites what it is given, so it refuses a Git work tree and
# leaves it as it was.
root="$sandbox/work-tree"
make_sources "$root" '#[doc(hidden)]
pub fn f() {}'
git -C "$root" init -q
rc=0
out=$(bash "$GATE" --check-sources "$root" 2>&1) || rc=$?
if [[ $rc -eq 2 && $out == *"pass a copy outside any Git work tree"* ]] &&
  grep -q 'doc(hidden)' "$root/crates/alint-core/src/extra.rs"; then
  pass=$((pass + 1))
else
  fail=$((fail + 1))
  echo "FAIL: --check-sources must refuse a Git work tree: rc=$rc, output: $out" >&2
fi

echo "test-public-api-parser-gate: $pass passed, $fail failed"
((fail == 0))
