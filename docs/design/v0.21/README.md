# v0.21: Internal relation IR (go / no-go)

Status: Accepted (2026-10-24). The eight decisions in "Decisions" are recorded in
[ADR-0023](../../adr/0023-relation-ir-key-parity-and-plugin-contract-0x.md); the corrections
review found are recorded in [ADR-0024](../../adr/0024-relation-ir-decisions-after-review.md),
which supersedes it and which this document follows.
Decisions it builds on: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md)
(DEC-2, DEC-5, DEC-7, DEC-8, DEC-10, DEC-13, DEC-14). Questions it decides: Q4, Q10, and Q13;
the port's results feed Q8, which is decided per kind later.
Program: [`../general-linter.md`](../general-linter.md), Phase 2 (section 11.5);
[`../plugin-architecture.md`](../plugin-architecture.md), slice P1.

v0.21 is the go/no-go proof for the general-linter program. It builds the internal relation layer
that Phase 3's selectors and constraints will later expose. It proves that layer on one new kind
and one shadowed port of an existing kind, and designs the plugin contract on the same types.
The only new user-facing surface is one rule kind: there is no `version: 2`, no new config
syntax, and no plugin host. For Rust callers, `alint-core` gains doc-hidden public modules with
no semver promise, `alint-rules` a crate feature, and `alint-testkit`'s runner an additive
`Runner::with_registry` (its `RunnerOptions` stays as it is).

It delivers four things together:

1. an **internal relation IR** in `alint-core`: resources, selectors, adapters, typed rows with
   provenance, relations, and limits;
2. **`key_parity`**, a new v1 kind and the first user of the IR (DEC-7): one reference file and
   a glob of peers must declare the same key set, including nested JSON and YAML locale files;
3. a **differential shadow harness** and the first port, `cross_file`'s `equals` relation (the
   path the `cross_file_value_equals` alias uses), with the existing implementation still
   authoritative;
4. the **plugin contract `0.x`** design: internal `Adapter` and `Analyzer` traits that the
   built-in row adapters and `key_parity` implement, an injectable registry, and a WIT draft.

`key_parity` also brings three new adapters: TOML rows, and the Android string-resource and
.NET `.resx` XML dialects (V21-3).

It also does the measurement work DEC-13 requires before any IR change can be judged.

## Why these belong in one release

An IR without a user is untested design. `key_parity` without the IR would be the standalone
evaluator DEC-8 rules out, and it would be replaced in Phase 3. The shadowed port is the only
evidence that the IR can carry existing semantics byte for byte, which is what "go" means. The
plugin contract has to be designed against the same row and provenance types, or it will leak
internal layouts across the plugin boundary (the failure mode `plugin-architecture.md` section
2 describes).

The stop rule from `general-linter.md` section 11.1 applies to this release. If the ported path
does not become simpler and more consistent, the program stops here; the IR, `key_parity`, and
the measurement work keep their standalone value.

## Starting point

Checked against the code at `6a75eb4d1` and rechecked at `fb9820f5e` (2026-10-24).

- **Extraction is string-only and per-consumer.** `ExtractSpec`
  (`crates/alint-core/src/extract.rs:92`) resolves to `Extract` (`:20`): `Structured` (RFC 9535
  JSONPath over the parsed `serde_json::Value`), `Lines`, `Regex` (capture group 1), or
  `WholeFile`. Results are strings plus a type tag for non-string matches (`ExtractedValues`,
  `:267`). Non-literal entries such as `${VAR}` are skipped with a note (`is_non_literal`, `:237`;
  `cross_file/eval.rs:298`). Each consumer enforces its own cardinality: `cross_file`'s `equals` requires exactly
  one source value (`crates/alint-rules/src/cross_file/eval.rs:310`). The users are `cross_file`
  and its `sync_from` fixer, `registry_paths_resolve`, `file_graph` content edges, manifest-derived
  scope, and the `file_content_matches` fact.
- **Spanned rows exist but nothing consumes them.** v0.19's duplicate-preserving
  `StructuredEntryRow` (`extract.rs:312`, built by `extract_entry_rows` at `:333`) covers JSON,
  YAML, dotenv, INI, and properties; TOML, XML, and HCL return `UnsupportedFormat`. Value spans
  resolve lazily through `structured_fix::resolve_value_span`
  (`crates/alint-core/src/structured_fix/mod.rs:50`) for every format but TOML. Two pointer
  types duplicate each other, and both are public: `PathSeg` (`structured_fix/mod.rs:38`) and
  `EntryPathSegment` (`extract.rs:275`, re-exported from the crate root).
- **`cross_file`** is `FullIndex { path_scope: false }`, with seven relations (`Equals`,
  `Subset`, `Superset`, `SetEquals`, `Identical`, `Resolves`, `Registered`;
  `cross_file/spec.rs:86`) and a 10,000-value cap (`cross_file/mod.rs:67`).
  `cross_file_value_equals` is one of the 11 aliases (`crates/alint-rules/src/lib.rs:810`).
- **Dispatch.** `KindMeta` declares four classes (`crates/alint-core/src/kind_meta.rs:17`); the
  102 canonical kinds split 56 per-file, 22 full-index, 14 rule-major scoped, and 10 single-shot.
  At run time there are two partitions: a rule-major parallel loop for every rule without a
  per-file form, and the file-major `run_per_file` loop
  (`crates/alint-core/src/engine/run.rs:714`) that shares one read per file (ADR-0003). Only
  per-file rules share that read: `cross_file` reads each file it extracts from on its own,
  straight from disk by its literal path, even when the index excludes it (`read_rel`,
  `eval.rs:923`).
