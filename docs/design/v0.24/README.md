# v0.24: saphyr exit, Phase 3 completion, owned walker, LSP rewrite, and backlog drain

Status: accepted (2026-11-22, ADR-0048); in progress (Phase 0, Phase 1 [19/19], Phase 2 [8/8], Phase 3 [5/5], Phase 4 [2/2 + audit], and Phase 5 [design/ADR-0050 + Batches 5a-5d] complete; Phase 6 release qualification next).
Decisions: [ADR-0048](../../adr/0048-v024-owner-decisions.md) (owner decisions and scope for v0.24);
[ADR-0049](../../adr/0049-baseline-preferred-paths-for-repeated-fingerprints.md) (baseline preferred visible paths for repeated fingerprints);
[ADR-0050](../../adr/0050-v024-phase-3-completion-design.md) (accepted: Phase 3 completion design decisions D1 to D11);
[ADR-0036](../../adr/0036-ayaml-issues-and-the-saphyr-exit.md) and
[ADR-0041](../../adr/0041-a-yaml-parser-facade-with-test-only-reference-parsers.md) (saphyr exit
and test-only reference parsers);
[ADR-0027](../../adr/0027-plugin-contract-0-1-stays-a-draft.md) (plugin contract 0.1 stays a draft
until v0.25).

v0.24 completes Phase 3 of [`general-linter.md`](../general-linter.md) (section 11.6), removes
`saphyr` and `saphyr-parser` from default release builds on `ayaml 0.2.3`, replaces `tower-lsp`
with `lsp-server` and `ignore`'s walker with an owned parallel walker, and drains all 41 open
GitHub issues (#545 to #621) filed through v0.23.0. Plugin hosts Slice P2 (native Rust plugins and
`alint build`) moves to v0.25 at the earliest (ADR-0048 decision 2).

## 1. Scope and issue map (41/41 issues)

Every open issue on `asamarts/alint` at the start of v0.24 (#545 to #621) is assigned to a concrete
phase below, alongside Phase 3 completion and post-v0.23 hygiene items. Issue #622 (`ajson`
evaluation) is tracked as a non-blocking research item.

### Phase 0: Plan, ADR, and quick hygiene

- Record [ADR-0048](../../adr/0048-v024-owner-decisions.md), this plan (`docs/design/v0.24/README.md`),
  the ADR index entry (`docs/design/README.md`), `docs/design/ROADMAP.md` (`roadmap.json`),
  `general-linter.md` (sections 11.1 and 11.6), and `plugin-architecture.md` (section 10.3).
- File #622 to evaluate a standalone bounded, span-native JSON/JSONC parser crate (`asamarts/ajson`).
- **#621:** update `.github/workflows/bench-record.yml` so `bench-gate --baseline` falls back to
  the latest existing `results.json` when the immediate previous tag was a packaging-only patch.
- Fix the nested single-backtick typo in `CHANGELOG.md` line 233 (allowing removal of the repair
  workaround in `alint.org/scripts/sync-from-alint.mjs`).
- Migrate deprecated `actions/attest-sbom` to SHA-pinned `actions/attest` in
  `.github/workflows/release.yml`.
- Bump `crates/alint-witforge-consumer`, `ci/scripts/witforge-consumer.sh`, and
  `ci/scripts/test-witforge-consumer.sh` to `witforge 0.6.0` (ADR-0048 decision 2).
- Review and merge green Dependabot PR `asamarts/alint.org#128`.

### Phase 1: Security, trust, correctness, and supply-chain diet (19 issues)

Parallelizable, low-conflict PRs closing P0/P1 security, trust, correctness, and quick dependency
items:

- **Security and trust (7 issues):**
  - **#609:** upgrade `roxmltree` from `0.20` to `0.21.2+` (CVE-2026-92987), verify 1 MiB and
    2 MiB debug/release stack headroom, re-verify `xml_scan.rs` lexer parity and the XML bomb
    suite (pivot to an iterative XML 1.0 parser only if `0.21.2+` fails headroom or parity).
  - **#545:** JetBrains plugin (`editors/jetbrains`): pass trusted-project state (`trusted: true`)
    in `initializationOptions`, restart the server on trust change, verify against JetBrains
    Marketplace denied-internal-API rules, add integration tests for both states, and update
    `editors/jetbrains/README.md`.
  - **#596:** enforce a per-rule hard finding cap (default 10,000 findings per rule, configurable
    via limits) for `constraint` `set` and `key_parity`, emitting one non-baselinable
    analysis-failure finding on breach (ADR-0048 decision 7a).
  - **#600:** escape bidi and zero-width characters in JUnit failure text and `--format agent`
    `human_message` / `agent_instruction` via `alint_core::invisible`, leaving machine path fields
    raw.
  - **#569:** bound `key_parity` key-path pointer memory on deep, wide documents below the default
    1 GiB decode cap (lazy pointer materialization or tighter row-pointer budget).
  - **#583:** escape iteration token values (`{path}`, `{dir}`, `{stem}`, etc.) in a nested rule's
    options for the syntax each option holds.
  - **#585:** make environment-value redaction structural and linear outside the loader (removing
    quadratic `remember_environment_values` scans and CLI text scrubbing).
