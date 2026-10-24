# v0.21: Internal relation IR (go / no-go)

Status: Proposed (2026-10-24). Not accepted: the decisions in "Decisions to take" are open, and
acceptance records them in a new ADR (ADR-0023).
Decisions it builds on: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md)
(DEC-2, DEC-5, DEC-7, DEC-8, DEC-10, DEC-13, DEC-14). Questions it decides: Q4, Q10, Q13, and
the first cut of Q8.
Program: [`../general-linter.md`](../general-linter.md), Phase 2 (section 11.5);
[`../plugin-architecture.md`](../plugin-architecture.md), slice P1.

v0.21 is the go/no-go proof for the general-linter program. It builds the internal relation layer
that Phase 3's selectors and constraints will later expose. It proves that layer on one new kind
and one shadowed port of an existing kind, and designs the plugin contract on the same types.
Nothing public changes shape except one new rule kind: there is no `version: 2`, no new config
syntax, and no plugin host.

It delivers four things together:

1. an **internal relation IR** in `alint-core`: resources, selectors, adapters, typed rows with
   provenance, relations, and limits;
2. **`key_parity`**, a new v1 kind and the first user of the IR (DEC-7): one reference file and
   a glob of peers must declare the same key set, including nested JSON and YAML locale files;
3. a **differential shadow harness** and the first port, `cross_file`'s `equals` relation (the
   path the `cross_file_value_equals` alias uses), with the existing implementation still
   authoritative;
4. the **plugin contract `0.x`** design: internal `Adapter` and `Analyzer` traits that the
   built-in row formats and `key_parity` implement, an injectable registry, and a WIT draft.

It also does the measurement work DEC-13 requires before any IR change can be judged.

## Why these belong in one release

An IR without a user is untested design. `key_parity` without the IR would be the standalone
evaluator DEC-8 rules out, and it would be replaced in Phase 3. The shadowed port is the only
evidence that the IR can carry existing semantics byte for byte, which is what "go" means. The
plugin contract has to be designed against the same row and provenance types, or it will leak
internal layouts across the plugin boundary (the failure mode `plugin-architecture.md` section
4.1 cites).

The stop rule from `general-linter.md` section 11.1 applies to this release. If the ported path
does not become simpler and more consistent, the program stops here; the IR, `key_parity`, and
the measurement work keep their standalone value.

## Starting point

Checked against the code at `6a75eb4d1` (2026-10-24).

- **Extraction is string-only and per-consumer.** `ExtractSpec`
  (`crates/alint-core/src/extract.rs:92`) resolves to `Extract` (`:20`): `Structured` (RFC 9535
  JSONPath over the parsed `serde_json::Value`), `Lines`, `Regex` (capture group 1), or
  `WholeFile`. Results are strings plus a type tag for non-string matches (`ExtractedValues`,
  `:267`), and non-literal entries such as `${VAR}` are dropped silently (`is_non_literal`,
  `:237`). Each consumer enforces its own cardinality: `cross_file`'s `equals` requires exactly
  one source value (`crates/alint-rules/src/cross_file/eval.rs:310`). The users are `cross_file`,
  `registry_paths_resolve`, `file_graph` content edges, manifest-derived scope, and the
  `file_content_matches` fact.
- **Spanned rows exist but nothing consumes them.** v0.19's duplicate-preserving
  `StructuredEntryRow` (`extract.rs:312`, built by `extract_entry_rows` at `:333`) covers JSON,
  YAML, dotenv, INI, and properties; TOML, XML, and HCL return `UnsupportedFormat`. Value spans
  resolve lazily through `structured_fix::resolve_value_span`
  (`crates/alint-core/src/structured_fix/mod.rs:50`) for every format but TOML. Two pointer
  types duplicate each other: `PathSeg` (`structured_fix/mod.rs:38`) and `EntryPathSegment`
  (`extract.rs:275`).
