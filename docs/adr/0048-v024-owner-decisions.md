---
status: accepted
date: 2026-11-22
decision-makers: asamarts
---

# 0048. v0.24 owner decisions: saphyr exit, Phase 3 completion, owned walker, LSP rewrite, backlog drain, and plugins to v0.25

## Status

Accepted (2026-11-22). Scopes v0.24 in [`v0.24/README.md`](../design/v0.24/README.md), completing
Phase 3 of [`general-linter.md`](../design/general-linter.md) (section 11.6), executing the YAML
saphyr exit of [ADR-0036](0036-ayaml-issues-and-the-saphyr-exit.md) and
[ADR-0041](0041-a-yaml-parser-facade-with-test-only-reference-parsers.md), draining the 41 open
GitHub issues (#545 to #621), and moving Slice P2 of the plugin hosts track
([ADR-0032](0032-v023-owner-decisions.md) decision 4,
[`plugin-architecture.md`](../design/plugin-architecture.md) section 10.3) from v0.24 to v0.25 at
the earliest. Amends [ADR-0032](0032-v023-owner-decisions.md) decision 4 and
[ADR-0034](0034-v2-version-boundary-and-phase-3-design.md) decision D3a (bundled `version: 2`
sources). Retires [ADR-0045](0045-count-the-walks-per-directory-probes-in-the-reference-gate.md)'s
per-directory probe allowance once the owned parallel walker lands (#573).

## Context

- **v0.23.0 shipped cleanly on 2026-11-22**, launching experimental `version: 2` (`equal`,
  `unique`, `set`), the pure-Rust `ayaml 0.2.2` check-path cutover, `--untrusted` / `--no-spawn`,
  and GHSA-78jj-xfpm-g6px.
- **Upstream crates are ready.** `ayaml 0.2.3` shipped with both `asamarts/ayaml#123` (plain
  scalar spans without trailing whitespace) and `asamarts/ayaml#128` (`SpannedTree` with path
  lookup), plus `ParseOptions` / `*_with` and `Error::masked()`, unblocking the saphyr exit (#614).
  `adotenv 1.5.4` and `witforge 0.6.0` are also released with zero open issues.
- **Open backlog of 41 GitHub issues (#545 to #621).** These span security and DoS hardening
  (#609 `roxmltree` CVE-2026-92987, #596 `G x V` finding explosion, #582 alias-expanded YAML trees,
  #569 `key_parity` pointer memory, #585 env redaction, #583 nested option escaping, #600 JUnit and
  agent bidi/zero-width escaping, #545 JetBrains workspace trust), YAML infrastructure (#614, #548,
  #549, #550), Phase 3 v2 gaps (#592 to #598, #566, #570), correctness and cross-platform bugs
  (#579 Windows path tokens, #601 baseline repeat stability, #564 JSON > `f64`, #572 duplicate
  keys, #546 `java-no-tracked-build`, #571 LSP error spans, #547 pre-0.22 baseline hint),
  performance and walker bounds (#620, #565, #584, #573, #568, #581), dependency modernization
  (#607 `tower-lsp`, #608 `hcl-rs`, #610 `java-properties`, #611 `directories`, #612
  `content_inspector`, #613 `node-tar`, #615 `adotenv`), and benchmark CI (#621).
- **Phase 3 and Plugin Hosts sizing.** Completing Phase 3 alongside all 41 issues is already a
  full, cohesive minor release. Plugin hosts Slice P2 (native Rust plugins, `alint-plugin-api`,
  `fixer` world, `alint build`, freezing `alint:plugin@0.1.0`) is an independent L-sized effort
  (6 to 10 engineer-weeks) that benefits from letting Phase 3's final row and adapter shapes settle
  before contract `0.1` freezes ([ADR-0027](0027-plugin-contract-0-1-stays-a-draft.md)).

## Decision

1. **No v0.23.1 patch release; roll everything cleanly into v0.24.0.** Security, trust, and
   correctness fixes (#609, #545, #600, #596, #579, #546) land early on `main` (Phase 1) and ship
   together in v0.24.0 rather than cutting a separate patch release immediately after v0.23.0.
2. **Defer Plugin Hosts Slice P2 to v0.25 at the earliest.** v0.24 keeps the witforge consumer
   current by bumping `crates/alint-witforge-consumer` and `ci/scripts/witforge-consumer.sh` to
   `witforge 0.6.0`, while `alint:plugin@0.1.0` stays a draft until P2 loads plugins in v0.25
   ([ADR-0027](0027-plugin-contract-0-1-stays-a-draft.md)). Amends ADR-0032 decision 4.
3. **Complete Phase 3 in v0.24, keeping `version: 2` experimental for one soak release.**
   - **3a (Scope and stability):** v0.24 delivers the rest of Phase 3 (design doc and ADR gate
     before implementation) covering #592 to #598, #566, #570, the remaining assertion operators,
     high-ROI Phase 8 stratum capabilities (gap B1 general duplicate keys, gap C4 semver range
     satisfaction, strict-JSON parse option, anti-joins / mixed-reader unions where justified),
     DEC-8 bundled templates/rulesets, and a corpus stratum re-score. `version: 2` stays
     experimental through v0.24 so the newly added operators soak for one release before freezing
     in v0.25 or v1.0.
   - **3b (`cardinality` and `group`):** fold `cardinality` into a unified `count` /
     `bounded-count` assertion (with `min`, `max`, `exact`, and optional `group_by`) and deliver
     `group` via orthogonal multi-field `group_by: [...]` across every assertion family (`equal`,
     `unique`, `set`, `reference`, `count`), verifying the two exact stratum units during the
     Phase 3 design pass.
   - **3c (`version: 2` source kinds):** open **bundled** (`alint://bundled/...`) `version: 2`
     sources in v0.24 so DEC-8 common policies can ship as opt-in bundled rulesets. Keep **remote**
     (`https://...`) and **nested** (`nested_configs: true`) `version: 2` sources refused until
     `version: 2` stabilizes in v0.25. Amends ADR-0034 decision D3a.
4. **Heavy dependency replacements (#609, #607, #608).**
   - **4a (#609 XML):** upgrade `roxmltree` from `0.20` to `0.21.2+` to eliminate CVE-2026-92987
     from `Cargo.lock`, measuring debug and release stack usage on 1 MiB and 2 MiB stacks and
     re-verifying `xml_scan.rs` parity and the bomb suite. Pivot to an iterative XML 1.0 parser
     only if `0.21.2+` fails stack headroom or parity.
   - **4b (#607 LSP):** migrate `alint-lsp` from unmaintained `tower-lsp 0.20` to `lsp-server`
     (maintained by `rust-analyzer`), dropping `tower-lsp`, `tower-service`, `tokio`, `tower`,
     `futures`, `dashmap`, and about 80 transitive crates, and aligning the server with alint's
     synchronous `rayon` engine.
   - **4c (#608 HCL):** check whether newer `hcl-rs` / `hcl-edit` add internal recursion and stack
     bounds; if upstream remains unbounded, run a focused feasibility and value spike in v0.24 to
     decide whether spinning up a standalone `asamarts/ahcl` crate is worthwhile, without blocking
     v0.24.0 on a full HCL2 parser rewrite.
5. **Owned parallel walker and ignore-file policy (#573, #568).**
   - **5a (#573):** own the parallel directory walker in `alint-core` on top of
     `ignore::gitignore::Gitignore`, inspecting ignore files from the directory listing with
     open-then-`fstat` checks so bounding ignore files costs zero extra `stat` calls per directory
     and retires ADR-0045's probe allowance. Keep the walker behind a clean internal boundary so
     extracting and publishing it as a standalone crate can be evaluated after it is proven.
   - **5b (#568):** skip unreadable, non-regular, or oversize ignore files **above** the checked
     root with a visible warning, keep strict walk failure for bad ignore files **inside** the
     checked root, and keep the 32 MiB ignore-file size cap (documented).
6. **Cross-platform path tokens and `java@v1` fix (#579, #546).**
   - **6a (#579):** render `{path}` and `{dir}` path tokens with `/` on every platform, enable
     cross-platform baseline fingerprint golden checks on Windows and macOS in CI, and clearly
     document in CHANGELOG and user docs that Windows-authored baselines covering `for_each_*`
     nested rules need regeneration (`alint baseline`).
   - **6b (#546):** keep `java-no-tracked-build` as `kind: dir_absent`, exclude `**/src/**/build`,
     and gate it on Gradle presence (`build.gradle`, `build.gradle.kts`, `settings.gradle`,
     `settings.gradle.kts`), with a ruleset test, CHANGELOG entry, and corpus replay verification.
7. **Finding bounds and JSON number range (#596, #564).**
   - **7a (#596):** enforce a per-rule hard finding cap (default 10,000 findings per rule,
     configurable via limits) for `constraint` `set` and `key_parity`; on breach, stop emitting
     further findings for that rule and emit one non-baselinable analysis-failure finding naming
     the cap and suppressed count. Normal repositories below the cap keep identical per-value
     findings and baseline fingerprints.
   - **7b (#564):** keep standard `serde_json` without `arbitrary_precision`, document the
     IEEE-754 `f64` number-range limit on JSON-reading kinds, and track evaluation of a dedicated
     bounded, span-native JSON/JSONC parser crate (`asamarts/ajson`) in #622 (non-blocking for
     v0.24.0).
8. **Unified two-step YAML track (#614, #548, #582, #549, #550).**
   - **Step 1 (#614 + `ayaml 0.2.3` + #549 + #550):** bump `ayaml` to `=0.2.3` (adopting
     `ParseOptions` / `*_with` and `Error::masked()` on redacted paths), migrate all four `saphyr`
     call sites (`structured_fix::yaml_`, `extract.rs`, `yaml_depth::mapping_subtree_is_literal`,
     `pinned_references`), remove `saphyr` and `saphyr-parser` from default release builds
     (test-only behind `yaml-reference`), add event-stream fuzzing to #482, add corpus replay
     compare mode (#549), and add the mid-size YAML `bench-scale` scenario (#550).
   - **Step 2 (#548 + #582):** add a memory-budgeted per-run structured-tree cache (skipping
     caching when only one structured rule exists, with LRU eviction under a byte cap) paired with
     a retained-memory bound on alias-expanded YAML trees (#582).

## Consequences

- **One focused, complete v0.24 release.** All 41 pre-existing open issues (#545 to #621) are
  explicitly mapped into phases, `saphyr` leaves default release builds, `tower-lsp` + `tokio`
  leave the dependency graph (~80 crates dropped, alongside `directories` / MPL-2.0,
  `content_inspector`, `java-properties` / `encoding_rs`, `saphyr`, and `node-tar`), and Phase 3
  completes with real user soak time before `version: 2` freezes.
- **Documented behavior changes for v0.24.0 CHANGELOG:**
  - Windows users with baselines containing `for_each_*` rules referencing `{path}` or `{dir}`
    must regenerate their baseline (`alint baseline`) as Windows path tokens now normalize to `/`
    to match Linux and macOS (#579).
  - `java@v1`'s `java-no-tracked-build` stops flagging source packages under `**/src/**/build` and
    pure-Maven repos (#546).
  - Rules emitting more than 10,000 missing-value findings in `constraint` `set` or `key_parity`
    now halt at the cap with a non-baselinable analysis failure (#596).
  - Bad ignore files above the checked root warn and are skipped rather than aborting the walk
    (#568).
- **Standalone crate evaluations tracked deliberately.** Rather than rushing speculative crate
  extractions, #608 (`ahcl`), #622 (`ajson`), and #573's walker seam are evaluated on concrete
  value and complexity before committing to separate repositories.
- **Plugin loading shifts to v0.25 at the earliest**, keeping `alint:plugin@0.1.0` a draft through
  v0.24 so Phase 3's final adapter and row shapes settle before the contract freezes.

## Considered Options

- **Release cadence:** cutting a narrow v0.23.1 patch for #609, #545, #600, #621, and
  `CHANGELOG.md:233` before starting v0.24.0 (faster delivery for JetBrains users and scanner
  alerts, at the cost of a full multi-channel release + site sync right after v0.23.0).
- **Plugin Hosts P2:** building Slice P2 inside v0.24 alongside all 41 issues and Phase 3 (huge
  release risk and freezes `alint:plugin@0.1.0` while Phase 3 adapters change), or doing P2
  design-only in v0.24.
- **Phase 3 and v2:** declaring `version: 2` stable in v0.24 (freezes newly added operators on day
  one without soak time); shipping separate overlapping `cardinality` and `group` assertion
  variants; opening remote and nested `version: 2` sources before v2 stabilizes.
- **Dependencies:** rewriting XML in v0.24 regardless of `roxmltree 0.21.2+` stack headroom;
  moving `alint-lsp` to `tower-lsp-server` (smaller diff, but keeps ~80 async crates); committing
  to a full custom HCL2 parser inside v0.24.0.
- **Walker and ignore policy:** carrying a published fork of `ignore` (smaller diff, extra crate
  to maintain on crates.io) or keeping `ignore 0.4.34` + ADR-0045 probes; raising the ignore cap
  to 100 MiB (larger `globset` memory amplification ceiling).
- **Findings and JSON:** per-group collapsing above K missing keys (creates a baseline-fingerprint
  cliff and does not bound G by itself); enabling `serde_json/arbitrary_precision` workspace-wide
  (heap-allocates every JSON number and alters `1 == 1.0` equality across all crates).

## More Information

- [`v0.24/README.md`](../design/v0.24/README.md): the v0.24 phased plan mapping all 41 issues
  (#545 to #621), Phase 3 completion, and release gates.
- [`general-linter.md`](../design/general-linter.md): sections 11.1, 11.6, and 12.4.
- [`plugin-architecture.md`](../design/plugin-architecture.md): section 10.3 (Slice P2).
- [ADR-0027](0027-plugin-contract-0-1-stays-a-draft.md),
  [ADR-0032](0032-v023-owner-decisions.md),
  [ADR-0034](0034-v2-version-boundary-and-phase-3-design.md),
  [ADR-0036](0036-ayaml-issues-and-the-saphyr-exit.md),
  [ADR-0041](0041-a-yaml-parser-facade-with-test-only-reference-parsers.md),
  [ADR-0045](0045-count-the-walks-per-directory-probes-in-the-reference-gate.md).
- Issues #545 to #622.