- **Baseline identity** is frozen by the golden gate
  (`crates/alint-e2e/tests/fingerprint_golden.rs`): 526 fingerprints over the 485 scenario files
  plus the v1 bundled-ruleset tests (DEC-5).
- **The v0.12 corpus was not replayable.** Its 110 configs (111 repositories were studied; tokio's
  config lives in `examples/`) are in `docs/design/v0.12/corpus/`, pinned in
  `docs/design/v0.12/case_study_repos.md` by 10-character SHA prefixes, which GitHub will not
  fetch by. 51 configs use the `equals` path (90 rules: 89 `cross_file_value_equals` and one
  explicit `relation: equals`), and 20 contain rules that run commands (83 rules). Since R5,
  `corpus/pins.tsv` holds the
  full SHAs.
- **Performance gating does not meet DEC-13.** The deterministic gate
  (`crates/alint-bench/benches/det_check.rs`, scenarios S1-S3) is advisory in CI and compares
  each pull request with its merge base. `docs/benchmarks/ANCHOR` names v0.20.0 with a +10%
  budget, not v0.18.2 with +3%, and no instruction counts are stored for v0.18.2. Macro results
  for v0.18.2 exist (`docs/benchmarks/macro/results/linux-x86_64/v0.18.2/`). S4 cannot run on
  v0.18.2: it extends the `versioning@v1` and `toolchain-pins@v1` bundles, which arrived in
  v0.19, and it is a revisioned workload that `bench-gate` resets across revisions. S3 reads
  `crates/*/Cargo.toml` from at least three rules, but they are native rules, not relations, so
  no existing scenario exercises section 9.1's relation coalescing.