- **`cross_file`** is `FullIndex { path_scope: false }`, with seven relations (`Equals`,
  `Subset`, `Superset`, `SetEquals`, `Identical`, `Resolves`, `Registered`;
  `cross_file/spec.rs:86`) and a 10,000-value cap (`cross_file/mod.rs:67`).
  `cross_file_value_equals` is one of the 11 aliases (`crates/alint-rules/src/lib.rs:810`).
- **Dispatch.** `KindMeta` declares four classes (`crates/alint-core/src/kind_meta.rs:17`); the
  102 canonical kinds split 56 per-file, 22 full-index, 14 rule-major scoped, and 10 single-shot.
  At run time there are two partitions: a rule-major parallel loop for every rule without a
  per-file form, and the file-major `run_per_file` loop
  (`crates/alint-core/src/engine/run.rs:714`) that shares one read per file (ADR-0003).
- **Baseline identity** is frozen by the golden gate
  (`crates/alint-e2e/tests/fingerprint_golden.rs`): 526 fingerprints over the 485 scenario files
  plus the v1 bundled-ruleset tests (DEC-5).
- **The v0.12 corpus is not replayable yet.** Its 110 configs live in
  `docs/design/v0.12/corpus/`, pinned in `docs/design/v0.12/case_study_repos.md` by 10-character
  SHA prefixes, which GitHub will not fetch by. Nothing reads the directory today.
- **Performance gating does not meet DEC-13.** The deterministic gate
  (`crates/alint-bench/benches/det_check.rs`, scenarios S1-S3) is advisory in CI and compares
  each pull request with its merge base. `docs/benchmarks/ANCHOR` names v0.20.0 with a +10%
  budget, not v0.18.2 with +3%, and no instruction counts are stored for v0.18.2. Macro results
  for v0.18.2 exist (`docs/benchmarks/macro/results/linux-x86_64/v0.18.2/`). S3 already reads
  `crates/*/Cargo.toml` from at least three rules, which is the shape section 9.1's coalescing
  scenario asks for.
- **Plugin groundwork.** `witforge` (`asamarts/witforge`, private) has its subset checker and
  wire lockfile (`559d82e`, 2026-10-14). Its Rust and protobuf emitters, slice P0, are not
  written. Every runtime path calls `builtin_registry()` directly, `RuleBuilder` is a function
  pointer, `Format` is a closed enum, and `ExtractSpec` has one field per format
  (`plugin-architecture.md` section 11).
- **`key_parity` has no code.** Flat-format parity is the cookbook's `env-key-parity` recipe, a
  `cross_file` `set_equals` over a `regex:` key extract.

## Scope

### Relation IR

A new internal module, `alint_core::relation`. It is not part of the public API: hidden from
docs and carrying no semver promise until the v1.0 freeze. It defines:

- **`Resource`**: a file in the run's index, identified by its index entry, with its normalized
  path (Q13).
- **`Selector`**: a compiled glob plus the existing scope gates, filtering the index. It is a row
  filter, never a path scope on a full-index rule (constitution invariant 2 stands; Q2 is Phase 3).
- **`Adapter`**: turns a resource's bytes into a row stream. The built-in row adapters wrap
  `extract_entry_rows` and the existing parsers.
- **`Row` and `Value`**: the value types decided by Q4.
- **`Relation`**: a finite, ordered multiset of rows with the canonical order of section 6.2
  (normalized path bytes, byte offset, structural pointer, label). Conversion to a set is an
  explicit operation.
- **Provenance**: eager resource id and structural pointer; byte and line ranges resolved lazily,
  only for failing rows, through `resolve_value_span`. `PathSeg` and `EntryPathSegment` merge
  into one pointer type.
- **Absence and cardinality**: missing, explicit `null`, and an extraction error are distinct row
  states. Every extractor declares its cardinality. A parse failure or limit hit is an analysis
  failure under DEC-2, never an empty relation.
- **Limits** (section 9.5): rows per extractor, total rows per run, distinct values, and
  equality-join inputs and outputs, each with a default and a hard cap.

