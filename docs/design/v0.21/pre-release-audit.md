# v0.21 pre-release audit and remediation plan

Status: Plan (2026-10-29). Audited `main` at `f4e5fa225`; version 0.20.1, v0.21.0 untagged.
Decisions: none new. The work stays inside [ADR-0024](../../adr/0024-relation-ir-decisions-after-review.md)
and the accepted [`README.md`](README.md); section 7 lists the calls that are the owner's.

[`r8-readiness.md`](r8-readiness.md) declared v0.21 a GO on performance, memory, and the corpus
replay. An independent pre-tag audit found that the GO cannot stand: `key_parity`, the release's
only new user-facing surface, misses real differences in common layouts, its documented `.env`
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

- `main` is clean and every workflow is green; `cargo fmt --check` passes; alint's own lint passes
  all 50 rules; `gen-facts`, `gen-schema`, and `gen-roadmap --check` pass; `facts.json`, the
  README, and the site agree on 103 kinds plus 11 aliases.
- The performance evidence in `r8-readiness.md` matches `docs/benchmarks/REFERENCE-v0.18.2.md`
  (839/441 reads, the instruction counts, 106/110 replayed with four expected skips).
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
| W1-4 | A literal `peers` entry is read only through the index (`Selector::select`, `relation/resource.rs:117`), so a gitignored literal peer is never found. The design's own example (`reference: .env.example`, `peers: .env`) reports "`peers` glob ".env" matched no files". `docs/rules.md:889` says peers respect `.gitignore`, contradicting the design. | Reproduced | A `peers` value with no glob metacharacters is a literal resource read from disk, as the design specifies; `rules.md` is corrected. | An e2e scenario with `.env` in `.gitignore`, firing and silent. |
| W1-5 | TOML inline tables are followed one level (`adapters/toml.rs:114`, `get_values()`), so `[x.a] b = 1` and `x = { a = { b = 1 } }` report "missing `x.a.b`" and "extra `x.a`" although they mean the same thing. | Read (reproduced by the audit pass) | Recurse into nested inline-table values. | An adapter unit test and a `key_parity` TOML case. |
| W1-6 | With `strip_root` or `plurals: cldr`, findings lose their line and column and the related reference location: `KeyFile::range` (`key_parity.rs:136`) looks the transformed key up in the untransformed key map. This matters for SARIF and the LSP. | Read (reproduced by the audit pass) | Look spans up in the transformed map, which already carries the right span. | A Rails YAML test asserting ranges on both findings. |
| W1-7 | `merge_plural_groups`' `all_categories` check (`key_parity.rs:799`) is always true, since only category-named members are collected, so an object holding `other` plus non-category keys is partly merged. A merged i18next base can also collide with an existing key of the same name. | Read | Merge only when every child of the parent is a CLDR category; leave a colliding base unmerged. | Unit tests for both cases. |
| W1-8 | TOML parse failures echo the source line, value included, although findings must quote keys only. | Read | Report the TOML error's position and reason without the source snippet. | A unit test asserting a value never reaches the message. |
| W1-9 | A reference path containing glob metacharacters would not match its own `--changed` edit, since the path scope compiles it as a glob. | Read | Escape the reference when building the scope. | A unit test. |
| W1-10 | `.resx` is not inferred from its extension, although the design says the extension picks the format except for `.xml`; `rules.md` says it must be explicit. | Read | Infer `.resx`; `.xml` stays explicit. | Extend `format_inference_and_models`. |
| W1-11 | Coverage the release acceptance lists is missing end to end: properties, dotenv, INI, nested YAML with `plurals: cldr`, i18next, duplicate keys (silent), a key-model mismatch, and an unparsable reference. `changed_key_parity_unrelated_skips.yml` cannot fail: its files already match, so a full run passes too. The pilot comparison test (`key_parity.rs:1435`) uses the same `.join(".")` as the bug and cannot catch it. | Read | Add the scenarios; give the skip scenario a tree that fails a full run; make the pilot oracle compare segment lists. | The scenarios themselves. |
| W1-12 | Invariant 4 (determinism) has no test: nothing runs `key_parity` with a shuffled index order or several thread counts. The output is deterministic as built (BTree types, sorted peers), but nothing proves it. | Read | A test that evaluates over shuffled index orders and thread counts and requires identical findings and fingerprints. | The test. |

Two documentation corrections travel with W1: a key-model mismatch is reported as an analysis
failure on that peer (the docs call it a configuration error), and `rules.md:893` says inserting
a missing key "is a Suggestion", which reads as if `alint fix` offers one; the rule has no fix.
The stale module doc in `key_parity.rs` ("TOML and the Android/.resx dialects arrive with R9")
is updated too.

