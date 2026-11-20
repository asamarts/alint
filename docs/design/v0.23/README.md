# v0.23: selectors, a first constraint slice, and the YAML groundwork

Status: accepted (2026-11-12, ADR-0032); in progress, phases 0 to 9 done (feature freeze and ayaml 0.2.2 qualification complete; phase 10 release prep next).
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
[ADR-0042](../../adr/0042-measure-the-yaml-parser-by-memory-and-wall-time.md) (measuring YAML
memory and wall time, after the audit);
[ADR-0043](../../adr/0043-trust-and-disclosure-decisions-from-the-post-v0221-audit.md) (the
trust and disclosure decisions of the audit after v0.22.1);
[ADR-0044](../../adr/0044-untrusted-runs-git-as-a-process-and-a-wider-memory-gate.md)
(`--untrusted`, Git as a process, and the wider YAML memory gate);
[ADR-0045](../../adr/0045-count-the-walks-per-directory-probes-in-the-reference-gate.md) (counting
the walk's per-directory ignore probes in the reference gate);
[ADR-0046](../../adr/0046-a-linear-extends-merge-sealed-options-and-the-rounds-open-questions.md)
(linear `extends:` merge, sealed `LspOptions`/`LoadOptions`, and round-2 open questions);
[ADR-0047](../../adr/0047-sarif-tells-apart-a-fingerprint-repeated-within-a-run.md) (SARIF
disambiguating repeated fingerprints within a run).

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

  alint parses YAML with ayaml 0.2.2 (bumped from 0.1.2 in phase 9), through `alint_core::yaml`.
  saphyr and saphyr-parser still give source spans and structured fixes; they leave in v0.24
  (ADR-0036).
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
  one exception (ADR-0033 decision 3): a configuration that uses `git_tracked_only` can gain or lose
  findings, three kinds and six bundled rules reword a message, and the CHANGELOG lists the change.
  The ayaml cutover (phase 9) is another: it reads some YAML differently and words parse errors
  differently, and the CHANGELOG lists each difference (ADR-0035 decision 5). The two audits since
  v0.22.1 (ADR-0043, ADR-0044) are the third: their security and correctness fixes change what some
  configurations do, and the CHANGELOG's "Upgrading from 0.22" list names each change a 0.22 setup
  can notice. Every difference in the replay must come from one of them. The replay of `main` at
  e7bb08a57 against v0.22.1 (run 43927008138) found three: item 12's reworded
  `java-no-tracked-build` message in gradle/gradle and quarkusio/quarkus, and the reworded message
  for a stream of more than one document in istio/istio.
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

Phase 10's release check has one step on alint.org, which this repository cannot make: once the
site's facts sync to v0.23.0, its `src/lib/rule-source-files.ts` needs
`constraint: 'constraint/mod'`, since the kind's source is a module directory, not
`constraint.rs` (audit round 4).

The #503 fix (item 12) is its own pull request, with `add -f` e2e scenarios for `file_absent`,
`dir_absent`, `file_exists` and `dir_exists`, and does not depend on the design. It merged as #509
on 2026-11-13.

The feature freeze is the point at which phases 1 to 8, the #503 fix and #580 (a linear
`extends:` merge, after phase 7; ADR-0046 decision 1) are merged. Phase 9 is decided then.

Phase 7 lands in four steps, each a pull request, and is complete: step 1 merged as #587, step 2
as #589, step 3 as #590, and step 4 is the last.

1. The kind, experimental, with `equal` (within groups, against a source row, within a row),
   `unique` (with or without `determines:`) and `set` against `{ group: }`; the version check per
   source, the refusal under `for_each_dir`, `for_each_file` and `every_matching_has`, the
   relations evaluated before the rules that read them, the path-free fingerprint, and the
   kind's marks in `rules list`, `rules show`, `facts.json`, the draft schema and its rule page.
2. `plurals: cldr` (a plural module shared with `key_parity`), `group_by` on a path or a path
   token, `groups:`, and `set` against a relation.
3. `--changed`: the trigger scope is the union of the plan's selectors, with a full-index test
   in place of `v010_cross_file_kinds_require_full_index_and_no_path_scope`; and the
   related-location gate over JSON, SARIF and the language server.
4. `explain`'s plan display and the `selector fields:` wording (which phase 6, #558, had
   already made), and no empty-success mutant left on the kind's error, truncation and
   type-mismatch paths (section 5).

## 5. Release acceptance