**Reads.** IR evaluation keeps ADR-0003's guarantee. A file selected by several relations, or by
relations and per-file rules, is read at most once per run, and decoded at most once per
(adapter, resource). This is measured by `read` syscall counts, not instruction counts.

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
  - `format` (inferred from the extension when omitted);
  - `direction` (`both`, `missing` or `extra`; default `both`);
  - `ignore_keys` (key-path globs).

  The key model is decision V21-4.
- **Findings:** one per missing key, at the peer file, with the reference key as a related
  location. One per extra key, at the peer key. Example message: "`locales/fr.json` is missing
  key `nav.home` (present in `locales/en.json`)". Keys appear in messages; values never do, so the
  redaction rules (DEC-3) are not engaged.
- **Failures:**
  - A missing reference file is a finding that always counts as error (DEC-2).
  - A peer that fails to parse is an analysis failure for that peer.
  - Duplicate keys within one file are not parity violations; they are B1 `no_duplicate_keys`'s
    concern. Rows keep them, and parity compares distinct key paths.
- **Dispatch and `--changed`:** `FullIndex { path_scope: false }`. It runs when the reference or
  any peer changed.
- **Fixes:** none in v0.21. ADR-0017 classifies key insertion as a Suggestion, and stub insertion
  waits for the generic edit plan.
- **Baseline key:** sorted identity fields (peer path, key path, direction), per DEC-5.
- **The usual kind obligations:** `KindMeta` and `TraceMeta`, rules reference and site page,
  firing and silent scenarios, schema, facts, and the README count claims (102 kinds become 103).

### Shadow harness and the first port

- **The port:** `cross_file` with `relation: equals` (what `cross_file_value_equals` resolves to)
  gets a second implementation on the IR.
- **Where the shadow runs:** a test-only `shadow-ir` feature runs both implementations and asserts
  byte-identical findings, messages, locations, notes, analysis failures, and baseline
  fingerprints. It runs in three places:
  - every scenario that uses `cross_file` `equals`;
  - a property-test generator over extract specs and `normalize:` lists;
  - the 110-config corpus at its pinned commits.
- **Corpus replay:** a new `xtask corpus-replay` resolves the pins to full SHAs (committed once),
  fetches each repository into a cache outside the tree, runs both implementations, and reports
  every difference. It runs on the canonical bench host or a scheduled job, never per pull request.
- **Authority:** the existing implementation stays authoritative. Production never runs the
  shadow, so users pay nothing for it. The old code is deleted in v0.22, after one release with
  zero differences (DEC-7).
- **Not in v0.21:** the other six `cross_file` relations, the remaining `ExtractSpec` consumers,
  and manifest-derived scope. Q8's first cut is recorded from what this port shows.

### Plugin contract `0.x` (slice P1)

- **Crate:** a new `alint-plugin-api` crate defines `Adapter` and `Analyzer` traits over contract
  types (rows, values, provenance, findings, related locations, capabilities, limits), never over
  engine internals.
- **Implementors:** the built-in row adapters and `key_parity` implement the traits. "Done"
  follows `plugin-architecture.md` section 10.3: a built-in adapter and analyzer run through the
  traits with no behaviour change.
- **Registry injection:** the CLI's check, fix and validate-config paths and the LSP take an
  injected registry instead of calling `builtin_registry()`. `Engine::new` already accepts any
  `RuleRegistry`.
- **WIT draft:** `alint:plugin@0.1.0` is committed as a design artifact, with one world per role
  (Q10), host-pushed inputs, and no host callbacks. Bindings are generated once witforge's
  emitters exist (decision V21-5).
- **Fixer role:** deferred to slice P2, with plugin fixers.

### Measurement (DEC-13)

- **Baseline:** record v0.18.2's deterministic instruction counts for S1-S3 on the canonical
  host, and commit them beside the macro results.
- **Gate:** add a cumulative mode to the deterministic gate that fails, rather than advises, when
  an IR pull request exceeds +3% against v0.18.2 on S1-S3.
