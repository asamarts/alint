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
| 8 | `did_you_mean` equals/matches hint | `alint-core/src/did_you_mean.rs:69` | Y | Y | Y | **no** | `xml_path_*` typos get a generic error, no "did you mean" |

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

The decision this doc asks to record: **`Format` is the single source of truth for the config-format axis, and every format-specific surface must cover every `Format` variant.**

The runtime is already uniform: `extract.rs`'s `structured()` takes any `Format`, and all parsing flows through `Format::parse`. The drift lives entirely in the config-facing *enumerations* that name formats by hand: `ExtractSpec`'s `toml`/`json`/`yaml` fields, the `<fmt>_path_*` kind names, `TargetFormat`'s variants, and the `did_you_mean` arms. serde needs an explicit field per format and the registry needs an explicit kind, so these cannot be *fully* auto-derived. The enforceable guarantee is therefore a parity gate, with an optional structural cleanup on top.

1. **Parity gate (the guarantee).** Add a canonical `Format::ALL` list to `structured_format.rs` (the SSOT owning its own inventory), then one test per surface asserting it covers `Format::ALL`, in the spirit of `structured_family_is_symmetric` (which already asserts the 12 kinds cover 4 formats x 3 ops). Concretely: `ExtractSpec` exposes a structured field for every variant (read from its schemars properties, case-folded against `Format::label()`, which is uppercase); `TargetFormat` maps every variant; the `did_you_mean` equals/matches arms cover every variant. Each is mutation-proof: drop a variant's coverage and the test fails. This reaches the hand-maintained surfaces a macro cannot.
2. **Structural cleanup (bounded, optional).** The `Extract` *runtime* enum can collapse its three structured variants into one `Structured(Format, String)`, since dispatch already just calls `Format::parse`; and `TargetFormat` (whose only non-redundant job is the `yml` spelling alias) can fold into `Format` plus an alias table. This shrinks the per-format edit surface but does not remove the serde-level config fields, which the gate still guards. It is a cleanup, not the guarantee.

### 3.3 `json_schema_passes` and XML

A JSON Schema validates any `serde_json::Value`, and XML already maps to one. So `format: xml` is a matter of adding the enum variant and its `Format::Xml` mapping. The caveat (section 4) is that the XML tree is stringly-typed, so a schema asserting `type: integer` will not match a stringified leaf; this must be documented, not silently surprising.

## 4. False-positive surface

- **XML's lossy, stringly-typed tree.** Every XML leaf is a string, attributes are `@name`, repeated elements are arrays, namespaces are flattened (documented at `structured_format.rs:6-13`). An `xml:` extract or an `xml` schema target inherits all of that. A `cross_file` `equals` over two XML leaves compares strings, which is correct for versions; a `json_schema_passes` `type: number` assertion over XML will always fail. Mitigation: the existing `xml_path` docs already cover this; the design doc and rule docs must cross-reference them for the new surfaces, and the `format: xml` docs must state the string-typing rule explicitly.
- **`registry_paths_resolve` exclude-query trap.** The `exclude_query` fallback at `registry_paths_resolve.rs:325` maps Json and Yaml explicitly and everything else (today Toml/Lines/Regex/WholeFile) to a TOML read. It is not a bug today because there is no `Extract::Xml`, but adding the `xml:` mode without a matching `Extract::Xml(_) => Extract::Xml(q)` arm here would silently parse an XML registry's `exclude_query` as TOML and drop every exclusion. The XML work must add that arm in the same change; a regression test locks it.
- **No new false positives in the extract path itself.** Because `structured()` is unchanged and XML parsing is the same code the rule family already ships, the `xml:` extract mode cannot behave differently from `xml_path_*`, which is corpus-tested.

## 5. Implementation notes

### 5.1 Blast radius for XML (Phase 1)

The audit confirms parsing is centralized, so this is mechanical, not parser work:

- `crates/alint-core/src/extract.rs` (the shared extract layer, the primary change): add `Xml(String)` to `Extract`; `xml: Option<String>` to `ExtractSpec`; the `xml` arm to `resolve()`, `From<Extract>`, and `extract_values` (`=> structured(Format::Xml, q, text)`). `structured()` needs zero changes. This single file propagates XML to `cross_file`, `file_graph`, and `registry_paths_resolve`.
- `crates/alint-rules/src/registry_paths_resolve.rs:325`: add the `Extract::Xml` arm to the `exclude_query` fallback.
- `crates/alint-rules/src/json_schema_passes.rs:47,264`: add `Xml` to `TargetFormat` and its `Format` mapping (or fold `TargetFormat` into `Format`; see 5.2).
- `crates/alint-core/src/did_you_mean.rs:69`: add `xml_path_equals`/`xml_path_matches` to the equals/matches confusion match arms.
- Regen + hand edits: `xtask gen-schema` refreshes the `ExtractSpec`-derived `$def` in `schemas/v1/config.json` (`:497`); the other three "toml/json/yaml" enumerations (`:1625` file_graph, `:3404` / `:3433` registry) are hand-written doc-comments on those structs and must be edited by hand, alongside the "toml/json/yaml" prose in the `extract.rs`, `file_graph`, and `registry` module docs. `gen-facts` if the kind/mode inventory is counted.

