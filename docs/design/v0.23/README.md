# v0.23: selectors, a first constraint slice, and the YAML groundwork

Status: accepted (2026-11-12, ADR-0032); in progress, phases 0 to 5 merged.
Decisions: [ADR-0032](../../adr/0032-v023-owner-decisions.md) (the owner decisions and the scope);
[ADR-0033](../../adr/0033-v023-slice-after-the-corpus-stratum.md) (the slice after the corpus
stratum, and the #503 fix);
[ADR-0034](../../adr/0034-v2-version-boundary-and-phase-3-design.md)
(the `version: 2` boundary and the Phase 3 design);
[ADR-0035](../../adr/0035-keep-the-early-ayaml-cutover.md) (keeping the early ayaml
cutover, and qualifying it before the release);
[ADR-0036](../../adr/0036-ayaml-issues-and-the-saphyr-exit.md) (filing ayaml gaps as ayaml
issues, and the saphyr exit in v0.24);
[ADR-0037](../../adr/0037-phase-3-design-refinements.md) (row-level `equal`, CLDR plurals in
`set`, and the corrections to ADR-0034 after an audit);
[ADR-0038](../../adr/0038-owned-yaml-types-and-the-api-gate.md) (the owned YAML types, the
public-API gate, and a sealed `alint_core::Error`);
[ADR-0039](../../adr/0039-the-yaml-oracle-as-a-dev-dependency-of-alint-core.md) (the harness's
oracle as a dev-dependency, superseded by ADR-0041);
[ADR-0040](../../adr/0040-the-v023-advisory-and-the-allowlist-approval.md) (the security
advisory, and the allowlist approved on ayaml's fixes);
[ADR-0041](../../adr/0041-a-yaml-parser-facade-with-test-only-reference-parsers.md) (the parser
facade with test-only reference parsers);
ADR-0042 (measuring YAML memory and wall time, after the audit);
[ADR-0043](../../adr/0043-trust-and-disclosure-decisions-from-the-post-v0221-audit.md) (the
trust and disclosure decisions of the audit after v0.22.1).

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
   starts. Both were approved on 2026-11-14: [the design](selectors-and-constraints.md) and
   [ADR-0034](../../adr/0034-v2-version-boundary-and-phase-3-design.md),
   which decides each question.
3. **Named selectors and relations,** top-level and inline, on the relation layer, in
   `version: 2` only (ADR-0034 decision 2).
   - Relations emit value-bearing rows of named scalar fields for JSON, YAML and TOML, which the
     dependency version policy needs (ADR-0033 decision 2). The value-bearing modes have their
     own adapter ids, and TOML values come from the TOML rows adapter (ADR-0034 decision 5).
   - Rows combine by grouping, by membership filters, and by an inner equality join with one side
     unique on the key (ADR-0034 decision 6).
   - A v2 source may name another local source's selectors and relations through an `extends:`
     alias (`deps.ranges`; ADR-0034 decision 4).
4. **The `constraint` kind, report-only,** with the assertion families the three conformance
   policies need: equality, unique/key and set (ADR-0033 decision 1). `set` covers inclusion,
   equality and disjointness, also per group. It has no fixes (section 8.3). `equal` also compares
   two fields of one row, which a join's output needs, and `set` can collapse CLDR plural groups as
   `key_parity` does (ADR-0037).
5. **Three conformance policies** as `version: 2` configurations: locale key parity (re-expressed
   from `key_parity` as per-locale set equality), dependency version consistency, and
   toolchain-pin consistency. Each ships firing and silent `alint test` cases and is measured
   against section 12.1.
6. **`version: 2`, experimental.**
   - The loader reads `version:` before any typed decode and dispatches on it. v2 accepts every
     v1 key and kind with v1's meaning (ADR-0034 decision 1).
   - In v0.23, v2 sources are local files; a bundled or remote source must be `version: 1`.
   - A draft `schemas/v2/config.json`, with a versioned `$id`.
   - `validate-config` for both versions; `list`, `explain` and `export-agents-md` render named
     selectors and relations.
   - `init` and `suggest` keep emitting v1. The documentation marks v2 experimental, and
     `validate-config` says so for a v2 configuration.

Also in v0.23:

7. **The v0.21 hidden-path fingerprint is no longer matched** (ADR-0030 decision 1, ADR-0032
   decision 5).
8. **YAML parser types leave the public API (#484)** for opaque alint-owned types (ADR-0032
   decision 3).
9. **`Violation`, `Context`, `RuleSpec`, `NestedRuleSpec` and `WalkOptions` are sealed** with
   constructors and setters (ADR-0032 decision 3), and so is `alint_core::Error`, without them
   (ADR-0038). A v1.0 item audits the other open public structs.
10. **Differential YAML parser fuzzing (#482).**
11. **The ayaml cutover is qualified before the release** (ADR-0035). #510 moved `main` to ayaml
    0.1.2 ahead of #482 and #483. Before v0.23.0, the #482 harness compares ayaml with
    `yaml_serde` and the `yaml_depth` guard is re-derived from ayaml's tokens (#483); #481 closes
    with the release. A difference the owner does not approve holds the release or reverts #510.
    This replaces ADR-0032 decision 2's "cut over only if ayaml qualifies before the freeze".
12. **`git_tracked_only: true` sees tracked files that an ignore file excludes (#503,** ADR-0033
    decision 3). Up to v0.22 the walker pruned such a file before the git-tracked index was built,
    so the documented case (a force-added file, or one committed before its ignore line) was never
    seen. The config's `ignore:` and the walker's other exclusions still apply. A v1 configuration
    that uses `git_tracked_only` can gain findings (`file_absent`, `dir_absent`) or lose them
    (`file_exists`, `dir_exists`). The kinds that check that a path resolves say "missing or
    ignored" where they said "on disk", and the bundled `java@v1` and `php@v1` rules that these
    changes touch reword their messages to match. This is the one exception to section 3's "v1 is
    unchanged". Done in #509.
13. **The sandbox and trust fixes.** v0.20.0 to v0.22.1 ran a spawning rule nested in `require:`
    without the spawn capability (#515), and passed a test's `given.env` to the Git commands the
    runner runs itself, so `GIT_CONFIG_*` or `LD_PRELOAD` could run code without it (#519). Both
    ship in v0.23.0 only, with no v0.22.2 (the owner's decision, 2026-11-14). The same releases
    read a fixture shaped like a bare repository as one, so a `core.fsmonitor` in it ran a
    command, and let a test's `when:` conditions and `${VAR}` expansions read the host's
    environment (#522). The review of #522 and its fix found eight more (#527), among them a
    chain of `given.tree` symlinks that wrote outside the sandbox, a fixture symlink that read a
    host file from the copy, and a fixer that could write `.git`, so Git ran a command from a
    configuration it wrote, in `alint test` and, from a ruleset `extends:` reads from the local
    tree, in `alint fix` since v0.2.1. ADR-0040 counted three of the eight. The audit after
    v0.22.1 found 13 more, in the language server, fixes, rule reads, configuration discovery
    and loading (status log, 2026-11-16). All ship in v0.23.0, fixed in public pull requests,
    with no v0.22.2. Security advisory GHSA-78jj-xfpm-g6px, a private draft now, covers every
    one and is published with v0.23.0 (ADR-0040, widened by ADR-0043).

Out of scope:
- **v0.24 or later:** the remaining assertion families (reference, cardinality, group, bounded
  count), the specialized kinds and templates of DEC-8, and a stable `version: 2`.
- **v0.24:** saphyr and saphyr-parser leave the release build. After #482 and #483, source
  spans, structured YAML fixes, `yaml_depth`'s literal-subtree check and `pinned_references` read
  ayaml's events instead (ADR-0036). saphyr stays as a test-only reference parser behind
  `alint-core`'s `yaml-reference` feature (ADR-0041).
- **Plugin hosts:** these start in v0.24 (ADR-0032 decision 4).
- **Phase 4:** stale exceptions.

## 2. Where the code stands

- **Relation layer** (`crates/alint-core/src/relation/`, about 3,600 lines, or 4,800 with its
  adapters).
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
  - Toolchain-pin consistency is `alint://bundled/toolchain-pins@v1`: 24 rules, 18 of them
    `cross_file`, over 17 facts, with `sync_from` fixes.
  - Dependency version consistency (gap C3) has no kind and no bundle.
- **Hidden-path fingerprints.** v0.22 writes the path-free form and still matches the v0.21 form,
  in `crates/alint-core/src/baseline.rs` and `crates/alint/src/check.rs`. The language server
  has a test for that matching.
- **Parser types in the public API.** The #484 list:
  - in `alint-core`: the `extra` fields and accessor, configuration masking, template rendering,
    and `Error::Yaml`;
  - six `alint-dsl` functions;
  - four `alint-testkit` items.

  alint parses YAML with ayaml 0.1.2 since #510, through `alint_core::yaml`. saphyr and
  saphyr-parser still give source spans and structured fixes; they leave in v0.24 (ADR-0036).
- **Open structs.**
  - `Violation`, `Context`, `RuleSpec`, `NestedRuleSpec` and `WalkOptions` were built with struct
    literals outside `alint-core` until phase 4 sealed them (#517, #518).
  - Excluding `alint-core`'s own source, the literal constructions numbered 23, 34, 1, 0 and 19,
    77 in all, tests included.
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
  - the corpus replay against v0.22.1, with no difference that the CHANGELOG does not explain.

  Item 7 does not change what alint writes, only which old baseline entries it matches. Item 12 is
  one exception (ADR-0033 decision 3): a configuration that uses `git_tracked_only` can gain or
  lose findings, three kinds and six bundled rules reword a message, and the CHANGELOG lists the
  change. The ayaml cutover (phase 9) is the other: it reads some YAML differently and words
  parse errors differently, and the CHANGELOG lists each difference (ADR-0035 decision 5). Every
  difference in the replay must come from one of them. The replay of `main` at e7bb08a57 against
  v0.22.1 (run 43927008138) found three: item 12's reworded `java-no-tracked-build` message in
  gradle/gradle and quarkusio/quarkus, and the reworded message for a stream of more than one
  document in istio/istio.
- **The version boundary of ADR-0022 holds.** A v2 source may extend v1 sources, never the
  reverse, at any recursive edge.
- **Trust is no weaker than v1** (section 7.1 rules 9, 11 and 12):
  - every named object carries its provenance;
  - references resolve by namespace, and reach another source only through an `extends:` alias;
  - an inherited name never shadows a local one;
  - the planner refuses a rule that would gain a capability through what it references.

  In v0.23 a v2 source is a local file (ADR-0034 decision 4). The section 12.2 trust tests cover
  aliased references between local sources and the refusal of a bundled or remote v2 source.
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
  parser's types. Phase 4 added `Sequence` and `Location` (ADR-0038).
  - `Value` and `Mapping` implement `Serialize` and `Deserialize`, offer the reads and edits that
    rules, templates, redaction and the loader use, and deserialize into a typed options struct.
  - `Error` gives a message and, where the parser has one, a location.
- **The parser stays private.** No public item names the parser crate; it appears only in the
  private fields of the `yaml` types. Workspace crates that parse YAML internally may keep calling
  the parser until the cutover, converting at the API boundary.
- **A gate.** Either a public-API check or a scan of the generated documentation fails CI if a
  public item of `alint-core`, `alint-dsl` or `alint-testkit` names the parser crate. The phase
  picks one that runs on stable Rust. It picked the scan, of documentation built with hidden
  items shown (ADR-0038).
- **Library users migrate once.** The CHANGELOG lists each changed signature with its
  replacement.

### 3.2 The fuzzing harness (#482)

- **What it compares.** It runs `yaml_serde` and ayaml through the entry points alint uses:
  `Value` parsing, the JSON tree the structured-format rules query, and Serde typed decoding into
  `Config` and of each rule into `RuleSpec`. Since #510 `yaml_serde` is the oracle only:
  `alint-core` depends on it behind the test-only `yaml-reference` feature, and no published
  crate depends on it at run time (ADR-0035, ADR-0041). alint reads no ayaml events in v0.23, so
  the event stream joins the harness with the saphyr exit in v0.24.
- **Inputs.** Generated token soup and documents, alint's committed YAML, and mutations of both.
- **Results.** Each input is classified into six classes: #482's five, with "only one rejects"
  split by which parser rejects. Documented deviations are an explicit allowlist, and any other
  difference fails.
- **When it runs.** A bounded run in `cargo test`, which CI's test job runs when a pull request
  or a push changes the crates, the workspace manifests or CI, and a long run on pull requests
  that touch YAML parsing, the configuration types, the manifests or any YAML file, weekly and on
  dispatch.
- **Against which ayaml.** It runs against the pinned release, and against ayaml's main branch
  through a `[patch.crates-io]` entry when re-qualifying, which applies while main's version
  still satisfies the exact pin.
- **As built (phase 5).** `docs/design/yaml-parser.md` describes the harness and the five groups
  of documented differences.

## 4. Phases

Each phase is one pull request, or a short series where noted, and each ends with a pointer to the
next. Phases 1, 4 and 5 do not depend on the design and can run alongside phases 2 and 3.

| Phase | Pull request | Changes behavior |
|---|---|---|
| 0 | This plan, ADR-0032, the roadmap section, and section 11.1's plugin row | no |
| 1 | Stop matching the v0.21 hidden-path fingerprint | yes (old baselines) |
| 2 | The corpus stratum: repositories chosen, pinned, configured and scored with section 12.3's buckets | no |
| 3 | The design document and the version-boundary ADR. **Owner gate:** implementation starts after approval. Approved in #512 (ADR-0034) | no |
| 4 | #484: the owned YAML types, the public-API gate and a sealed `alint_core::Error` (4.1); constructors and setters for the five structs, with the workspace converted (4.2); then sealing the five (4.3) | API only |
| 5 | #482: the differential fuzzing harness, its allowlist, and the pull-request and scheduled runs; then the parser facade with test-only reference parsers and a compare mode, in three short phases (ADR-0041, `yaml-parser.md`) | no |
| 6 | The version dispatch, named selectors and relations, per-source namespaces with `extends:` aliases, value-bearing entry rows, the equality join, the draft v2 schema, and `validate-config`, `list`, `explain` and `export-agents-md` for v2 | v2 only |
| 7 | The `constraint` kind: equality (with row-level `equal`), unique/key and set (with `plurals: cldr`), report-only, and the related-location gate over JSON, SARIF and the language server | v2 only |
| 8 | The three conformance policies with `alint test` cases, and the section 12.1 measurements | v2 only |
| 9 | Qualify the cutover that #510 made: #483, and the #482 harness's verdict with an approved allowlist. Otherwise the owner holds the release or reverts #510 (ADR-0035). Approved on 2026-11-15, on condition that v0.23.0 is tagged on an ayaml release that fixes asamarts/ayaml#132, #133 and #137 (ADR-0040). Since 2026-11-16 the condition is an ayaml release with the fixes ayaml's main has for every gap alint filed, #132, #133, #137, #139, #140, #141, #146 and #147, and ADR-0042's condition on memory (ADR-0043). The bump follows `yaml-parser.md`'s "Bumping ayaml" | yes (parser) |
| 10 | Documentation, the CHANGELOG (re-run `alint baseline`; the library migration; the `git_tracked_only` change; the affected versions of item 13's fixes), facts, publishing the security advisory drafted for item 13 (GHSA-78jj-xfpm-g6px, a private draft; ADR-0040, widened by ADR-0043; step 5 of RELEASING.md's documentation checklist), and the release check | no |

The #503 fix (item 12) is its own pull request, with `add -f` e2e scenarios for `file_absent`,
`dir_absent`, `file_exists` and `dir_exists`, and does not depend on the design. It merged as #509
on 2026-11-13.

The feature freeze is the point at which phases 1 to 8 and the #503 fix are merged. Phase 9 is
decided then.

## 5. Release acceptance

- **v1 is unchanged** apart from item 12 and the ayaml cutover. The e2e suite passes, and the
  corpus replay against v0.22.1 shows no difference that the CHANGELOG does not explain: item
  12's (ADR-0033 decision 3) and the cutover's (ADR-0035 decision 5).
- **The stratum is scored** and recorded in `general-linter.md` section 12, with the bucket shares
  before and after the slice. Phase 8 re-scores it with the shipped slice and compares the result
  with ADR-0033's projections.
- **The three conformance policies** meet section 12.1:
  - each is a documented v2 configuration of bounded length, with firing and silent `alint test`
    cases;
  - for each policy that already has a specialized v1 spelling, that spelling stays shorter;
  - SARIF, JSON and the language server reach all of a finding's evidence, under a completeness
    gate phase 7 adds;
  - the trust tests pass for local v2 sources and refuse bundled and remote ones;
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
- **ayaml qualification.** `main` runs ayaml since #510, before the harness that qualifies it.
  If #483 is not done by the freeze, or the harness of #482 (merged in #521) or #483 finds a
  difference the owner does not approve, the release waits or #510 is reverted (ADR-0035). The
  corpus replay of every pull request compares against v0.22.1, so a difference on real
  repositories shows up before then.

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
- 2026-11-13: the stratum's three defects are fixed, in v0.23 only (the owner's decision). What
  they add to the public API serves alint's own crates and is hidden from the documentation (the
  placeholder helpers only since the audit of 2026-11-16). #507 (#502): a nested rule that needs
  the relation caches builds them itself. #508 (#504): a nested `key_parity` loads with a
  templated `reference:` and settles the format, `strip_root` and `ignore_keys` per iteration.
  #509 (#503, item 12): the git-tracked views add the tracked paths that only an ignore file hid.
- 2026-11-13: an audit of #506 to #509 (#511) hardened the three fixes:
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
- 2026-11-14: phase 3 (#512). The owner approved [the design](selectors-and-constraints.md)
  and [ADR-0034](../../adr/0034-v2-version-boundary-and-phase-3-design.md),
  taking each question in turn.
  - v2 is a superset of v1, dispatched on `version:` before any typed decode, and only local
    files may be v2 in v0.23.
  - Under `--changed`, a full-index rule's path scope decides whether it runs, never what it
    sees. This amends constitution invariant 2.
  - Two outcomes differ from the recommendations: names reach another source through an
    `extends:` alias, and v0.23 adds an inner equality join with one side unique on the key.
  - Phase 6 grows by the version dispatch, the per-source namespaces and the join. If it cannot
    carry the join before the feature freeze, this log records the move (ADR-0034 consequences).
- 2026-11-14: #510 moved `main` from `yaml_serde` to ayaml 0.1.2, ahead of #482 and #483, so
  item 11's condition did not hold. The owner kept it and moved the qualification before the
  release ([ADR-0035](../../adr/0035-keep-the-early-ayaml-cutover.md)).
- 2026-11-14: an audit of #510 to #512 (#513) fixed:
  - **a regression from #510:** ayaml's default caps on events, collection entries, anchors and
    alias uses refused valid files that v0.22 reads (a 9 MB file failed with "events limit
    exceeded"). Every ayaml decode now goes through `alint_core::yaml`, which lifts those caps
    and keeps the input, depth and alias-expansion limits; a dogfood rule keeps other files from
    calling ayaml's decoders;
  - **from #511:** a nested rule's load error turned text that looked like a rendered placeholder
    (`json_path_equals`, `pkg_dir_layout`) back into a token; only whole rendered values of the
    spec's own templates are restored now;
  - **older:** where the file system ignores case, `fix --dry-run` said a create resolved its rule
    when the path existed under another spelling, which the real fix reports as unresolved;
  - **CI:** the Windows and e2e `cargo test --workspace` runs stopped at the first failing test
    binary; they run every binary, and a dogfood rule keeps `--no-fail-fast` in CI;
  - the parser's observable differences from v0.22 are in the CHANGELOG.
- 2026-11-14: design corrections (#514).
  - An audit of the Phase 3 design and ADR-0034 found 31 issues: five high (H1 to H5), 16
    medium (M1 to M16) and ten low. #513 fixed the stale YAML text (H5). The design now fixes the
    rest, adds syntax tables for relations and `constraint`, and adds a false-positive section.
  - Two owner decisions ([ADR-0037](../../adr/0037-phase-3-design-refinements.md)): `equal`
    compares two fields of one row, which makes the join usable (nextcloud-b02's equality part),
    and CLDR plural collapse is `set`'s `plurals: cldr`, decided per pair of groups as
    `key_parity` does.
  - ADR-0037 also records the corrections that change an ADR-0034 decision: path-free
    `constraint` fingerprints, groups with no rows, the invariant 2 test (replaced, not
    extended), a required `unique:` side, version dispatch for fragments and non-literal versions,
    literal names, `read:` formats, `constraint`'s registration and the facts field.
  - [ADR-0036](../../adr/0036-ayaml-issues-and-the-saphyr-exit.md): ayaml gaps are filed as ayaml
    issues (asamarts/ayaml#123 to asamarts/ayaml#128), and saphyr leaves in v0.24.
- 2026-11-14: phase 4.1 (#516), [ADR-0038](../../adr/0038-owned-yaml-types-and-the-api-gate.md).
  - No public item of `alint-core`, `alint-dsl` or `alint-testkit` names a YAML parser crate.
    `alint_core::yaml` holds alint's own `Value`, `Mapping`, `Sequence`, `Error` and `Location`,
    which wrap the parser's privately. The `extra` fields, the YAML error variants and the
    `TreeSpec` methods use them; the `render_*` functions and six `alint-dsl` rejection
    functions became crate-private.
  - `ci/scripts/public-api-parser-gate.sh`, run by `docs.sh`, documents the three crates with
    their dependencies and hidden items shown, and fails on a page that links to a parser crate.
  - `alint_core::Error` is `#[non_exhaustive]`. The ROADMAP's v1.0 item says what v0.23 seals,
    and a new v1.0 item audits the other open public structs.
  - cargo-mutants over the phase showed that `alint test` ran a spawning rule nested in
    `require:` without the spawn capability, since v0.20.0; #515 fixes it.
  - Next: phase 4.2, constructors and setters for the five structs.
- 2026-11-14: phase 4.2 (#517).
  - `Context`, `RuleSpec`, `NestedRuleSpec` and `WalkOptions` have a constructor for their
    required parts and a `with_*` setter for each other field, as `FixContext` and `Config` do,
    and `Violation` gains three setters, which the review after phase 4 removed again: its
    fields stay public to assign. `Context` is `Clone`.
  - The workspace builds the five through them. A build with the five marked
    `#[non_exhaustive]` found every struct literal outside `alint-core`: 77 sites in
    `alint-rules`, `alint-output`, `alint`, `alint-lsp`, `alint-dsl`, `alint-testkit` and
    `alint-e2e`, most of them in tests.
  - Next: phase 4.3, `#[non_exhaustive]` on the five.
- 2026-11-14: phase 4.3 (#518).
  - `Violation`, `Context`, `RuleSpec`, `NestedRuleSpec` and `WalkOptions` are
    `#[non_exhaustive]`, which completes ADR-0032 decision 3: with `alint_core::Error` from 4.1,
    `FixContext`, `Config` and `CompiledNestedSpec` from v0.22, nine `alint-core` types are
    sealed.
  - A doctest on a `#[cfg(doctest)]` item keeps them sealed: for each of the nine, a block that
    builds it with a functional record update, or matches every variant of `Error` without a
    wildcard arm, must fail to compile.
  - Next: phase 5, the differential fuzzing harness (#482).
- 2026-11-14: the owner decided that #515 ships in v0.23.0 and that there is no v0.22.2.
  SECURITY.md supports only the latest minor. Whether to publish a GitHub security advisory is
  decided before the release (phase 10).
- 2026-11-14: the review of #515 to #518 found that `alint test` passed `given.env` to its own
  Git commands, the `given.git` setup and alint-core's Git reads, since v0.20.0: `GIT_CONFIG_*`
  could name an fsmonitor or filter command, and `LD_PRELOAD` a library, with no spawn
  capability, and the setup ran before the capability check. Those commands now get a fixed
  environment; the fix ships with #515 in v0.23.0 (scope item 13). The nested-rule check now
  follows `require:` only, as the trust gate does.
- 2026-11-14: the review after phase 4 (#520).
  - A corpus replay of 126 repositories, `main` at #518 against the commit before #515, found no
    difference in `check` output.
  - **The public-API gate** documents the three crates with every feature, strips each spelling
    of `doc(hidden)`, and fails on a `cfg` or `cfg_attr` predicate on `doc` and on an exported
    macro that names a parser crate. Any documented crate whose name contains `yaml` or `saphyr`
    counts as a parser. `test-public-api-parser-gate.sh` checks each rule.
  - **The sealed-type doctests** each have a compiling twin, so a block that fails for another
    reason is caught, and a unit test checks that the `Error` blocks name every variant.
  - **The owned YAML types** gained the conversions, predicates, `tag()`, indexing, mutable and
    owned iteration and `Extend` that 0.22's types had; `value[0]` finds an integer key, as
    before. The CHANGELOG lists what remains different.
  - **Removed:** `Violation`'s three new setters and `RuleSpec::with_sensitive_fields`, whose
    fields are assigned; the unused `miette` dependency; the redundant `homepage` keys; and
    `alint-bench`'s build dependencies on crates only its benches use.
  - The records now give the counts phase 4 measured.
  - **Mutation testing.** Three mutants that make an owned iterator endless allocated until the
    hosted runner was lost, twice. `ci/scripts/mutants.sh` now caps each process's address space
    at 12 GiB with no core files, so such a mutant counts as caught; a test pins each iterator's
    length and reverse order, which 15 mutants had escaped.
  - Next: phase 5, the differential fuzzing harness (#482).
- 2026-11-14: phase 5, the differential harness (#482).
  - `alint-core` compares ayaml with `yaml_serde` 0.10.7 at the value, JSON and typed-decode entry
    points, over generated token soup and documents, the repository's YAML, and mutations of
    both. The bounded run is an ordinary test; `yaml-differential.yml` runs the long run on
    parser and lockfile changes, weekly and on dispatch.
  - The corpus has no difference. Over 24 seeds and 7.66 million inputs, every difference is
    documented: 23 deviations and 5 location classes, in five groups (`yaml-parser.md`). Two
    are ayaml defects in malformed documents, filed as asamarts/ayaml#132 and #133.
  - The CHANGELOG now lists each change in what alint accepts and reads. The event stream joins
    the harness in v0.24.
  - Next: #483 and phase 6; phase 9 brings the allowlist to the owner.
- 2026-11-15: the review of #520.
  - **Correction to the 2026-11-14 entry on #520.** The gate did not strip each spelling of
    `doc(hidden)` (`#[doc(alias = "x", hidden)]` stayed hidden), and the owned YAML types did not
    have all that 0.22's types had: `&value == 1`, `Value::from(vec![1, 2])`, `value.get(0)` and
    slicing a `Sequence`, among others, compiled against 0.22 and fail now, and
    `mapping["missing"]`, which panicked, is null. (`mapping["a"] = v` compiles, as it did:
    this change adds index assignment, which adds a missing key where 0.22 panicked.) The
    CHANGELOG under-listed the differences; it and `yaml-parser.md` now list every one.
  - **The owned YAML types** gain `Value::get_mut`, indexing by `&String`, index assignment on
    `Mapping`, `Mapping::values_mut`, concrete key and value iterators, and `FusedIterator` and
    `Clone` on the iterators. `tag()` documents what it returns for verbatim, `%TAG` and core
    tags, and assigning a key on a tagged null no longer panics.
  - **The public-API gate** reads every `.rs` file under each crate, with literals and comments
    removed as Rust reads them, and fails on a `hidden` in any `doc(...)` list, an attribute a
    macro builds, a `cfg` on `doc` inside a `cfg_attr`, a public item under a platform,
    profile or `not(feature)` cfg, an `include!` or `#[path]` outside the crate, an exported
    macro in brackets, and a crate without `#![deny(unnameable_types)]`. The three crates deny
    that lint, and `alint-core` exports the 14 fix-operation option structs it made nameable.
    A hidden, feature-gated canary in each crate must show up in every scan, so a build that
    drops `--all-features`, the stripping or the dependencies' documentation fails.
  - **Mutation testing** runs cargo-mutants in a transient systemd scope whose `MemoryMax` is
    the 12 GiB cap, where sudo allows it, so several runaway test processes together cannot
    exhaust the runner; the per-process cap stays.
  - Next: #483 and phase 6; phase 9 brings the allowlist to the owner.
- 2026-11-15: two review rounds of the differential harness.
  - An input-wide entry now explains a difference only when its trigger causes it: the harness
    rewrites the input without the trigger and compares again. One difference can no longer mask
    another: a rewrite after which a parser rejects what it read, or both reject for an error the
    rewrite made, an error that moves to an earlier line, two errors a location class would take
    for one, and a value error in place of a syntax error each have a rule, and a test puts
    differences of three kinds beside every input-wide entry's construct.
  - The harness also decodes each rule into `RuleSpec`, each rule a case of its own, generates
    configurations and the shapes the review found missing, and its workflow watches every YAML
    file.
  - 42 long runs, 13.40 million inputs, leave no unexplained difference: 33 deviations and 7
    location classes (`yaml-parser.md`). The review found ayaml defects in malformed documents,
    filed as asamarts/ayaml#137 and #138 and as a comment on #132, and documented one more
    difference, where ayaml reads YAML 1.2 and libyaml does not: `[!!str :a]` holds the string
    `:a`, which libyaml reads as a mapping.
  - Next: #483 and phase 6; phase 9 brings the allowlist to the owner.
- 2026-11-15: #522 and #483.
  - **#522** closes two more ways a policy test reached outside its fixture: alint's own Git
    reads took a fixture shaped like a bare repository for one and ran its `core.fsmonitor`, and
    `when:` conditions and `${VAR}` expansions read the host's environment. Every Git command
    alint runs sets `safe.bareRepository=explicit`.
  - **#483** moves the `yaml_depth` lexer to the tokens of ayaml's compatibility reader and drops
    the ayaml 0.1.1 shims. Re-deriving it found five ways the old lexer hid structure that ayaml
    reads. Long runs of the parity property found a hang in the new lexer, fixed before it
    merged, and three places where ayaml's compatibility reader disagrees with itself
    (asamarts/ayaml#139, #140, #141). ayaml bounds depth and aliases itself, so none of them let
    a bomb through. After the last fix, 12 million parity cases found nothing. Mutation testing
    then found 45 lexer mutants that no test caught: the code behind the seven that changed no
    token is gone, and for each of the others the tests hold the inputs where it reads
    otherwise, as ayaml decides them.
  - Next: phase 6; phase 9 brings the allowlist to the owner.
- 2026-11-15: the owner's decisions on the advisory, the allowlist and a parser facade.
  - **The advisory** (ADR-0040): no v0.22.x patch. A GitHub security advisory, drafted as
    GHSA-78jj-xfpm-g6px and published with v0.23.0, covers the four fixes of #515, #519 and
    #522, and the holes the review of #522 found, which #527 fixes in public like the first
    four: a chain of `given.tree` symlinks that wrote outside the sandbox, a fixture symlink
    that read a host file from the copy, and a fixer that could write `.git`, so that Git ran a
    command from a configuration it wrote, in `alint test` and, from a ruleset `extends:` reads
    from the local tree, in `alint fix` since v0.2.1.
  - **The allowlist** (ADR-0040): approved, on condition that v0.23.0 is tagged on an ayaml
    release that fixes asamarts/ayaml#132, #133 and #137, after the ayaml-bump procedure. A
    difference the bump adds comes back to the owner.
  - **A parser facade** (ADR-0041, superseding ADR-0039): `alint_core::yaml` parses through a
    backend, and alint-core's test-only `yaml-reference` feature adds libyaml and a compare mode
    that checks every decode in the workspace's tests against libyaml and saphyr, so ayaml can
    be held to older parsers while it is developed.
  - The reviews of #522 to #525 found the holes above, gaps in the public API gate and the
    mutation script's memory limit (#523), two constructs that hid nesting from the depth guard,
    which ayaml's own limits still stopped (#524), and ways the harness could explain a
    difference its trigger did not cause (#525). The pull requests that follow fix them.
  - Next: those fixes and the facade; then phase 6.
- 2026-11-15: the review of #523.
  - **Correction to the 2026-11-15 entry on the review of #520.** It listed `mapping["a"] = v`
    among what no longer compiles; #523 itself made it compile, and the entry now says so. The
    lists of differences named `untag`, which `yaml_serde` 0.10.7 keeps private, and missed
    `IntoDeserializer`, the `Deserializer` on `&Value`, and the entry types and owned key and
    value iterators of `yaml_serde::mapping`; the CHANGELOG and `yaml-parser.md` now list them.
  - **The public-API gate** fails on any `cfg` that is not on features or tests, wherever it is,
    unless its allowlist names it with a reason, and skips test-only code; the review's shapes
    (a `cfg(windows)` module of impls, a `const _` block, a variant, a trait item, a
    `cfg(panic = "abort")` function) passed before. It also fails on any `$` in an attribute,
    `cfg_select!`, a renamed or macro-passed `include`, and an exported macro naming a parser
    as a bare word or with a raw name, and its scan no longer counts a prose link to a parser's
    issue. `yaml-parser.md` records why `ref_cast`'s hidden impls stay outside the gate.
  - **Mutation testing.** A transient scope stopped everything in it, cargo-mutants included,
    when the kernel killed one test process at its `MemoryMax`, so a runaway mutant ended the
    run with nothing reported. The scope's `OOMPolicy` is `continue` now, and the self-test
    checks it in a real scope where the user's systemd allows one.
  - `Mapping`'s documentation says its string-key methods find untagged keys only, and tests
    pin that and the auto traits of every owned type.
  - Next: the other review fixes and the facade; then phase 6.
- 2026-11-15: the review of #525.
  - **A correction to the 2026-11-15 entry on two review rounds of the differential harness.** Its
    long runs left no unexplained difference partly because a location class took two errors for
    one: ayaml's error and the oracle's at another place, among them the oracle's "more than one
    document", which has no place, and its error about a character its reader refuses, which it
    gives before its scanner reads a token. And the test that puts differences beside the input-wide
    entries' constructs left three out: the implicit-key limit counted in characters, the typed
    decode's core tags and the content of a top-level block scalar.
  - A location class now explains two errors only when no prefix of the input shows them to be
    two. It never takes the oracle's "more than one document" when the oracle read the line of
    ayaml's error, and only the class about a character takes the oracle's reader error. A
    rewrite that turns an ayaml value error into a syntax error hides the value error. A rewrite
    changes its construct only, which a defect planted in the text around it checks with no
    tolerance. Two rule errors are one only when they agree on the field and on the problem. An
    explanation keeps no entry the case does not need, and ayaml's alias errors are known by
    their messages, since ayaml 0.1.2 gives no error of kind `Alias` or `Tag`.
  - Where ayaml's scanner reads a malformed document otherwise than YAML 1.2, the rewrites follow
    it: the tabs it rejects, the characters it keeps in what it reads as a quoted scalar, the
    indentation it reads a line with and the `!<!>` it rejects. `!<!>` becomes `!<!t>`, so that a
    node keeps its properties.
  - The masking test now covers the key limit at the value and JSON entry points and the typed
    decode's core tags beside `-0`. A top-level block scalar's content takes in the lines around
    it, so no difference can sit beside it. The five entries that no long run used now explain
    differences, and the allowlist holds 39 deviations, 5 of them new, and 8 location classes,
    one of them new: `!<>`, which names nothing and which both parsers reject.
  - Long runs with seeds 1, 2, 3, 7 and 11, 1.60 million inputs, leave no unexplained
    difference. They make no implicit key of more than 1,024 bytes and at most 1,024 characters,
    so `implicit-key-length-in-characters` is checked by its example and the masking test alone.
  - Two more ayaml defects, filed as asamarts/ayaml#146 and #147. In a valid document, a
    document marker after a top-level block scalar's empty lines, one of them indented, is read
    as content, as in `|`, a line of two spaces and `...`
    (`document-marker-after-indented-empty-lines`). And a block scalar header after properties
    alone on their line, as a tag alone under `-`, is accepted under its parent's indentation,
    by both of ayaml's readers, where #132 is the compatibility reader's after an anchor.
  - **Mutation testing.** cargo-mutants over the review's changes left 59 of 646 mutants that no
    test caught. For 49 of them a test now holds an input the mutant reads otherwise: how fair a
    rewrite is that reveals a parser error of ayaml's, where a split cuts and what it counts
    against the comparison budget, a tie for the first error, which lines stand alone after a
    prefix, a narrow entry past a revealed error, a rule entry with one of its two errors, the
    places of a document marker and of a quoted line that starts with `%`, the tabs that are
    content, the characters ayaml may not have read, and the predicates' lines and tags. The
    code behind 8 that no input tells apart is gone, among it the fields a rule needs in the
    rule-error check: serde reports a field's own error before it finds one missing, so the
    field alone shows it. Of the other two, one is a shortcut that only saves a search, and one
    is the stub that stands for the planted defects outside the tests, which no test build
    compiles. The long runs read the same after the changes.
  - Next: the parser facade; then phase 6.
- 2026-11-16: long runs with seeds 4 to 12.
  - Seeds 4, 6, 8, 9, 10 and 12 left 18 unexplained cases from 10 inputs, none a new ayaml defect.
    Four were harness bugs: a rewrite that turned ayaml's value error into its "more than one
    document" was taken to hide the value error, though ayaml reports a second document only after
    the first one's value holds no error; a byte-order mark past a line's start counted as white
    space; and the tab rewrite kept a tab before a closing bracket or a comment on a flow line,
    which ayaml exempts from a flow line's indentation, and a tab ayaml rejects only because a tab
    before it ended a block scalar. The search now also splits a case whose first error, the
    oracle's, is in a flow collection ayaml read: the prefix through the collection's line, and the
    input with the collection empty. `tab-as-white-space` now describes the flow lines, and
    `block-scalar-under-an-anchor` is an input-wide entry, so a difference beside it needs its own.
  - Long runs with seeds 1 to 12, 3.83 million inputs, leave no unexplained difference.
    cargo-mutants leaves no mutant of the changes uncaught; two make the tab rewrite's loop endless
    and time out.
- 2026-11-16: the parser facade (ADR-0041, #530).
  - `alint_core::yaml` decodes text through `yaml_backend`, ayaml by default. alint-core's test-only
    `yaml-reference` feature adds `ALINT_YAML_PARSER=libyaml` and `compare`: in compare mode a
    decode returns ayaml's result, and a difference from libyaml that neither saphyr's reading nor
    the harness's allowlist explains panics. CI's `YAML compare` job runs the workspace's tests that
    way, `ci/scripts/yaml-reference-gate.sh` fails if a build without the feature depends on
    `yaml_serde`, and a dogfood rule keeps `yaml_serde::` calls in the facade and the harness.
  - The workspace's 4,465 tests pass in compare mode, with 40 differences explained: 35 by the
    allowlist, 4 by saphyr's vote, and one typed decode put down to core tags. The long runs'
    reports count no difference that only the vote explains: the allowlist explains every one. Tests
    that measure ayaml itself, the depth guard's and the harness's, decode with ayaml under any
    backend; three of #529's failed under `libyaml` before.
  - **For the owner, in phase 9.** The allowlist approved in ADR-0040 had 33 deviations and 7
    location classes. The reviews since added 6 deviations and 1 class:
    `anchor-name-before-a-colon-and-a-flow-indicator` (asamarts/ayaml#139) and
    `document-marker-after-indented-empty-lines` (asamarts/ayaml#146), ayaml defects;
    `empty-flow-implicit-key`, a YAML 1.2 reading; `negative-zero-in-an-unsigned-field`, the core
    schema's `-0`; `rule-that-is-null` and `enum-as-a-one-entry-mapping`, rules both parsers refuse
    with other messages; and the class `verbatim-tag-that-names-nothing`, `!<>`, which both reject.
  - **ayaml.** 0.1.3 was published on 2026-11-15 without fixes for asamarts/ayaml#132, #133 and
    #137, so v0.23.0 waits on a later release. #526's description said that the release "fixes
    asamarts/ayaml#132", which GitHub read as a closing keyword, so merging it closed that issue; it
    is reopened. The two defects the review found are asamarts/ayaml#146 and #147.
  - Next: phase 6; phase 9 brings the allowlist, with the entries above, to the owner.
- 2026-11-16: the audit of the changes since v0.22.1: engine, templates and rules.
  - **Correction to the 2026-11-13 entry on the stratum's three defects.** It said the fixes
    changed no public API. #508 and #511 added `PathTokens::placeholder`, `has_placeholder`,
    `first_placeholder`, `same_iteration_path`, `unrender_placeholders` and
    `NestedRuleSpec::templates` to `alint-core`'s documented API, though they work only inside
    the hidden `template::Preparation`. They are hidden now, like `Preparation`, for alint's own
    crates, and the entry now says so. `templates_rendering` replaces the three that read a
    placeholder's spelling.
  - A nested `key_parity` decides at load from the templates the configuration wrote, not from
    the rendered text, so a name spelled like a placeholder (`data/_stem_`) is a load error, as
    at the top level. On Windows it reads a reference that `{path}` rendered with `\` as the
    index spells it. Glob `peers` are tested only under their literal directories.
  - The git-tracked views skip #509's search for what an ignore file hid when every tracked path
    is a walked one, the common case; the audit measured building the views at 6.1 times the
    instructions of v0.22.1. With one `git_tracked_only` rule over a committed 10k tree, they
    now take 9.4M instructions in place of 47.5M (Callgrind, bench profile), and `det_check`
    gains that cell. Looking walked paths up by a binary search over the sorted entries, in
    place of the path set the search builds, doubled its instructions, so it keeps the set.
- 2026-11-16: the audit of everything since v0.22.1.
  - Seven reviewers read #490 to #530, 37 commits, on `main` at e7bb08a57: the engine, templates
    and rules; YAML robustness; the library API; security; YAML as users meet it; the documents
    and release records; and performance. They found 80 problems, 10 of them high: 17 in the
    documents, 16 in user-facing YAML, 15 in security (13 holes, and the draft advisory's
    mistakes), 12 in robustness, 8 in the engine, 6 in the API and 6 in performance.
  - The fixes: #538 (the API gate and compare mode) and #539 (the engine, templates and rules,
    the entry above), merged; #540 (structured YAML fixes' spans), #541 (fixes and rule reads
    kept out of `.git` and alint's configuration), #542 (the language server's workspace trust
    and `--no-spawn`) and #543 (YAML guards, `pinned_references` and configuration loading
    against hostile input); then the security follow-ups (Git named by `--git-dir`, discovery
    stopped at the repository's top, extended rules' reads, `extends:` schemes), the YAML
    messages, the performance gates and the documents.
  - Deferred, as alint issues: #545 (the JetBrains plugin passes the IDE's trusted-project state
    to the server), #546 (`java-no-tracked-build` reports Java source packages named `build`),
    #547 (a hint to regenerate a baseline written before v0.22), #548 (parse each structured
    file once per run, for v0.24) and #549 (run the corpus replay in YAML compare mode).
  - The decisions: ADR-0043 records the advisory widened to the new holes, a repository's own
    configuration trusted like a Makefile, `--no-spawn` and the server's trust state, no rule
    reading a Git directory, discovery that stops at the repository's top, what extended
    rulesets may not do, the one-entry mapping refused where an enum's name goes, and the YAML
    messages; it also corrects ADR-0040's count of seven holes and widens phase 9's ayaml
    condition. ADR-0042 records YAML memory and wall time.
  - Filed with ayaml: asamarts/ayaml#160 (an indentation mistake reported as unexpected content),
    #161 (an unclosed quote's error does not say where it opened), #162 (a type error does not
    name the value), #163 (peak heap per input byte), #164 (a mapping lookup by `&str`
    allocates) and #165 (depth refused only after the whole event stream, and a flow implicit
    key's scan that grows with depth times length).
  - ayaml's main closed every gap alint had filed against the compatibility reader, #132, #133,
    #137, #139, #140, #141, #146 and #147, in its pull requests #152 and #154 to #159, none
    released yet. Phase 9's condition is now a release with all of them (ADR-0043), with the
    checklist in `yaml-parser.md` ("Bumping ayaml").
  - The corpus replay of e7bb08a57 against v0.22.1 (run 43927008138): of 130 repositories, 123
    replayed, 4 skipped because their configurations fail to load in both versions
    (facebook/react, gin-gonic/gin, neovim/neovim, spring-projects/spring-boot), none crashed,
    and 3 differ. gradle/gradle and quarkusio/quarkus report `java-no-tracked-build`'s reworded
    message (item 12). istio/istio's two release-notes rules report "more than one document in
    stream at line 10 column 1" where v0.22.1 said "deserializing from YAML containing more
    than one document is not supported", which the CHANGELOG lists. Section 3 and section 5 now
    accept the differences the CHANGELOG explains, not only item 12's. The replay runs the
    release build, so it compares ayaml with libyaml only through what alint reports; running
    it in compare mode as well is #549.
  - Next: the remaining audit branches, then phase 6.
