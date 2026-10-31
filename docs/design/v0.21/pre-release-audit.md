# v0.21 pre-release audit and remediation plan

Status: Plan (2026-10-29). Audited `main` at `f4e5fa225`; version 0.20.1, v0.21.0 untagged.
Progress: W1 done (#407, merged 2026-10-30: all twelve items, the review's follow-ups, and
ADR-0025's `key_parity` changes, with tests), and a review of that increment fixed more (section
3); W4 housekeeping and the six Dependabot merges done 2026-10-30; W2 done in #411 except W2-7,
which waits on witforge 0.1.0 ([`witforge-adoption.md`](witforge-adoption.md)); W3 and the rest
of W4 not started.
Decisions: the owner decided every item in section 7 on 2026-10-30; the four that change the
design are recorded in [ADR-0025](../../adr/0025-v021-pre-release-decisions.md), which partly
supersedes [ADR-0024](../../adr/0024-relation-ir-decisions-after-review.md) decision 4.

[`r8-readiness.md`](r8-readiness.md) declared v0.21 a GO on performance, memory, and the corpus
replay. An independent pre-tag audit found that the GO cannot stand: `key_parity`, the release's
only new rule kind, misses real differences in common layouts, its documented `.env`
setup fails, and the go/no-go assessment the release acceptance requires was never written. The
release is on hold until workstreams W1 and W2 below are done and gated.

## 1. Method

Three independent review passes (the v0.21 code against the design's invariants, documentation
and generated metadata, CI and release tooling) each reported findings with file and line. Every
finding in sections 3 to 6 was then re-checked: the behavioural ones by running the binary built at
`37038dd5d` (code-identical to `f4e5fa225`) against a scratch repository, or by a scratch
worktree; the rest by reading the code. "Reproduced" means a run showed the failure; "read" means
the code plainly does it.

## 2. What holds

These were checked by the review passes and found sound.

- `main` is clean and every push workflow on `f4e5fa225` is green (the scheduled mutants run is
  not; see W4); `cargo fmt --check` passes; alint's own lint passes
  all 50 rules; `gen-facts`, `gen-schema`, and `gen-roadmap --check` pass; `facts.json`, the
  README, and the site agree on 103 kinds plus 11 aliases.
- The performance evidence in `r8-readiness.md` matches `docs/benchmarks/REFERENCE-v0.18.2.md`
  (839/441 reads, the instruction counts), and the replay figures match corpus run 40912352842
  (106/110 replayed with four expected skips).