## 4. W2: the other gaps behind `r8-readiness.md`'s "pass" rows

The owner's direction is to fix each gap where possible rather than downgrade the claim. A gap
that cannot be closed in v0.21 is listed with its reason and the readiness row says so.

| ID | Gap | Evidence | Fix |
|---|---|---|---|
| W2-1 | Registry injection is incomplete. `alint test` builds `Runner::new` without `with_registry(cli.registry())` (`crates/alint/src/test_cmd.rs:115`); `rules show` validates the kind against the static catalog, then `expect`s the injected registry to have it (`crates/alint/src/rules.rs:413`), a panic for a registry without a built-in kind; `rules list`, LSP hover, `init` (`init.rs:411`), and `suggest` (`suggest/output.rs:389`) read static tables or `builtin_registry()`. No test proves a custom kind reaches a rule through the CLI, the LSP, or the testkit. | Read | Thread the registry through each path, return an error instead of panicking, and add one test per surface that registers a custom kind and observes it run. |
| W2-2 | `ci/scripts/check-shadow-feature-tree.sh` passes while `shadow-ir` leaks: with `default = ["leak"]` and `leak = ["alint-rules/shadow-ir"]` in `crates/alint/Cargo.toml` it printed "absent" for every build and exited 0, while `cargo tree -i alint-rules -f '{p} [{f}]'` showed `[shadow-ir]`. It also omits the `fs-benches` feature and per-target resolution the bench and release builds use. | Reproduced (scratch worktree) | Inspect `alint-rules`' resolved features with `cargo tree -i alint-rules -e normal,build -f '{p} [{f}]'` for every build's exact package, feature, and target set, and add a negative control that injects a leak and requires the script to fail. |
| W2-3 | Most relation limits are defined but never enforced: `RowsPerRun`, `JoinInputs`, and `JoinOutputs` have no use outside `limits.rs`, `RunBudget` is exported and unused, and `DistinctValues` appears only in a doc comment. The `DecodeCache` key ignores limits, so the first consumer's limits win. | Read | Enforce `RowsPerRun` through `RunBudget` in the decode path and `DistinctValues` at set conversion; key the decode cache on the effective limits; mark the join limits as reserved for Phase 3, where joins arrive, with a test that keeps them unused until then. |
| W2-4 | The `plugin_api` import-boundary test scans text: it checks only lines starting with `use`/`pub use` plus `crate::` substrings, so `super::super::X` in type position, an external crate path such as `serde::Serialize` in a derive, or a `use` split across lines passes. The module is clean today. | Read | Parse the module and visit every path, allowing only `std`, `core`, `alloc`, the module's own items, and bounded `super::`. |
| W2-5 | Invariant 6: `FileIndex::read_cache`, `decode_cache`, and `enable_relation_caches` (`walker.rs:315-329`) are documented public methods that return or touch doc-hidden relation types. The test checks only the `mod` declarations. | Read | `#[doc(hidden)]` on the three methods, asserted by the existing visibility test. |
| W2-6 | The contract bridge (`relation/contract.rs`) does not check a finding's or related location's path, or a row's resource, against the input resources, and accepts any index id. Latent until a plugin host exists. | Read | Validate them in the bridge and reject what does not belong to the input. |
| W2-7 | Nothing in CI checks the WIT draft; R7's evidence is a manual `witforge check`, and nothing compares `plugin_api` with `plugin.wit`. | Read | witforge is private and preparing 0.1.0. Once it is installable in CI, add the check; until then the readiness row records a manual check at a stated commit. |
| W2-8 | The go/no-go assessment is missing. The release acceptance and the stop rule (`general-linter.md` section 11.1) require a written judgment of whether the ported path became simpler and more consistent, with evidence; `r8-readiness.md` covers performance and the replay only. | Read | Write it in `r8-readiness.md` from the port's diff, the shadow's shared code paths, and W1's lessons; the verdict is the owner's (section 7). |
| W2-9 | S4's workload changed without a revision bump. #399 changed `lorem_bytes`, which S4's polyglot tree also uses (`crates/alint-bench/src/tree.rs:375/407/439`), but S4 stayed at revision 3 (`xtask/src/bench/mod.rs:210`). At the tag, `bench-gate` resets S1-S3 and SFIX, so S4 is the only scenario still compared with v0.20.1, across a changed workload. S4's generated sources also still lack the final newline #397 added for S2. | Read | Bump S4 to revision 4 and newline-terminate its generated sources, and say so in the changelog. |
| W2-10 | `alint validate-config` replaces every error with `<redacted>` for a rule that has any env-interpolated field, even an error that quotes no value. It blocks one corpus config (`facebook/react`) and is tracked nowhere. Pre-existing, found during R5. | Reproduced | Redact only errors that can quote an interpolated value; keep structural errors readable. |

