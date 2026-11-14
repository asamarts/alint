#!/usr/bin/env bash
# Regression tests for public-api-parser-gate.sh's scan, on hand-made
# documentation trees: a clean tree passes, a link to any parser crate from any
# page of the three crates fails and names the page, a link from a source page
# or another crate's page does not count, and a missing crate is an error.
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

for parser in ayaml saphyr saphyr_parser yaml_serde serde_yaml; do
  for crate in alint_core alint_dsl alint_testkit; do
    doc="$sandbox/$parser-$crate"
    make_tree "$doc"
    printf '<a href="../../%s/struct.Mapping.html">Mapping</a>\n' "$parser" \
      >>"$doc/$crate/yaml/struct.Value.html"
    expect "a $crate page linking $parser fails" 1 "$crate/yaml/struct.Value.html" "$doc"
  done
done

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