- The `import_gate` literal prefilter (#400) cannot produce a false negative: the preset regex is
  case-sensitive and every match contains `use`.
- Deep input fails loudly without a crash in every `key_parity` format (JSON, flow YAML, Android,
  `.resx` at 100k levels; TOML headers at 50k segments), under 45 MB RSS.
- Path confinement holds: `RelPath::parse` rejects absolute and escaping paths, and literal reads
  go through `resolved_within_root`.
- A shadow mismatch panics; only `alint-e2e` (a dev-dependency) and the corpus replay enable
  `shadow-ir`. No relation or `plugin_api` type derives serde or `JsonSchema`.
- `deny.toml` has an empty ignore list and `cargo deny` passes; the MSRV job is green.
- No new published crate since v0.20.1, so no crates.io trusted-publisher bootstrap is needed.

## 3. W1: `key_parity` correctness (blocks the tag)

| ID | Finding | Evidence | Fix | Gate |
|---|---|---|---|---|
| W1-1 | Since #388 the analyzer joins key segments with `.` (`key_parity.rs:227`) while the rule looks the result up by `render_key()`, which brackets a segment containing `.`, `[`, `]`, a quote, or nothing. Dotted keys are never reported in either direction (a `.properties` peer missing `app.title` passes; same for `.resx` `button1.Text` and dotted dotenv or INI keys), and a flat `{"nav.home":…}` compares equal to a nested `{"nav":{"home":…}}`, which the key model (README, "Key model (V21-4)") forbids. | Reproduced | The analyzer renders keys with the same injective `Pointer::render` the rule uses, so set difference runs on distinct key identities. | Unit and e2e tests with dotted, bracketed, and empty-segment keys in properties, dotenv, INI, JSON, and `.resx`, plus flat-versus-nested. |
| W1-2 | A peer with no comparable keys gets no "missing" findings: the analyzer builds its per-path sets from rows, so a peer with no rows has no entry. `fr.json` = `{}`, an empty `.resx`, and a peer whose every key `ignore_keys` removes all pass. | Reproduced | The analyzer is built per peer and knows the peer path, so an empty peer set still compares against the reference. | Unit and e2e tests for an empty peer and a fully ignored peer. |
| W1-3 | `if let Ok(diffs)` (`key_parity.rs:554`) turns an analyzer error, including the bridge's finding-count limit, into "no findings", breaking invariant 2 (fail loud). `key_rows` drops a row it cannot build (`filter_map`) for the same effect. | Read | An analyzer error becomes an `evaluation` analysis failure on the peer; rows are built from the existing `Resource` values and an unbuildable row is an error. | A unit test that forces the analyzer error path. |
| W1-4 | A literal `peers` entry is read only through the index (`Selector::select`, `relation/resource.rs:117`), so a gitignored literal peer is never found. The design's own example (`reference: .env.example`, `peers: .env`) reports "`peers` glob ".env" matched no files other than the reference". `docs/rules.md:889` says peers respect `.gitignore`, contradicting the design. | Reproduced | A `peers` value with no glob metacharacters is a literal resource read from disk, as the design specifies; `rules.md` is corrected. | An e2e scenario with `.env` in `.gitignore`, firing and silent. |
| W1-5 | TOML inline tables are followed one level (`adapters/toml.rs:114`, `get_values()`), so `[x.a] b = 1` and `x = { a = { b = 1 } }` report "missing `x.a.b`" and "extra `x.a`" although they mean the same thing. | Read (reproduced by the audit pass) | Recurse into nested inline-table values. | An adapter unit test and a `key_parity` TOML case. |
| W1-6 | With `strip_root` or `plurals: cldr`, findings lose their line and column and the related reference location: `KeyFile::range` (`key_parity.rs:136`) looks the transformed key up in the untransformed key map. This matters for SARIF and the LSP. | Read (reproduced by the audit pass) | Look spans up in the transformed map, which already carries the right span. | A Rails YAML test asserting ranges on both findings. |
| W1-7 | `merge_plural_groups`' `all_categories` check (`key_parity.rs:799`) is always true, since only category-named members are collected, so an object holding `other` plus non-category keys is partly merged. A merged i18next base can also collide with an existing key of the same name. | Read | Merge only when every child of the parent is a CLDR category; leave a colliding base unmerged. | Unit tests for both cases. |
| W1-8 | TOML parse failures echo the source line, value included, although findings must quote keys only. | Read | Report the TOML error's position and reason without the source snippet. | A unit test asserting a value never reaches the message. |
| W1-9 | A reference path containing glob metacharacters would not match its own `--changed` edit, since the path scope compiles it as a glob. | Read | Escape the reference when building the scope. | A unit test. |
| W1-10 | `.resx` is not inferred from its extension, although the design says the extension picks the format except for `.xml`; `rules.md` says it must be explicit. | Read | Infer `.resx`; `.xml` stays explicit. | Extend `format_inference_and_models`. |
| W1-11 | Coverage the release acceptance lists is missing end to end: properties, dotenv, INI, nested YAML with `plurals: cldr`, i18next, duplicate keys (silent), a key-model mismatch, and an unparsable reference. `changed_key_parity_unrelated_skips.yml` cannot fail: its files already match, so a full run passes too. The pilot comparison test (`key_parity.rs:1435`) uses the same `.join(".")` as the bug and cannot catch it. | Read | Add the scenarios; give the skip scenario a tree that fails a full run; make the pilot oracle compare key identities (#407 compares the injective renderings, which is equivalent to comparing segment lists). | The scenarios themselves. |
| W1-12 | Invariant 4 (determinism) has no test: nothing runs `key_parity` with a shuffled index order or several thread counts. The output is deterministic as built (BTree types, sorted peers), but nothing proves it. | Read | A test that evaluates over shuffled index orders and thread counts and requires identical findings and fingerprints. | The test. |

Two documentation corrections travel with W1. First, a key-model mismatch that only reading a
peer reveals is an analysis failure on that peer, while `rules.md` and this directory's README
(line 196, corrected with this plan) called every mismatch a configuration error; ADR-0024 says only that the comparison is
refused, and a mismatch the configuration itself shows (an explicit `format`, or a literal peer)
stays a configuration error. Second, `rules.md:893` says inserting a missing key "is a
Suggestion", which reads as if `alint fix` offers one; the rule has no fix. The stale module doc
in `key_parity.rs` ("TOML and the Android/.resx dialects arrive with R9") is updated too.

#407 also makes changes the table does not list: a missing literal peer is a fail-loud analysis
failure (before, a gitignored peer never reached the index, and an indexed one that was missing
was a missing-file violation); the key-set analyzer rejects a row with no key path or from a
third file instead of ignoring it; both have tests. It also adds `allow_missing_peers` and the
`ignore_keys` name matching of ADR-0025, and updates the CHANGELOG `key_parity` entry and the
schema descriptions of `peers`, `format`, `ignore_keys`, and `allow_missing_peers`, which W3's
changelog item builds on.

A review of the whole increment after #407 merged found more in `key_parity`, fixed with tests in
the follow-up pull request:

- **Values in parse errors.** An INI parse error quoted the offending line and an XML parse
  error could quote an entity name, so a secret on a malformed line reached the finding. Both now
  report a position only, as the other formats do.
- **Plural groups decided per file.** CLDR plural groups were merged in each file on its own, so
  a group complete in the reference and partial in a peer was compared as one key against loose
  members. Groups are now decided over each reference-peer pair.
- **`strip_root` on a file without one object root.** A leaf root or a stray second top-level key
  was compared unstripped and reported every key as different. Such a file is now an analysis
  failure (the reference stops the run; a peer is skipped), and `strip_root` on a format without
  nesting is a configuration error.
- **An explicit `format` on a peer of another key model** was compared under the forced format.
  A peer whose extension implies another key model is now refused as before, with or without
  `format`, and so is a literal peer at configuration time.
- **Bracket-shown names in `ignore_keys`.** A single-name key shown as `['app.title']` could not
  be ignored by that form; it now can. An index pattern (`[0]`), which never matches a key, is a
  configuration error, and so is a `peers` value with surrounding whitespace.
- **YAML complex keys** (a mapping or sequence used as a key) were flattened silently; they are
  now an analysis failure.
- Smaller: messages name formats by their configuration spelling, the report notes no longer
  print a peer's path twice, and the glob matchers compare bytes without allocating.

## 4. W2: the other gaps behind `r8-readiness.md`'s "pass" rows

The owner's direction is to fix each gap where possible rather than downgrade the claim. A gap
that cannot be closed in v0.21 is listed with its reason and the readiness row says so.

| ID | Gap | Evidence | Fix |
|---|---|---|---|
| W2-1 | Registry injection is incomplete. `alint test` builds `Runner::new` without `with_registry(cli.registry())` (`crates/alint/src/test_cmd.rs:115`); `rules show` validates the kind against the static catalog, then `expect`s the injected registry to have it (`crates/alint/src/rules.rs:413`), a panic for a registry without a built-in kind; `rules list` (`KIND_CATEGORIES`) and LSP hover (`alint-lsp/src/render.rs:54`) read static tables. (`init` and `suggest` never consult a registry at run time.) No test proves a custom kind reaches a rule through the CLI, the LSP, or the testkit. | Read | Thread the registry through each path, return an error instead of panicking, and add one test per surface that registers a custom kind and observes it run. |
| W2-2 | `ci/scripts/check-shadow-feature-tree.sh` passes while `shadow-ir` leaks: with `default = ["leak"]` and `leak = ["alint-rules/shadow-ir"]` in `crates/alint/Cargo.toml` it printed "absent" for every build and exited 0, while `cargo tree -i alint-rules -f '{p} [{f}]'` showed `[shadow-ir]`. It also omits the `fs-benches` feature and per-target resolution the bench and release builds use. | Reproduced (scratch worktree) | Inspect `alint-rules`' resolved features with `cargo tree -i alint-rules -e normal,build -f '{p} [{f}]'` for every build's exact package, feature, and target set, and add a negative control that injects a leak and requires the script to fail. |
| W2-3 | Most relation limits are defined but never enforced: `RowsPerRun`, `JoinInputs`, and `JoinOutputs` have no use outside `limits.rs`, `RunBudget` is exported and unused, and `DistinctValues` appears only in a doc comment. The `DecodeCache` key leaves limits out by design (`cache.rs:234`), so two consumers with different limits would share the first one's decode; latent today, since every caller passes `Limits::default()`. | Read | Enforce `RowsPerRun` through `RunBudget` in the decode path and `DistinctValues` at set conversion; key the decode cache on the effective limits; mark the join limits as reserved for Phase 3, where joins arrive, with a test that keeps them unused until then. |
| W2-4 | The `plugin_api` import-boundary test scans text: it checks only lines starting with `use`/`pub use` plus `crate::` substrings, so `super::super::X` in type position, an external crate path such as `serde::Serialize` in a derive, or a `use` split across lines passes. The module is clean today. | Read | Parse the module and visit every path, allowing only `std`, `core`, `alloc`, the module's own items, and bounded `super::`. |
| W2-5 | Invariant 6: `FileIndex::read_cache`, `decode_cache`, and `enable_relation_caches` (`walker.rs:315-329`) are documented public methods that return or touch doc-hidden relation types. The test checks only the `mod` declarations. | Read | `#[doc(hidden)]` on the three methods, asserted by the existing visibility test. |
| W2-6 | The contract bridge (`relation/contract.rs`) does not check a finding's or related location's path, or a row's resource, against the input resources, and accepts any index id. Finding paths are not even normalized or confined: `to_violation` copies them as given (`contract.rs:206`), while row paths at least pass `RelPath::parse` (`contract.rs:146`). Latent until a plugin host exists. | Read | Validate them in the bridge and reject what does not belong to the input. |
| W2-7 | Nothing in CI checks the WIT draft; R7's evidence is a manual `witforge check`, and nothing compares `plugin_api` with `plugin.wit`. | Read | The tag waits for witforge 0.1.0 (ADR-0025 decision 4): pin it, then run the check, generation, and regeneration `--check` in default CI. #409 adds the generated consumer crate, `ci/scripts/witforge-consumer.sh` with its controls, and a test that compares `plugin_api` with the generated types; the review, the decisions (contract 0.1 stays a draft, ADR-0027), and the path to the CI job are in [`witforge-adoption.md`](witforge-adoption.md). |
| W2-8 | The go/no-go assessment is missing. The release acceptance and the stop rule (`general-linter.md` section 11.1) require a written judgment of whether the ported path became simpler and more consistent, with evidence; `r8-readiness.md` covers performance and the replay only. | Read | The verdict is decided (ADR-0025 decision 3: GO on consistency, three simplicity gates in v0.22). Write the assessment in `r8-readiness.md` from the port's diff, the shadow's shared code paths, and W1's lessons. |
| W2-9 | S4's workload changed without a revision bump (decided: revision 4, terminated sources, and a same-session kbench run of v0.20.1 against v0.21 on the revision-4 tree, recorded in `r8-readiness.md`). #399 changed `lorem_bytes`, which S4's polyglot tree also uses (`crates/alint-bench/src/tree.rs:375/407/439`), but S4 stayed at revision 3 (`xtask/src/bench/mod.rs:210`). At the tag, `bench-gate` resets S1-S3 and SFIX, so S4 is the only scenario still compared with v0.20.1, across a changed workload. S4's generated sources also still lack the final newline #397 added for S2. | Read | Bump S4 to revision 4 and newline-terminate its generated sources, and say so in the changelog. |

Accepted as is: `cargo test --workspace` unifies `alint-e2e`'s dev-dependency feature, so the CLI's
own test binaries run with the shadow enabled. That adds checking and never reaches a shipped
build (W2-2 guards that).

## 5. W3: documentation and release preparation

- **CHANGELOG `[Unreleased]`:** one `### Fixed` (there are two), one section order (Security
  first when present, then Added, Changed, Performance, Fixed, as 0.18.0 to 0.20.1 use;
  `RELEASING.md` step 2 names no Performance section, so it gains one), the `import_gate` prefilter under Performance,
  the plugin-contract entry reworded to say what shipped (doc-hidden modules with no semver
  promise, no plugin host; the public additions are `Runner::with_registry` and the test-only
  `shadow-ir` feature), the benchmark entry corrected (every size, and #399's whitespace change),
  W1 and W2's user-visible fixes, and the summary paragraph at the cut.
- **Stale ADR-0023 citation:** the ROADMAP's v0.21 section (`docs/design/ROADMAP.md:1673`, public on
  alint.org) cites ADR-0023 alone; it gains ADR-0024, which supersedes it. (`REFERENCE-v0.18.2.md`
  and `general-linter.md` already cite both.)
- **`plugin-architecture.md`:** status "not yet implemented" and "there is no injection hook"
  (line 441) predate slice P1.
- **`RELEASING.md`:** `ci.yml` is described as "Self-hosted Linux" (push events run on
  `ubuntu-latest`); the yank step omits `alint-testkit`; the step 3 preflight description omits
  the `shadow-feature` and `secrets-inv` steps; step 4's `git add` list omits
  `docs/design/ROADMAP.md`; the `publish = false` note no longer fits.
- **`key_parity` in the narrative docs:** the cookbook recipe "Keep `.env` keys aligned with
  `.env.example`" still uses a `cross_file` workaround; the changed-mode page's list of kinds
  skipped when their scope misses the diff lacks `key_parity`; the cross-file concepts page and
  the README's cross-file tour do not mention it. Concept pages reach alint.org from `main`, so
  pre-release additions carry `alint:since=0.21` markers.
- **`xtask --help`:** `bench-gate` has an empty description and its text sits in `micro-det`'s
  (`xtask/src/main.rs:202-215`).
- **`docs/adr/`:** no index. The bundled `docs/adr` ruleset's `adr-filename-pattern` applies to
  `docs/adr/*.md`, so an index there needs an exclusion or a home elsewhere (for example a table
  in `docs/design/README.md`).
- **Absolute paths in read errors:** a literal path that is a directory or another non-regular
  file is reported with its absolute path (`alint-rules/src/io.rs:14`,
  `relation/cache.rs:206`), so the message depends on the machine. Fix both together: the
  `cross_file` shadow compares the two paths' messages.
- **`cross_file/shadow.rs` module doc:** it still says read/decode failures and regex extracts
  are skipped; since #403 they are compared, and the only remaining skip is a path that is not a
  valid `RelPath`.
- **This directory:** the README's "Status and next steps" reads out of order (R9 first, R1
  last); `r8-readiness.md` moves from GO to HOLD now and returns to a verdict after W1 and W2.
- **Release steps** (after W1-W3): `bump-version.sh 0.21.0`, preflight and the MSRV check, a
  fresh corpus replay (W1 already changes the engine: #407 touches `alint-core`'s TOML adapter),
  tag, the bench-record PR
  and the alint.org bench snapshot, the Action doc pin, and the alint.org pins and `STATE.md`
  (kinds 102 to 103, headline 113 to 114).

## 6. W4: CI, dependencies, and housekeeping

- **Redaction hides structural configuration errors.** Every configuration load (not only
  `validate-config`) replaces each validation error with `<redacted>` for a rule that has an
  env-interpolated field or `sensitive: true` (`alint-dsl/src/lib.rs:539-549` and `:553-555`), even an error that
  quotes no value. It hid the message of `facebook/react`'s own configuration error during R5
  (its `scope_filter.changed_since` is invalid; the redaction did not cause the failure). It is
  pre-existing, tracked nowhere, and behind no readiness row, so it does not block the tag:
  redact only errors that can quote a value, and keep structural errors readable.
- **Mutants nightly never runs.** `mutants.yml:34` targets the self-hosted `alint` runner, whose
  listener is held (`ci.yml` routes every job to `ubuntu-latest`). Every scheduled run since
  2026-09-22 was cancelled after about a day in the queue; the last success (2026-09-18) was a
  tiny crate. Replace it with a pull-request `--in-diff` job (section 7, item 3).
- **No scheduled advisory check.** Deny and Audit run only when Rust, supply-chain, `.github/`,
  or `ci/` files change, and in the release preflight, so a new RustSec advisory surfaces on the
  next such pull request. Add a weekly scheduled run.
- **Unpinned tools:** `ci/scripts/audit.sh` installs `cargo-audit` and `mutants.yml` installs
  `cargo-mutants` without a version (`cargo-deny` is pinned).
- **MSRV headroom is zero:** nine locked packages (`ignore`, `globset`, `encoding_rs`, `sha2`,
  `proptest`, and four `gungraun` crates) already require exactly 1.88, so the next dependency to
  raise it fails the MSRV job. Write the rolling policy into `CONTRIBUTING.md` and
  `RELEASING.md` (section 7, item 8).
- **Release toolchain:** `RELEASE_RUST` is 1.98.1 while stable is 1.99.1; bump it at the cut, in
  its own pull request (section 7, item 8).
- **Dependabot:** done 2026-10-30. #398 (similar, ureq), #378 (Gradle wrapper), #376
  (rust-cache), #377 (vscode-languageclient 11; no code change, the VS Code floor stays
  `^1.91.0`), #373 (jsonschema 0.60, a pre-1.0 minor that can change `json_schema_passes`
  messages), and #375 (release actions that pull-request CI does not exercise, so v0.21's release
  is their first run) merged one at a time with `main` green between them (section 7, item 8).
  The review that followed re-pinned `Swatinem/rust-cache` to the commit its `v2.9.3` comment
  names (#376 had pinned an untagged commit), took jsonschema 0.60.2, and added a test that schema
  compilation never retrieves a remote or file `$ref`. The corpus replay against v0.20.1 covers
  the six. None touches `roxmltree`, which stays at 0.20.
- **Action pins can drift from their comments.** Nothing checks that a pinned commit is the one
  its `# vX.Y.Z` comment names; #376 showed that Dependabot can pin an untagged commit. Add a CI
  check that resolves each commented tag and compares the commit.
- **Housekeeping:** 104 local branches, 63 fully merged; remote branches `feat/key-parity`,
  `feat/key-parity-formats`, `feat/relation-ir-r2` (merged) and `bench-record/v0.18.3` (PR
  closed); extra worktrees, one of which (an older benchmark worktree) holds staged changes;
  `target/` at 47 GB. Done as section 7, item 4 records.

## 7. Owner decisions

All decided on 2026-10-30. Items 1, 5, 6, and 7 change the design and are recorded in
[ADR-0025](../../adr/0025-v021-pre-release-decisions.md).

1. **The go/no-go verdict (W2-8).** GO on consistency; v0.21 ships. Phase 3 starts only if v0.22
   makes the IR path authoritative for `cross_file` `equals` and deletes the old code, keeps
   that code within today's 190 non-comment lines (134 `equals`-only lines, as ADR-0026
   measures it from 2026-10-30) with no calls into `cross_file::eval`, and
   shows zero e2e and corpus differences with the budgets held (ADR-0025 decision 3).