No new dependency. `xml_to_value` stays private; reuse is through the already-public `Format::Xml.parse()`.

### 5.2 Collapsing the three enums (the durable fix)

Adding XML variant-by-variant closes today's gap but leaves the drift risk. The durable change is the parity gate from 3.2 (a `Format::ALL`-covered test per surface); it is what actually prevents the next format from shipping half-wired. The bounded structural cleanup rides on top of it:

- Collapse `Extract`'s four structured runtime variants (json/yaml/toml/xml) into `Structured(Format, String)` (dispatch already calls `Format::parse`). The `ExtractSpec` config fields stay per-format (a serde requirement) but are now guarded by the gate. **As built (Phase 1c): done.** It also lets `registry_paths_resolve`'s `exclude_query` fallback reuse the registry's own format via one `Extract::Structured(fmt, _) => Extract::Structured(*fmt, q)` arm instead of an explicit per-format match with a TOML default, which removes the "a new format's structured `exclude_query` silently parses as TOML" trap class outright.
- Fold `TargetFormat` into `Format` + an alias map (its only non-redundant job is the `yml` spelling). **As built (Phase 1c): kept `TargetFormat`, deliberately** (decision recorded 2026-08-31). `Format` derives no `Deserialize`/`JsonSchema` today; folding `TargetFormat` in would add both to the core enum, and because schemars does not emit serde `alias`es, `yml` would drop out of the published config-schema enum unless re-added via field-level schema attributes that just re-express `TargetFormat`. The parity gate (`target_format_covers_every_format`) already makes `TargetFormat` drift-safe, so the fold is a lateral move; `TargetFormat` stays as the clean expression of "`Format`'s labels plus the `yml` alias".
- Leave `did_you_mean` hand-maintained; the gate covers it.

Constitution note: the constitution requires every registered kind to have firing and silent scenarios (constitution section 8) and the structured family to stay symmetric; ADR-0016 extends that invariant to the extract and schema-target surfaces.

### 5.3 Phase 1.5: dotenv (full wiring)

dotenv is greenlit alongside the XML work: it is cheap and ubiquitous, and it exercises the `Format::ALL` seam end to end. Because a `Format` variant is all-or-nothing under the parity gate, `Format::Dotenv` is wired on every surface:

**As built (Phase 1.5, decision 2026-09-01): a hand-rolled parser, NOT `dotenvy`.** The plan below assumed `dotenvy` could return literal values with substitution off, but `dotenvy` 0.15.7 expands `${VAR}` / `$VAR` **unconditionally** (confirmed in its `parse.rs`; only single-quotes / `\$` suppress it per value, with no global toggle), which is environment-dependent and wrong for static analysis. So alint ships a small, dependency-free literal `.env` parser (`crates/alint-core/src/dotenv.rs`) instead of `dotenvy`: zero new dependencies, which also fits the supply-chain stance. Follow-ups logged: release it as its own crate (a literal-values `dotenvy` alternative), and/or contribute a no-substitution mode upstream to `dotenvy`. Wrinkle (b) in the first bullet (the `dotenvy` substitution-off mapper) is superseded by this; everything else in the section stands as built.

- `Format`: add `Dotenv`, a `.env` arm in `parse` (via `dotenvy` 0.15, MIT, the maintained fork of the RUSTSEC-2021-0141 `dotenv`), and a `label`. Two dotenv-specific wrinkles: (a) detection cannot reuse `detect_from_path`'s extension match, because a bare `.env` has no extension (and `.env.local` has extension `local`), so it needs a filename branch (`.env`, `.env.*`); (b) `dotenvy` substitutes `${VAR}` by default, but a linter must keep values literal (deterministic, environment-independent), so the ~10-line mapper reads the raw pairs into a flat `Value::Object` of strings with substitution off. Values are not coerced (stringly-typed, like XML).
- structured_path: three `dotenv_path_{equals,matches,absent}` builders + registrations. **Kind count 93 -> 96.** Flat keys make these useful: `dotenv_path_equals $.NODE_ENV == "production"`, `dotenv_path_absent $.AWS_SECRET_ACCESS_KEY`.
- extract (`Structured(Format::Dotenv, q)`), `json_schema_passes` (`format: dotenv`), and the `did_you_mean` arms.
- The full drift sweep the XML/absent work already exercised: schema branches + dispatch, all_kinds fixture, facts (96), a docs H3 + fail/pass `docs:` scenarios per kind, README/about counts, C4 model, options snapshot.