- **v1 is unchanged** apart from item 12, the ayaml cutover, the audits' fixes (section 4) and
  #590's rename under `--changed`: a renamed file changes its old path too, so a rule whose
  `paths:` match only the old path now runs (after `git mv LICENSE LICENSE.txt`, a `file_exists`
  rule for `LICENSE` fires, and `fix --changed` recreates it), and `pair_changed_together` and
  `changeset_requires_path`'s `when_changed:` can newly fire. The e2e suite passes, and the corpus
  replay against v0.22.1 shows no difference that the CHANGELOG does not explain: item 12's
  (ADR-0033 decision 3), the cutover's (ADR-0035 decision 5), the audits' (ADR-0043, ADR-0044)
  and #590's (its "Upgrading from 0.22" entry).
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
  - for the v2 policies, wall-clock and instructions against the v1 spellings they re-express;
  - YAML memory (ADR-0042, ADR-0044): decoding a YAML document into the structured rules' tree,
    ayaml's peak heap is within 1.2x of libyaml's on each of the memory test's nine shapes
    (OpenAPI documents of 37 KB, 1.1 MB and 4.4 MB, and 1 MB block and flow mappings and
    sequences). The release preflight's `yaml-memory-release.sh` measures it, and fails until an
    ayaml release meets it.

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
  corpus replay, weekly and on demand (`corpus-replay.yml`), compares against v0.22.1, so a
  difference on real repositories shows up before then.

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
- 2026-11-16: the audit of the changes since v0.22.1: YAML memory and the performance gates
  (ADR-0042).
  - **ayaml costs more than libyaml on files of 10 KB and up.** On the quiet benchmark host,
    `main` took +9% to +54% wall time and 1.3x to 2.9x peak RSS against v0.22.1, with instruction
    counts flat, so the deterministic gate, Y-C4's measure, did not see it. ayaml 0.1.2 peaks at
    about 65 bytes of heap per input byte, libyaml at 35 to 39 (asamarts/ayaml#163). The owner
    decided that v0.23.0 waits for an ayaml release within 1.2x of libyaml's peak heap; section 5
    lists it.
  - **The gates.** alint-core's `yaml_memory` test, a binary of its own with a counting
    allocator, decodes OpenAPI documents of 37 KB and 1.1 MB through both parsers: ayaml peaks
    at 67.1 and 64.5 bytes per input byte, libyaml at 37.8 and 37.4, ratios 1.78 and 1.73. CI's
    YAML Compare job holds it to 1.95, and the release preflight to 1.2, which fails today.
    `det_check` gains `yaml_openapi_50x37k`, one `yaml_path_matches` rule over 50 such documents
    of 37 KB: 941.2M instructions (release binary; about 20 s under Callgrind).
  - **A budget for large YAML parses.** One YAML parse at the 32 MiB input cap peaked at 4.5 to
    6.2 GB, not the "~1 GB" the cap's documentation claimed, and the per-file rules parse in
    parallel. Parses over 1 MiB now share a budget of 32 MiB of input in flight, held while the
    parser runs: `Format::parse`, `extract`'s entry rows, the structured-fix span resolver and
    `pinned_references`.
  - A kbench cell for wall time and RSS of mid-size YAML is not added: `bench-scale` records no
    RSS, and a new scenario needs a results location the published history does not read
    (#550).
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
- 2026-11-17: the audit's fixes merged, and phase 6.
  - The audit's branches merged as #538 to #543, #551 (the security follow-ups), #552 (the YAML
    messages), #554 (the performance gates) and #556 (the documents). The draft advisory
    GHSA-78jj-xfpm-g6px now lists all 21 items, the eight packages and the patches. Long runs of
    the YAML harness then failed seeds 2, 3, 10 and 12 on main, where alint's own words for a tag
    in a rule met the reference parser's; #557 pairs them, and seeds 1 to 12 pass. Pull requests
    run seed 1 only, so a change to alint's YAML error wording needs seeds 1 to 12 as well.
  - Phase 6, in one pull request: the version dispatch (section 3), named selectors and relations
    with per-file namespaces and `extends:` aliases (sections 4, 5 and 10), the plan check at load
    (section 8), a hidden relation evaluator for phase 7 (value-bearing entry rows, `read:`,
    `union:`, the inner equality join, `transform:`, `where:`, limits), `list` and
    `export-agents-md` for named objects, the draft schema `schemas/v2/config.json` and the page
    `docs/site/configuration/version-2.md` (section 11). Relations stay inert until `constraint`.
  - Three independent reviews (security and trust, semantics against this design, integration and
    cost) found 23 defects, fixed in the same pull request with a gate each. The release blocker:
    the dispatch parsed every version 1 config twice, +10% to +53% instructions to load and +44%
    to +50% on the `dsl_extends` micro-benchmark, which the release gates at +3%; loading is now
    within +0.7% of main. Also fixed: quadratic loads of a hostile version 2 config (minutes),
    environment values leaking through a JSONPath hint, `list` and joins, TOML dotted keys in
    inline tables, a `where:` result that depended on predicate order, and no cap on a relation's
    cells (`CellsPerRun`, 5,000,000).
  - Each open question took the stricter reading, which a later release can relax: an `.alint.d`
    fragment has a namespace of its own (ADR-0022 is silent; section 10 says each source has one),
    number and null literals in `in` and `not_in` are refused (YAML does not keep a number's
    lexeme), `semver-*` does not apply to a path token, `strip-root` comes first and once, and
    `as:` is refused on remote and bundled `extends:` entries.
  - Next: phase 7, the `constraint` kind.
- 2026-11-17: the second audit round.
  - Six reviewers audited #538 to #558 on `main` at 6837a4dfa (the first audit's fixes and phase
    6) and reported about 66 findings, 4 of them high. The scheduled YAML harness run failed at
    seed 60 on a block scalar's indentation indicator; #559 fixed the harness's line reader.
  - The high findings: an extended or nested config promoted a fix to apply without asking by
    writing `applicability: {safe: ~}` (since v0.17.0); `--no-spawn` and an untrusted language
    server ran Git, which runs the commands a repository's `.git/config` names; an empty `.git`
    that another user planted defeated discovery's owner check; and `--no-spawn` still fetched a
    remote `extends:` whose URL may carry an environment value.
  - Owner decisions, recorded in ADR-0044: a global `--untrusted` for a repository the user does
    not trust, with `--no-spawn` refusing every process, Git included; and the 1.2x memory
    release bound on each of the memory test's nine shapes, where ayaml 0.1.2 measures 1.73x to
    3.06x (reported to asamarts/ayaml#163). In ADR-0045 (with #560): the walk keeps checking
    each directory's ignore files before `ignore` reads them, which the crate gives no hook for
    and upstream declined to bound, and the reference perf gate counts those probes per
    directory; #573 looks at reading ignore files from the walk's own listing instead.
  - The pull requests: #560 (security), #562 (YAML), #563 (relations and `key_parity`), #577 (the
    `version: 2` loader), #576 (the engine) and #578 (these documents). Each fix has a gate test
    that fails when the fix is reverted. #561 audits the VS Code extension's npm dependencies
    against a waiver list, since no picomatch release fixes GHSA-fc5q-39xc-vjpf. Before the merges,
    all five fix branches were merged in one tree and the workspace's tests run there. #574 checks
    the subject a squash merge writes (#563's title made a 126-character one on `main`), and #575
    fixes three gaps the YAML harness showed with `ALINT_YAML_DIFF_SPECIALS=0`, which the weekly
    run does not set. A review of #577 and #578 before they merged found that an `extends:` entry
    was charged only for its merge, after `only:` narrowed its source, so entries naming one large
    source in other words loaded it whole each time unbounded; #577 charges the load, and #580
    tracks merging a list in linear time. Left open: #564 to #573 and #579 to #581.
  - The advisory GHSA-78jj-xfpm-g6px gained the fix promotion, the structured-fix crashes of
    #540, unbounded ignore-file and baseline reads, and the network `extends:` path: 26 items.
  - A trial of ayaml's main branch (d5c1cf7) needs two API changes and fails 14 of about 4,850
    tests, all of them pinned messages; its peak heap is about 1.26x to 1.29x libyaml's on the
    OpenAPI shapes. At a561ae2, later that day, the OpenAPI shapes are within 1.2x and the six
    1 MB shapes at 1.53x to 2.32x (asamarts/ayaml#178). [`yaml-parser.md`](../yaml-parser.md)
    records the trials and the bump steps.
  - For phase 7: the relation evaluator's run-wide counters (cells, bytes, rows) are shared, so
    which relation reaches a cap depends on the order rules run in. Phase 7 evaluates relations
    in plan order before the rules fan out, or documents the difference.
  - Next: phase 7, the `constraint` kind; the ayaml bump when a release meets the conditions.
- 2026-11-17: the round's open questions and the third audit.
  - Owner decisions, recorded in ADR-0046, each the recommended option: #580 merges an
    `extends:` list in linear time before v0.23.0, after phase 7; discovery keeps climbing past
    another user's checkout and its warning says how to use the config it skips; a bad ignore
    file still stops the walk, with the 32 MiB cap (#568 waits on #573); an absolute local
    `extends:` stays refused without `allow_out_of_root`; the language server's holes stay in
    GHSA-78jj-xfpm-g6px; `LspOptions` and `LoadOptions` are sealed, with setters; and the YAML
    alias factor stays 64, set again from the memory test at the ayaml bump.
  - A third audit, of #560 to #578 on `main` at 38947f8c5, reported 25 findings: security
    (SEC-1 to SEC-5), YAML fixes (YAML-1, YAML-2), the engine (ENG-1 to ENG-4), the `version: 2`
    loader (DSL-1 to DSL-5), and tests and CI (T1 to T9). The one high finding: `--untrusted`, an
    untrusted editor workspace and `alint test` accepted a config's `allow_out_of_root:`, so a
    repository could copy a file from outside itself with `content_from: ../secret.txt`. They now
    refuse it before any `extends:` resolves.
  - Security: an ignore file that gives its size as zero (`/proc/self/pagemap`) is read to count
    it, the `git_branch` fact reads `.git/HEAD` only within 64 KiB, and `suggest` and
    `export-agents-md` take `--no-spawn` and `--untrusted`. YAML: two rules' structured fixes are
    checked beside another rule's free-form edit of the same file, and the parse budget's gap for
    trees alias expansion leaves is documented (#582).
  - The engine: a nested rule's per-entry templates (`pair`'s `partner`, `unique_by`'s `key`,
    `cross_file`'s `register_as`, `command`'s arguments) are text once an iteration rendered them,
    where a directory named `x{stem}` had its token expanded twice; a join partner of a sensitive
    field is sensitive; a gate covers every scope narrowing under a nested rule; and `key_parity`
    holds each key path once, in its file's decode, with every decode's key paths charged to the
    run's `bytes_per_run`. A 320 KB reference 120 levels deep and one peer peaked at 2,300 MiB and
    now at 437 MiB; #569 stays open for the decodes themselves.
  - The loader: #577 had made 1,000 entries naming one empty file below an environment-chosen
    entry take 68 s (DSL-1). Each source is now parsed once per hash or name and charged its size
    against 64 MiB, `.alint.d/` drop-ins are charged as `extends:` merges are, the memo key no
    longer hashes `trusted_extends:`, and a file below an environment-chosen entry shows as
    `<redacted>` whatever its name's length.
  - Tests and CI: a one-commit pull request's squash subject is checked as its commit's subject,
    an npm-audit waiver fails once `npm audit fix` can fix its advisory, unit tests no longer read
    the host's global excludes file, the bounded-read tests have deadlines and twice-the-cap
    files, and the process-wide Git denial, the empty untrusted environment and the extends
    budgets' sizes are pinned.
  - The fixes went to four branches (security and the owner's calls, the loader, the nested
    templates, `key_parity`), merged in one tree with the workspace's tests run there, and landed
    in one pull request; each has a gate test that fails with the fix reverted. Filed: #583
    (escaping iteration values in nested options), #584 (per-iteration work the scope gate cannot
    see) and #585 (environment-value redaction outside the loader). Left open: #564 to #573 and
    #579 to #585.
  - The advisory GHSA-78jj-xfpm-g6px, still a draft, has 27 items: item 20 (`git_branch` read
    `.git/HEAD` without bound) is new, and items 15, 17 and 19 are corrected.
  - Next: phase 7, the `constraint` kind; then #580; the ayaml bump when a release meets the
    conditions.
- 2026-11-18: #586 merged, and phase 7 step 1.
  - #586 landed the third audit's fixes and the owner's calls. Its Windows runs found that a
    `registry_paths_resolve` glob entry, joined to its registry's directory as a `Path`, spelled
    its directories with `\` and so still tested every entry in the tree; a scope now splits its
    literal directory prefix on `\` where `\` separates. The advisory's patch list names #586.
  - Phase 7 is split into four steps (section 4). Step 1 adds the `constraint` kind, experimental
    and only in `version: 2`: `equal` in its three forms, `unique` with or without
    `determines:`, and `set` against `{ group: }`. A rule's kind is checked against the version
    of the source that writes it, a relation reader is refused under `for_each_dir`,
    `for_each_file` and `every_matching_has`, the engine evaluates the relations a rule reads
    before the rules run, and a finding's fingerprint holds no path.
  - An independent review of step 1 reproduced 12 findings and 6 test gaps, each fixed with a
    gate test that fails with its fix reverted. Related locations are built only for the 20 a
    finding lists (8,000 distinct values under `determines:` took 62 s), a row of each other
    value first. A relation the environment selects has its findings' paths and values redacted,
    as a version 1 rule's environment-written `paths:` has. A sensitive key makes every field
    that spells it sensitive. A file is checked as UTF-8 once, not per finding. Long values are
    cut in messages and held as a digest in keys. A repeated `unique` key is keyed by its values
    alone, so renaming a file keeps it baselined. Findings beside an analysis failure are
    withheld, a finding about a join's `unique:` side sits at the row's own entry, and the file a
    place is read from passes the evaluator's symlink checks. Sensitive values still share the
    key word `sensitive` and are told apart by count: a digest of a low-entropy secret would be a
    dictionary oracle (`redaction.rs`).
  - Next: phase 7 step 2 (`plurals: cldr`, `group_by` on a path, `groups:`, `set` against a
    relation).
- 2026-11-18: phase 7 step 2.
  - The `constraint` kind's `set` is complete. `plurals: cldr` merges CLDR plural groups of an
    `entry.path` field over each pair of groups, with the plural logic `key_parity` uses, now in one
    module, `plural_groups.rs`. Under `subset_of:` and `disjoint_from:` a comparison decides only
    the groups around the group's own keys, so it costs time in the group's keys, not the
    reference's; `equal_to:` merges the reference's keys again for each group, to list the ones the
    group lacks, so it costs time in the reference's keys per group. A `set` grouped by `path` or a
    path token gives every resource the relation reads a group, rows or not, except one whose path
    alone `where:` rejects (its path-token fields after their transforms, any other predicate
    unknown), and `groups:`, with `equal_to:` only, names groups that must exist, whose findings
    have no path. A `set` against `{ relation: R, field: F }` reads `R` before the rule, as an
    input: `R`'s failures withhold the rule's findings, its notes are reported once with the
    input's, and the environment redaction covers it. A sensitive `F` makes the compared field
    sensitive at load, with every field that spells the same values, so `explain` shows it (audit
    round 4 corrected "`explain` and `list`": `list` shows no fields). `R` is a name, never an
    inline relation.
  - Section 6's identity of `unique` without `determines:` now reads as step 1 built it.
  - Next: phase 7 step 3 (the `--changed` trigger scope, and the related-location gate over
    JSON, SARIF and the language server).
- 2026-11-18: phase 7 step 3.
  - Under `--changed`, a `constraint` rule runs when a changed path is in its trigger scope: the
    selectors of every relation its plan reads, followed through joins, `in:` and `not_in:` lists
    and a `set`'s other relation. A selector's `scope_filter:` narrows the trigger as it narrows
    what the selector reads, and a changed `has_ancestor` marker or manifest triggers too (section
    9 now says so). The rule then judges the whole snapshot. The kind declares
    `FullIndex { path_scope: true }`, through a trigger scope of its own rather than
    `Rule::path_scope`, which also drives `expect_matches`, tracing and the sensitive-path checks.
  - `every_full_index_kind_receives_the_full_index_under_changed` replaces
    `v010_cross_file_kinds_require_full_index_and_no_path_scope`: over every kind the registry
    marks full-index (25) and every corpus scenario that configures one inline, over an inline
    tree, in a config that loads, a probe in place of the rule records what the engine hands it
    under `--changed` (a relation reader's rows cell by cell), which must be what a full run hands
    it. Constitution invariant 2 names it.
  - The JSON report gains `related_locations` and `related_locations_omitted` on each violation,
    note and baselined finding, left out when empty and for a redacted path. A gate over one
    fixture with a finding of each family checks that JSON, SARIF and the language server carry
    every related location the engine produced, in order, with the count of the rest.
  - Review fixes: the changed set lists a rename under both of its paths (`--no-renames`), which
    also reaches the five version 1 kinds with a path scope; a changed `.gitignore` or `.ignore`
    triggers each selector whose `paths:` can match below it, and a changed `has_ancestor` marker
    only such a selector; a change that cannot run a rule reads none of its trigger's manifests.
    Section 9 and `docs/rules.md` list what Git does not report (an edit to a symlink's target, a
    file Git ignores under `respect_gitignore: false`, an ignore file outside the diff).
- 2026-11-19: phase 7 step 4, and phase 7 is complete.
  - `alint explain <rule>` shows a `constraint` rule's resolved plan, in human and JSON output (a
    `plan` object, which the trace's rule document carries too): the assertion in words; every
    relation the rule reads, in the order a run evaluates them, named as the config writes each
    reference that reaches it (with the file that declares it when two relations would read alike)
    and marked as the input, an inline input or the `set` target; each one's source (a selector with
    its paths, `scope_filter:` and resolved manifests, a join, or a union's parts), reader, fields,
    transforms and `where:`, with the file that declares it; the selectors of its `--changed`
    trigger scope; and its cost class. `Plan::evaluation_order` gives the order, the run evaluates
    the relations its rules read in that order, and the evaluator's own walk shares its post-order
    walk (`Plan::post_order`); `TriggerScope::origins` names the trigger's selectors. Phase 6 (#558)
    had already renamed `selectors:` to `selector fields:`.
  - The plan shows configuration only, never a value read from a file, except the paths a
    manifest-derived `scope_filter:` resolves to, as for any rule (ADR-0010). What the environment
    wrote stays redacted: a named selector that reads it, or a relation whose own definition reads
    it (the plan now records `definition_reads_environment` beside the transitive flag), shows each
    value it holds as `<redacted>`; an environment-chosen `extends:` entry names its files
    `<redacted>`; an environment-written assertion is `<redacted>` whole. A sensitive field's
    `where:` literals, patterns and transforms, and the literals the assertion compares with it, are
    `<redacted>` in the plan and in the rule's options. A version 1 rule's `explain` is byte for
    byte what it was, against snapshots taken before the change, and the v2 all-kinds fixture drives
    each source shape through the ADR-0012 explain gates.
  - Review fixes: each relation in the JSON plan has an `id`, its place in the list, and a join's
    sides, an `in:` list's relation, the trigger's inline selectors, `input` and `set_target` name
    it by that; the human plan lists each spelling of a relation two aliases reach and adds the
    declaring file where two relations would read alike, so every reference names one relation.
    The run evaluates the relations its rules read in `Plan::evaluation_order`, as `explain` lists
    them, with a test that fails when the two differ. The trace schema's `plan` is an object or
    `<redacted>`, and no other string.
  - Mutants: cargo-mutants 27.1.0 over `constraint/` and `plural_groups.rs` (alint-rules), and
    `relation/trigger.rs` and `relation/eval/{failure,mod,ops,read}.rs` (alint-core), file by file:
    each mutant ran against its crate's tests, a survivor against four crates' (alint-core,
    alint-dsl, alint-rules, alint), and a survivor of those against the whole workspace. Of 832
    mutants, 57 survived the workspace before the new tests, and 14 survive after them (one more is
    a timeout: `index += 1` made `*=` in `places.rs` loops forever, so the tests catch it by their
    timeout). The tests that kill the 43 cover the plan check's refusals, the families' and the
    `set`'s messages and places, `where:` on a missing value, a membership clash's witness, the
    bound-path and strip-root errors, a per-file extractor limit, the run's bytes for a decode, a
    note, a shared key path and a stripped one, a sensitive value's note, and the type a sensitive
    field's membership clash names for a string literal (review found `kind`'s string arm reachable
    there, through `bare_type`, after it was first called equivalent). Equivalent, 4: in `set.rs` a
    group's first path kept when equal (`<` made `<=`) and the distinct-value limit checked again at
    an unchanged count (`>` made `>=`); in `eval/mod.rs` a guard on an empty failure list that
    refinement never leaves non-empty with rows; in `read.rs` a non-string kind's size, which is one
    byte (`*` made `/`). Defensive, 10: the cycle guard of `path_levels` (`>` made `==` or `>=`),
    which an acyclic plan never reaches; `not_a_member` (2), as an entry row is always an object
    member; and `wrong_source` (6), as the plan check refuses a source the part's reader does not
    give.
  - Next: #580 (an `extends:` list merged in linear time, before v0.23.0, ADR-0046 decision 1),
    then phase 8 (the three conformance policies).
- 2026-11-19: audit round 4, over aacb561ac..c3906120c (phase 7 steps 1 to 4, #587, #589, #590,
  #591, and #588).
  - Six reviewers covered the kind's semantics; security and bounds; `--changed`, Git and the output
    formats; the documents, schemas and `explain`; tests and hygiene; and journeys from end to end.
    None of their findings was high. Five were medium: placing findings kept every file it read
    until the rule ended, so eight files of 32 MiB of line breaks held 2,380 MiB for one rule; a
    message quoted a key path whole, so 1,000 rows repeating the value of a 1 MB key made 1 GB of
    messages; a stub locale (`fr:`) failed `strip-root` and so withheld every locale's verdict;
    `fix`'s redaction of a rule over a relation the environment selects had no test; and alint.org
    showed unreleased text. The rest were low.
  - The fixes went to four branches, merged in one tree (`fix/r4-audit`) with the workspace's tests
    run there; each fix has a gate test that fails with the fix reverted.
    - `--changed`, Git and outputs (fix/r4-chg): a submodule whose recorded commit moved, or an
      untracked repository, which Git lists as one changed directory, now runs the `constraint`
      rules whose selectors can match below it; a pass decides each trigger scope once, testing a
      selector against only the changed paths below its literal directories;
      `changeset_requires_path`'s `add_glob:` passes `--find-renames`, whatever `diff.renames` says;
      SARIF tells apart a fingerprint repeated within a run by occurrence, as GitLab output does
      (ADR-0047); and gates cover `fix`'s redaction, the `--changed` skip of relation evaluation,
      and the e2e scenarios under `fix`, directory changes, deletions and renames.
    - Relations and the loader (fix/r4-rel): a `where:` or `transform:` entry that reads the
      environment redacts the finding paths of every rule over its relation, through joins, `in:`
      lists and `set` targets; under `strip-root` a top-level key that holds nothing reads as a file
      with no keys; a finding about a `read:` value has its line and column, found only in the
      files a finding or related location sits in (`relation::eval::find_spans`, called from
      `constraint`'s `Places::resolve`); and the loader and `NamedRelation::is_inline` share one
      name predicate, `relation::plan::is_name`.
    - Messages and places (fix/r4-msg): a key path is cut at 64 characters per key and 256 in all,
      and key paths and `{{row.FIELD}}` escape controls, bidi and zero-width characters; placing
      findings reads one file at a time, charged to `bytes_per_run`, and every related row is listed
      or counted; a sensitive cell shows only where it starts; a missing value is `missing` in every
      family, and a `union:` part that read nothing is named; did-you-mean under the kind's options
      suggests the field rather than the typo, with version 1's messages unchanged.
    - Docs, schemas and `explain` (fix/r4-doc): `ci/scripts/check-docs-overlay-since.py` fails CI's
      Docs job, and the local preflight, on a line added since the latest `v*` tag outside a
      since-block in a page docs-bundle.yml overlays from main, or in a released kind's
      `docs/rules.md` section, and the lines it found are marked (corrections of shipped behaviour
      as `since=0.22.1`); the draft version 2 schema's `constraint` branch is derived from the
      kind's option types, and a test holds it to the builder's verdict over 2,343 generated shapes
      and every `constraint` config alint-dsl's tests build; the trace schema describes the `plan`
      key by key; `explain`'s trigger heading names every change that runs the rule; the generated
      rule indexes mark version 2 kinds; and the documents, the CHANGELOG and ADR-0047 record the
      round.
  - A second pass: an independent review of the merged fixes found 7 defects, all fixed.
    - Placing findings took each rule's largest file from `bytes_per_run` and never gave it back,
      so which rules failed depended on thread scheduling (one 31 MiB file and 40 rules gave 7
      failures, a different 7 each run). Placing now takes nothing, and checks each file against
      what the run's relations left (`DecodeCache::may_hold`): the 40 rules give 40 findings.
    - Finding every `read:` value's span as it was read cost 2.4 times the run over 3,000 passing
      `package.json` files (669M instructions, against 280M now) and 5 times for a 12 MB file. A
      span is now found only for a value a finding places, and the parsed tree is let go before
      the walk over the document's entries: the 12 MB file with a finding peaks at 386 MB, against
      400 MB when every span was found as it was read.
    - A removed submodule ran no rule, since the trigger asked the walk which changed paths are
      directories. Git now marks them by their gitlink mode in `--raw` diffs (`ChangedSet`), with
      `diff-files --ignore-submodules=none` for a submodule changed before `git add`.
    - `may_match_below` counts components and tests each leading one, literal or wildcard, so an
      untracked repository, which Git lists on every run without `--base`, runs fewer rules: a
      selector of `a/[!x]*.txt` no longer runs its rule for `a/sm`.
    - A row's file, a note's file and the file that declares a relation reached messages raw; they
      are escaped as values are, and an analysis failure's text is escaped whole.
    - The test of related rows at one place fails again with its fix reverted: it read `lines:`
      values, which now have places, and reads a JSON array's values instead.
    - `docs/rules.md` said only an environment value outside a field's `where:` and `transform:`
      redacts a rule's findings, the opposite of the first pass's fix; it now says such an entry
      redacts them too, and the CHANGELOG says so.
    - Also: an empty changed set runs no rule, even through a selector that does not compile;
      `explain`'s trigger heading names ignore files only when the walk reads them, and names a
      union's parts `union[0]` and on; a `strip-root` failure and a skipped-value note cut what
      they quote; and SARIF numbers a repeated fingerprint by what each finding shows, baselined
      results first, with GitLab numbering the same way (ADR-0047). A repeat keeps its
      fingerprint while the repeats that sort before it stay; the baseline suppresses by count in
      that order, so a new repeat that sorts first shifts the rest (#601).
  - A third pass: a review of the second pass found 8 more defects, all fixed.
    - `may_match_below` ruled out patterns whose class can match `/` (`a[!x]sm` below `a`), so
      `--changed` skipped a rule it should run. Such a class now leaves the component count
      unknown, and an exhaustive check over 5,692 patterns holds it to globset.
    - `diff-files --ignore-submodules=none` ran `git status` inside every submodule on every
      `--changed` run (326 to 81,789 `stat` calls with four 20,000-file submodules); `=dirty`
      reports the same commit moves without it.
    - `alint fix` evaluated relations lazily, so near `bytes_per_run` it could place findings
      where `check` failed; it now evaluates them once a pass, before its first rule as `check`
      does, and checks every rule of the pass against what they took, rows as well as decodes.
      (The third pass evaluated them again after each applied fix, which read a later relation
      once per fix and failed `rows_per_run` under `fix --diff`; the fourth pass keeps the
      pass's record across a fix and clears nothing under `fix --diff`.)
    - Placing checks a file's size before finding its `read:` places, counts the parse it reads
      them from, and places a cell only where the file still holds what was read there (a 32-bit
      fingerprint in the span), so a file changed during the run is placed nowhere.
    - `--show-notes` and the language server named an escaped note's file twice; the language
      server also showed a hidden note's path, as it had since v0.20.0.
    - The docs said a baselined repeat keeps its fingerprint whatever live repeats do; they now
      say which repeats keep it. `git submodule deinit`, `update --init` and edits in a
      submodule's work tree join the list of changes that run no rule.
    - Two changed lines that CI's mutants found untested (a rename record in `--raw -z` diffs, a
      JSON parse skipped when placing) have tests.
  - A fourth pass: a review of the third found 3 defects, all fixed.
    - The third pass's `fix` read every later relation again after each applied fix: with 10
      creates ahead of a `constraint` rule, `fix` ran 11.15G instructions against 2.01G, and
      `fix --diff` failed `rows_per_run` on 6,500 manifests. Each pass now evaluates its relations
      once, before the first rule that reads one, keeps that record across a fix, and clears
      nothing under `fix --diff`, which writes nothing (2.04G).
    - `fix --diff` placed a finding where `check` refused; the kept record makes `check`,
      `fix --dry-run`, `fix --diff` and a real fix's first pass agree at every limit swept.
    - Placing undercounted a JSONC file's copy without comments (8 MB counted as 464 B) and the
      TOML lexer's tokens (about 7 times the text); both are counted, and `strip_jsonc` makes one
      copy where it made a `char` vector and two strings, which also halves version 1's cost on
      an 8 MB JSONC file (750.6M to 388.8M instructions).
    - Two Windows-only test expectations and the `=dirty` submodule gate were tightened.
  - A fifth pass: a review of the fourth found that the relation phase cleared decodes that were
    still valid and threw away what it had read at the next fix (`check --format sarif` +79%
    when its first rule read no relation, `fix --dry-run` failing `rows_per_run` around
    `key_parity`, `fix` +38% with a create between two readers). A pass now evaluates its
    relations before its first rule, as `check` does, records what that phase took, does nothing
    on an index `check` already evaluated, and after an applied fix lets go only of what the fix
    made untrue: a move drops its paths' reads and decodes and the relation memo, a command
    fixer clears everything, and a create or content edit keeps them. Over 72 cells (18 shapes
    in `fix`, `--diff`, `--dry-run` and SARIF) it costs at most 1.43% more instructions than
    before the round, and 37 scenarios give identical output, exit codes and trees in 7 modes.
  - Filed: #592 (two fields of one array element in one row, and a transform that splits a value),
    #593 (a `bound:` query that may match nothing), #594 (a `message:` placeholder for the value a
    `set` finding is about), #595 (placing a finding that a group lacks a value at the file the
    value belongs in), #596 (bounding a rule's findings when every group lacks the whole reference
    set, G × V today), #597 (version 1 nits: list the formats on an unknown `--format`, and name a
    null root in `key_parity`'s `strip_root` error) and #598 (let a stub locale root name its group
    when `group_by` reads `entry.root`), #600 (escape bidi and zero-width characters in JUnit
    failure text and agent-format messages, from before 0.23) and #601 (keep the findings a
    baseline suppressed when a new repeat sorts first). Left open: #564 to #573, #579 to #585 and
    #592 to #601.
  - For phase 8: locale parity written as a version 2 `set` costs about 1.5x the instructions and
    1.9x the peak RSS of `key_parity` (100,000 rows: 6.31G instructions and 144 MB against 4.17G and
    77 MB; Mastodon's YAML locales: 20.5G and 318 MB against 14.5G and 179 MB).
  - `--changed` with 200 `constraint` rules and changed files outside every selector ran 4.5G
    instructions at 10,000 changed files and 41.3G at 100,000, against 1.56G and 10.0G for a full
    check; deciding each trigger scope once brings them to 0.79G and 4.6G. Placing findings one file
    at a time takes the memory case from 2,380 MiB to 101 MiB for one rule.
  - Next: #580 (merge an `extends:` list in linear time, before v0.23.0, ADR-0046 decision 1), then
    phase 8 (the three conformance policies).
- 2026-11-19: #580, the `extends:` merge in linear time (ADR-0046 decision 1). An `extends:` list
  and the `.alint.d/` drop-ins merge into one accumulator (alint-dsl `merge.rs`), so a merge
  costs what its entry brings in, and a property test holds it to the old merge's left fold on
  generated lists. Each merge is charged that, and once more each rule and template it merges
  onto an earlier one of its id, in its own source too: a flat list of distinct rulesets that
  override none of each other's rules or templates, and repeat no id within one, stays within the
  budget while they hold at most 2,097,152 entries in all, where it refused the 91st ruleset of
  1,000 rules or the 2,895th one-rule file. A rule or template that an earlier entry naming the
  same source wrote last is not merged again, nor charged, so `[a, b]` 1,000 times over rulesets
  of 2,000 rules (#567) is refused by a budget with each later entry looking each rule up.
  - Next: phase 8 (the three conformance policies).
- 2026-11-20: phase 8, the three conformance policies, section 12.1 measurements and the corpus
  stratum re-scoring.
  - **Conformance policy scenarios:** 14 `alint test` cases (7 passing, 7 failing) under
    `crates/alint-e2e/scenarios/check/version_two/conformance/` cover dependency version
    consistency (`jest-23`, `trpc-05`, `yarnpkg-berry-01`), merged per-locale key parity
    (`forem-01`, including relocated keys across files, `strip-root`, `plurals: cldr`, prefix
    filtering and `groups:`), per-file JSON locale parity, toolchain-pin consistency
    (`signal-desktop-28`, including missing-pin findings), and aliased local `extends:`
    composition (`support/shared_deps.yaml`). The scenario inventory grows to 680 (586 general,
    94 bundled), and `scenario-fingerprints.json` adds only the new findings (zero existing
    fingerprints changed).
  - **Documentation and specialized-kind length check (section 12.1):**
    `docs/site/configuration/version-2.md` now shows both the shorter v1 spellings (`key_parity`,
    `cross_file` `equals`) and their v2 `constraint` equivalents, all validated by
    `version_two_schema.rs` (2 extended/fragment files, 2 v1 configs, 12 v2 configs). Specialized
    v1 spellings stay shorter: 8 vs 17 non-comment lines (158 vs 388 B) for per-file JSON locale
    parity, 9 vs 18 lines for YAML, 11 vs 15 lines (391 vs 561 B) for `signal-desktop-28`
    toolchain pins, and 8 vs 15 lines against `toolchain-pins@v1` for multi-surface Node pins.
  - **Performance (release binary, Linux x86_64):** per-file v2 locale `set` runs at 1.50x to
    1.83x `Ir` and 1.24x to 1.81x peak RSS of `key_parity` (100k JSON rows: 840.3 ms / 4,703.2M
    `Ir` / 113.0 MiB against 360.2 ms / 3,134.8M `Ir` / 62.5 MiB); `signal-desktop-28` toolchain
    pins in a 10k-file tree match `cross_file` within 1% to 3% `Ir` (22.8 ms / 157.3M `Ir`
    against 22.3 ms / 155.2M `Ir`), and a single multi-surface v2 Node union uses 11% fewer `Ir`
    than the three bundled Node rules; dependency version consistency scales linearly across 100
    (4k rows: 39.6 ms / 100.6M `Ir` / 19.1 MiB), 500 (25k rows: 185.3 ms / 539.0M `Ir` /
    39.3 MiB) and 1,000 packages (50k rows: 379.7 ms / 1,065.7M `Ir` / 64.3 MiB).
  - **Corpus stratum re-scoring and section 11.1 stop rule:** auditing all 69 units of ADR-0033's
    projected set against the shipped slice confirms 39 clean A (30 B-config, 9 entry-values),
    moves 6 from B to C (awkward) and leaves 24 B, with 4 separate C->A promotions from
    `entries:` key selection (`corpus-stratum-log.md`, `general-linter.md` section 12.4).
    Coverage moves from Today: `locale` 22% to 28%, `dep-version` 43% to 54% (57% with C->A),
    `toolchain-pin` 63% to 63%, `other` 56% to 59%, pooled 51% to 55%. Binary verification caught
    that `entries: leaves: true` misses duplicate non-leaf YAML keys with disjoint children and
    keys inside sequences (`mattermost-56`, `zulip-28`). The section 11.1 stop rule is met, and
    v0.23 reaches feature freeze (phases 1 to 8, #503 and #580 complete).
  - Next: phase 9 (ayaml bump once an upstream release carries ADR-0043's compatibility fixes and
    meets ADR-0042/ADR-0044's 1.2x memory gate across all nine shapes), then phase 10 (release).
- 2026-11-20: phase 9, ayaml 0.2.2 bump and qualification (#481, ADR-0040, ADR-0042, ADR-0043,
  ADR-0044, ADR-0046 decision 7).
  - Bumped exact pin from `=0.1.2` to `=0.2.2`, adapted `yaml.rs` (`parse_events_eager_with_limits`)
    and `yaml_decode.rs` (`Span::start()` + `.with_location`), removed the `#139`/`#140`/`#141`
    workarounds from `yaml_depth.rs`, removed all four ayaml-defect allowlist entries
    (`block-scalar-under-an-anchor`, `ayaml-error-without-a-place`,
    `document-marker-after-indented-empty-lines`,
    `anchor-name-before-a-colon-and-a-flow-indicator`) and narrowed
    `byte-order-mark-starting-a-line` (34 deviations, 0 new deviations, 8 location classes), and
    updated `yaml_differential.rs` for ayaml 0.2.2's streaming parser-to-value error order.
  - **Qualification runs:** all 12 long runs (`ALINT_YAML_DIFF_SEED=1..12`, 3,848,472 inputs)
    passed with 0 unexplained differences, 0 panics and 0 vote-only differences; 3× 2,000,000-case
    runs of both `yaml_depth::tests::scanner_never_misses` properties (12M cases) passed; full
    workspace `ci/scripts/yaml-compare.sh` passed with all 5 saphyr-vote lines reviewed as
    documented YAML 1.2 readings.
  - **Memory gate:** `ci/scripts/yaml-memory-release.sh` passed across all nine shapes (each parser
    measured in its own child process): `0.578x`, `0.583x`, `0.584x` (OpenAPI 37 KB / 1.1 MB /
    4.4 MB, ~21.8 B/input byte vs libyaml's 37.3–37.8), `0.673x` (block mapping), `0.626x` (flow
    mapping), `0.517x` (flow sequence of ints), `0.719x` (flow sequence of `1`), `0.731x` (block
    sequence of `value N`), and `0.965x` (block sequence of words). Lowered regression bounds to
    `0.58`–`1.07` and `EXPANSION_BYTES_PER_COST_BYTE` from 64 to 32.
  - Next: run `corpus-replay.yml` against v0.22.1 via workflow dispatch (Bumping ayaml step 6),
    then phase 10 (release documentation, `bench-record`, security advisory GHSA-78jj-xfpm-g6px,
    and v0.23.0 release check).