Accepted as is: `cargo test --workspace` unifies `alint-e2e`'s dev-dependency feature, so the CLI's
own test binaries run with the shadow enabled. That adds checking and never reaches a shipped
build (W2-2 guards that).

## 5. W3: documentation and release preparation

- **CHANGELOG `[Unreleased]`:** one `### Fixed` (there are two), the release's section order
  (Added, Changed, Performance, Fixed, Security), the `import_gate` prefilter under Performance,
  the plugin-contract entry reworded to say what shipped (doc-hidden modules with no semver
  promise, no plugin host; the public additions are `Runner::with_registry` and the test-only
  `shadow-ir` feature), the benchmark entry corrected (every size, and #399's whitespace change),
  W1 and W2's user-visible fixes, and the summary paragraph at the cut.
- **Stale ADR-0023 citations:** the ROADMAP's v0.21 section (public on alint.org),
  `REFERENCE-v0.18.2.md:4`, and `general-linter.md`'s Phase 2 text cite ADR-0023 alone; they
  gain ADR-0024, which supersedes it.
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
- **`docs/adr/`:** no index; add a README listing each ADR and its status.
- **This directory:** the README's "Status and next steps" reads out of order (R9 first, R1
  last); `r8-readiness.md` moves from GO to HOLD now and returns to a verdict after W1 and W2.
- **Release steps** (after W1-W3): `bump-version.sh 0.21.0`, preflight and the MSRV check, a
  fresh corpus replay if any dependency or engine change lands first, tag, the bench-record PR
  and the alint.org bench snapshot, the Action doc pin, and the alint.org pins and `STATE.md`
  (kinds 102 to 103, headline 113 to 114).

## 6. W4: CI, dependencies, and housekeeping

- **Mutants nightly never runs.** `mutants.yml:34` targets the self-hosted `alint` runner, whose
  listener is held (`ci.yml` routes every job to `ubuntu-latest`). Every scheduled run since at
  least 2026-09-30 was cancelled after about a day in the queue. Move it to `ubuntu-latest` (its
  240-minute timeout fits) or disable the cron.
- **No scheduled advisory check.** Deny and Audit run only when Rust or supply-chain files change,
  so a new RustSec advisory surfaces on the next Rust pull request. Add a weekly scheduled run.
- **Unpinned tools:** `ci/scripts/audit.sh` installs `cargo-audit` and `mutants.yml` installs
  `cargo-mutants` without a version (`cargo-deny` is pinned).
- **MSRV headroom is zero:** six locked dependencies already require exactly 1.88, so the next
  dependency to raise it fails the MSRV job. Decide the bump policy before it happens.
- **Release toolchain:** `RELEASE_RUST` is 1.98.1 while stable is 1.99.1; a bump is optional, in
  its own pull request.
- **Dependabot:** #398 (similar, ureq), #378 (Gradle wrapper), #376 (rust-cache), and #377
  (vscode-languageclient 11; no code change, the VS Code floor stays `^1.91.0`) are green and
  can merge before the cut, one at a time. #375 updates release-only actions that CI never
  exercises, and #373 (jsonschema 0.60) is a breaking pre-1.0 bump; both wait until after
  v0.21.0. None touches `roxmltree`, which stays at 0.20.
- **Housekeeping (owner approval before any deletion):** 104 local branches, 63 fully merged;
  remote branches `feat/key-parity`, `feat/key-parity-formats`, `feat/relation-ir-r2` (merged)
  and `bench-record/v0.18.3` (PR closed); five extra worktrees, one of which
  (`~/dn0-19-alint.XOcCz0/source`) holds staged changes; `target/` at 47 GB.

## 7. Owner decisions

1. **The go/no-go verdict (W2-8).** The evidence is drafted for you; whether the port is simpler
   and more consistent, and so whether Phase 3 starts, is the stop rule's call.
2. **S4 (W2-9).** Recommended: revision 4 with terminated sources. The alternative, documenting
   the change and keeping revision 3, leaves the only cross-release scenario comparing two
   workloads.
3. **Mutants (W4).** Recommended: run it on `ubuntu-latest`. The alternative is disabling the
   cron until the self-hosted listener returns.
4. **Housekeeping (W4).** Which branches and worktrees to delete.

## 8. Order

W1 first (it changes the release's only user-facing kind), then W2, then W3 and the release
steps. W4's CI items are independent and can land at any time; the Dependabot merges and the
corpus replay re-run follow the order in section 6. Each workstream lands as its own pull
request with a test for every fix, per invariant 9.
