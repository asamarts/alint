#!/usr/bin/env bash
# Regression tests for public-api-parser-gate.sh, on hand-made trees.
# - The scan: a clean tree with its canaries passes; a link to any parser crate
#   (a listed one, or a documented crate named like one) from any page of the
#   three crates fails and names the page; a link from a source page or another
#   crate's page does not count; a missing crate, a missing or unlinked canary,
#   and missing parser documentation are errors.
# - The source checks: every spelling of `#[doc(hidden)]` is removed, in every
#   .rs file under a crate's directory; a `hidden` left in a doc list, an
#   attribute a macro builds, a `cfg` predicate on `doc` at any depth, a public
#   item under a platform, profile or `not(feature)` cfg (unless allowlisted), an
#   `include!` or `#[path]` that leaves the crate, a crate without
#   `#![deny(unnameable_types)]` or with an `allow` of it, and an exported macro
#   naming a parser crate with any delimiter fail; comments, string and
#   character literals and private macros do not count, and cannot hide code.
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
    printf '<a href="../../serde/trait.Serialize.html">Serialize</a>
<a href="https://doc.rust-lang.org/nightly/alloc/string/struct.String.html">String</a>\n' \
      >"$doc/$crate/yaml/struct.Value.html"
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

expect_sources() {
  local name=$1 want_rc=$2 want_text=$3 body=$4 root rc=0 out
  root="$sandbox/sources-$((pass + fail))"
  make_sources "$root" "$body"
  if [[ -n ${5-} ]]; then
    mkdir -p "$(dirname "$root/$5")"
    printf '%s\n' "$6" >"$root/$5"
  fi
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
#[cfg(unix)]
fn private_on_unix() {}
#[cfg(debug_assertions)]
pub(crate) fn crate_only() {}
pub fn statements() {
    #[cfg(windows)]
    let _ = 1;
}
#[path = "inner/more.rs"]
mod more;
#[cfg_attr(unix, path = "inner/more.rs")]
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
expect_sources "a doc attribute a macro builds fails" 2 "a macro builds a doc attribute" \
  'macro_rules! m { ($a:meta) => { #[doc($a)] pub fn f() -> ayaml::Value { todo!() } } }
m!(hidden);'
expect_sources "an attribute a macro builds fails" 2 "a macro builds an attribute" \
  'macro_rules! m { ($a:meta) => { #[$a] pub fn f() -> ayaml::Value { todo!() } } }'
expect_sources "a cfg a macro builds fails" 2 "a macro builds a cfg" \
  'macro_rules! m { ($p:meta) => { #[cfg($p)] pub fn f() -> ayaml::Value { todo!() } } }'
expect_sources "cfg(not(doc)) fails" 2 "predicate on \`doc\`" '#[cfg(not(doc))]
pub fn a() {}'
expect_sources "cfg(any(test, doc)) fails" 2 "predicate on \`doc\`" '#[cfg(any(test, doc))]
pub fn a() {}'
expect_sources "cfg_attr on doc fails" 2 "predicate on \`doc\`" '#[cfg_attr(doc, allow(dead_code))]
pub fn a() {}'
expect_sources "a cfg on doc inside a cfg_attr fails" 2 "a cfg predicate on \`doc\`" \
  '#[cfg_attr(all(), cfg(not(doc)))]
pub fn a() -> ayaml::Value { todo!() }'

# Public items rustdoc never sees on the documented target and profile.
expect_sources "a public fn under cfg(windows) fails" 2 "\`pub fn a\` exists only under" \
  '#[cfg(windows)]
pub fn a() -> ayaml::Value { todo!() }'
expect_sources "a public item under cfg(not(debug_assertions)) fails" 2 \
  "\`pub struct S\` exists only under" '#[cfg(not(debug_assertions))]
#[derive(Debug)]
pub struct S(pub ayaml::Value);'
expect_sources "an impl under a target cfg fails" 2 "\`impl From<ayaml::Value> for Value\`" \
  '#[cfg(target_os = "macos")]
impl From<ayaml::Value> for Value { fn from(_: ayaml::Value) -> Self { todo!() } }'
expect_sources "a public item under cfg_attr(unix, cfg(...)) fails" 2 "\`pub fn a\`" \
  '#[cfg_attr(all(), cfg(unix))]
pub fn a() {}'
expect_sources "a public item under cfg(not(feature)) fails" 2 "\`pub fn a\` exists only under" \
  '#[cfg(not(feature = "strategies"))]
pub fn a() -> ayaml::Value { todo!() }'
expect_sources "a module under #![cfg(unix)] with a public item fails" 2 "\`the module\`" \
  '#![cfg(unix)]
pub fn a() {}'
# The allowlist names an item it accepts, in a copy of the gate.
GATE_UNDER_TEST="$sandbox/allowing-gate.sh"
sed 's/^PLATFORM_ALLOWLIST=()$/PLATFORM_ALLOWLIST=("crates\/alint-core\/src\/extra.rs|pub fn a")/' \
  "$GATE" >"$GATE_UNDER_TEST"
if cmp -s "$GATE" "$GATE_UNDER_TEST"; then
  fail=$((fail + 1))
  echo "FAIL: could not add an allowlist entry to a copy of the gate" >&2
fi
expect_sources "an allowlisted public item under cfg(windows) passes" 0 "" '#[cfg(windows)]
pub fn a() {}'
expect_sources "the allowlist names one item only" 2 "\`pub fn b\` exists only under" \
  '#[cfg(windows)]
pub fn b() {}'
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