Complexity: low. One new dependency, `dotenvy` (MIT, cargo-deny-clean). The parity gate turns "wire it everywhere" into a checklist, not a judgment call, which is exactly why dotenv is a safe first exercise of the seam.

### 5.4 New formats (Phase 2, demand-gated)

The remaining candidates stay out of this change and are added only behind a concrete case-study or issue demand signal. **As built (Phase 2, 2026-09-01): Java `.properties` shipped first** (demand: user greenlight), using the `java-properties` crate (the doc's vetted-crate approach was chosen over hand-rolling, since unlike dotenvy it returns literal values; verified). `Format::Properties` + `properties_path_*` (kinds 96 -> 99) wired on every surface behind the parity gate; flat literal-string map, dotted keys stay one key. INI and HCL remain demand-gated below. The seam is the same as dotenv's: one arm in `Format::parse` + `label` + `detect_from_path`, then the parity gate makes the rest a checklist. All have a maintained, cargo-deny-clean crate; the cost is the up-front `Value`-mapping design, not the parsing.

| format | leading crate (license) | value fit | effort | biggest mapping risk |
|---|---|---|---|---|
| Java `.properties` | `java-properties` 2.0 (MIT) | flat `HashMap<String,String>` to a flat Object | low | dotted keys (`a.b.c`) are ONE opaque key in Java; flat is faithful, nesting-on-`.` diverges and hits alint's JSONPath dotted-key bracket quirk |
| INI / `.cfg` | `ini` (rust-ini) 0.21 (MIT) | 2-level section-key map | low-med | duplicate keys (rust-ini can keep them as an array; avoid `configparser`, which lowercases keys and is last-wins) plus where the pre-section "global" keys live |
| HCL (Terraform) | `hcl` (hcl-rs) 0.19 (MIT/Apache) | `hcl::Value` is JSON-native, no mapper | med | a block type is an object when it appears once but an ARRAY when repeated (path shape depends on the file); `var.x` / `${...}` expressions arrive unevaluated as opaque strings |

Effort is driven by mapping semantics, not parsing: HCL has the easiest parse (native `Value`) but the leakiest projection, so it is `med`, not `high`. With 5.2's runtime collapse landed, each is one `Format::parse` arm plus its config fields, not three enums.

## 6. Tests

- **Parity gate** (per surface, against `Format::ALL`): `ExtractSpec` exposes a structured field for every variant; `TargetFormat` (if kept) maps every variant; the `did_you_mean` arms cover every variant; and the existing `structured_family_is_symmetric` covers the rule kinds. Each is mutation-proofed (drop a variant's coverage -> fail).
- **XML e2e scenarios**, firing and silent, per constitution 8 and ADR-0014: `cross_file` with `extract: { xml: ... }` (a `.csproj` version match and mismatch); `file_graph` over an XML reference; `registry_paths_resolve` with an XML registry and an `exclude_query`; `json_schema_passes` `format: xml`, plus the auto-detected `.csproj` path with no explicit `format:` (locking the section 7, question 2 resolution).
- **dotenv e2e scenarios**: fail/pass for each of `dotenv_path_{equals,matches,absent}`, plus a `cross_file` `extract: { dotenv: ... }` and a `json_schema_passes` `format: dotenv`.
- **Unit**: `extract_values` over `Extract::Xml` and `Extract::Dotenv`; the `registry` `exclude_query` XML arm; `did_you_mean` on `xml_path_*` / `dotenv_path_*`; the dotenv mapper (flat object, literal `${VAR}`).
- **Regression**: the `registry_paths_resolve` XML `exclude_query` no longer parses as TOML.

## 7. Open questions

All four resolved with the maintainer (2026-08-31):

1. **Mechanism weighting.** **Resolved: gate + bounded cleanup.** The `Format::ALL` parity gate is the guarantee; on top of it, collapse `Extract`'s runtime to `Structured(Format, q)` and fold `TargetFormat` into `Format`, so a future format is one code path (5.2).
2. **Latent XML in `json_schema_passes`.** **Resolved: document and test it, add explicit `format: xml`.** No breaking change; the accidental auto-detect path (`.csproj` -> XML) becomes an intentional, tested capability.
3. **New-format scope.** **Resolved: a supported `Format` variant is wired on every surface**, enforced by the parity gate. Extract-only "partial formats" are not a maintained tier; adding a `Format` variant means adding it everywhere.
4. **Prioritization.** **Resolved: dotenv is greenlit as Phase 1.5** (full wiring, 5.3), shipped alongside the XML leveling. Java `.properties`, INI, and HCL stay demand-gated (5.4).