- **Plugin groundwork.** `witforge` (`asamarts/witforge`, private) has its subset checker and
  wire lockfile (`6ce6076`, 2026-10-24). Its Rust and protobuf emitters, slice P0, are not
  written. The contract subset forbids recursive types (`plugin-architecture.md` section 3.2).
  Every runtime path calls `builtin_registry()` directly (check, fix and validate-config, the
  LSP, `alint test`'s runner, `rules`, `init`, `suggest`, and `export-agents-md`), `RuleBuilder` is a function
  pointer, `Format` is a closed enum, and `ExtractSpec` has one field per format
  (`plugin-architecture.md` section 11).
- **`key_parity` has no code.** Flat-format parity is the cookbook's `env-key-parity` recipe, a
  `cross_file` `set_equals` over a `regex:` key extract.

## Scope

### Relation IR

A new module, `alint_core::relation`: `#[doc(hidden)] pub`, so `alint-rules` can build on it,
with no semver promise until the v1.0 freeze. It defines:

- **`Resource`**: a confined repository path with its origin: an index entry, or a literal path
  read from disk (as `cross_file` reads its source and target files, even ignored ones), with a
  resource-level missing state. Paths compare byte for byte after normalization on every
  operating system; case folding is an explicit per-comparison opt-in (V21-2, Q13).
- **`Selector`**: a compiled glob plus the existing scope gates, filtering the index. It decides
  which rows a relation holds; whether a rule runs under `--changed` is its dispatch, below.
- **`Adapter`**: turns a resource's bytes into a row stream. The built-in row adapters wrap
  `extract_entry_rows` and the existing parsers. A second built-in adapter wraps today's
  `Extract` (every format, plus regex, lines, and whole file) and emits its string values, type
  tags, and notes unchanged; the `equals` port uses it.
- **`Row` and `Value`**: `null`, boolean, a number that keeps its source lexeme, string, bytes,
  normalized path, lists, and records (V21-1), plus the original value where a transform changed
  it (section 6.2; `cross_file`'s messages quote both). Domain scalars such as `semver` are added
  later, once several kinds consume them: additive inside alint, but a new contract minor version
  for the WIT, where a new variant case is a breaking change (`plugin-architecture.md` section
  8.1). In memory a `Value` nests. The contract form is a flat node list with indices, because
  WIT has no recursive types; it is a std-only type in `alint_core::plugin_api`, and
  `alint_core::relation` converts to and from it.
- **`Relation`**: a finite, ordered multiset of rows. The canonical order is path (segment by
  segment, the walker's index order), structural pointer, label, and emission order, with the byte
  offset only as a tie-breaker where it is known: rows from the `Extract` wrapper have no pointer
  or offset. Conversion to a set is an explicit operation.
- **Provenance**: eager resource and structural pointer where the adapter has one. Adapters that
  know spans (entry rows, TOML through `toml_edit`, the XML dialects through node ranges) carry
  them eagerly. Rows from the `Extract` wrapper carry neither a pointer nor a span, exactly as
  `cross_file` reports today. One internal pointer type replaces both `PathSeg` and
  `EntryPathSegment` inside the layer; both public types stay, converting into it.
- **Absence and cardinality**: missing, explicit `null`, and an extraction error are distinct row
  states. Every extractor declares its cardinality. A parse failure or limit hit is an analysis
  failure under DEC-2, never an empty relation.
- **Limits** (section 9.5): rows per extractor, total rows per run, distinct values, and
  equality-join inputs and outputs, each with a default and a hard cap.

**Reads.** IR evaluation extends ADR-0003's guarantee to relations. A file selected by several
relations, or by relations and per-file rules, is read at most once per run, and decoded at most
once per (adapter, resource). R2 asserts it with a counting test over a test-only relation
consumer; R3 adds the relation scenario (`key_parity` and per-file rules over the same files),
which measures it by `read` syscall counts. S1-S4 contain no relations (S3's `cross_file` rules
are the authoritative, non-IR implementation), so they cannot.

### `key_parity`

```yaml
- id: locales-match-english
  kind: key_parity
  reference: locales/en.json
  peers: "locales/*.json"
  level: error
```

- **Options:**
  - `reference` (one path) and `peers` (a glob; the reference is excluded automatically);
  - `format` (inferred from the extension when omitted, see below);
  - `direction` (`both`, `missing` or `extra`; default `both`);
  - `plurals` (`exact` or `cldr`; default `exact`);
  - `strip_root` (default `false`): when every compared file has exactly one top-level key, as
    Rails locale files do (`en:`, `fr:`), compare below it;
  - `ignore_keys` (key-path globs: `*` within a segment, `**` across segments).
- **Formats (V21-3):**
  - The five row formats: JSON, YAML, dotenv, INI, and Java properties.
  - TOML, through a new lossless row adapter on `toml_edit`. `[nav] home = ...` and
    `nav.home = ...` are the same key. TOML forbids duplicate keys, so one is a parse failure.
  - Android string resources: `<string>`, `<string-array>`, and `<plurals>` elements, keyed by
    (element type, `name`), since a `<string>` and a `<plurals>` may share a name. Entries marked
    `translatable="false"` exist only in the default `values/` by design and are skipped.
  - .NET `.resx`: string `<data>` entries, keyed by `name`. Entries with a `type` or `mimetype`
    (non-string resources) and designer metadata (names starting with `>>` or `$this`) are
    skipped. .NET falls back to the neutral file for missing keys, so `direction: extra` is the
    usual setting for `.resx`.

  These are `key_parity`'s own format names, not new `Format` variants: `Format` is a closed
  public enum whose every variant all format-aware kinds must handle, and neither dialect is a
  general format. `key_parity`'s `KindMeta` lists only the general formats it reads (JSON, YAML,
  TOML, dotenv, INI, properties), so `facts.json` claims no generic XML support; the rules
  reference documents the two dialects. The extension picks the format, except for `.xml`:
  generic XML has no key model, so an `.xml` reference needs `format: android-strings`. HCL and
  any other format are rejected at config load. Files are compared only within one key model,
  and a mismatch is a config error: the tree formats (JSON, YAML, TOML) together, the flat
  formats (dotenv, properties, whose keys are single segments) together, and INI (`[section,
  key]`) and each XML dialect on their own. Both XML dialects parse through the existing guarded
  XML path (the `roxmltree` 0.20 pin and its depth pre-scan).
- **Key model (V21-4):** a key is a leaf path, and arrays are values that are never descended.
  A key's identity is its segment list, so `{"nav.home": 1}` and `{"nav": {"home": 1}}`, or a
  TOML quoted key `"a.b"` and the dotted `a.b`, are different keys. Messages, `ignore_keys`, and
  baseline keys use one rendering: segments joined by `.`, indices as `[n]`, and bracket notation
  (`nav['a.b']`) for a segment that is empty or contains `.`, `[`, `]`, or a quote, with `\`
  and `'` escaped inside the brackets, so distinct keys never render alike. Properties keys are
  single segments.
  YAML aliases and merge keys (`<<: *defaults`) are an analysis failure in a compared file:
  rows do not expand them, and comparing unexpanded keys would report false drift. With
  `plurals: cldr`, a CLDR plural group counts as one key:
  - an object whose keys are all CLDR categories (`zero`, `one`, `two`, `few`, `many`, `other`)
    and include `other` (Rails YAML);
  - sibling keys `<base>_<category>` that include `<base>_other` (i18next JSON).

  Android `<plurals>` is one key in either mode. Under `cldr`, only a wholly missing or extra
  plural key is reported. Checking each locale's exact category set needs per-locale CLDR data
  and is not in v0.21.
- **Findings:** one per missing key, at the peer file, with the reference key as a related
  location. One per extra key, at the peer key. Example message: "`locales/fr.json` is missing
  key `nav.home` (present in `locales/en.json`)". Keys appear in messages; values never do. The
  experimental trace preview (ADR-0022) shows `key_parity`'s rows as keys only, so no value
  reaches any output and the redaction rules (DEC-3) have nothing to redact.
- **Failures:**
  - A missing reference file is a finding that always counts as error (DEC-2).
  - A reference or a peer that fails to parse is an analysis failure for that file.
  - A `peers` glob that matches no file is an error-level finding: a misconfiguration, never a
    pass.
  - Duplicate keys within one file are not parity violations; they are the concern of the planned
    B1 `no_duplicate_keys` kind. Rows keep them, and parity compares distinct key paths.
  - The `reference`, and any `peers` entry without glob characters, is a literal path read from
    disk even when ignored, as `file_exists` treats literal paths and `cross_file` its files.
    Peer globs select from the index, which honours `.gitignore`. So `.env` parity is written
    `reference: .env.example` with `peers: .env`.
- **Dispatch and `--changed`:** `FullIndex { path_scope: true }` with the reference and the peers
  as its scope, the class `file_exists` and three other kinds already use: under `--changed` it is
  skipped unless a changed or created path matches the scope, and otherwise evaluates over the
  full index. The scope is the rule's whole context, so this does not narrow what it compares
  (section 9.1). Constitution invariant 2 forbids narrowing a full-index rule's evaluation; this
  only decides whether the rule runs. R3 tests that the scope covers both the reference and the
  peers, that a peer-only edit runs the rule, and that an unrelated edit skips it.
- **Fixes:** none in v0.21. ADR-0017 classifies key insertion as a Suggestion, and stub insertion
  waits for the generic edit plan.
- **Baseline key:** sorted identity fields (peer path, key path, direction), per DEC-5.
- **The usual kind obligations:** `KindMeta` and `TraceMeta`, family assignment, rules reference
  and site page, firing and silent scenarios, schema, facts, and every count claim (102 kinds
  become 103): the README, the site concepts page and its diagram, and the alint.org about page
  that its head-parity check reads.

### Shadow harness and the first port

- **The port:** `cross_file` with `relation: equals` (what `cross_file_value_equals` resolves to)
  gets a second implementation on the IR. It extracts through the `Extract` wrapper adapter, so
  it covers every format `cross_file` accepts and depends on no new adapter (R9). What it proves
  is the relation, comparison, finding, and baseline path on the IR; typed extraction for every
  format is not in v0.21. It emits findings in the existing implementation's order (the index
  order of its targets), not the relation's canonical order, so the comparison can be
  byte-identical. S3 runs `cross_file_value_equals`, so the v0.22 switch lands on the scenario
  with the least budget headroom.
- **Where the shadow runs:** a test-only `shadow-ir` feature runs both implementations and asserts
  byte-identical findings, messages, locations, notes, analysis failures, baseline fingerprints,
  fixability, and `sync_from` proposed edits. It runs in three places:
  - every scenario that uses `cross_file` `equals`;
  - a property-test generator over extract specs and `normalize:` lists;
  - the corpus configs that load on the current release (106 of 110 today; 49 of them carry 87
    `equals`-path rules), at their pinned commits.
- **Corpus replay:** `xtask corpus-replay` reads the full-SHA pins, fetches each repository at
  depth 1 into a cache outside the source tree (by default `target/corpus-replay`), and runs alint
  over it. Nothing from a corpus repository runs: spawning, keyring, and catalog-unknown rule kinds
  and spawning fix operations are removed first, a config with custom facts is refused, and alint
  gets a fresh `HOME` and an absolute-only `PATH`. `replay-expectations.tsv` lists the configs
  that do not replay (four today, holding three `equals` rules); any other skip, and any local
  fetch failure, fails the run. With `--compare` it runs a second binary and reports every
  repository whose JSON report (proposed edits included) or baseline fingerprints differ. Once R4
  lands, the shadow comparison runs inside a binary built with `shadow-ir`, and its differences
  fail the replay. It never runs per pull request.
- **Authority:** the existing implementation stays authoritative. Production never runs the
  shadow, so users pay nothing for it (V21-8): the shadow is compiled only under the `shadow-ir`
  feature, and there is no runtime flag. The feature is defined on `alint-rules` and enabled only
  from `alint-e2e`, which depends on `alint-rules` directly. It is not enabled from `xtask`:
  `bench-record.yml` builds `-p alint -p xtask` together, and feature unification would put the
  shadow into the benchmarked binary. Corpus replay builds its own binary with
  `--features alint-rules/shadow-ir`. A feature on a published crate is visible on crates.io, so
  "test-only" is enforced by a gate: CI checks the feature trees of the release builds (as
  `release-binary.sh` builds them) and of the bench builds, and fails if `shadow-ir` appears. The
  old code is deleted in v0.22, after one release with zero differences (DEC-7).
- **Where replay runs (V21-7):** a `corpus-replay` workflow on the canonical bench host's runner
  (`[self-hosted, linux, bench]`), dispatched before each release that contains IR changes and
  weekly on a schedule, with its cache on that host. The runner executes one job at a time, so a
  replay never overlaps a benchmark. `RELEASING.md` gains the pre-release step.
- **Not in v0.21:** the other six `cross_file` relations, the remaining `ExtractSpec` consumers,
  and manifest-derived scope. The port's results feed Q8.

### Plugin contract `0.x` (slice P1)

- **Module (V21-5):** a doc-hidden `alint_core::plugin_api` module defines `Adapter` and
  `Analyzer` traits over contract types (rows, values, provenance, findings, related locations,
  capabilities, limits), never over engine internals. A test fails if the module imports
  anything but `std`, `core`, `alloc`, and its own items. It is not a separate crate in v0.21:
  the published `alint-core` would depend on it, so it would have to be published too, with the
  new-crate bootstrap in `RELEASING.md`. It moves into a published `alint-plugin-api` crate with
  slice P2.
- **One internal trait set:** the engine keeps `alint_core::relation::Adapter` and its kin,
  which the built-in formats implement (section 9.4: every host feeds one internal trait set).
  The contract traits in `plugin_api` reach the engine through a bridge that wraps a contract
  adapter or analyzer as the internal trait (R6a). "Done" follows `plugin-architecture.md`
  section 10.3: a built-in adapter and analyzer run through the contract traits and the bridge
  with no behaviour change.
- **Contract types:** the flat value form (R1), rows, provenance, findings, related locations,
  capabilities, and limits, all std-only.
- **Registry injection:** every runtime call site listed under "Starting point" takes an
  injected registry instead of calling `builtin_registry()`. For `alint-testkit`'s runner, which
  is published API with an exhaustive `RunnerOptions`, that is an additive
  `Runner::with_registry`. `Engine::new` already accepts any `RuleRegistry`.
- **WIT draft:** `alint:plugin@0.1.0` is committed as a design artifact, with an `adapter` and
  an `analyzer` world (Q10; the `fixer` world follows with slice P2), host-pushed inputs, and no
  host callbacks. Bindings are generated once witforge's emitters exist (decision V21-5).
- **Fixer role:** deferred to slice P2, with plugin fixers.

### Measurement (DEC-13)

- **Two references (V21-6):** v0.18.2 is the blocking reference for relation-layer changes, as
  below. `docs/benchmarks/ANCHOR` (v0.20.0, +10%) keeps serving every release's bench gates
  unchanged.
- **What counts as a relation-layer change** (for pull requests): the relation layer and plugin
  traits, the engine, extraction and its parsers, `cross_file`, `key_parity`, and the shared
  reader, each module root in both its `foo.rs` and `foo/` forms, plus the gate itself and what
  it measures with. `ci/scripts/detect-changes.sh` holds the list, and
  `ci/scripts/test-detect-action-changes.sh` pins it.
- **Instruction gate:** S1-S3 at 1k and 10k, at most +3% against v0.18.2. The v0.18.2 binary is
  rebuilt in the same job with the same toolchain (built with lints capped, so a new lint cannot
  fail frozen source) and measured by the current harness, so the comparison has no committed
  baseline and no host or toolchain drift; the recorded counts are for the record. It runs on
  relation-layer pull requests and on every Rust push to `main`. The budget is cumulative and the
  comparison absolute, so a change outside the relation layer that spends S3's headroom is caught
  where it lands, even when a newer push cancelled an earlier run. It fails red and counts in the
  CI summary; `main` has no required checks, so on a push it reports after the merge. Only the
  alint binary comes from v0.18.2: the harness, the generated trees, and the gungraun runner are
  the current ones, so a gungraun bump does not touch the reference side.
- **S4 is not compared with v0.18.2.** It cannot run there (see "Starting point"), and the
  deterministic per-PR gate excludes it too (`det_check` measures S1-S3 only). S4 is checked at
  release time, by `bench-gate` against the previous release and against `ANCHOR`.
- **Syscalls:** four families, counted for the alint process and its threads only (spawned `git`
  children vary by host): read (`read`, `pread64`, `readv`, `preadv`, `preadv2`), stat
  (`newfstatat`, `fstat`, `statx`, `stat`, `lstat`, `access`, `faccessat`, `faccessat2`,
  `readlink`, `readlinkat`, `statfs`, `fstatfs`, and the `getxattr` calls), open (`openat`,
  `openat2`, `open`), and directory listing (`getdents64`, `getdents`).
  `ci/scripts/syscall-counts.py` holds the list. For S1-S3 at 10k files, no family may grow by
  more than 32 calls over v0.18.2: fixed per-run probes differ by a few calls (R0 found `main`
  probing `.alint.d`, and a CPU-count read that some runs make and others do not), while a
  per-file regression adds thousands. A trace that is empty, or of a run that did not complete,
  is a tooling failure, never zero.
- **Relation scenario:** R3 adds `relation_coalescing.yml` (`key_parity` plus a per-file rule
  over the same `crates/*/Cargo.toml` files). It has no v0.18.2 counterpart, so it runs in a
  separate `relation_grp` that the absolute reference gate filters out, and is budgeted
  separately against its first committed measurement (section 9.6, "new capabilities"); see
  [`docs/benchmarks/REFERENCE-v0.18.2.md`](../../benchmarks/REFERENCE-v0.18.2.md#relation-scenario).
- **Release check:** before the release, on kbench with caches dropped, S1-S3 x {full, changed}
  against v0.18.2: wall-clock medians at 100k (30 runs) and at 1M (bench-scale's 1M sampling,
  one warm-up and three runs: at 30 runs S3 alone would take about 16 hours) at most +3%, and
  peak RSS at 1M at most +10%.

## Decisions

Taken one at a time on 2026-10-24 and recorded in
[ADR-0023](../../adr/0023-relation-ir-key-parity-and-plugin-contract-0x.md), which also lists the
rejected options. [ADR-0024](../../adr/0024-relation-ir-decisions-after-review.md) supersedes it,
recording the corrections review found, including how decision V21-6's budget is measured.

| ID | Question | Decision |
|---|---|---|
| V21-1 (Q4) | Which value types are core? | Primitives with lexeme-preserving numbers, `path`, lists, and records. `semver` and SPDX wait for several consumers; `semver_range` lands as a standalone library first |
| V21-2 (Q13) | How are paths compared in relations? | Byte-exact after normalization on every operating system, with case folding as an explicit per-comparison opt-in. Today's `resolves` lookup is already exact, but on `Path` components, so Windows separators differ; normalization removes that |
| V21-3 | Which formats does `key_parity` cover in v0.21? | The five row formats, plus TOML (a new row adapter) and two XML resource-bundle dialects, Android string resources and .NET `.resx`, as `key_parity`'s own format names |
| V21-4 | What is a key in a nested file? | Leaf key paths, arrays treated as leaves, plus an opt-in `plurals: cldr` that counts a CLDR plural group as one key |
| V21-5 (Q10, P0) | How does the contract proceed while witforge's emitters do not exist? | Hand-written traits now in a doc-hidden `alint_core::plugin_api` module (not a crate: one the published `alint-core` depends on must itself be published), the WIT draft committed, bindings generated when P0 lands, and a published crate with P2 |
| V21-6 (DEC-13) | How does the frozen v0.18.2 reference coexist with `docs/benchmarks/ANCHOR`? | Both: v0.18.2 is the blocking reference for relation-layer changes (S1-S3; S4 cannot run on v0.18.2), and the anchor stays for release gates. If `main` is already over +3% against v0.18.2, the numbers come back for a decision first |
| V21-7 | Where does corpus replay run? | On the canonical bench host's runner, before each release with relation-layer changes and weekly, with spawning rules removed |
| V21-8 | Does the shadow run anywhere outside tests? | No: tests and corpus replay only, under the `shadow-ir` feature |

## Non-goals

v0.21 does not add:

- `version: 2`, named selectors or relations, or the generic `constraint` kind (Phase 3);
- a plugin host, a published plugin API crate, or plugin fixers (slices P2 and later);
- `key_parity` fixes, or `placeholder_parity` (D2);
- an HCL row adapter, generic XML rows (only the two resource-bundle dialects land), typed
  extraction for the `equals` port, YAML alias or merge-key expansion, or per-locale checks of
  CLDR plural categories;
- plan-level `explain --trace` (it needs the Phase 3 plan);
- deletion of any existing implementation (that happens in v0.22, after a zero-difference release).

## Work items and dependencies

| ID | Deliverable | Depends on | Completion evidence |
|---|---|---|---|
| R0 | the v0.18.2 reference measured; the blocking reference gate | none | `main` within budget on S1-S3 (`docs/benchmarks/REFERENCE-v0.18.2.md`); the gate passes on its own pull request in CI and fails a regression above +3% against v0.18.2 |
| R1 | `alint_core::relation` types (resources with origins, selectors, rows with original values, relations in canonical order, provenance, absence states, cardinality, limits, the internal pointer type) and the flat contract form of `Value` in `alint_core::plugin_api` with its import-boundary test | R0 | unit and property tests for canonical order, the three absence states, every limit, and the nested-to-flat round trip |
| R2 | Built-in row adapters and the `Extract` wrapper on the IR, with read coalescing | R1 | S1-S3 syscall counts within the gate's allowance; one read and one decode per (adapter, resource), asserted by a counting test with a test-only relation consumer alongside per-file rules |
| R3 | `key_parity`; the relation scenario | R1, R2 (R9 for its formats) | firing and silent scenarios across all eight formats, nested locales, `strip_root` on Rails files, `plurals: cldr` (nested YAML, i18next suffixes, Android `<plurals>`), `.env` against `.env.example`, a key-model mismatch rejected, every failure case above, a reference-only and a peer-only `--changed` edit that run and an unrelated one that skips, keys-only trace previews, golden fingerprints; the relation scenario's syscall and instruction counts recorded |
| R4 | Shadow harness and the `cross_file` `equals` port | R1, R2 | zero differences on every scenario and on the property generator's cases |
| R5 | `xtask corpus-replay`, full-SHA pins, cache, expectations, and difference report | none (R4 for the shadow comparison) | replays every corpus config that loads at its pin on kbench (106 of 110), the four that do not are listed in the expectations; once R4 lands, zero shadow differences there |
| R6a | contract traits, the bridge to the internal trait set, and registry injection (`Runner::with_registry` included) | R1, R2 | a built-in adapter runs through the contract traits and the bridge; golden fingerprints unchanged |
| R6b | `key_parity` through the contract traits | R3, R6a | `key_parity`'s scenarios pass through the bridge unchanged |
| R7 | WIT `alint:plugin@0.1.0` draft (bindings if P0 has landed) | R6a | witforge's subset checker accepts the draft |
| R9 | TOML row adapter; Android string-resource and `.resx` dialect adapters | R1, R2 | per-adapter unit tests, duplicate and parse-failure cases, the dialect skip rules, the XML depth guard exercised, eager spans for findings |
| R8 | Docs, changelog, generated artifacts, budget report, go/no-go assessment | R0-R7 (R6a and R6b), R9 | preflight, docs export, generated checks, and the section 9.6 table filled in |

R0 and R5 can start immediately and in parallel. R1 waits for R0's baselines. R3, R4, R6a, and R9
can run in parallel once R2 lands; `key_parity` is built and tested on the five row formats first,
and gains the R9 formats as each adapter lands. R6 and R7 are part of v0.21 (ADR-0024 decision
5); only R7's generated bindings wait for witforge's emitters.

## Cross-cutting invariants

1. **No behaviour change for existing configurations.** Findings, messages, exit codes, and
   baseline fingerprints stay byte-identical; the golden gate and the shadow harness assert it.
2. **Fail loud.** A parse failure, type mismatch, or limit hit is an analysis failure, never an
   empty or partial relation that passes (section 6.3).
3. **Read once.** No file is read twice in one run because the IR selected it (ADR-0003); R2's
   counting test and R3's relation scenario assert it.
4. **Determinism.** Rows, related locations, and truncations follow the canonical order; a test
   runs `key_parity` with shuffled index order and several thread counts and requires identical
   output. The port keeps the existing implementation's output order and is held to it by the
   shadow comparison.
5. **Budgets.** The section 9.6 budgets hold against v0.18.2, enforced for IR pull requests.
6. **Internal stays internal.** No IR type appears in `alint-core`'s documented API, in JSON
   output, or in the report schemas. Rows stay visible only through the experimental trace
   preview (Q7, ADR-0022). `relation` and `plugin_api` are `#[doc(hidden)] pub` (a test checks
   the declarations), and the generated, gated report schemas gain no field.
7. **Contract types only.** Nothing crosses the plugin traits except contract types; a test
   fails if `alint_core::plugin_api` imports anything but `std`, `core`, `alloc`, and itself.
8. **The shadow never ships.** The release and bench builds' feature trees exclude `shadow-ir`,
   and CI asserts it.
9. **Every invariant has a gate.** A fix or feature is complete only when a test asserts its
   invariant.

## Release acceptance

v0.21 is complete only when all of the following are observed:

- `key_parity` passes its scenarios in all eight formats (the five row formats, TOML, Android
  string resources, and `.resx`), including nested JSON and YAML locale files, `plurals: cldr`
  on nested YAML, i18next suffixes, and Android `<plurals>`, Rails files with `strip_root`,
  `.env` against `.env.example`, a missing reference, an unparsable reference and peer, a
  `peers` glob with no match, duplicate keys, a key-model mismatch, and `--changed` edits to the
  reference, to a peer, and to an unrelated file. Its trace is complete, and its fingerprints are
  stable across repeated and shuffled runs.
- The `cross_file` `equals` port shows zero differences on every scenario that uses it, on the
  property generator, and on every corpus config that loads (106 of 110 today, with the rest
  listed in `replay-expectations.tsv`).
- The S1-S3 instruction counts are within +3% of v0.18.2, their syscall families within the
  gate's allowance, the relation scenario is recorded, and the 100k and 1M wall-clock and RSS
  budgets hold on kbench.
- A built-in adapter and `key_parity` run through the plugin traits with the golden fingerprints
  unchanged.
- The go/no-go assessment is written: whether the ported path became simpler and more consistent,
  with the evidence. If it did not, the stop rule applies and Phase 3 does not start.
- Full workspace, cross-platform, shell, advisory, and preflight checks pass, and generated
  artifacts (schema, facts, docs export, roadmap) regenerate cleanly.

## Risks

1. **witforge's emitters slip.** The hand-written traits (V21-5) keep v0.21 independent of them.
2. **Relation-major evaluation adds reads.** The relation scenario and the counting test catch
   the v0.14 S2 read-path class, which instruction counts cannot see, before any kind moves.
3. **Corpus replay cost and safety.** It is cached and scheduled, not run per pull request.
   Repositories that disappeared upstream are reported as unavailable and must be listed in the
   expectations; any local failure fails the run. Nothing from a corpus repository runs.
4. **Size.** Phase 2 is an L-sized phase, and V21-3 adds three adapters to it. R9 does not block
   R4, and `key_parity` is useful on the five row formats before R9 completes.
5. **New XML adapters reopen the parser denial-of-service class.** Both dialects parse through
   the existing guarded XML path; R9's evidence includes the depth guard.
6. **The budget is already spent.** If R0, or the release check's wall-clock and RSS runs, find
   `main` over budget against v0.18.2 before any relation-layer change, the gate cannot be set as
   written; the numbers go back for a decision (V21-6). On S3 the headroom is already small (about
   0.6% to 1%), and two single runs of the same binary can differ by up to about 1.5% there.

## Status and next steps

- **R9, done.** The TOML row adapter uses the lossless `toml_edit` tree;
  Android `<string>`/`<string-array>`/`<plurals>` and .NET `.resx` string
  entries parse through the guarded XML path. `key_parity` now covers all eight
  formats, with per-dialect key models, pass/fail scenarios, and eager key
  spans.
- **R4, done.** The `shadow-ir` feature is defined on `alint-rules` and enabled
  only from `alint-e2e`'s dev-dependency (and corpus replay). `cross_file`'s
  `relation: equals` now runs through `ExtractAdapter` and the run-scoped
  read/decode caches beside the authoritative implementation, and the complete
  finding debug snapshots are compared. The property generator covers generated
  source/target values and `normalize:` lists (including the non-literal note
  path), and the full e2e suite runs with the shadow enabled: zero differences.
  Regex extracts (whose target-side syntax filters are outside the generic
  adapter) and read/decode failures deliberately remain authoritative-only in
  this increment; they are still covered by the existing cross_file tests.
  `ci/scripts/check-shadow-feature-tree.sh`, wired into Build and preflight,
  proves the release, `-p alint -p xtask`, and criterion feature trees exclude
  `shadow-ir`.
- **R3, done.** `key_parity` is registered for JSON, YAML, TOML, dotenv, INI,
  Java properties, Android string resources, and .NET `.resx`, with the
  documented `direction`, `strip_root`, `plurals: cldr`, and `ignore_keys`
  options, fail-loud missing/parse/empty-peer outcomes, baseline keys, a
  `--changed` path scope, a keys-only preview on the extraction trace stage,
  unit tests, and firing/silent plus `--changed` policy scenarios. The relation
  benchmark scenario is `relation_coalescing.yml`: with `key_parity` alone the 10k tree makes
  839 read calls, with the per-file rule alone 441, and with both 839, so the two consumers share
  the run-scoped cache. Its instruction and syscall counts are recorded in
  [`REFERENCE-v0.18.2.md`](../../benchmarks/REFERENCE-v0.18.2.md#relation-scenario).
- **R2 implementation notes:** [`r2-row-adapters.md`](r2-row-adapters.md) records
  the row/diagnostic model, the strict-versus-lossy decoding split, and the
  read/decode cache design that R2 implements.
- **R2, done.** `alint_core::relation::adapters` now provides `EntryRowsAdapter`
  (JSON, YAML, dotenv, INI, and Java properties; key rows with eager spans) and
  `ExtractAdapter` (today's `Extract`; lossy decoding, non-string kind
  diagnostics, non-literal notes). `RowState::Key` keeps a structural key
  distinct from an explicit `null`. `ReadCache` and `DecodeCache` use per-key
  `OnceLock`s; the engine enables them only when a rule returns
  `uses_relations()`, shares them with `--changed` and git-filtered indexes, and
  retains only paths a relation claims. `resolved_within_root` moved to
  `alint-core::pathsafe` so the cache and `cross_file` share one confinement
  implementation. The test-only relation consumer proves one read, one hit, and
  one decode alongside a per-file rule. The v0.18.2 reference gate is green on
  the audited code: S1 -5.85%/-8.34%, S2 -3.15%/-3.31%, S3 -0.18%/+0.65%, and
  the 10k syscall families are within two calls of the reference.

- **R0, done.** On every S1-S3 cell, `main` at `fb9820f5e` uses no more instructions than v0.18.2
  (S1 -7% to -8%, S2 about -3%, S3 -0.04% to -0.2%), and its read/stat-family syscall counts match
  within fixed per-run probes, so the budget is not already spent. S3 has the least headroom, and
  one rerun of the same binary moved S3 at 10k by 0.73% on a 24-thread host. The
  `perf-gate-reference` job and `ci/scripts/reference-perf-gate.sh` implement the gate, which
  passed on its own pull request in CI. A synthetic regression of +5.2% over v0.18.2 on S1 fails
  it (a direct run of the gate's gungraun filter and limit), and a +4% change over `main` passes,
  because S1 starts 7-8% under v0.18.2. Details and the recorded counts:
  [`docs/benchmarks/REFERENCE-v0.18.2.md`](../../benchmarks/REFERENCE-v0.18.2.md).
- **R6a, in progress.** `alint_core::plugin_api` now carries std-only `Adapter` and
  `Analyzer` traits over contract resources, rows, findings, related locations,
  capabilities, and limits. `relation::ContractAdapter` bridges a contract adapter
  into the engine, `relation::InternalContractAdapter` proves a built-in entry
  adapter round-trips through the contract, and `alint-testkit` gained additive
  `Runner::with_registry`. The import-boundary test now rejects `crate::plugin_api`
  lookalikes and allows only the module's own submodules. Remaining: widen registry
  injection to the CLI/LSP/DSL runtime call sites and bridge the analyzer role
  through a built-in analyzer.
- **R5, done** (#362). The first full replay on the bench host (run 40144052081, 2026-10-24)
  replayed 106 of the 110 configs at their pins in under ten minutes, with 83 spawning rules
  removed from 20 configs. Four configs no longer load on the current release and are reported
  as skipped: three override a bundled rule whose options changed (`gin-gonic/gin`,
  `neovim/neovim`, `spring-projects/spring-boot`), and `facebook/react` sets
  `scope_filter.changed_since` on `git_commit_no_fixup`, which takes `since` instead. Its error
  reads only `<redacted>`: a rule with an env-interpolated field has every validation error
  redacted, even one that quotes no value, which is a usability bug of its own. 49 of the
  replayed configs carry 87 `equals`-path rules. R4's shadow is part of the
  replay binary (`-p alint --features alint-rules/shadow-ir`), so any shadow
  difference aborts the repository; `xtask` is built separately without the
  feature.
- **Review round 2026-10-25.** A second audit corrected the replay (no false passes, no command
  execution, fingerprints and proposed edits compared, expectations), the gate (the engine root
  module, untraceable runs, frozen-source lints, attribution), and this design (resource origins,
  row order, the module visibility, the R2/R3 cycle, the R6 split). ADR-0024 records the
  decisions; it was accepted on 2026-10-25, including decision 6's measurement of the budget.
- **R1, done.** `alint_core::relation` holds the types: normalized paths that order like the
  walked index; structural pointers with the one injective rendering; resources with origins (index
  entry or literal path) and a read outcome that keeps "missing" distinct; selectors; values with
  lexeme-preserving numbers; rows with an optional pointer, emission ordinal, optional span, the
  three absence states, and the original value; relations in canonical order with explicit set
  conversion and deterministic truncation; cardinality; every limit with its default and hard cap,
  plus the shared run budget; and the internal `Adapter` trait. The flat contract form of a value
  is in `alint_core::plugin_api`, which a test keeps free of engine imports, and `relation`
  converts to and from it (iteratively, so value depth cannot exhaust the stack). Both modules are
  `#[doc(hidden)] pub`, which tests check. Unit and property tests cover canonical order and its
  independence from input order, the absence states, every limit, the nested-to-flat round trip,
  and rejection of malformed node lists (shared or backward children, cycles, unreachable nodes,
  size and depth limits).
- **Next:** finish R6a's registry injection and analyzer bridge, then R7's WIT
  draft and R6b's `key_parity` bridge proof. R3 (`key_parity`), R4 (the shadow
  harness), R5 (corpus replay), and R9 (its TOML and XML dialect formats) are
  complete.