- **Correctness and usability (7 issues):**
  - **#579:** normalize `{path}` and `{dir}` path tokens to `/` on every platform, enable
    cross-platform fingerprint golden gates on Windows and macOS in CI, and clearly document the
    Windows baseline regeneration note in `CHANGELOG.md` and user docs (ADR-0048 decision 6a).
  - **#546:** update `java@v1`'s `java-no-tracked-build` (keeping `kind: dir_absent`) to exclude
    `**/src/**/build` and gate on Gradle presence (`build.gradle`, `build.gradle.kts`,
    `settings.gradle`, `settings.gradle.kts`), with a ruleset test, CHANGELOG entry, and corpus
    replay verification (ADR-0048 decision 6b).
  - **#601:** preserve baselined findings when a new repeat of the same fingerprint sorts earlier.
  - **#564:** document the IEEE-754 `f64` number-range limit on JSON-reading kinds (ADR-0048
    decision 7b; `ajson` evaluation tracked in #622).
  - **#547:** print a one-line regeneration hint when a loaded pre-0.22 baseline has unmatched
    hidden-path entries.
  - **#597:** list valid formats on unknown `--format`, and name a null root clearly in
    `key_parity`'s `strip_root` error.
  - **#581:** intern provenance marker source strings in `alint-dsl/src/trust.rs` like field-source
    markers.
- **Quick supply-chain cleanups (5 issues):**
  - **#611:** replace `directories 6.0.0` (single call site in `alint-dsl/src/extends/cache.rs`),
    removing 20 transitive crates and the sole MPL-2.0 copyleft exception (`option-ext`) in
    `deny.toml`.
  - **#612:** replace unmaintained `content_inspector 0.2.4` (single call site in
    `alint-rules/src/io.rs`) with an internal text/binary heuristic.
  - **#613:** replace `node-tar` in `editors/vscode/src/download.ts` with built-in `node:zlib` and
    a minimal USTAR single-file extractor.
  - **#615:** adopt `adotenv 1.6.0` `ParseOptions`, multi-BOM, `EditOptions`, compact bare-entry
    parsing, and indexed unique lookup, dropping local dotenv workarounds.
  - **#610:** replace `java-properties 2.0.0` with an internal dependency-free spanned
    `.properties` parser, dropping 14 transitive crates (including direct `encoding_rs` and
    `lazy_static`).

### Phase 2: Unified YAML track and span-enabled diagnostics (8 issues)

Executed in three ordered steps (ADR-0048 decision 8):

- **Step 2a -- saphyr exit (#614, #549, #550):**
  - **#614:** bump `ayaml` from `=0.2.2` to `=0.2.3`, migrate deprecated `*_with_limits` calls to
    `ParseOptions` / `*_with`, adopt `Error::masked()` on redacted paths, migrate all four `saphyr`
    call sites (`structured_fix::yaml_`, `extract.rs`, `yaml_depth::mapping_subtree_is_literal`,
    `pinned_references`) onto `ayaml`'s spanned event stream (`parse_events_with` + `LineIndex`),
    remove `saphyr` and `saphyr-parser` from default release builds (staying test-only behind
    `yaml-reference` per ADR-0041), enforce zero default-build dependency in
    `ci/scripts/yaml-reference-gate.sh`, and add event-stream comparison to the #482 differential
    harness.
  - **#549:** add an optional `compare_mode` input to `.github/workflows/corpus-replay.yml`.
  - **#550:** add a `bench-scale` wall-time and peak-RSS scenario over mid-size YAML files (e.g.,
    50 OpenAPI-shaped documents).
