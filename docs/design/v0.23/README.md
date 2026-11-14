# v0.23: selectors, a first constraint slice, and the YAML groundwork

Status: proposed (2026-11-12).
Decisions: [ADR-0032](../../adr/0032-v023-owner-decisions.md) (the owner decisions and the scope);
[ADR-0033](../../adr/0033-v023-slice-after-the-corpus-stratum.md) (the slice after the corpus
stratum, and the #503 fix).

v0.23 starts Phase 3 of [`general-linter.md`](../general-linter.md) (section 11.6), which v0.22.0
unblocked by meeting all three gates of Phase 2 (section 11.1, "v0.22 gate results"). Phase 3 is
large, so v0.23 carries its design and a first slice, and v0.24 the rest (ADR-0032 decision 1).
The release also lays the groundwork for a YAML parser change and makes the last planned break of
the `alint-core` API before v1.0.

## 1. Scope

The Phase 3 slice (ADR-0032 decision 1, amended by ADR-0033 decisions 1 and 2):

1. **A corpus stratum for the flagship policies.** An i18n stratum and a JavaScript-monorepo
   stratum join the pinned corpus. They are scored with section 12.3's buckets before the design
   commits to operators, because the v0.12 backlog has no locale, key-parity or
   dependency-version findings (section 12).
2. **The Phase 3 design document and the version-boundary ADR.** `v0.23/selectors-and-constraints.md`
   answers:
   - Q1: where selectors and relations live, and what becomes of ADR-0010's `path_sets:`;
   - Q2: how selectors on full-index rules interact with `--changed`, which amends constitution
     invariant 2;
   - Q6: the join vocabulary and its caps;
   - Q12: namespacing, provenance, no shadowing, and capability monotonicity;
   - Q15: `when` in v2;
   - section 7.8: the v2 schema, the commands, the generated documentation and the facts counts.

   An ADR records the version boundary (DEC-9). The owner approves both before implementation
   starts.
3. **Named selectors and relations,** top-level and rule-local, on the relation layer, in
   `version: 2` only. The `entry` adapter can emit value-bearing (key path, value) rows for JSON,
   YAML and TOML, which the dependency version policy needs (ADR-0033 decision 2).
4. **The `constraint` kind, report-only,** with the assertion families the three conformance
   policies need: equality, unique/key and set (ADR-0033 decision 1). `set` covers inclusion,
   equality and disjointness, also per group. It has no fixes (section 8.3).
5. **Three conformance policies** as `version: 2` configurations: locale key parity (re-expressed
   from `key_parity` as per-locale set equality), dependency version consistency, and
   toolchain-pin consistency. Each ships firing and silent `alint test` cases and is measured
   against section 12.1.
6. **`version: 2`, experimental.**
   - A draft `schemas/v2/config.json`, with a versioned `$id`.
   - `validate-config` for both versions; `list` and `explain` render named selectors and
     relations.
   - `init` and `suggest` keep emitting v1. The documentation marks v2 experimental, and
     `validate-config` says so for a v2 configuration.

Also in v0.23:

7. **The v0.21 hidden-path fingerprint is no longer matched** (ADR-0030 decision 1, ADR-0032
   decision 5).
8. **YAML parser types leave the public API (#484)** for opaque alint-owned types (ADR-0032
   decision 3).
9. **`Violation`, `Context`, `RuleSpec`, `NestedRuleSpec` and `WalkOptions` are sealed** with
   constructors and setters (ADR-0032 decision 3). This completes the v1.0 roadmap item.
10. **Differential YAML parser fuzzing (#482).**
11. **If ayaml 0.1.2 meets the criteria before the feature freeze,** the `yaml_depth` guard is
    re-derived (#483) and alint cuts over (#481). Otherwise both move to the next minor release
    (ADR-0032 decision 2).
12. **`git_tracked_only: true` sees tracked files that an ignore file excludes (#503,** ADR-0033
    decision 3). Up to v0.22 the walker pruned such a file before the git-tracked index was built,
    so the documented case (a force-added file, or one committed before its ignore line) was never
    seen. The config's `ignore:` and the walker's other exclusions still apply. A v1 configuration
    that uses `git_tracked_only` can gain findings (`file_absent`, `dir_absent`) or lose them
    (`file_exists`, `dir_exists`). The kinds that check that a path resolves say "missing or
    ignored" where they said "on disk", and the bundled `java@v1` and `php@v1` rules that these
    changes touch reword their messages to match. This is the one exception to section 3's "v1 is
    unchanged". Done in #509.

Out of scope:
- **v0.24 or later:** the remaining assertion families (reference, cardinality, group, bounded
  count), the specialized kinds and templates of DEC-8, and a stable `version: 2`.
- **Plugin hosts:** these start in v0.24 (ADR-0032 decision 4).
- **Phase 4:** stale exceptions.

## 2. Where the code stands

- **Relation layer** (`crates/alint-core/src/relation/`, about 3,600 lines).
  - It provides resources, typed rows and values with provenance, row adapters (entry, extract,
    TOML, XML), run-scoped read and decode caches, limits, paths and pointers, and the draft
    contract traits.
  - `key_parity` and every `cross_file` value relation read through it.
  - There are no named selectors. Each kind selects its own files (`paths:`, `scope_filter:`,
    manifest predicates).
- **Configuration versions.** Only `version: 1` exists (`Config::CURRENT_VERSION`). The loader
  validates each source's `version` and `requires.alint` before merging (ADR-0022) and rejects any
  other version.
- **Conformance policies today.**
  - Locale key parity is the `key_parity` kind (v0.21).
  - Toolchain-pin consistency is `alint://bundled/toolchain-pins@v1`: 41 rules, mostly
    `cross_file`, with `sync_from` fixes.
  - Dependency version consistency (gap C3) has no kind and no bundle.
- **Hidden-path fingerprints.** v0.22 writes the path-free form and still matches the v0.21 form,
  in `crates/alint-core/src/baseline.rs` and `crates/alint/src/check.rs`. The language server
  has a test for that matching.
- **Parser types in the public API.** The #484 list:
  - in `alint-core`: the `extra` fields and accessor, configuration masking, template rendering,
    and `Error::Yaml`;
  - two `alint-dsl` functions;
  - four `alint-testkit` items.

  alint parses YAML with `yaml_serde` 0.10.7.
- **Open structs.**
  - `Violation`, `Context`, `RuleSpec`, `NestedRuleSpec` and `WalkOptions` are struct literals
    outside `alint-core`.
  - Excluding `alint-core`'s own source, the literal constructions number about 56, 34, 17, 1 and
    19, tests included.
  - `FixContext`, `Config` and `CompiledNestedSpec` were sealed in v0.22.
- **Corpus.** 110 configurations under `docs/design/v0.12/corpus/`, at pinned commits, replayed by
  `xtask corpus-replay` and `corpus-replay.yml`. None is an i18n-heavy repository or a
  JavaScript workspace chosen for its version policy; phase 2 added 20 that are (section 7).

## 3. Design constraints

The selector and constraint design is the phase 3 document's job (section 4). This plan fixes what
it must respect:

- **v1 is unchanged.** A v1 configuration keeps its findings, messages, fingerprints and exit
  codes (section 7.1 rule 10). Evidence:
  - the e2e suite;
  - the corpus replay against v0.22.1, with zero differences.

  Item 7 does not change what alint writes, only which old baseline entries it matches. Item 12 is
  the one exception (ADR-0033 decision 3): a configuration that uses `git_tracked_only` can gain
  or lose findings, three kinds and six bundled rules reword a message, the CHANGELOG lists the
  change, and every difference in the replay must come from it.
- **The version boundary of ADR-0022 holds.** A v2 source may extend v1 sources, never the
  reverse, at any recursive edge.
- **Trust is no weaker than v1** (section 7.1 rules 9, 11 and 12):
  - every named object carries its provenance;
  - references resolve by namespace;
  - an inherited name never shadows a local one;
  - the planner refuses a rule that would gain a capability through what it references.

  The section 12.2 trust tests cover v2 sources from remote `extends:`.
- **Bounded and fail-loud.**
  - A v2 plan's cost is estimated before the walk and bounded (rule 7).
  - Incomplete analysis is an analysis failure, never an empty success: mutation tests on the
    error, truncation and type-mismatch paths kill every such mutant (section 12.1).
- **Performance.** v1 configurations stay within section 9.6's budgets against v0.18.2 on the
  deterministic gate. The conformance policies' v2 configurations are benchmarked against the v1
  kinds and bundles they re-express.
- **One API break, before Phase 3 adds fields.** Items 8 and 9 land before selectors and
  `constraint`, so the fields those add to `Violation` and `RuleSpec` are additive.

### 3.1 The owned YAML types (#484)

- **The types.** `alint_core::yaml` holds `Value`, `Mapping` and `Error`, opaque wrappers over the
  parser's types.
  - `Value` and `Mapping` implement `Serialize` and `Deserialize`, offer the reads and edits that
    rules, templates, redaction and the loader use, and deserialize into a typed options struct.
  - `Error` gives a message and, where the parser has one, a location.
- **The parser stays private.** No public item names the parser crate; it appears only in the
  private fields of the `yaml` types. Workspace crates that parse YAML internally may keep calling
  the parser until the cutover, converting at the API boundary.
- **A gate.** Either a public-API check or a scan of the generated documentation fails CI if a
  public item of `alint-core`, `alint-dsl` or `alint-testkit` names the parser crate. The phase
  picks one that runs on stable Rust.
- **Library users migrate once.** The CHANGELOG lists each changed signature with its
  replacement.

### 3.2 The fuzzing harness (#482)

- **What it compares.** It runs `yaml_serde` and the ayaml candidate through the entry points alint
  uses: Serde typed decoding, `Value` parsing and the event stream.
- **Inputs.** Generated token soup, alint's committed YAML, and mutations of that YAML.
- **Results.** Each input is classified into the five classes #482 lists. Documented deviations are
  an explicit allowlist, and any other difference fails.
- **When it runs.** A bounded run on pull requests that touch YAML parsing, and a longer scheduled
  run.
- **Against which ayaml.** It runs against ayaml's latest release, and against its main branch when
  re-qualifying.

## 4. Phases

Each phase is one pull request, or a short series where noted, and each ends with a pointer to the
next. Phases 1, 4 and 5 do not depend on the design and can run alongside phases 2 and 3.

| Phase | Pull request | Changes behavior |
|---|---|---|
| 0 | This plan, ADR-0032, the roadmap section, and section 11.1's plugin row | no |
| 1 | Stop matching the v0.21 hidden-path fingerprint | yes (old baselines) |
| 2 | The corpus stratum: repositories chosen, pinned, configured and scored with section 12.3's buckets | no |
| 3 | The design document and the version-boundary ADR. **Owner gate:** implementation starts after approval | no |
| 4 | #484: the owned YAML types and the public-API gate; then sealing the five structs, with constructors (a short series) | API only |
| 5 | #482: the differential fuzzing harness, its allowlist, and the pull-request and scheduled runs | no |
| 6 | Named selectors and relations, value-bearing entry rows, the draft v2 schema, and `validate-config`, `list` and `explain` for v2 | v2 only |
| 7 | The `constraint` kind: equality, unique/key and set, report-only | v2 only |
| 8 | The three conformance policies with `alint test` cases, and the section 12.1 measurements | v2 only |
| 9 | If ayaml 0.1.2 qualifies: #483, then #481. Otherwise a status-log entry that moves them | yes (parser) |
| 10 | Documentation, the CHANGELOG (re-run `alint baseline`; the library migration; the `git_tracked_only` change), facts, and the release check | no |

The #503 fix (item 12) is its own pull request, with `add -f` e2e scenarios for `file_absent`,
`dir_absent`, `file_exists` and `dir_exists`, and does not depend on the design. It merged as #509
on 2026-11-13.

The feature freeze is the point at which phases 1 to 8 and the #503 fix are merged. Phase 9 is
decided then.

## 5. Release acceptance

- **v1 is unchanged** apart from item 12. The e2e suite passes, and the corpus replay against
  v0.22.1 shows no differences other than those item 12 explains (ADR-0033 decision 3).
- **The stratum is scored** and recorded in `general-linter.md` section 12, with the bucket shares
  before and after the slice. Phase 8 re-scores it with the shipped slice and compares the result
  with ADR-0033's projections.
- **The three conformance policies** meet section 12.1:
  - each is a documented v2 configuration of bounded length, with firing and silent `alint test`
    cases;
  - for each policy that already has a specialized v1 spelling, that spelling stays shorter;
  - SARIF and the language server reach all of a finding's evidence (the ADR-0012 completeness
    gates);
  - the trust tests pass for v2 sources;
  - no "empty success" mutant survives on the `constraint` kind's error, truncation and
    type-mismatch paths.
- **The public-API gate holds:** no parser type in a public signature. Each sealed struct has a
  constructor that code outside the workspace can use.
- **Performance holds:**
  - the deterministic gate for v1 against v0.18.2;
  - bench-record against v0.22.1;
  - for the v2 policies, wall-clock and instructions against the v1 spellings they re-express.

## 6. Risks

- **Scope.** v0.23 is a long release with several tracks. The owner gate after phase 3 is the point
  to cut the slice. If phases 6 to 8 cannot be ready, v0.23 can ship phases 1 to 5 (and 9) with the
  design, and the slice moves to v0.24. The status log records that choice.
- **Thin demand.** The stratum may show little demand for one of the three policies. The design
  then chooses the assertion families from what the stratum shows, and section 12 records the
  score that the stop rule uses.
- **Experimental churn.** Early v2 users may have to rewrite configurations.
  - `validate-config` names the experimental status.
  - The schema `$id` is versioned.
  - The CHANGELOG lists every v2 change until v2 is stable.
- **The API break's reach.** Sealing `Violation` and `Context` touches every rule's tests and the
  out-of-tree users of `alint-core`. Phase 4 converts the workspace mechanically, adds
  constructors before any literal is removed, and keeps the change apart from behavior changes.
- **ayaml timing.** If ayaml 0.1.2 is released just before the freeze, re-qualification (with the
  #482 harness) may not finish. The freeze date then decides, and #483 and #481 move rather than
  delay the release.

## 7. Status log

- 2026-11-12: plan and ADR-0032 (phase 0, #499).
- 2026-11-12: phase 1 (#500). `check --baseline`, `fix --baseline`, the language server and
  `alint baseline`'s regeneration guard match only the path-free hidden-path fingerprint.
  `baseline::Fingerprints` and `baseline::fingerprints` are gone, and `baseline::apply` takes a
  `String` fingerprinter again, as in v0.21.
- 2026-11-12: phase 2 (#501). The corpus stratum: 20 repositories (10 i18n, 10 JavaScript
  monorepos), 1,558 units after an audit of every unit, configurations in the pinned corpus.
  Today, after the slice, and with entry values: `locale` 23%, 28%, 30%; `dep-version` 43%, 46%,
  57%; `toolchain-pin` 63%, 63%, 63%. The ranking and the inputs to phase 3 are in
  `general-linter.md` section 12.4. Two need the owner's decision: an entry-values adapter for
  the dependency version policy, and `set` against `cardinality` in the slice (a new ADR, since
  ADR-0032 is accepted). The stratum found three alint defects: #502, #503 and #504.
- 2026-11-13: phase 2 review (#505). A check of the stratum records against the frame's rulings, the
  configuration comments and the published figures moved eight units (log, "Review"):
  `locale` is 22%, 26% and 29% today, after the slice and with entry values; the other families
  and the pooled figures are unchanged. Two new `xtask` gates check the records' change entries
  and the configurations' unit labels.
- 2026-11-13: ADR-0033 (#506) records the owner's decisions on the stratum. The slice's
  assertion families are equality, unique/key and set, and `cardinality` moves to v0.24. Phase 6
  adds value-bearing entry rows for JSON, YAML and TOML. #503 is fixed in the behavior (item 12),
  the one exception to "v1 is unchanged". The stratum's rulings stand. Projected under the
  amended slice, with entry values: `locale` 35%, `dep-version` 61%, `toolchain-pin` 63%, pooled
  58%.
- 2026-11-13: the stratum's three defects are fixed, in v0.23 only (the owner's decision), with
  no public API change. #507 (#502): a nested rule that needs the relation caches builds them
  itself. #508 (#504): a nested `key_parity` loads with a templated `reference:` and settles the
  format, `strip_root` and `ignore_keys` per iteration. #509 (#503, item 12): the git-tracked
  views add the tracked paths that only an ignore file hid.
- 2026-11-13: an audit of #506 to #509 hardened the three fixes:
  - a nested rule leaves the run caches alone;
  - nested rules of every kind are prepared with placeholders that their options accept (the
    `_stem_` form), and an error shows the template;
  - the directories above a hidden tracked path are tracked;
  - `fix --dry-run` and the language server agree with `check`;
  - the symlink check costs about 6,000 syscalls on a 20,000-file tree, where it cost 1.26
    million.

  Corpus replays against v0.22.1, of #509 and of this audit (124 repositories replayed each),
  differ in two, gradle/gradle and quarkusio/quarkus, and only in the reworded
  `java-no-tracked-build` message; the reported paths are the same. Both repositories have Java
  packages named `build`, which that rule reports, so its advice to untrack now applies only to a
  directory that holds build output. The four repositories the replays skip fail to load under
  both versions.