2. **S4 (W2-9).** Revision 4 with newline-terminated sources, plus a same-session kbench run of
   v0.20.1 against v0.21 on the revision-4 tree (100k and 1M), recorded in `r8-readiness.md`.
3. **Mutants (W4).** Replace the nightly with an advisory `cargo mutants --in-diff` job on pull
   requests that change Rust (`ubuntu-latest`, missed mutants in the job summary), keep
   `workflow_dispatch` for targeted runs, and pin `cargo-mutants`. A full-crate run of
   `alint-rules` or `alint-core` has thousands of mutants and cannot fit any job timeout.
4. **Housekeeping (W4).** Done 2026-10-30: the 61 local branches fully merged into `main` and
   checked out nowhere were deleted by exact name, and so were the remote branches
   `feat/key-parity`, `feat/key-parity-formats`, `feat/relation-ir-r2` (squash-merged) and
   `bench-record/v0.18.3` (PR #328, closed). That benchmark worktree, the other worktrees,
   the unmerged branches, and `target/` were kept.
5. **Flat keys that contain a dot.** Shown as written in dotenv, properties, and `.resx`
   (`app.title`), with `ignore_keys` matching the name there and path patterns matching segment
   by segment elsewhere (ADR-0025 decision 1).
6. **A missing literal peer.** Fail-loud by default; `allow_missing_peers: true` turns an absent
   peer into an informational note (ADR-0025 decision 2).
7. **WIT acceptance (W2-7).** No tag until witforge 0.1.0 is published and the regeneration
   check runs in default CI (ADR-0025 decision 4).
8. **Toolchains and dependencies (W4).** MSRV: a written rolling policy (the MSRV may rise to
   any stable release at least six months old, only when a dependency or language feature needs
   it, in a minor release or a security patch, with a changelog line), staying on 1.88 for now.
   Dependabot: all six open pull requests (#398, #378, #376, #377, #373, #375) merge now, one at
   a time with `main` green between them (done 2026-10-30). `RELEASE_RUST`: bumped at the cut, in its own pull
   request, to the newest stable at least two weeks old, per `RELEASING.md`.

## 8. Order

W1 first (it changes the release's only user-facing kind), then W2, then W3 and the release
steps. W4's CI items are independent and can land at any time; the Dependabot merges follow
section 6, and the corpus replay re-runs as a release step (section 5), after the last engine or
dependency change. Each workstream lands as its own pull
request with a test for every fix, per invariant 9.