- **Syscalls:** record `read` and `stat` counts (strace) for S1-S4 at 10k files, and fail an IR
  pull request that increases them.
- **Release check:** before the release, measure wall-clock medians (30 runs, caches dropped) and
  peak RSS at 100k and 1M files on kbench against v0.18.2: at most +3% and +10% respectively.

## Decisions to take

Each decision is taken on its own, in this order, before work item R1 starts. The
recommendation is listed first.

| ID | Question | Options | Recommendation |
|---|---|---|---|
| V21-1 (Q4) | Which value types are core? | (a) primitives with lexeme-preserving numbers, `path`, lists, and records; (b) also `semver` and SPDX scalars now | (a): semantic scalars wait for several consumers; `semver_range` stays a standalone library |
| V21-2 (Q13) | How are paths compared in relations? | (a) byte-exact after normalization, case folding opt-in per comparison; (b) follow the host file system | (a): host-dependent results break determinism and the cross-platform test class |
| V21-3 | Which formats does `key_parity` cover in v0.21? | (a) the five row formats (JSON, YAML, dotenv, INI, properties); (b) also TOML, which needs a new row adapter; (c) also XML resource bundles | (a): covers JSON and YAML locale files, `.env`, and `.properties`; TOML and XML rows follow demand |
| V21-4 | What is a key in a nested file? | (a) leaf key paths, arrays treated as leaves; (b) every object path; (c) leaf paths that descend into arrays by index | (a), with an `arrays: descend` option later if asked: locale arrays are values, and index paths make reordering look like drift |
| V21-5 (Q10, P0) | How does the contract proceed while witforge's emitters do not exist? | (a) hand-written traits now, WIT draft committed, bindings generated when P0 lands, `alint-plugin-api` unpublished in v0.21; (b) block P1 on P0; (c) defer the contract to v0.22 | (a): DEC-14e requires designing the contract with the IR, but nothing outside alint consumes it until P2 |
| V21-6 (DEC-13) | How does the frozen v0.18.2 reference coexist with `docs/benchmarks/ANCHOR` (v0.20.0, +10%)? | (a) both: DEC-13's blocking gate for IR pull requests, the anchor for release gates; (b) move the anchor to v0.18.2 at +3% for everything | (a): the anchor accepted the v0.18-v0.20 costs deliberately; DEC-13 protects configurations that do not use relations |
| V21-7 | Where does corpus replay run? | (a) on the canonical bench host before each release and weekly; (b) a scheduled hosted CI job; (c) per pull request | (a): 110 clones are too slow for pull requests, and the bench host already holds large caches |
| V21-8 | Does the shadow run anywhere outside tests? | (a) tests and corpus replay only; (b) also an opt-in `ALINT_SHADOW_IR` runtime flag | (a): a runtime flag would ship a second implementation's cost and its diagnostics surface |

## Non-goals

v0.21 does not add:

- `version: 2`, named selectors or relations, or the generic `constraint` kind (Phase 3);
- a plugin host, a published plugin API crate, or plugin fixers (slices P2 and later);
- `key_parity` fixes, or `placeholder_parity` (D2);
- TOML, XML, or HCL row adapters (unless V21-3 says otherwise);
- plan-level `explain --trace` (it needs the Phase 3 plan);
- deletion of any existing implementation (that happens in v0.22, after a zero-difference release).

## Work items and dependencies

