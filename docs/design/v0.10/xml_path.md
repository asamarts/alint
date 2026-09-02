# `xml_path_matches` + `xml_path_equals` — JSONPath over XML

Status: **Implemented** — lands with the pair in v0.10 (this
commit; rule kind #7 of the case-study coverage push, a two-kind
item). Was a design draft (2026-05-18). v0.10 demand #7 (2
sources, ROADMAP-canonical). One design doc for both kinds: they
are the XML arm of the existing structured-query family and share
its entire implementation — only the parse step differs. Open
questions resolved on implementation: local-name /
namespace-insensitive mapping, namespace-aware mode deferred
(Q1); `@attr` / `#text` / repeated→array (xmltodict-style)
convention (Q2); all leaf values are strings, no type coercion
(Q3); `roxmltree` chosen over `quick-xml` (Q4); empty element →
`null` (Q5). v0.10 post-audit hardening (P1): an explicit
`MAX_XML_DEPTH` recursion bound — see *Recursion bound
(security)* below.

Demand evidence:
[`docs/development/launch-evidence.md`](../../development/launch-evidence.md)
(`xml_path_matches` / `xml_path_equals`, 2 sources: spark — 49
`pom.xml`; dotnet/runtime — ~2,300 XML manifests at one OOM
bigger scale; "completes the structured-query family
JSON/YAML/TOML/XML") and the per-repo tracker in
[`examples/README.md`](../../../examples/README.md#primitive-demand-tracker)
(`xml_path_*` row: spark, dotnet-runtime). Canonical scope:
[`../ROADMAP.md`](../ROADMAP.md#v010--case-study-coverage-push)
(#7; the `dotnet@v1` bundled ruleset #10 depends on it).

## Problem

The structured-query family (`{json,yaml,toml}_path_{equals,matches}`)
already lets a single rule assert a JSONPath (RFC 9535) value
across formats — every target coerces through serde into one
`serde_json::Value` tree, so the query / equals / matches /
`if_present` / fast-path machinery is format-agnostic. XML is
the conspicuous gap, and it is exactly where two large adopter
classes live:

- **Maven `pom.xml`** (spark: 49 of them): "every module's
  `<parent><version>` equals the reactor version", "no
  `<dependency>` without a `<version>` (or a managed one)",
  "`<maven.compiler.release>` is 17".
- **.NET project XML** (dotnet/runtime: ~2,300 `.csproj` /
  `.props` / `.targets`): "`<TargetFramework>` is `net8.0`",
  "every `<PackageReference>` has a `Version` attribute",
  "`<Nullable>` is `enable`". The `dotnet@v1` bundled ruleset
  (#10) is blocked on this.

There is no XML equivalent today, and `file_content_matches` on
raw XML text is the wrong tool (whitespace, attribute order,
comments, CDATA all defeat a line regex). This completes the
family: JSON / YAML / TOML / **XML**, one query language.

## Surface area

Two new rule kinds, `xml_path_equals` and `xml_path_matches`,
registered exactly like the existing six. `version: 1`
unchanged. Identical option surface to the rest of the family
(`paths`, `path`, `equals` | `matches`, `if_present`):

```yaml
- id: csproj-targets-net8
  kind: xml_path_equals
  paths: "**/*.csproj"
  path: "$.Project.PropertyGroup.TargetFramework"
  equals: "net8.0"
  level: error

- id: pom-deps-are-versioned
  kind: xml_path_matches
  paths: "**/pom.xml"
  path: "$.project.dependencies.dependency[*].version"
  matches: '^\d'
  level: warning

- id: every-packageref-has-a-version
  kind: xml_path_matches
  paths: "**/*.csproj"
  path: "$.Project.ItemGroup.PackageReference[*]['@Version']"
  matches: '.'
  level: error
```

Implementation is a `Format::Xml` variant added to the existing
`structured_path.rs` `Format` enum plus two thin builder
wrappers (`xml_path_equals_build` / `xml_path_matches_build`)
that mirror `toml_path_*_build`. The only new logic is
`Format::Xml.parse(text) -> serde_json::Value`. Everything
downstream — `JsonPath::query`, `check_match`, `if_present`,
the literal-paths fast path, parse-error-as-violation — is
reused verbatim.

## The XML → `serde_json::Value` mapping (the core decision)

XML has no native arrays, no scalar types, and a namespace
model JSON lacks. The mapping is the well-understood
**xmltodict** convention, chosen because it is predictable and
the JSONPath a user writes reads like the XML they see:

1. An element's value is built into a JSON object:
   - each **attribute** → key `@name` → its value (a string);
   - **child elements**: key = the child's local name. One
     occurrence → that child's value; **repeated siblings of
     the same name → a JSON array** in document order (so
     `<dependency>` ×N → `dependency: [ … ]`, queryable as
     `dependency[*]`);
   - non-whitespace **text** alongside attributes/children →
     key `#text`.
2. **Leaf shortcut:** an element with text but **no attributes
   and no child elements** maps directly to its text *string*
   (`<TargetFramework>net8.0</TargetFramework>` →
   `"net8.0"`), so the common case queries cleanly
   (`$.Project.PropertyGroup.TargetFramework`).
3. The document maps to `{ <root-element-name>: <root value> }`
   — the root element name is the first path segment
   (`$.Project…`, `$.project…`).
4. A truly empty element (`<X/>`, no text/attrs/children) →
   `null`.

**Namespaces are flattened to the local name**
(namespace-insensitive). `<project xmlns="http://maven.apache.org/POM/4.0.0">`
→ key `project`; `<modelVersion>` → `modelVersion`; a prefixed
`<ns:foo>` → `foo`. This is deliberate (Open question 1): it is
the entire reason Maven POM queries "just work" without the user
encoding namespace URIs into every JSONPath. The cost is
documented in the false-positive surface.

**All scalar values are strings.** XML is untyped; no `4.0.0` →
number, no `true` → bool, no `8` → integer guessing. This is the
single loudest gotcha and is documented in the rule docs and
mirrors the existing JSONPath dashed-key bracket-notation
ergonomics note.

### Parser dependency

`roxmltree` (`MIT OR Apache-2.0`; its only dep `xmlparser` is
the same — both inside the cargo-deny allow-list, no `deny.toml`
change). Chosen over `quick-xml`: `roxmltree` parses to a
read-only DOM in one call, so the XML→`Value` recursion is ~30
lines over `node.children()` (clippy-clean, well under the
100-line cap), versus `quick-xml`'s stateful Start/End/Text
event loop. Read-only, no codegen, no `unsafe` in our use,
small dep tree. Added as a workspace dependency, consumed by
`alint-rules` only.

### Recursion bound (security)

`xml_to_value` recurses once per nesting level. `roxmltree`'s
default `ParsingOptions` bounds total node count but **not**
nesting depth, and the other structured formats' parsers
(`serde_json` / `serde_yaml` / `toml`) carry library-internal
recursion limits that the XML arm would otherwise lack. A
crafted or accidental deeply-nested document
(`<a><a>…</a></a>` ×N, a few hundred KB) would overflow the
stack and **abort the whole `alint` process** — an unrecoverable
Rust `abort()`, not a catchable panic, with no per-file
isolation — reachable from any passively-linted repo the moment
an `xml_path_*` rule or the `dotnet@v1` ruleset (which targets
`**/*.csproj`) is active. (XXE / billion-laughs is *not* a
vector: `roxmltree` defaults `allow_dtd: false` + a loop
detector; depth recursion is the only XML DoS.)

**Resolved (v0.10 post-audit P1):** `element_to_value` carries
an explicit `depth` and refuses to descend past
`MAX_XML_DEPTH` (256 at v0.10, lowered to 128 in the v0.16
pre-release hardening for a wider overflow margin on 1 MB test /
musl stacks — still orders of magnitude beyond any real
`.csproj` / `pom.xml`, far below the overflow depth). Past the
bound it returns an `Err` that flows through the **existing**
parse-error path: one ordinary "not a valid XML document: XML
nesting exceeds the maximum supported depth (128)" violation
for that file, per-file contained, no abort. This brings the
XML arm to the same hardening posture the other formats already
have.

## Semantics

Identical to the rest of the structured-query family (it *is*
the family, with one more `Format`):

- Multiple matches ⇒ every match must satisfy the op; each
  failing match is one violation at that file.
- Zero matches ⇒ one "path produced no match" violation, unless
  `if_present: true` (then silent).
- `xml_path_matches` on a non-string match ⇒ the family's clear
  "value at path is not a string" violation (only relevant for
  object/array matches, since every XML *leaf* is a string).
- Unparseable / not-well-formed XML ⇒ one parse-error violation
  per file (same as bad JSON/YAML/TOML — surfaced, not skipped).
- Per-file dispatch + the literal-paths fast path are inherited
  unchanged (critical at the dotnet/runtime ~2,300-manifest
  scale; `paths: "**/*.csproj"` is a glob so it takes the
  normal scoped scan, same class as the other family kinds).

## False-positive surface

- **Type is always string.** `equals: 4.0.0` (parsed as a
  YAML float by the config loader) will never equal
  `<modelVersion>4.0.0</modelVersion>` (→ `"4.0.0"`). Document
  loudly: quote the expected value (`equals: "4.0.0"`) or use
  `xml_path_matches`. Booleans/ints likewise.
- **Namespace flattening.** Two children with the same local
  name but different namespaces become one (merged into the
  array). None of the demand sources do this (POM and .NET
  project XML don't sibling-collide across namespaces); a
  namespace-aware mode is Open question 1. Documented.
- **Mixed content** (`<p>text <b>bold</b> more</p>`) collapses
  the loose text into `#text` and loses interleaving order.
  Config/manifest XML (the entire demand) is element/attribute
  data, not prose markup; documented as out of scope, not a
  silent wrong answer.
- **Single vs. array.** A path with exactly one
  `<dependency>` maps it to an object (a string for a leaf
  element), not a 1-element array. So `dependency[*]` does NOT
  reliably paper over cardinality: over a single leaf it matches
  nothing, and `dependency[*]['@id']` / `dependency[*].child`
  read the members of the lone object instead of descending — so
  the single case is a silent miss while the many case works.
  The cardinality-independent idiom is recursive descent
  (`dependency..['@id']`, scoped under a parent so it does not
  over-match), used when a rule must handle both one and many;
  otherwise write the query for the data's actual shape. (This
  corrects the original note, which claimed `[*]` "works for one
  or many" — it does not for leaf elements or child/attribute
  descents.)
- **`@`/`#text` collision.** An XML attribute literally named
  `text` is `@text`; a child element named `#text` cannot occur
  (`#` is not a valid XML name start char), so the sentinel is
  safe. Noted for completeness.
- **Comments / PIs / CDATA.** Comments and processing
  instructions are dropped; CDATA is treated as text. Standard,
  documented.

## Implementation notes

- `crates/alint-rules/src/structured_path.rs`: add
  `Format::Xml`; `parse` gets an `Xml` arm calling a new
  `xml_to_value(text) -> std::result::Result<Value, String>`
  (errors stringified like the others); `label()` → `"XML"`;
  `detect_from_path` → `xml` / `csproj` / `props` / `targets` /
  `vbproj` / `fsproj` / `nuspec` (kept correct since it's
  `pub(crate)`, though the `xml_path_*` builders pass
  `Format::Xml` explicitly like the rest of the family).
- `xml_to_value`: `roxmltree::Document::parse`, then a
  recursive `element_to_value(node)` implementing the
  convention above. Pure function, no `Context`, no I/O.
- Two builders: `xml_path_equals_build` →
  `build_equals(spec, Format::Xml, "xml_path_equals")`;
  `xml_path_matches_build` likewise. Zero new option types —
  `EqualsOptions` / `MatchesOptions` are reused.
- Not spawn-capable (pure parse + query) — the
  `SPAWNING_RULE_KINDS` trust gate is N/A here, but the
  "does this kind spawn?" checklist item was still evaluated
  (see `feedback_spawn_kinds_must_be_gated`).
- No `include_str!`, nothing leaves the crate.

## Tests

- `.csproj`: leaf element (`$.Project.PropertyGroup.TargetFramework`
  `== "net8.0"`); attribute
  (`PackageReference[*]['@Version']` matches `\d`); `equals`
  pass + mismatch + missing-path.
- `pom.xml` with the Maven default namespace: namespace-flattened
  query works (`$.project.modelVersion == "4.0.0"`); repeated
  `<dependency>` → array, `dependency[*].artifactId` matches.
- **Recursion bound:** a deeply-nested document (nesting past
  `MAX_XML_DEPTH`) ⇒ exactly one parse-error violation for that
  file, **no process abort** (regression test for the P1).
- `if_present: true` silences a missing path; not-well-formed
  XML ⇒ one parse-error violation; empty element ⇒ `null`
  (a non-string-match message under `xml_path_matches`).
- String-typing gotcha: `equals: "8"` matches `<n>8</n>`, a
  bare `equals: 8` does not (regression-guards the documented
  behaviour).
- Lockstep with the codebase invariants (same checklist #1–#6
  followed): both kinds registered (+ both in the registry
  test list); `rule_xml_path_equals` / `rule_xml_path_matches`
  `$def` + dispatch `$ref` in both mirrored `config.json`
  (mirroring the `toml_path_*` defs); two `all_kinds.yaml`
  entries; regenerated default-options snapshot; the two
  multi-kind `docs/rules.md` H3 headings extended to include
  `xml_path_equals` / `xml_path_matches` (so
  `xtask docs-export --check` fans them out and stays green);
  rule count **+2** (76 → 78) across README ×2 /
  `docs/site/about` / `coverage_audit_readme_claims` — the
  first v0.10 item that moves the count by 2, not 1;
  `coverage_audit_*` audits; CHANGELOG `[Unreleased]` Added
  (the seventh v0.10 item, explicitly a two-kind addition).
- **Bench-compare threshold:** XML parse is O(file) like the
  other formats; per-file dispatch + literal fast path
  inherited. Full-run S-class wall must not regress vs the
  pre-phase baseline (`xtask bench-gate`, per `RELEASING.md`).

## Open questions

Resolve inline when implementation lands.

1. **Namespace-aware mode.** v0.10 flattens to local names
   (makes POM/.csproj queries ergonomic — the demand).
   A future `xml_namespaces: true` (or a `{uri}local` key
   convention) for repos that genuinely sibling-collide across
   namespaces; deferred until a demand source needs it (none of
   the 2 do).
2. **Convention knob.** xmltodict-style is the default and only
   mode. Alternatives (Parker, BadgerFish) drop attributes or
   restructure; not worth a knob until asked. The `@`/`#text`
   sentinels are fixed in v0.10.
3. **Type coercion.** Everything is a string. An opt-in
   `xml_coerce_scalars:` (numeric/bool inference) is a v0.11
   ergonomics call; v0.10 stays predictable and documents
   "quote your `equals:`".
4. **Parser.** `roxmltree` (read-only DOM, MIT/Apache, tiny).
   Revisit only if a demand source needs streaming for a
   single huge XML file (none do; the scale is *many small*
   manifests, which per-file dispatch already handles).
5. **Empty element.** `<X/>` → `null`. Alternative `""` was
   considered (friendlier `equals: ""`); `null` is more honest
   and `if_present` already covers "may be absent". Documented.
