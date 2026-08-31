# Design doc: config-format coverage parity

Status: Draft. (Draft | Implemented in <commit> | Superseded by <doc>.)
Decisions: proposes ADR-0016 (`Format` is the single source of truth for config-format coverage; format-specific surfaces must cover every `Format` variant, enforced by a parity gate).
Demand evidence: [issue #212](https://github.com/asamarts/alint/issues/212). Surfaced by the `yaml_path_absent` kind shipping without its json/toml/xml siblings (fixed in #211 by the `structured_family_is_symmetric` gate); this doc generalizes that fix to the rest of the codebase.

## 1. Problem

alint reads four config formats (JSON, YAML, TOML, XML) into one `serde_json::Value` tree through a single entry point, `structured_format::Format::parse`, so a single RFC 9535 JSONPath engine reasons about one tree shape. That design is sound. The problem is that **format coverage is uneven across the features built on top of it.**

XML is fully wired into the structured-query rule family (all 12 `{json,yaml,toml,xml}_path_{equals,matches,absent}` kinds, guarded by the `structured_family_is_symmetric` test). But it is missing everywhere else a format is named:

- The **extract layer** (`alint-core/src/extract.rs`), shared by `cross_file`, `file_graph`, and `registry_paths_resolve`, offers `extract: { toml | json | yaml | lines | regex | whole_file }`. There is no `xml:`, even though the `structured()` helper it dispatches to already calls `Format::parse` and is entirely format-agnostic. A user who can write `xml_path_equals` cannot write `extract: { xml: "$.Project.PropertyGroup.Version" }`, so a version-sync rule across `.csproj` files, or a reference graph over `pom.xml`, is impossible today.
- **`json_schema_passes`** carries its own `TargetFormat` enum (Json / Yaml / Yml / Toml) with no `Xml`, so `format: xml` cannot be requested. (Auto-detection from `.csproj`/`.xml` latently produces an XML target, but that path is undocumented and untested.)
- The **`did_you_mean` field hint** that turns a `matches:` typo on an `_equals` rule into "did you mean `equals`?" is wired for json/yaml/toml only, so `xml_path_*` mistakes fall through to a generic error.

The root cause is structural: there are **three parallel enumerations of the format axis**, each maintained by hand:

| enum | file | purpose |
|---|---|---|
| `structured_format::Format` | `crates/alint-core/src/structured_format.rs:19` | the parse axis (the SSOT), all four formats |
| `Extract` / `ExtractSpec` | `crates/alint-core/src/extract.rs:17,38` | extract structured modes, missing `xml` |
| `json_schema_passes::TargetFormat` | `crates/alint-rules/src/json_schema_passes.rs:47` | schema-validation target, missing `xml` |

Because they are independent, they drift. This is the same failure class as `yaml_path_absent` shipping without its siblings, one level up: a format added to `Format` does not automatically reach every surface, and nothing fails when it does not.

Two disambiguation notes so the doc is not misread: there are two *other* `enum Format` in the tree that are **out of scope** (`alint_output::Format`, the report serializer; `pair_hash::Format`, a digest-appearance flag). Neither is the config-parse axis. And `markdown_paths_resolve` is a filesystem-path scanner over markdown inline code, not a structured-format parser, so it is not part of this axis either.

## 2. Surface area

### 2.1 Master coverage table (current state)

Legend: J=JSON, Y=YAML, T=TOML, X=XML. `lines`/`regex`/`whole_file` are the format-agnostic extract modes.

| # | Surface | file:line | J | Y | T | X | Gap |
|---|---|---|:-:|:-:|:-:|:-:|---|
| 1 | `Format` enum (SSOT): `parse`/`label`/`detect_from_path` | `structured_format.rs:19` | Y | Y | Y | Y | none |
| 2 | structured_path rule family (12 kinds) | `structured_path.rs:454`; `lib.rs:371` | Y | Y | Y | Y | none (symmetric; gated) |
| 3 | `Extract`/`ExtractSpec` (shared extract) | `extract.rs:17,38,157` | Y | Y | Y | **no** | **root gap** |
| 4 | `cross_file` (+ `cross_file_value_equals`) | `cross_file.rs:64,74`; `lib.rs:429` | Y | Y | Y | **no** | inherits #3 |
| 5 | `registry_paths_resolve` | `registry_paths_resolve.rs:79,325` | Y | Y | Y | **no** | inherits #3 + `exclude_query` fallback misroutes XML to TOML (`:325`) |
| 6 | `file_graph` (`edges.from_content.extract`) | `file_graph.rs:82`; `lib.rs:431` | Y | Y | Y | **no** | inherits #3 |
| 7 | `json_schema_passes` (`TargetFormat`) | `json_schema_passes.rs:47` | Y | Y | Y | latent | `format: xml` unrequestable; auto-detected XML validated but undocumented/untested |
| 8 | `did_you_mean` field hint | `did_you_mean.rs:69` | Y | Y | Y | **no** | `xml_path_*` typos get a generic error |

Surfaces 1 and 2 are already complete. Surfaces 3 through 8 are the work.

### 2.2 The YAML the user gains

```yaml
# cross_file / file_graph / registry_paths_resolve extract, new `xml:` mode:
- id: csproj-versions-in-sync
  kind: cross_file
  source:  { file: "Directory.Build.props", extract: { xml: "$.Project.PropertyGroup.Version" } }
  targets: { files: "src/*/*.csproj", extract: { xml: "$.Project.PropertyGroup.Version" } }
  relation: equals
  level: error

# json_schema_passes against an XML target:
- id: csproj-shape
  kind: json_schema_passes
  paths: "**/*.csproj"
  format: xml            # today rejected; the schema is applied to the xml->Value tree
  schema: ".alint/csproj.schema.json"
  level: warning
```

No new option *keys* are introduced beyond the `xml:` extract mode and the `xml` value for the existing `format:` field. The change is additive and backward compatible.

## 3. Semantics

### 3.1 Extract via `Format::Xml`

`extract.rs`'s `structured(fmt, query, text)` already does `fmt.parse(text)` then runs the JSONPath and keeps string-valued matches. Adding `Extract::Xml(q) => structured(Format::Xml, q, text)` reuses that path verbatim. XML leaves are always strings under the xmltodict mapping, so an XML extract yields string values (a `.csproj` `<Version>1.2.3</Version>` extracts as `"1.2.3"`), exactly what `cross_file`/`file_graph`/`registry_paths_resolve` consume. Dispatch class is unchanged (cross-file, pure-parse, no spawn).

### 3.2 The parity mechanism (the architectural core, ADR-0016)

The decision this doc asks to record: **`Format` is the single source of truth for the config-format axis, and every format-specific surface must handle every `Format` variant.** Two complementary enforcement layers:

1. **Structural (preferred where cheap):** derive the per-format handling from `Format` instead of re-listing it. `Extract`'s three structured arms are a hand-copy of `Format`'s json/yaml/toml variants; they can instead iterate `Format`. `TargetFormat` in `json_schema_passes` exists only to add a `yml` spelling alias and (accidentally) drop `xml`; it should be folded into `Format` plus an alias table, collapsing three enums toward one.
2. **A parity gate (backstop):** a registry/enum test, in the spirit of `structured_family_is_symmetric`, that asserts each format-specific surface covers all `Format` variants. This catches a hand-maintained surface (like `did_you_mean`) that a macro does not reach. It is the same mutation-proven pattern: remove a variant, the test fails.

### 3.3 `json_schema_passes` and XML

A JSON Schema validates any `serde_json::Value`, and XML already maps to one. So `format: xml` is a matter of adding the enum variant and its `Format::Xml` mapping. The caveat (section 4) is that the XML tree is stringly-typed, so a schema asserting `type: integer` will not match a stringified leaf; this must be documented, not silently surprising.

## 4. False-positive surface

- **XML's lossy, stringly-typed tree.** Every XML leaf is a string, attributes are `@name`, repeated elements are arrays, namespaces are flattened (documented at `structured_format.rs:6-13`). An `xml:` extract or an `xml` schema target inherits all of that. A `cross_file` `equals` over two XML leaves compares strings, which is correct for versions; a `json_schema_passes` `type: number` assertion over XML will always fail. Mitigation: the existing `xml_path` docs already cover this; the design doc and rule docs must cross-reference them for the new surfaces, and the `format: xml` docs must state the string-typing rule explicitly.
- **`registry_paths_resolve` exclude-query misroute.** The `exclude_query` fallback at `registry_paths_resolve.rs:325` maps Json/Yaml/else->Toml; an XML registry's `exclude_query` would parse as TOML and silently drop everything. This is a real bug that the XML work must fix in lockstep (add the `Xml` arm), not a new risk introduced by leveling.
- **No new false positives in the extract path itself.** Because `structured()` is unchanged and XML parsing is the same code the rule family already ships, the `xml:` extract mode cannot behave differently from `xml_path_*`, which is corpus-tested.

## 5. Implementation notes

### 5.1 Blast radius for XML (Phase 1)

The audit confirms parsing is centralized, so this is mechanical, not parser work:

- `crates/alint-core/src/extract.rs` (the only logic file): add `Xml(String)` to `Extract`; `xml: Option<String>` to `ExtractSpec`; the `xml` arm to `resolve()`, `From<Extract>`, and `extract_values` (`=> structured(Format::Xml, q, text)`). `structured()` needs zero changes. This single file propagates XML to `cross_file`, `file_graph`, and `registry_paths_resolve`.
- `crates/alint-rules/src/registry_paths_resolve.rs:325`: add the `Extract::Xml` arm to the `exclude_query` fallback.
- `crates/alint-rules/src/json_schema_passes.rs:47,264`: add `Xml` to `TargetFormat` and its `Format` mapping (or fold `TargetFormat` into `Format`; see 5.2).
- `crates/alint-rules/src/did_you_mean.rs:69`: add the `xml_path_equals`/`xml_path_matches` arms.
- Regen: `xtask gen-schema` (the `ExtractSpec` schemars derive refreshes `schemas/v1/config.json` at `:497,:1625,:3404,:3433`), plus the Rust doc-comments that enumerate "toml/json/yaml" in `extract.rs`, `file_graph`, and `registry` module docs. `gen-facts` if the kind/mode inventory is counted.

No new dependency. `xml_to_value` stays private; reuse is through the already-public `Format::Xml.parse()`.

### 5.2 Collapsing the three enums (the durable fix)

Adding XML variant-by-variant closes today's gap but leaves the drift risk. The durable change is to reduce the three parallel enums to one authority:

- Derive `Extract`'s structured arms from `Format` (iterate variants) rather than hand-listing json/yaml/toml.
- Fold `TargetFormat` into `Format` + an alias map (its only non-redundant job is the `yml` spelling).
- Keep `did_you_mean` hand-maintained but gate it with the parity test.

Constitution note: the constitution requires every registered kind to have firing and silent scenarios (section 8) and the structured family to stay symmetric; ADR-0016 extends that invariant to the extract and schema-target surfaces.

### 5.3 New formats (Phase 2, evaluation)

The single plug-in seam is one arm in `Format::parse` + `label` + `detect_from_path`. Once a format yields the shared `Value` tree it flows into the 12 structured_path kinds (add the three `<fmt>_path_*` builders), `json_schema_passes` (free via detect), and extract (add the `Extract::<Fmt>` variant, or free if 5.2 lands). Evaluation of candidate formats:

| format | tree fit | parser | effort | demand | recommendation |
|---|---|---|---|---|---|
| dotenv (`.env`) | flat key=value -> flat object of strings | trivial / hand-written | low | high (ubiquitous) | strong candidate first |
| Java properties | key=value, dotted keys | `java-properties` crate or hand-written | low-med | moderate (JVM) | candidate; decide flat vs dotted-nesting mapping |
| INI / `.cfg` | sections -> nested map | `rust-ini` / `serde_ini` | medium | moderate | candidate; needs duplicate-key + section conventions |
| HCL (Terraform) | labeled/repeated blocks + attrs | `hcl-rs` | high | high (infra) | defer or scope separately; block semantics need XML-level mapping design |

Each new format needs the same up-front `Value`-mapping design XML got (type coercion, key conventions), which is where the real cost is, not the parsing. Recommendation: do not add a new format speculatively. Add one only behind a case-study or issue demand signal, dotenv first if any, and land 5.2 before the second format so the cost is one arm, not three.

## 6. Tests

- **Parity gate** (generalized `structured_family_is_symmetric`): assert `Extract` covers every `Format` structured variant and (if kept) `TargetFormat` maps every `Format`; mutation-proof it (remove a variant -> fail).
- **e2e scenarios** for each newly XML-covered surface, firing and silent, per constitution 8 and ADR-0014 (executed-fixture examples): `cross_file` with `extract: { xml: ... }` (a `.csproj` version match and mismatch); `file_graph` over an XML reference; `registry_paths_resolve` with an XML registry and an `exclude_query`; `json_schema_passes` `format: xml` (valid and invalid `.csproj`).
- **Unit**: `extract_values` over `Extract::Xml`; the `registry` `exclude_query` XML arm; `did_you_mean` on `xml_path_*`.
- **Regression**: a test that the `registry_paths_resolve` XML `exclude_query` no longer parses as TOML.

## 7. Open questions

1. **Mechanism weighting.** Derive-from-`Format` (structural) for `Extract`, fold `TargetFormat` into `Format`, and keep the parity gate as the backstop, vs. gate-only (leave the three enums, just add a test that fails on drift). Recommendation: do both structural collapses and the gate; the gate alone leaves the hand-copy that caused the gap. Resolve inline when the work lands.
2. **Latent XML in `json_schema_passes`.** Auto-detected `.csproj` targets are validated today with no `format: xml` and no tests. Make it explicit and tested now, or gate XML behind an explicit `format:` to remove the surprise? Recommendation: document and test the existing behavior; add explicit `format: xml`.
3. **New-format scope.** Do we add the full structured_path family (`<fmt>_path_*`, 3 kinds) for a new format, or only the extract mode, when demand appears? A new format in the rule family is a larger, kind-count-bumping surface than an extract mode. Recommendation: extract mode first, rule kinds only on separate demand.
4. **Prioritization.** Ship Phase 1 (XML leveling + parity gate) on its own; treat Phase 2 (new formats) as demand-gated and out of this change. Confirm.