| ID | Deliverable | Depends on | Completion evidence |
|---|---|---|---|
| R0 | v0.18.2 instruction and syscall baselines; cumulative blocking gate mode for IR pull requests | none | baselines committed; the gate fails a synthetic +4% change and passes v0.21's base |
| R1 | `alint_core::relation` types, provenance, absence states, limits, merged pointer type | decisions; R0 | unit and property tests for canonical order, the three absence states, and every limit |
| R2 | Built-in row adapters on the IR with read coalescing | R1 | `read` counts on S1-S4 unchanged; one decode per (adapter, resource) asserted by a counting test |
| R3 | `key_parity` | R1, R2 | firing and silent scenarios across the five formats, nested locales, `.env`/`.env.example`, a reference-only `--changed` edit, complete trace, golden fingerprints |
| R4 | Shadow harness and the `cross_file` `equals` port | R1, R2 | zero differences on every scenario and on the property generator's cases |
| R5 | `xtask corpus-replay`, full-SHA pins, cache, and difference report | none | replays the 110 configs at their pins on kbench; R4 shows zero differences there |
| R6 | `alint-plugin-api` traits and registry injection | R1 | a built-in adapter and `key_parity` run through the traits; golden fingerprints unchanged |
| R7 | WIT `alint:plugin@0.1.0` draft (bindings if P0 has landed) | R6 | witforge's subset checker accepts the draft |
| R8 | Docs, changelog, generated artifacts, budget report, go/no-go assessment | R0-R7 | preflight, docs export, generated checks, and the section 9.6 table filled in |

R0 and R5 can start immediately and in parallel. R1 waits for the decisions. R3 and R4 can run
in parallel once R2 lands. If the release runs long, R6 and R7 move to v0.22 rather than
delaying R3 and R4.

## Cross-cutting invariants

1. **No behaviour change for existing configurations.** Findings, messages, exit codes, and
   baseline fingerprints stay byte-identical; the golden gate and the shadow harness assert it.
2. **Fail loud.** A parse failure, type mismatch, or limit hit is an analysis failure, never an
   empty or partial relation that passes (section 6.3).
3. **Read once.** No file is read twice in one run because the IR selected it (ADR-0003); the
   syscall gate asserts it.
4. **Determinism.** Rows, related locations, and truncations follow the canonical order;
   repeated and shuffled parallel runs produce identical output.
5. **Budgets.** The section 9.6 budgets hold against v0.18.2, enforced for IR pull requests.
6. **Internal stays internal.** No IR type appears in `alint-core`'s public API or in JSON
   output. Rows stay visible only through the experimental trace preview (Q7, ADR-0022).
7. **Contract types only.** Nothing crosses the plugin traits except contract types.
8. **Every invariant has a gate.** A fix or feature is complete only when a test asserts its
   invariant.

## Release acceptance

v0.21 is complete only when all of the following are observed:

- `key_parity` passes its scenarios in every covered format, including nested JSON and YAML
  locale files, `.env` against `.env.example`, a missing reference, an unparsable peer, duplicate
  keys, and an edit to the reference alone under `--changed`. Its trace is complete, and its
  fingerprints are stable across repeated and shuffled runs.
- The `cross_file` `equals` port shows zero differences on every scenario that uses it, on the
  property generator, and on the 110-config corpus replay.
- The S1-S3 instruction counts are within +3% of v0.18.2, `read` and `stat` counts have not
  increased, and the 100k and 1M wall-clock and RSS budgets hold on kbench.
- A built-in adapter and `key_parity` run through the plugin traits with the golden fingerprints
  unchanged.
- The go/no-go assessment is written: whether the ported path became simpler and more consistent,
  with the evidence. If it did not, the stop rule applies and Phase 3 does not start.
- Full workspace, cross-platform, shell, advisory, and preflight checks pass, and generated
  artifacts (schema, facts, docs export, roadmap) regenerate cleanly.

## Risks

1. **witforge's emitters slip.** V21-5 option (a) keeps v0.21 independent of them.
2. **Relation-major evaluation adds reads.** R2's syscall gate catches the v0.18.0 regression
   class before any kind moves.
3. **Corpus replay cost.** It is cached and scheduled, not run per pull request. Repositories
   that disappeared upstream are reported and skipped, not counted as differences.
4. **Size.** Phase 2 is an L-sized phase. The order above lets R6 and R7 slip without
   weakening the go/no-go evidence.

## Next step

Take decisions V21-1 to V21-8 one at a time, record them in ADR-0023, and mark this document
accepted. R0 and R5 need no decision and can start first.