- **Step 2b -- bounded shared parse cache and alias-tree memory (#548, #582):**
  - **#548 + #582:** add a memory-budgeted per-run structured-tree cache (`SharedValue`, skipping
    run-level caching on per-file-only runs while sharing per file across 2+ rules, with LRU
    eviction under a byte budget) and bound retained memory of alias-expanded YAML trees (#582).
    Verify peak RSS on 100k/1M and #550.
- **Step 2c -- span-enabled config diagnostics (#572, #566, #571):**
  - **#572:** reject duplicate keys in `vars:`, and point duplicate-entry errors at the repeated
    key rather than the mapping's first key (using `ayaml` spans).
  - **#566:** report unknown top-level keys in `version: 2` configs with line and column.
  - **#571:** in `alint-lsp`, map config load errors with key paths (`rules[3].paths`,
    `relations.r.fields.name.at`) to their exact range in `.alint.yml` via `ayaml`'s spanned event
    stream and `LineIndex`.

### Phase 3: Nested-rule performance and owned parallel walker (5 issues)

- **Nested-dispatch optimization wave (#620, #565, #584):**
  - **#620:** hoist invariant per-iteration work in `for_each_dir` and `every_matching_has`
    (recovering the +17.9% to +19.9% v0.23 micro-det instruction regression).
  - **#565:** narrow remaining nested-rule scans (`cross_file`, `file_graph`, etc.) to the
    iterated directory's entries.
  - **#584:** narrow per-iteration work invisible to the `Scope::matches` gate
    (`generated_file_fresh`, exact-dir `dir_contains`, etc.).
- **Owned parallel walker and ignore policy (#573, #568):**
  - **#573:** replace `ignore`'s walker with an owned parallel directory walker in `alint-core`
    built on `ignore::gitignore::Gitignore`, inspecting ignore files from each directory's listing
    with open-then-`fstat` validation so bounding ignore files adds zero per-directory `stat`
    probes. Retire ADR-0045's per-directory probe allowance from `ci/scripts/reference-perf-gate.sh`.
    Keep the walker behind a clean internal module boundary so standalone crate extraction can be
    evaluated after it is proven on the parity suite, 130-repo corpus, and 1M-file benchmarks.
  - **#568:** skip bad ignore files above the checked root with a visible warning, keep strict
    failure for bad ignore files inside the checked root, and document the 32 MiB cap (ADR-0048
    decision 5b).

### Phase 4: LSP rewrite and HCL evaluation spike (2 issues)

- **#607:** migrate `crates/alint-lsp` from unmaintained `tower-lsp 0.20` to `lsp-server`
  (dropping `tower-lsp`, `tower-service`, `tokio`, `tower`, `futures`, `dashmap`, and the rest of
  the 19-crate async LSP dependency subtree; net `-17` `Cargo.lock` entries). Verify full unit,
  trust, audit, and VS Code / JetBrains E2E parity (including workspace trust gating, stale
  re-eval cancellation, and clean `exit` teardown).
- **#608:** evaluate newer `hcl-rs` / `hcl-edit` for internal recursion bounds; completed by
  upgrading to `hcl-rs 0.20` / `hcl-edit 0.10.0`, retiring the 512 MiB parse thread, 64 KiB cap,
  and hand-rolled recursion pre-scanner, and declining a standalone `asamarts/ahcl` crate (see
  [hcl-spike.md](./hcl-spike.md); ADR-0048 decision 4c).

### Phase 5: General-linter Phase 3 completion (6 issues + roadmap)

Gated by the Phase 3 completion design document ([phase-3-completion.md](./phase-3-completion.md))
and owner ADR approval ([ADR-0050](../../adr/0050-v024-phase-3-completion-design.md), `accepted`):

- **Phase 5 design and owner gate (accepted, ADR-0050):** locked exact `version: 2` syntax, error
  semantics, and cost bounds for the v0.24 Phase 3 surface, verifying the two stratum
  `cardinality` / `group` units (`forem-01`, `backstage-25`) against ADR-0048 decision 3b.
- **Open Phase 3 issues (#592, #593, #594, #595, #598, #570):**
  - **#592:** multi-field array-of-objects extraction into one row + string split/prefix transform
    (unlocks 12 stratum units).
  - **#593:** optional `bound:` query matching nothing without failing the relation.
  - **#594:** `{{value}}` message placeholder for missing values in `set` findings (redaction- and
    escape-safe).
  - **#595:** place missing-value `set` findings at the target file the value belongs in.
  - **#598:** allow a stub locale root (`fr:`, `fr: {}`) to name its group under
    `group_by: entry.root`.
  - **#570:** release preflight gate ensuring `schemas/v2/config.json`'s `$id` release constant
    advances whenever the draft v2 schema changes.
- **Remaining Phase 3 operators and high-ROI stratum capabilities:**
  - `reference` assertion (11 stratum units), unified `count` / `bounded-count` assertion (`min`,
    `max`, `exact`, optional `group_by`), orthogonal multi-field `group_by: [...]` across all
    assertion families, negated equality, and inline literal `set` targets.
  - Gap B1: general duplicate-key detection across YAML (unblocked by Phase 2's `ayaml` spans)
    and structured formats (`mattermost-56`, `zulip-28`).
  - Gap C4: semver range satisfaction and version comparison (21 stratum units; moves
    `toolchain-pin` from 63% to 83%).
  - Strict-JSON parse option (promotes 10 locale C units to A across 8 repos).
  - Per-group cross-relation `set` comparison and mixed-reader `union:` per-part `fields:` (with
    `join: { kind: anti | left }` deliberately deferred out of v0.24 per ADR-0050 D3).
- **DEC-8 bundled `version: 2` rulesets (ADR-0048 decision 3c):**
  - Allow bundled (`alint://bundled/...`) `version: 2` sources (keeping remote and nested
    `version: 2` refused until v2 stabilizes in v0.25).
  - Ship first-party opt-in bundled ruleset(s) for high-demand v2 policies (such as dependency
    version consistency) with `alint test` ownership cases.
- **Stratum re-scoring and section 12.1 measurements:** re-score the 20-repo corpus stratum
  against shipped v0.24, record updated coverage shares and v2 vs v1 benchmarks in
  `general-linter.md` section 12.4. `version: 2` remains experimental through v0.24.

### Phase 6: Release qualification and v0.24.0 cut

- Pre-release security, trust, and correctness audit covering every new surface (owned walker,
  `lsp-server` transport, bundled v2 sources, `ayaml 0.2.3` events/spans, new Phase 3 operators).
- Full release qualification:
  - `#482` differential YAML harness (value + event streams, seeds 1 to 12) + `yaml-compare.sh` +
    `yaml-memory-release.sh` + `yaml-reference-gate.sh` (verifying zero `saphyr` / `yaml_serde`
    in default release builds).
  - Corpus replay against v0.23.0 (130 repositories, including compare-mode run via #549): zero
    crashes and every diff explained in CHANGELOG (including #546).
  - Deterministic micro-bench gate (`micro-det`), `reference-perf-gate.sh` (without ADR-0045's
    probe allowance), and macro `bench-scale` on the canonical bench host (S1 to S4, SFIX, and
    the new #550 mid-size YAML scenario) verifying wall-clock and peak-RSS budgets.
  - Cross-platform CI green across Linux, macOS, and Windows (including the new cross-platform
    baseline fingerprint golden gate from #579).
- Documentation and CHANGELOG ("Upgrading from 0.23" prominently calling out Windows baseline
  regeneration for #579, `java-no-tracked-build` for #546, the 10k per-rule finding cap for #596,
  parent ignore-file warning for #568, and v2 additions), release-cut doc updates gated on the
  `v0.23.0` overlay until tag time (`docs/rules.md` lines 972, 974, 1046, 1054 updating pre-v0.24
  `constraint` prose for the 7 assertion families, v0.25 stability timeline, and `reference`
  triggers/redaction; `docs/site/concepts/composition/bundled-rulesets.md` updating `24` -> `25`
  bundled rulesets and the `alint://bundled/<name>@<rev>` note), tag `v0.24.0`, verify all release
  and post-publish smoke channels, review `bench-record`, and sync `alint.org`.

## 2. Out of scope (deferred deliberately)

- **v0.25 at the earliest:** Plugin Hosts Slice P2 (native Rust plugins, `alint-plugin-api`,
  `fixer` world, `alint build`, freezing `alint:plugin@0.1.0`; ADR-0048 decision 2).
- **v0.25 or v1.0:** stabilizing `version: 2` and opening remote and nested `version: 2` sources
  (ADR-0048 decisions 3a and 3c).
- **Non-blocking evaluations:** standalone `asamarts/ahcl` (#608 spike), standalone
  `asamarts/ajson` (#622), and standalone extraction of #573's owned parallel walker.

## 3. Phases summary

| Phase | Contents | Key issues / gates |
|---|---|---|
| 0 | ADR-0048, this plan, roadmap/general-linter/plugin-arch updates, #622, CI/release hygiene, `witforge 0.6.0`, `alint.org#128` | #621, #622, docs & witforge gates |
| 1 | P0/P1 security, trust, correctness, and quick dependency cleanups | #609, #545, #596, #600, #569, #583, #585, #579, #546, #601, #564, #547, #597, #581, #611, #612, #613, #615, #610 |
| 2 | Unified YAML track (`ayaml 0.2.3`, saphyr exit, bounded shared parse cache, alias-tree cap, span diagnostics) | #614, #549, #550, #548, #582, #572, #566, #571 |
| 3 | Nested-rule optimization wave and owned parallel walker | #620, #565, #584, #573, #568 |
| 4 | `alint-lsp` migration to `lsp-server` and HCL evaluation spike | #607, #608 |
| 5 | General-linter Phase 3 completion (design + ADR owner gate -> implementation -> stratum re-score) | #592, #593, #594, #595, #598, #570 + Phase 3 operators, B1, C4, strict-JSON, bundled v2 |
| 6 | Pre-release audit, full qualification (YAML, corpus, benches, cross-platform), v0.24.0 cut & site sync | all release gates |

## 4. Risks

- **Scope breadth.** Draining 41 issues alongside Phase 3 completion touches the walker, YAML
  engine, LSP transport, and `version: 2` evaluator. Strict phase ordering keeps the major
  structural shifts isolated (Phase 2 YAML -> Phase 3 walker -> Phase 4 LSP -> Phase 5 Phase 3
  completion) so each lands with green differential, corpus, and benchmark gates before the next
  starts.
- **Walker parity (#573).** Re-implementing `ignore`'s traversal carries edge-case risk across
  `.ignore`/`.gitignore`/`info/exclude`/global excludes, linked worktrees, overrides, and symlink
  loops. Mitigated by running full walker parity tests and the 130-repo corpus replay directly
  against `ignore 0.4.34` before deleting the old walker path.
- **LSP transport rewrite (#607).** Replacing `tower-lsp` touches shutdown, debouncing,
  cancellation, and workspace trust gating. Mitigated by full LSP unit, trust, audit, and VS Code
  / JetBrains E2E gates (including #545).
- **Shared parse cache RSS (#548 + #582).** Caching trees without a strict byte cap could lift
  peak RSS on large repos. Mitigated by bounding retained bytes, skipping caching for single-rule
  configs, and gating on 100k/1M and #550 RSS runs.
- **JetBrains Marketplace moderation (#545).** Trusted-project APIs vary across platform builds
  and Marketplace rejects internal APIs that `intellij-plugin-verifier` passes. Mitigated by
  checking `verifyNoMarketplaceDeniedApis` and Marketplace rules across `sinceBuild`..`untilBuild`.

## 5. Status log

- 2026-11-22: Phase 0 started. Owner decisions 1 to 8 settled and recorded in
  [ADR-0048](../../adr/0048-v024-owner-decisions.md); this plan written; #622 filed for `ajson`
  evaluation.
- 2026-11-22: Phase 0 complete. Merged #623 (ADR-0048, this plan, roadmap/design updates), #626
  (`url 2.5.9` for RUSTSEC-2026-0592 in `Cargo.lock` and `editors/zed/Cargo.lock`), #625
  (`witforge 0.6.0` consumer bump), #624 (#621 `bench-record.yml` baseline fallback + test,
  `release.yml` `actions/attest@v4.2.2`, `CHANGELOG.md:233`), and `asamarts/alint.org#128`.
  Post-Phase-0 audit closed a PR-body baseline-description edge case, added prerelease filtering
  and explicit `fetch-tags: true` in `bench-record.yml`, added legacy/wrong-generator header
  controls to `test-witforge-consumer.sh`, and refreshed `witforge-adoption.md`.
  Next: Phase 1 Batch 1 (P0 security and trust: #609, #545, #596, #600, #569, #583, #585).
- 2026-11-23: Phase 1 Batch 1 complete and audited. Merged #627-#633 closing all 7 P0 security and
  trust issues (#600, #609, #545, #596, #569, #583, #585). Post-Batch-1 adversarial audit closed
  remaining gaps in JetBrains trust lifecycle and open-tab reconnection (#634), top-level `limits:`
  configuration and full per-decode/per-rule enforcement across INI/XML/v2/constraint/key_parity,
  overlapping-pattern Aho-Corasick redaction and short-value structural masking across `fix`/`lsp`/
  `extends`, nested option escaping (`key_parity.ignore_keys`, `scope_filter`, JSONPath I-Regexp,
  `(?x)` whitespace, `git_commit_message.since`), JUnit/agent sanitization, and `roxmltree` 0.21.2
  duplicate-default-`xmlns` parity and attribute value spans (#635).
- 2026-11-23: Phase 1 Batch 2 complete and audited (#636, #637, #643; 7 correctness and usability
  issues: #579 `/` path tokens + cross-platform Windows/macOS fingerprint golden gates + Windows
  baseline regeneration docs, #546 `java@v1` `java-no-tracked-build` `has_gradle` gate +
  `**/src/**/build` exclusion, #601 ADR-0049 preferred visible paths on baseline entries preserving
  suppressed repeats and SARIF fingerprints, #564 IEEE-754 `f64` JSON number-range documentation,
  #547 pre-0.22 hidden-path baseline regeneration hint, #597 `--format` valid-format list +
  `key_parity` `strip_root` null-root error and empty-object root support, #581 interned
  `PROVENANCE_MARKER` source strings). Post-Batch-2 adversarial audit (#643) fixed O(K^2)
  duplicate-merge sorting, bounded baseline `paths` deserialization, hardened ADR-0030 forged-rule
  stripping, tightened pre-0.22 SemVer parsing and false-hint gating, normalized remaining Windows
  message/redaction/locator paths, validated `--format` up front across all subcommands, and
  replaced unbudgeted `single_root_kind` with O(1)-memory `strip_root_shape`.
- 2026-11-23: Phase 1 Batch 3 complete and audited (#644, #645, audit follow-up; 5 quick
  supply-chain cleanups -- completing all 19/19 Phase 1 issues): #611 replaced `directories` with
  an internal platform cache helper (including `std::env::home_dir()` fallback when `HOME` /
  `USERPROFILE` is unset/empty) and removed the sole MPL-2.0 copyleft exception (`option-ext`) and
  unused `0BSD` from `deny.toml` and `about.toml`; #612 replaced `content_inspector 0.2.4` in
  default release builds (still transitive dev-only via `trycmd`/`snapbox`) with an internal SIMD
  NUL/BOM/magic + 64-bit SWAR control-byte classifier; #613 replaced `node-tar` in `editors/vscode`
  with built-in `node:zlib` and a hardened, bounded in-memory USTAR/PAX/GNU-LongName single-file
  extractor plus unit and real-`tar` interop tests; #615 adopted `adotenv 1.6.0`
  (`ParseOptions::with_max_lines`, `BomPolicy::StripAll`, `Document::edit_with_options`,
  `parse_entries_compact_with_options`, `LookupIndex::unique`) with pre-encode budget guards;
  #610 replaced `java-properties 2.0.0` and `encoding_rs` with an internal streaming, span-native
  `.properties` parser supporting UTF-16 surrogate pairs, trailing EOF backslashes, whole-document
  verified edits, and whitespace-separated `=`/`:` value escaping (#646 closed the Batch 3 audit).
- 2026-11-23: Phase 2 complete and audited (#648, #649, #650, plus Phase 2 audit follow-up; all
  8/8 unified YAML and span-enabled diagnostic issues closed): Step 2a (#648) bumped `ayaml` to
  `=0.2.3`, migrated all four span-dependent paths (`structured_fix::yaml_`, `extract/yaml_rows`,
  `yaml_depth::mapping_subtree_is_literal`, `pinned_references`) onto `ayaml`'s spanned event
  stream via `alint_core::yaml`, removed `saphyr` and `saphyr-parser` from default release builds
  (#614, enforced by `yaml-reference-gate.sh` and `.alint.yml`), added `compare_mode` to
  `corpus-replay.yml` (#549), and added `peak_rss_kb` plus the opt-in `SYAML` mid-size YAML
  scenario to `bench-scale` (#550). Step 2b (#649) added content-addressed per-run and per-file
  structured-tree sharing via `SharedValue` and `Context::parse_structured` across rules,
  relations, manifest predicates, and structured facts (#548) and a `Send + Sync` `RetainedLease`
  bound by alias-replayed bytes across all input sizes (#582). Step 2c (#650) rejected duplicate
  `vars:` keys and relocated duplicate-key/field errors to the repeated key (#572), added line and
  column locations to `version: 2` unknown top-level keys (#566), and mapped `alint-lsp` config
  load, rule-build, `when:`, `limits`, `baseline`, and `--no-spawn` errors to their UTF-16 key-path
  spans (#571). Post-Phase-2 audit (#651) closed remaining span, cache, lease, LSP, and docs gaps.
- 2026-11-24: Phase 3 complete (all 5/5 nested-rule performance and owned-walker issues closed):
  Batch 3a (#652) hoisted invariant per-iteration work across `for_each_dir` /
  `every_matching_has` (-41% to -59% wall time on `cross_file_rules` Criterion benchmarks, #620)
  and narrowed all remaining whole-tree index scans across rules, facts, relations, and engine
  helpers with a debug-only `FileIndex` scan counter (#565, #584). Batch 3b (#653) replaced
  `ignore::WalkBuilder` with an owned parallel walker in `alint-core` (`walker_parallel.rs`) built
  on `ignore::gitignore::Gitignore` and `rayon::scope`, inspecting ignore files from `read_dir`
  listings with open-then-`fstat` bounded reads (zero per-directory `stat` probes), retiring
  ADR-0045's per-directory probe allowance from `ci/scripts/reference-perf-gate.sh` (#573), and
  skipping bad ignore files above the checked root with a visible warning while keeping strict
  failure inside the checked root and documenting the 32 MiB cap (#568). Post-Phase-3 audit
  remediation hardened `walk_subtrees` (exact on-disk casing, unified root `WalkCtx`, ancestor
  cycle seeding), made traversal and `IgnoreNode`/`ExplicitAncestor` drop iterative, added Unix
  `O_NONBLOCK` ignore-file opens and strict non-regular `.git` handling, restored redaction on
  `InstantiatedBuilder::build`, narrowed `visit_sensitive_paths` on all 12 remaining nestable
  rules and `TriggerScope::visit_matched_paths`, and fixed two vacuous scan regression tests
  (#654).
- 2026-11-24: Phase 4 complete (all 2/2 LSP and HCL issues closed): Batch 4a (#655, closes #607)
  migrated `crates/alint-lsp` from unmaintained `tower-lsp 0.20` and `tokio` to synchronous
  `lsp-server 0.10` + `lsp-types 0.95` + `crossbeam-channel 0.5` with an 8 MiB worker pool,
  generation-coalesced full checks, strict lock ordering, LSP 3.17 lifecycle error codes, and
  `Connection::memory()` protocol tests, removing 19 async crates (net `-17` `Cargo.lock` entries)
  with all 40/40 CI checks green (including Windows/macOS cross-platform and all 4 editor LSP E2E
  suites). Batch 4b (#656, closes #608) upgraded `hcl-rs` from `0.18` to `0.20` (`hcl-edit 0.10.0`),
  verified upstream's native `MAX_NESTING_DEPTH = 64` and `MAX_OPERATOR_DEPTH = 128` bounds across
  43 stress shapes (640 KiB release / 1,408 KiB Linux debug), retired the 512 MiB parse thread,
  64 KiB cap, and 270-line hand-rolled recursion pre-scanner, lifted `MAX_HCL_BYTES` to 4 MiB,
  eliminated `hcl::from_str`'s `O(N^2)` `IntoJsonSpec` `VecMap` bottleneck, and recorded the spike
  and no-`ahcl` decision in [hcl-spike.md](./hcl-spike.md). Post-Phase-4 adversarial audit
  remediation hardened `alint-lsp` (unbounded outbound relay decoupling `bounded(0)` stdio teardown,
  dedicated `alint-lsp-check` thread preventing worker-pool starvation, `shutdown` quiescing and
  in-flight `codeAction` draining, `$/cancelRequest` support, monotonic `doc_seq`, non-backwards
  `apply_check` merge, last-folder removal cleanup, panic protection, and `InvalidData` scrubbing)
  and hardened `structured_format/hcl.rs` + `structured_fix::hcl` (cumulative 64-level block+label
  depth in both parse and fix paths, direct `hcl::edit::structure::Body -> Value` conversion,
  1,024-entry object cap + 64M cumulative quadratic work budget, scanner fixes for odd `$`/`%` runs,
  Unicode heredocs, line-1 heredoc fallback, `%{if}`/`%{for}` directives, `BlockBrace` vs
  `ObjectBrace`, 256-level scan-depth cap, and single-parse batch HCL span resolution; #658).
- 2026-11-24: Phase 5 design and owner gate accepted: authored, audited, refined, and accepted
  [phase-3-completion.md](./phase-3-completion.md) and
  [ADR-0050](../../adr/0050-v024-phase-3-completion-design.md) locking decisions D1 to D11
  (composite `reference`, unified `count`, orthogonal multi-field `group_by` + per-group `set` +
  inline literal `set` targets, unified `compare`, offline `satisfies:` / `semver_eval.rs` [Gap C4],
  v1 `no_duplicate_keys` [Gap B1], strict-JSON mode, #592 `records:` + parameterized transforms +
  mixed-reader `union:`, #593/#594/#595/#598/#570, DEC-8 bundled `version: 2`, and `RELEASE = "v0.24.0"`
  schema/tooling updates), verifying the two stratum `cardinality`/`group` units (`forem-01`,
  `backstage-25`) against ADR-0048 decision 3b.
  Next: Phase 5 implementation batches 5b to 5d (operators, B1, C4, strict JSON, #592,
  bundled `version: 2`, stratum re-score) and Phase 6 release qualification.
- 2026-11-24: Phase 5 Batch 5a complete (5/5 v2 polish and preflight issues closed): added
  optional `bound:` fields (`optional: true`, `on_missing: skip | missing`) across `alint-core`,
  `alint-dsl`, `inspect_plan`, and `schemas/v2/config.json` (#593); added single-pass,
  redaction-aware `{{value}}` interpolation in `constraint` `set` `message:` templates with
  load-time validation (#594); added unique structural path-component target-file placement for
  missing-value `set` findings (#595); preserved transformed/filtered single-root stub rows on
  `RelationRows::stub_rows` so `group_by: entry.root` forms zero-row groups anchored at stub
  locale files (#598); and added `ci/scripts/check-v2-schema-id.sh` (+ hermetic 11-case test
  suite), wired into `preflight.sh`, `docs.sh`, `release.yml`, and `bump-version.sh` (#570).
  Post-Batch-5a adversarial audit (#662) closed 22 findings across optional `bound:` skip ordering
  and sibling error preservation, `stub_rows` memory bounds and `shrink_to_fit`, `{{value}}`
  redaction under `sensitive: true` / `reads_environment`, O(1) structural counterpart lookup, and
  `check-v2-schema-id.sh` hardening.
- 2026-11-25: Phase 5 Batch 5b complete (ADR-0050 D1, D2, D3, D4, and D8 / #592): added
  `records:` array-of-objects extraction across JSON/YAML/TOML with exact source spans and
  preserved number lexemes (`record: <JSONPath>`, optional `optional: true`), parameterized string
  transforms (`split`, `strip_prefix`, `strip_suffix`, `prefix`, `suffix`) with env-interpolated
  `split.part`, and mixed-reader `union:` per-part `fields:` (#592); added `reference` single-key
  and composite-key foreign-key assertions with `skip_missing`, `{{value}}`, sensitivity closure,
  and `TriggerScope` (D1); added unified `count` assertions (`min`/`max`/`exact`, `distinct`,
  per-row emptiness mode, grouped + `groups:` + `resource_groups` + `stub_rows`, D2); added
  orthogonal multi-field `group_by: [...]` across `equal`/`set`/`count`, per-group cross-relation
  `set` (`to.group_by`, `all_target_groups`), and inline literal `values:` targets (D3); added
  unified `compare` assertions (`eq`/`ne`/`lt`/`le`/`gt`/`ge` under `exact`/`lexical`/`numeric`,
  fail-loud on `Null`/`Missing`, D4); and updated `schemas/v2/config.json`, `inspect_plan`,
  `schemas/experimental/rule-trace.json`, and `docs/rules.md`. Post-Batch-5b audit (#664) closed
  remaining edge cases in `records:` bounds/spans, per-part union sensitivity closure, `compare`
  formatting, and test coverage.
- 2026-11-25: Phase 5 Batch 5c complete (ADR-0050 D5, D6, and D7): added zero-dependency
  `semver_eval.rs` (`node`, `cargo`, `pep440-basic`) powering `assert.satisfies` and
  `compare: { order: semver }` (D5, Gap C4); added `no_duplicate_keys` per-file rule kind
  (canonical kind #105) across YAML/JSON/dotenv/properties/INI/TOML/HCL with exact spans and
  first-occurrence related locations (D6, Gap B1); and added cache-isolated `strict: true` RFC 8259
  JSON parse mode across v1 (`json_path_*`, `json_schema_passes`, `key_parity`,
  `no_duplicate_keys`, `extract: { json: ... }`) and v2 (`entries:`, `records:`, `read:`) readers
  (D7). Post-Batch-5c adversarial audit (#666) closed semver edge cases (partial ranges, tilde,
  caret, PEP 440, negative zero, whitespace), `no_duplicate_keys` bounds and YAML/INI/HCL/JSON
  edge cases, and strict-JSON cache/path parity.
- 2026-11-25: Phase 5 Batch 5d complete (#667, ADR-0050 D10 and D11): enabled bundled
  `version: 2` rulesets (`alint://bundled/...` from `version: 2` configs with `as:` aliases,
  ADR-0022 direction check as user config error, `SourceLocator::Bundled`, zero
  `Config::config_files` pollution), shipped `alint://bundled/js/workspace-deps@v2` with passing
  and firing policy tests, bumped `schemas/v2/config.json` `$id` to `RELEASE = "v0.24.0"`, and
  re-scored the 20-repository corpus stratum. Post-Batch-5d adversarial audit closed diamond
  env-redaction order dependence and alias-correlation in `list`/`export-agents-md`, hardened
  `decode_bundled` validation parity and drop-in remediation hints, tightened
  `js/workspace-deps@v2` globs and protocol matching, extended remaining `rulesets/` audits,
  `docs-export` v2 sections/slugs, `facts.json` collision checks, and `version_two_schema` to v2
  bundled rulesets, and tightened the 20-repo stratum re-score to Scenario 2 (demoting
  `immich-52` and `nextcloud-b02` to `B->C`, keeping `fluentui-08` in C, and binary-verifying
  all corrected stratum shapes). Next: Phase 6 release qualification.
