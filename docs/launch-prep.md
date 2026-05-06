# alint Production Launch — End-to-End Plan

Living doc. Captures the path from the current state (v0.9.14 — feature-complete
v0.9 series, fully-automated bench-record CI, no public outreach yet) to a
public launch backed by real-repo case studies and a marketing site that earns
attention rather than just hosts documentation.

**Status: 2026-05-06.** P1 done; **P2a-full COMPLETE (20 of 20)**;
**P2b Wave 1 COMPLETE (5 of 20 polyglot repos — curated pre-launch
subset: NixOS/nixpkgs, bazel, TensorFlow, apache/spark, vscode)**;
**v0.9.16 release commit landed locally** (was v0.9.15 plan; rolled
v0.9.15 into v0.9.16 with the deny_unknown_fields uniformity audit
pre-tag).

`docs/development/CONFIG-AUTHORING.md` now catalogues **21 pitfalls**
(17 from P2a + 2 from P2b Wave 1: #18 `.gitignore` masks tracked-file
presence checks; #19 `root_only: true` + multi-component literals
silently no-match). Every one is caught somewhere in the toolchain
(schema at edit time, parse error at load time, runtime audit at PR
time, or documented + smoke-fixture-pinned). 7 silently-broken
structured-path rules in committed pilot+Wave 1 configs were fixed
across the validation pass.

P2b Wave 1 saturation findings: every reasonable-information P2b case
study mostly RECONFIRMED existing v0.10+ candidates with deeper
demand-signal data. `cross_file_value_equals` now at 9 sources;
`registry_paths_resolve` at 8+; `cross_language_implementation_complete`
gets its 2nd source (TF — joins arrow as v0.11+ flagship). Three
ship-target promotions: `apache/governance@v1` bundled ruleset moves
"v0.10+ idea → v0.10 ship-target" (3 Apache TLPs converge); new
`xml_path_*` family proposed (completes the structured-query family
JSON/YAML/TOML + XML); `respect_gitignore: false` per-rule knob
proposed to fix pitfall #18.

Next: tag v0.9.16 (manual gate; triggers bench-record + crates.io +
Homebrew + Docker + docs-bundle publish chain) → P3 marketing publish
coordinated with the docs roll.

## State of the world (audit at 2026-05-05)

| Surface | Current | Gap |
|---|---|---|
| **README.md** | Comprehensive, but reads like a spec, not marketing. References v0.9.6 (we're at v0.9.14). Agentic angle buried in long lists. | Hero rewrite around speed/agentic/extensible; refresh version refs; add 60-second quickstart near the top. |
| **alint.org** | Has structure (60 rule kinds, 19 rulesets, etc.) + OG image + Twitter cards. Same not-punchy framing as the README. | Hero rewrite, comparison page, examples gallery, public bench page, migration guides, SEO infra, AI/LLM discovery files. |
| **GitHub repo About** | `description: null`, `homepage: null`, `topics: []` | Set all three; topics target the discovery-search vocabulary. |
| **Discussions** | Disabled | Enable; this is the intended low-friction support channel for a launch. |
| **Issue / PR templates** | None | Add bug-report, feature-request, config-help templates + PR template. |
| **`CONTRIBUTING` / `CODE_OF_CONDUCT` / `SECURITY`** | All missing | Tablestakes for a public OSS launch. `SECURITY.md` is especially load-bearing for a build-tool — vulnerability disclosure path. |
| **`examples/` directory** | None | Becomes the home of P2 case studies — real configs from real repos. |
| **Comparison page** | None on README/alint.org | alint vs Repolinter, ls-lint, Megalinter, EditorConfig, custom-shell — direct table. |

---

## Phased plan

```
P1   Repo hygiene             ──┐ (DONE)
P1.5 v0.9.15 config DX        ──┼──► P3 Marketing refresh ──► P4 Launch ──► P5 Post-launch
     hardening (6 phases)       │              ▲                              │
P2a  Validation pass          ──┤              │                              │
     (20 repos, diverse)        │ (P2a findings inform P1.5 phase 3+5;        │
P2b  Validation pass          ──┘  P2 case studies become P3 evidence)        │
     (20 polyglot monorepos,                                                  │
     ongoing post-launch)        ───────────────────────────────────────────► │

Sequencing nuance: P2a-full (15 remaining repos) runs BEFORE
v0.9.15 Phase 3-6 (did-you-mean errors + JSON Schema +
validate-config subcommand) so the DX fixes target the full
pitfall catalogue rather than just the pilot's 12.
```

### P1 — Repo hygiene & community foundation (~1.5 days, DONE 2026-05-05)

Foundational; happens first because launch traffic is unpredictable and these need
to be live before the first link is shared.

- ✅ `CONTRIBUTING.md`
- ✅ `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1)
- ✅ `SECURITY.md` (PGP/contact + 90-day disclosure window)
- ✅ `.github/ISSUE_TEMPLATE/{bug-report,feature-request,config-help}.yml`
- ✅ `.github/pull_request_template.md`
- ✅ GitHub repo About: description, homepage, 10 topics
- ✅ Discussions enabled
- ✅ README hero rewrite + version refs refreshed to v0.9.14
- ✅ `examples/` directory scaffold

### P1.5 — v0.9.15 config DX hardening (~3-4 days, IN PROGRESS)

Surfaced by the P2a pilot — 12 schema/language pitfalls hit while writing
configs for the first 5 repos. Two layers of prevention:

1. **Editor / write-time** — JSON Schema for `.alint.yml` (Phase 5); ~80 % of
   pitfalls caught before save.
2. **Parser / load-time** — clearer errors with did-you-mean suggestions
   (Phases 3-4) for the residual 20 %.

Six sub-phases:

- **Phase 1** — `docs/development/CONFIG-AUTHORING.md` findings doc. ✅ DONE.
- **Phase 2** — `coverage_audit_examples_parse.rs` audit (every
  `examples/*/.alint.yml` MUST load + build cleanly). ✅ DONE.
  *(Already caught one bug on its first run — duplicate `level:` in airflow.)*
- **Phase 3** — Did-you-mean parse errors. ✅ DONE. Implementation
  hooks at the central `RuleRegistry::build` boundary (no per-rule
  edits): a curated rename map for the highest-drift schema renames
  (`argv→command`, `secondary→partner`, `style→target`,
  `pattern→prefix|suffix`, `matches↔equals` for the structured-path
  family) plus a Levenshtein fallback (distance ≤ 2) for the long
  tail. 18 unit tests + 9 integration tests through the real build
  path. Also added `#[serde(deny_unknown_fields)]` to the structured-
  path Options structs so the `matches:` ↔ `equals:` rename surfaces
  as an unknown-field error rather than missing-required.
- **Phase 4** — Domain-specific error messages. ✅ DONE. Covers:
  pitfall #10 (JSONPath dashed-key bracket-notation, also inside
  filter contexts), #11 (`scope_filter.has_ancestor` path-separator
  → `paths:` glob hint), #12a (`&&`/`||`/`!` symbols → `and`/`or`/`not`
  keyword hint), #12b (`iter.*` method-call shape → `matches`
  operator hint), #15 (`file_starts_with.prefix: ""` →
  `file_min_lines: 1` hint). New `jsonpath_diagnostics` module in
  alint-core (9 unit tests); `when:` parser enriched in-place
  (post-error diagnosis). 10 integration tests through the real
  build path. **Investigation drive-by**: a previously-claimed
  pitfall #18 (JSONPath outer-parens filter) was disproven —
  `serde_json_path` 0.7.x accepts outer parens; the original report
  had mis-attributed a dashed-key error to the parens. Pitfall
  catalogue dropped 18 → 17. apache-arrow case study + master
  CONFIG-AUTHORING.md updated.
- **Phase 5** — JSON Schema editor-LSP wiring. ✅ DONE. The schema at
  `schemas/v1/config.json` was already largely complete (60-rule
  dispatch, `unevaluatedProperties: false` on the rule node — see
  rule_kind_dispatch composition with rule_common). Phase 5 audited
  vs. the pitfall catalogue and wired the workflow:
  - **Spot-check audit** — `coverage_audit_schema_drift.rs` validates 5
    canonical-correct configs + 5 pitfall configs (#1, #4, #9, #15, #16)
    against the live schema using the `jsonschema` crate. Drift in
    pitfall coverage surfaces as CI failure.
  - **Drift audit** — same file checks every registered rule kind has a
    `rule_kind_dispatch` $ref + every dispatch entry maps back to a
    registered kind. Catches "added rule, forgot schema" at PR time.
  - **Magic-comment rollout** — every example config under
    `examples/*/.alint.yml` now ships with the
    `# yaml-language-server: $schema=…` modeline. New audit
    (`every_example_carries_the_yaml_language_server_directive`) keeps
    that invariant.
  - **CONFIG-AUTHORING.md** — new "Editor LSP via the JSON Schema"
    section with adopter copy-paste line + table of pitfalls the
    schema catches at edit time + explicit note on what it doesn't
    catch (the runtime-semantic pitfalls #13/#14/#17 → Phase 7).
  Net engineering scope was much smaller than the original 2-3 day
  estimate (schema was already there); the value-delivery work
  (audits + magic comment + docs) shipped in well under a day.
- **Phase 6** — `alint validate-config <path>` subcommand. ✅ DONE.
  Parse-only command (no tree walk) that runs the full load + build +
  when-parse path and reports pass/fail. Accepts a file path or a
  directory (discovers `.alint.yml`). Two output formats:
  - `--format human` (default) — human one-liner on stdout, error
    chain on stderr. Matches the existing CLI's stdout/stderr split.
  - `--format json` — stable envelope `{valid, rule_count, config_path,
    error}` for editor LSP / pre-commit / CI consumption.
  Exit codes: 0 valid, 1 invalid, 2 invocation error. Did-you-mean
  hints from Phases 3-4 flow through transparently. Two trycmd
  snapshot tests cover the success + did-you-mean-flowing-through
  cases.
- **Phase 7** — Smoke-test fixture audit. ✅ DONE. New audit at
  `crates/alint-e2e/tests/coverage_audit_smoke_fixtures.rs` walks
  `crates/alint-e2e/fixtures/smoke/<scenario>/` directories, builds
  the engine for each, runs against the embedded `tree/`, and
  asserts the actual `rule_id → violation_count` matches the
  scenario's `expected.toml`. Sanity-verified: deliberately dropping
  `(?m)` from a fixture rule made the audit fail; restoring it
  returned to green. Initial coverage: 4 fixtures targeting the
  runtime-semantic pitfalls — `content_matches_multiline_anchor`
  (#13), `path_equals_native_bool` (#16), `path_matches_string_field`
  (sanity baseline), `array_set_membership_workaround` (#17). The
  `expected.toml` shape is documented for future expansion;
  `crates/alint-e2e/fixtures/smoke/README.md` walks an author through
  adding a new fixture.

**Sequencing decision:** Phases 3-7 land AFTER P2a-full. Reasons:
- The new examples-parse audit dropped iteration cost per case study;
  doing the 15 remaining was cheap.
- More repos surface more pitfalls — Phases 3-4 hand-curated suggestions
  benefit from the full 18-pitfall catalogue.
- Phase 5 JSON Schema work targets the right fields when the most-misused
  ones are known.
- Phase 7 fixture design benefits from knowing the 4 runtime-semantic
  pitfall shapes the fixtures need to assert against.

### P2a — First 20 repos, single-language + diverse-ecosystem (~10-15 days)

Diverse ecosystems + scales + tooling shapes. Becomes the case-study foundation
for P3 and the gap-catalogue for v0.10+.

**Pilot status (5 of 20 done, committed):** kubernetes, rust-lang/rust, deno,
airflow, turbo. Each has a per-repo case study + working `.alint.yml` at
`examples/<owner>-<repo>/`. The pilot iteration surfaced the 12 pitfalls now
documented in `docs/development/CONFIG-AUTHORING.md`.

**Wave 1 status (DONE):** clap, tokio, ruff, uv, typescript. Surfaced 3
pitfalls (§13-15) + 6 rule-kind candidates.

**Wave 2 status (DONE):** next.js, pnpm, react, prettier, cpython.
Surfaced 1 pitfall (§16 — `*_path_matches` against bool fields) + 6
more rule-kind candidates + **caught 6 silently-broken bool-match
rules in committed configs** (fixed).

**Wave 3 status (DONE):** golang/go, helm, arrow, pytorch, nodejs/node.
Surfaced 2 pitfalls (§17 `*_path_equals` against `[*]`; §18 JSONPath
outer-parens) + 9 more rule-kind candidates + **caught 1 silently-broken
array-semantics rule in deno** (fixed).

All 20 configs parse cleanly under `coverage_audit_examples_parse.rs`.

**Five distinct positioning narratives** crystallised across the full
P2a pass — one per "shape" of OSS project structural-validation
reality:

| # | Narrative | Strongest data points | Use case |
|---|---|---|---|
| 1 | "Replaces N hand-rolled validation scripts" | kubernetes (50→17), airflow (109 hooks→40 %), cpython (12 surfaces consolidated) | Repos with verify-script sprawl |
| 2 | "Catches conventions your pipeline assumes but doesn't verify" | tokio (15 conventions, 0 scripts), uv (67-crate workspace), pnpm (`meta-updater` plugin replaced), react (codes.json + version-sync), nodejs/node (15-year-old conventions enforced via human review) | Repos that rely on convention without explicit checks |
| 3 | "Adds structural floor on top of mature tooling" | typescript (eslint+dprint+knip), ruff (900+ Python rules, 0 internal-crate), prettier (5 net-new gates), helm (Trojan-Source defence + GHA hardening on top of golangci-lint) | Repos with mature tooling but missing structural layer |
| 4 | "Replaces the structural subset of your custom orchestration layer" | pytorch (49 of 57 lintrunner adapters ≈ 86 % structural) | Repos that built their own lint-orchestration tool |
| 5 | "Encodes conventions enforced only by code-review discipline" | golang/go (zero workflows / Makefile / golangci.yml; 31-rule alint config encodes the contract for the first time) | Tightly-curated minimal-tooling segment |

**Polyglot wins (Wave 2 + Wave 3):**
- **next.js** — first hybrid pnpm + Cargo dual-workspace win. *"Drift no per-language linter catches because each linter only sees half the tree."*
- **apache/arrow** — the polyglot flagship: 6 languages in one tree, 21 lint hooks across 14 tool repos, 0 tools that see cross-language conventions. *"alint is the layer that does."* Live findings against the real arrow clone: 16 source files missing the Apache header (all listed in `dev/release/rat_exclude_files.txt`).

These two case studies anticipate P2b's full polyglot mandate and may
end up driving the launch-marketing message ahead of any single-language
case study.

| # | Repo | Ecosystem | Why |
|---|---|---|---|
| 1 | `rust-lang/rust` | Rust mega-monorepo | Has `src/tools/tidy` — a custom Rust binary doing exactly alint's job |
| 2 | `tokio-rs/tokio` | Rust workspace | Clean, well-curated, baseline case |
| 3 | `astral-sh/uv` | Rust + pyo3 | Modern multi-language Rust monorepo |
| 4 | `astral-sh/ruff` | Rust linter for Python | Direct comparable as a tool; we can dogfood-cross |
| 5 | `clap-rs/clap` | Rust workspace | Small, focused — quick win + baseline |
| 6 | `denoland/deno` | Rust + JS + TS | Multi-language, custom validation scripts |
| 7 | `microsoft/typescript` | TS mega-repo | Hand-rolled validation, lint-baseline files |
| 8 | `vercel/next.js` | TS monorepo | Highly conventional, pnpm-workspace |
| 9 | `pnpm/pnpm` | TS monorepo | pnpm itself; defines the workspace shape |
| 10 | `facebook/react` | JS/TS multi-package | Yarn-workspace, conventions per package |
| 11 | `prettier/prettier` | JS, well-curated | Mature, opinionated structure |
| 12 | `python/cpython` | Python + C | Make-driven, custom check scripts in `Tools/` |
| 13 | `apache/airflow` | Python plugin-heavy | Provider-package conventions, lots of structural rules |
| 14 | `kubernetes/kubernetes` | Go mega-monorepo | `hack/verify-*.sh` is *literally* this tool's use case |
| 15 | `golang/go` | Go canonical | Tightly-curated, minimal external tooling — ground-truth case |
| 16 | `helm/helm` | Go modular | Smaller Go monorepo, modular structure |
| 17 | `apache/arrow` | Multi-language (C++/Java/Python/Rust/Go) | Per-language subdir conventions; cross-language structural rules |
| 18 | `pytorch/pytorch` | C++/Python/CUDA | Massive multi-language; complex conventions |
| 19 | `vercel/turbo` | Rust monorepo orchestrator | Modern Rust + custom validation |
| 20 | `nodejs/node` | C++/JS, mature | Long-curated, deeply-conventional |

### P2b — 20 polyglot monorepos (~10-15 days, can run concurrent with P3 or post-launch)

Multi-language monorepos stress alint differently than single-language ones —
per-subtree conventions, polyglot bundle composition, scope-filter on
heterogeneous trees. These are the canonical use cases that motivate
`extends + nested_configs + scope_filter`, so they're the strongest stories
for the launch.

| # | Repo | Languages | Why |
|---|---|---|---|
| 21 | `bazelbuild/bazel` | Java + Go + C++ + Python | Canonical multi-language build system |
| 22 | `microsoft/vscode` | TS + C++ (native) + Python | One of the most-watched OSS repos; massive multi-language tree |
| 23 | `angular/angular` | TS monorepo + Bazel build | Tightly conventional, per-package rules, `ng-packagr` ceremony |
| 24 | `nrwl/nx` | TS monorepo | Nx ITSELF is a monorepo tool — interesting cross-comparison |
| 25 | `electron/electron` | C++ + JS + Python | Native + web hybrid; per-platform conventions |
| 26 | `tensorflow/tensorflow` | C++ + Python + Java + JS (TFJS) | ML mega-monorepo; ~80k files; perf stress test |
| 27 | `apache/spark` | Scala + Java + Python + R | Polyglot data engine; per-language module conventions |
| 28 | `apache/beam` | Java + Python + Go (+ TypeScript SDK) | *Explicitly* polyglot; cross-language SDK conventions |
| 29 | `prisma/prisma` | Rust (query engine) + TS (client) | Modern hybrid; rust-engine + ts-client subdirs |
| 30 | `temporalio/temporal` | Go core + per-SDK languages | Workflow orchestration; many sub-projects |
| 31 | `istio/istio` | Go + many control plane components | Service mesh; multiple Go modules in one tree |
| 32 | `grafana/grafana` | Go backend + TS frontend | Widely-deployed; clean backend/frontend split |
| 33 | `cockroachdb/cockroach` | Go (DB) + TS (UI) + C++ (libs) | Distributed DB; multi-tier monorepo |
| 34 | `directus/directus` | TS monorepo (API + admin app + extensions) | Headless CMS; pnpm-workspace conventions |
| 35 | `supabase/supabase` | TS + Go + Rust + Python | Modern Firebase alt; many engines in one repo |
| 36 | `NixOS/nixpkgs` | Nix + Python build scripts | **~150k+ files** — the largest non-trivial repo on this list; the scale stress-test candidate |
| 37 | `hashicorp/terraform` | Go + HCL + JS UI | Infrastructure-as-code; per-provider conventions |
| 38 | `flutter/flutter` | Dart + Java + Kotlin + Swift + C++ | Cross-platform UI framework; per-platform native dirs |
| 39 | `dotnet/runtime` | C# + C++ + native (per-arch) | Microsoft's CLR runtime; multi-arch + multi-language |
| 40 | `protocolbuffers/protobuf` | C++/Java/Python/Ruby/Go/JS/Obj-C/C#/PHP | Generated bindings for ~10 languages; per-language subdir conventions |

**Why P2b matters for launch positioning:** the "language-agnostic linter for
repository structure" pitch lands harder when paired with case studies showing
alint configs that work cleanly across `protobuf`'s 10 languages or `bazel`'s 4.
Single-language wins (P2a) prove correctness; polyglot wins (P2b) prove the
unique value prop.

### Per-repo workflow (2-4 hr per repo)

1. Shallow clone (depth=1)
2. **Inventory existing structural-check tooling** — grep for `hack/verify-*`,
   `scripts/lint-*`, `Makefile` lint targets, `.eslintrc` rules that aren't AST
   checks, `.editorconfig`, `.gitattributes`, custom shell pipelines in CI yml
3. **Categorise each check** — what shape of rule (filename / content /
   cross-file / structure)
4. **Build matching alint config** — start from the bundled rulesets that fit
   (`rust@v1`, `node@v1`, etc.), add per-repo custom rules
5. **Parse-validate the config** — `./target/release/alint check --config
   examples/<owner>-<repo>/.alint.yml examples/<owner>-<repo>/` MUST exit
   without a `building rule "..."` / `loading config` / `invalid options`
   error. Tool-not-on-PATH errors from `command:` rules ARE expected and
   indicate the rule structure is correct. **The kubernetes pilot iteration
   surfaced 8 schema-level bugs that wouldn't have shown up without this
   step.** Subagents writing configs against memory of the schema (vs.
   reading `crates/alint-rules/src/<kind>.rs::struct Options`) are the
   highest-failure-rate work — bake this validation in.
6. **Run + compare** — alint output vs the existing tool's output. Note: false
   positives, false negatives, perf delta
7. **Gap catalogue** — for each existing check alint can't express, write a
   one-line "needs rule kind X" note feeding the v0.10+ design
8. **Per-repo case study** — one markdown page in
   `examples/<owner>-<repo>/README.md` with the inventory + the alint config +
   the comparison

For P2b polyglot repos: one extra step — explicitly catalogue which conventions
are *cross-language* (e.g., "every language subdir has a README" →
`for_each_dir` rule) vs *per-language* (e.g., "Python files need a license
header" → `scope_filter: { has_ancestor: setup.py }`).

**Approach to scale:** Start with 5 representative repos to validate the
methodology, iterate the per-repo template based on what we learn, then dispatch
the remaining 15 in batches of 3-5 (possibly with subagents for the inventory
phase).

### Rule-kind candidates surfaced by P2a (FINAL — 20 of 20 done)

Aggregated from the per-repo `examples/<owner>-<repo>/README.md` gap
catalogues. Primary input to v0.10+ rule-kind design. Demand counted as
the number of distinct repos that surface the same need (saturation
signal). Sorted by demand strength.

**v0.10 — design first** (≥4 sources, broad applicability, clear shape):

| Candidate | Demand | Notes |
|---|---|---|
| `registry_paths_resolve` (every path/key in a registry file resolves to an on-disk artefact) | rust + clap + cpython (×2) + next.js + arrow + pytorch + nodejs/node (**7 sources, 8 confirmations**) | **Highest-leverage gap in P2a** (cpython subagent's words). tidy::triagebot, .github/settings.yml, CODEOWNERS, cpython check-c-api-docs symbol↔docs + .gitattributes generated markers, arrow `dev/release/rat_exclude_files.txt`, pytorch HEADER_ONLY_LINTER, node `tools/dep_updaters/update-<libname>.{sh,mjs}` ↔ `deps/<libname>/`. |
| `cross_file_value_equals` (incl. `cross_file_field_equals` variant + pnpm's "key-set membership" sub-shape) | airflow + tokio + clap + uv + react + pnpm + nodejs/node + pytorch (**8 sources**) | **Past-saturation demand.** split-workspace lockfile sync, root README ↔ per-crate README, version-in-CHANGELOG, pnpm `meta-updater`'s 13 cross-package invariants, react's `ReactVersion.js` propagated to 3 per-package fields, nodejs `tools/eslint-rules/*` ↔ `eslint.config.mjs`, pytorch WORKFLOWSYNC. |
| `ordered_block` (lines between marker pairs sorted unique under configurable comparator) | rust + airflow + tokio + cpython + arrow + golang/go (**6 sources**) | tidy::alphabetical + spellcheck.dic + airflow allowed-imports + cpython `Modules/Setup` + arrow + golang/go `api/go1*.txt` golden files. |

**v0.10 — design if cheap** (2-3 sources, broad-enough applicability):

| Candidate | Demand | Notes |
|---|---|---|
| `for_each_leaf_dir` / `iter.is_leaf` accessor | prettier + rust + ruff (3) | Leaf-walk variant of `for_each_dir`. Extends existing rather than new kind. |
| `balanced_delimiters` + `file_pair_block_match` | rust + cpython (×2) (3 confirmations) | tidy::rustdoc_css_themes + cpython Argument Clinic block markers. |
| `import_gate` (forbid imports of pattern X in path scope Y) | k8s + airflow + golang/go + pytorch (4) | Recurring shape; pytorch IMPORT_LINTER reconfirms. |
| `generated_file_fresh` (run a generator and diff the output against the on-disk file) | uv + cpython + pytorch (3-4) | uv `cargo dev generate-*`, cpython `cases_generator`, pytorch NATIVEFUNCTIONS + GENERATED_SHIMS_VERSION. Tension: alint's deliberate non-goal is running codegen — propose as opt-in primitive. |
| `pair_hash` (computed property of file A appears at offset Y in file A) | k8s + tokio + golang/go (3) | spellcheck.dic header line + k8s manifest checksum + **golang/go FIPS** (highest-stakes use case to date — CMVP submission references the file format). |
| `json_schema_passes` config-shape mode (validate a config file against an inline JSON Schema) | k8s + turbo (2) | Replaces hand-rolled `argv:`-shape checks. |
| `command_idempotent` mode (run tool in --check mode, fail if working-tree would change) | ruff + prettier (2) | mdformat, markdownlint, prettier, ruff-format, dprint-check all share this shape. |
| `json_key_value_forbidden` (JSON-aware variant of `file_content_forbidden`) | prettier + turbo + uv (3) | "no foo:bar in any package.json under packages/" pattern. |
| **`*_path_contains` (set-membership shorthand for "value X is present in array at JSONPath Y")** | helm + deno (existing fix) (2) | **Resolves pitfall #17.** Direct response to the `*_path_equals + [*]` semantics gap. |

**v0.10 single-source** (worth designing if cheap, lower demand):

| Candidate | Source | Notes |
|---|---|---|
| `pair_inverse` (every partner traces back to a primary; reverse of `pair`) | ruff | Snapshot freshness; covers `cargo insta --unreferenced=reject`. |
| `pair_count` (assert N≥1 partner files match a registry entry) | typescript | Diagnostic-ID-appears-in-baseline; airflow `check-no-new-airflow-exceptions` family. |
| `regex_resolves_in_file` (pre-release-replacement pattern in file A resolves to a real string in file B) | clap | cargo-release `pre-release-replacements` integrity. |
| `archive_contents_matches` (open `*.{whl,tar.gz,zip}`, compare member set against expected) | uv | check_uv_wheel_contents.py; applies to every Python package on PyPI. |
| `dir_name_matches_field` (directory basename matches a field inside a manifest in that directory) | turbo + next.js (2) | per-package `name` field in package.json must equal directory name. |
| `violation_baseline` (suppress N existing violations; fail if N grows) | deno | TS-style `lint:` baseline files. |
| `file_non_empty` (convenience for "file exists + has ≥1 byte"; resolves pitfall #15) | uv (implicit) | Cleaner than `file_min_lines: 1`. |
| `unique_by` cross-dir mode | prettier | PR-number uniqueness across changeset categories. |
| `registry_append_only` (HEAD's JSON object keys are a superset of HEAD~1's; no key reassignment) | react | **Novel git-history-aware shape.** Covers `codes.json`; generalises to i18n/feature-flag/API-endpoint registries. |
| `not_executable` (file doesn't have +x bit) | pytorch | Convenience over `command:` shellout. |
| `line_spacing` (every non-blank line followed by N blanks) | pytorch (MERGE_CONFLICTLESS_CSV) | Niche format; defer. |
| `directory_hash` / `pair_hash` extension (content-hash of dir drives downstream cache) | pytorch (`.ci/docker/`) | CI-rebuild-trigger pattern. |
| `yaml_path_implication` ("if path X = V₁ then path Y = V₂") | pytorch (NO_WORKFLOWS_ON_FORK) | Covers conditional-config patterns. |
| `cross_language_implementation_complete` (every type in a schema spec has a per-language test fixture) | arrow | **Polyglot flagship primitive — v0.11+.** Tailor-made for arrow's `format/Schema.fbs` story. |
| `file_hash_not` / hash-denylist (assert file's content hash is NOT in a known-bad set) | Repolinter migration draft | Repolinter's `file-hash-not` axiom; current alint workaround is `file_content_forbidden` against known-bad substring or `command:` shelling to `sha256sum`. |
| `dir_basename_case` (basename-case rule on directories specifically; current `filename_case` is file-basename-only) | ls-lint migration draft | ls-lint's `.dir:` config has no clean alint shape today — the regex fallback works but is awkward enough that ls-lint adopters notice. |
| `dir_min_files` (directory contains ≥ N matching files) | ls-lint migration draft | ls-lint's `exists:N` config for N>0; orthogonal to existing `dir_contains` (which is presence) and `max_files_per_directory` (which is upper-bound). |
| `filename_case` keyword extensions: `point.case` keyword + `kebab\|Pascal` alternation operator | ls-lint migration draft | Cosmetic DX additions to existing `filename_case` rule; regex fallback works today but the extensions match ls-lint's vocabulary 1:1 for migrators. |

**v0.10 low-priority / niche** (single-source, narrow applicability):

| Candidate | Source | Notes |
|---|---|---|
| `json_key_sort_order` (alphabetical key-order assertion on JSON objects) | pnpm | High machinery cost (serde_json into Value loses key order). |
| `column_alignment` (text aligned to column N or next multiple) | cpython | CODEOWNERS GitHub-username alignment. Defer. |
| `file_header_consistency` (`file_header.alt_pattern` field for "drifted" historical headers) | nodejs/node | Joyent BSD/MIT banner drift; cleaner outcome is editorial cleanup. |

**Bundled-ruleset candidates** (no new rule kinds, just composition):

| Ruleset | Replaces | Source |
|---|---|---|
| `python/pep-621-shape@v1` | `validate-pyproject` shellouts, manual PEP 621 checks | uv |
| `rust/cargo-release-conventions@v1` | clap's `pre-release-replacements` patterns | clap |
| `monorepo/pnpm-workspace@v1` (extension of existing) | per-package field discipline pnpm dogfoods via `meta-updater` | pnpm |
| `apache/governance@v1` (LICENSE+NOTICE+KEYS+RAT discipline) | hand-rolled Apache-RAT shellouts | arrow |
| `cncf/owners@v1` (OWNERS file shape per k8s sig conventions) | k8s-sig OWNERS rules in helm-style projects | helm |
| `ruby@v1` / `swift@v1` / `objective-c@v1` / `erlang@v1` / `elixir@v1` (per-language baselines + corresponding `has_<lang>` facts) | Repolinter's per-language `*-codeofconduct` etc.; one-liner `file_exists` workaround works today | Repolinter migration draft |

**Process meta-findings:**
- Parse-validation catches schema errors but cannot catch pitfalls #13,
  #14, #16, #17 — all four produce silently-wrong runtime behaviour. A
  v0.9.16+ "smoke-test fixture" audit is proposed at the bottom of
  `docs/development/CONFIG-AUTHORING.md`.
- `--format json` output filters out passing per-file rules (engine
  optimisation), which can mislead a config author into thinking rules
  aren't loaded. `alint list --config <path>` is the authoritative view.
- 7 silently-broken structured-path rules in committed pilot+Wave 1
  configs (6 bool-match in Wave 2; 1 array-semantics in Wave 3) were
  caught + fixed only because subsequent case studies happened to surface
  the underlying pitfall. Without the smoke-test fixture audit, this
  pattern (rule loads, builds, evaluates the wrong way) would persist
  indefinitely. **The audit gap is the single most load-bearing
  v0.9.16+ item.**

**Migration-guide additions:** the three migration-guide drafts
(`drafts/migrate-from-repolinter.md`, `drafts/migrate-from-ls-lint.md`,
`drafts/migrate-from-custom-bash.md`) surfaced 5 additional rule-kind
candidates (`file_hash_not`, `dir_basename_case`, `dir_min_files`,
`filename_case` keyword extensions) and 5 bundled-ruleset candidates
(`ruby@v1` etc.). Custom-bash mapped its 6 unmapped patterns 1:1 to
existing v0.10+ candidates — no new candidates from that draft. The
candidate count is therefore at ~28 total across all P2a + migration
work, with 3 broad-applicability v0.10 must-haves (`registry_paths_resolve`,
`cross_file_value_equals`, `ordered_block`) leading by demand strength.

### P2b Wave 1 — pre-launch curated subset (5 of 20 polyglot repos done)

Five repos chosen for unknown-unknown coverage — scale stress, build-
system shape, per-language API parity, 4-language polyglot, and
flagship-visibility apples-to-apples comparison. All 5 ship as
`examples/<owner>-<repo>/` case studies and pass the audit.

| Repo | Why it was chosen | Headline finding |
|---|---|---|
| **NixOS/nixpkgs** | Scale stress (~150k+ files; upper-bound test) | At 39,101 sparse-cloned files + 20,678 `pkgs/by-name/*/*/` package directories, alint's full 79-rule pass — including the headline `for_each_dir` over the by-name tree — completes in **273 ms wall-clock**. `for_each_dir` confirms-scales gracefully; the "any size repo" pitch is now empirically defensible. Live findings: 2 legitimate `.bundle/` violations. |
| **bazelbuild/bazel** | BUILD-file shape (Starlark — no P2a sample); honest "where alint stops" demo | bazel is THE polyglot build-system case study where alint's "scope is filesystem shape, not language semantics" non-goal becomes most visible — alint owns the file-structure layer, `buildifier` owns the Starlark AST layer. ~38 % out-of-scope (highest in P2a+P2b corpus); 422 Starlark units shelled out. **Surfaced new pitfall #18** (`.gitignore` masks tracked-file presence checks). |
| **tensorflow/tensorflow** | ~80k files + per-language API parity | TF stacks TWO discipline layers — file-shape parity (every TFLite Swift `Sources/<X>.swift` has `Tests/<X>Tests.swift`) AND API-shape parity (1,185 textproto goldens lock the public Python surface; 10 distinct API-bearing language surfaces). alint expresses Layer 1 cleanly today (5 known TFLite Swift drifts surfaced); Layer 2 is exactly the v0.11+ `cross_language_implementation_complete` shape — **TF + arrow now jointly demand-drive it** (2 of 2 confirmed). **Surfaced new pitfall #19** (`root_only: true` + multi-component literals silently no-match). |
| **apache/spark** | 4-language polyglot + Maven multi-module | arrow + spark + airflow give us **3 Apache TLPs with 9 of 12 governance artefacts converging** — `apache/governance@v1` bundled ruleset promotes from "v0.10+ idea" to **"v0.10 ship-target"**. Maven multi-module surfaces a **NEW v0.11+ `xml_path_*` family** (parses `pom.xml`'s `<modules>` section directly); generalises to Ant, Gradle XML, .nuspec, .csproj — **completes the structured-query family** (currently json/yaml/toml; xml is the missing fourth). |
| **microsoft/vscode** | Flagship visibility (~160k stars) + apples-to-apples `build/hygiene.ts` | **alint covers ~75 % of `build/hygiene.ts`'s 8 distinct hygiene checks (6 of 8) declaratively in one config.** "alint is what `build/hygiene.ts` would look like as a tool, not a per-repo script" — concrete + verifiable launch claim. **Live tree run: 222 violations, zero false positives** (105 unpinned actions, 47 missing final newlines, 9 workflows without `contents: read`, …). Plus: of vscode's 45 in-tree custom eslint rules, **ZERO are alint-shaped** — every single one is a TSESTree visitor. Cleanest demonstration of the alint/eslint non-overlap boundary in any P2 study. |

### P2b Wave 1 — net-new rule-kind candidates

Saturation analysis: most P2b results were *reconfirmations* of existing
v0.10+ candidates. Net-new additions:

- **`xml_path_matches` / `xml_path_equals`** (spark) — completes the
  structured-query family. Generalises to every XML-config format
  (Maven `pom.xml`, Ant `build.xml`, Gradle XML, NPM `.nuspec`,
  .NET `.csproj`). **Strong v0.10 candidate alongside the existing
  high-priority three.**
- **`cross_language_registry_consistency`** (spark — same shape as
  arrow's `cross_language_implementation_complete`) — variant where
  the parity is between a registry file and a per-language manifest
  rather than between source-and-test pairs.
- **`markdown_template_match`** (TF — TFSA advisories follow a
  6-section template) — single-source, defer.
- Three vscode-only refinements (low priority): `indent_style.skip_block_comment_continuation`,
  `file_is_ascii.allow:` + `file_is_ascii.skip_per_line_marker:`,
  `file_content_matches_or_marker`.
- Two bazel-only deferred: `starlark_glob_resolve` (tree-sitter-starlark
  cost), `bazelrc_path_*`.

### P2b Wave 1 — bundled-ruleset promotion

| Bundled ruleset | Status before P2b | After |
|---|---|---|
| `apache/governance@v1` | v0.10+ idea (single source: arrow) | **v0.10 ship-target** (3 sources: arrow + spark + airflow; 9 of 12 governance artefacts converge) |

### P2b Wave 1 — demand reconfirmations

| Candidate | Pre-P2b sources | Post-P2b sources |
|---|---|---|
| `cross_file_value_equals` | 8 | **9** (vscode `checkCopilotEnginesVersion`) — most consumer-facing case yet |
| `registry_paths_resolve` | 7 | **8+** (nixpkgs alone has 3 registries; TF + spark add more) |
| `generated_file_fresh` | 4 | **6** (bazel + TF + spark) |
| `cross_language_implementation_complete` | 1 (arrow) | **2** (arrow + TF) — now demand-validated; v0.11+ flagship |
| `*_path_contains` | 2 | **3** (helm + deno + bazel) |

### P2b Wave 2 — 5 more polyglot repos (4 of 5 done; flutter in flight)

5 monorepos curated for *platform-driven* polyglot density —
distinct from the *data-format-driven* polyglot density of Wave 1.
At time of writing (4 of 5 audits landed):

| Repo | Rules | Headline |
|---|---|---|
| `angular/angular` | 50 | TypeScript framework with 16 packages; `goldens/public-api/<pkg>/index.api.md` discipline locks the TS API surface of 13/16 packages — canonical single-language `cross_language_implementation_complete` instance |
| `istio/istio` | 65 | Single-module Go monorepo with 9 Helm charts, Prow CI (no GHA), CODEOWNERS not k8s-OWNERS; per-chart image-hub at *different* JSONPath positions per file — surfaces pitfall #20 + a `value_extractor:` design candidate |
| `dotnet/runtime` | 60 | **1,091 .csproj files** (sparse checkout, src/libraries alone is 902); **234 solution files** + **257 Directory.Build.{props,targets}** + **520 .props/.targets** = ~2,300 distinct XML manifests stress-testing the v0.10 `xml_path_*` candidate at one order of magnitude bigger scale than spark |
| `protocolbuffers/protobuf` | 108 | **10 in-tree language bindings** (cpp, java, python, csharp, ruby, php, objc, hpb, upb, rust) + 1 spun-out (dart) with per-binding wire-format failure-allowlist files (failure_list_<lang>.txt) and per-binding GHA test workflow — densest single-repo source for `cross_language_implementation_complete`, ~45 cross-language assertions one rule would express |
| `flutter/flutter` | 39 | **Platform-driven** polyglot variant — single Dart framework, native-OS embedders (Android/iOS/macOS/Linux/Windows/Fuchsia/GLFW + ABI) as peer subdirs under `engine/src/flutter/shell/platform/`, each implementing the same surface (audit pending final aggregation) |

Wave 2 totals so far: **322 rules across 4 case studies** (flutter
adds 39 once aggregated).

### P2b Wave 2 — 2 new pitfalls (#20, #21)

Both surfaced by `istio/istio`:

- **#20 — Cross-file value-equality across structurally-different files
  needs per-file value extraction.** istio's per-chart image-hub
  setting lives at `_internal_defaults_do_not_set.global.hub` in some
  charts and `_internal_defaults_do_not_set.hub` in others. One shared
  `path:` on `cross_file_value_equals` (v0.10 candidate) misses half
  the files. Workaround: 5 `file_content_matches` rules. v0.10 design
  candidate: `value_extractor:` block with `{path-pattern: extractor}`
  mapping.
- **#21 — `yaml_path_*` rules error on multi-document YAML files.**
  The serde_yaml-backed parser rejects multi-doc streams. istio's
  release-notes `releasenotes/notes/50328.yaml` is multi-doc; a
  natural `yaml_path_equals` rule on `$.kind` runtime-errors instead
  of returning a verdict. v0.10 candidate: `multi_doc_mode: error |
  first | every` knob, default `error` so existing configs don't
  silently change behaviour.

Both documented in CONFIG-AUTHORING.md as pitfalls #20 and #21.

### P2b Wave 2 — bundled-ruleset promotion

| Bundled ruleset | Status before Wave 2 | After |
|---|---|---|
| `dotnet@v1` (proposed) | not on candidate list | **v0.10 ship-target** (justified by dotnet/runtime + adopter surface: every dotnet/* + every Azure SDK + every microsoft/* .NET project); 12 of 14 dotnet-specific rules in the case study consolidate into one `extends:` line |

### P2b Wave 2 — rule-kind candidate promotions

| Candidate | Pre-Wave-2 status | Post-Wave-2 |
|---|---|---|
| `xml_path_matches` / `xml_path_equals` | v0.10 candidate (1 source: spark) | **v0.10 ship-target** (2 sources: spark + dotnet/runtime; dotnet stress-tests at ~2,300 manifests vs spark's 49 pom.xml) |
| `cross_language_implementation_complete` | v0.11+ flagship (2 sources) | **v0.11+ ship-target** (4 sources: arrow + TF + protobuf + angular; protobuf is the densest with ~45 cross-language assertions, angular gives the within-language source↔golden variant) |
| `ordered_block` | v0.10 candidate (6 sources) | **v0.10 ship-target** (7 sources: protobuf failure_list files; ties with `registry_paths_resolve` at the top of the v0.10 backlog) |
| `cross_file_value_equals` | 9 sources | **10 sources** (istio per-chart image-hub joins; istio also surfaces the per-file extractor variant — pitfall #20 design candidate) |

### P2b Wave 2 — net-new rule-kind candidates

Saturation continues — Wave 2 adds zero net-new candidates beyond the
v0.10/v0.11 backlogs. Every Wave 2 finding is either a reconfirmation
of existing candidates with deeper data, or a refinement (the istio
`value_extractor:` shape is a refinement of `cross_file_value_equals`
rather than a new candidate). Pitfall discovery rate, by contrast,
held steady at 2 (#20, #21) — both runtime-semantics class, same
flavour as Wave 1's #18/#19.

### Saturation analysis (when to stop adding repos)

By Wave 3, ~80 % of new rule-kind candidates surfaced were *single-source*
or refinements of existing ones; ≥3-source candidates stopped appearing
after Wave 2. Existing v0.10 candidates instead saw 3rd-7th confirmations
with fresh substantive use cases (FIPS for `pair_hash`, lintrunner for
`cross_file_value_equals` + `import_gate` + `generated_file_fresh`,
arrow's `rat_exclude_files.txt` for `registry_paths_resolve`). Pitfall
discovery rate also tailed: pilot 12 → Wave 1 (3) → Wave 2 (1) → Wave 3
(2). The Wave 3 pair surfaced a *runtime semantics* class (#17, #18)
distinct from the schema/regex classes, suggesting a remaining unknown
unknown but a converging surface.

**Implication for P2b:** the polyglot wave should optimise for *new
narrative shapes* (multi-language stress, scale stress) rather than
*new rule-kind candidates*. Most P2b case studies will reconfirm
existing v0.10 candidates with deeper data, not surface new ones.

### P3 — Marketing refresh (~5-6 days; depends on P2a)

#### P3.1 Hero + content

Three concrete value props, evidence-backed from P2:

1. "Sub-second on 100K-file repos" (cite v0.9.13 100k bench: S3 1.13s)
2. "Agentic-aware: structured `agent` output format + `agent-hygiene` ruleset for AI-touched repos"
3. "60 rule kinds + 19 bundled ecosystem rulesets — zero plugins to install" (P2b Wave 2: dotnet/runtime + protobuf both validate the v0.10 `xml_path_*` + `cross_language_implementation_complete` candidates with massive scale evidence)

Pages to add:
- `alint.org/compare` — direct table: alint vs Repolinter (archived), ls-lint, Megalinter, custom shell scripts
- `alint.org/examples` — gallery of P2 case studies. "alint in production at: rust-lang/rust, kubernetes, deno, …"
- `alint.org/benchmarks` — public-facing version of HISTORY.md
- `alint.org/migrating-from/{repolinter,ls-lint,custom-bash-scripts}` — step-by-step

README hero — match alint.org messaging. 5-line punch + quickstart.
CLI demo — asciinema or animated GIF embedded in README + alint.org hero.

#### P3.2 SEO (~1.5 days)

| Item | Why |
|---|---|
| `sitemap.xml` (auto-generated) | Search Console + crawler discoverability |
| `robots.txt` with explicit AI crawler rules | Allow good crawlers; explicit posture |
| Canonical `<link rel="canonical">` per page | Avoids duplicate-content penalties |
| Per-page `<title>` + `<meta description>` | Each page ranks for its own keywords |
| H1/H2/H3 hierarchy audit (one H1 per page) | SEO + accessibility |
| Image alt text audit | A11y + image search |
| Schema.org JSON-LD: `SoftwareApplication`, `Article`, `BreadcrumbList` | Rich-result eligibility |
| Lighthouse pass + fix any < 90 | Page-quality signal |
| Internal linking (rule pages ↔ ruleset pages) | Topic-cluster authority |
| Submit `sitemap.xml` to Google Search Console + Bing | Faster indexing |
| Keyword-targeted landing pages: `/repolinter-alternative`, `/monorepo-linter`, `/agent-friendly-linter`, `/language-agnostic-linter`, `/repository-structure-linter` | High-intent + low-competition |

**Keyword strategy:**
- "repolinter alternative" / "repolinter replacement" — high intent (Repolinter archived 2026-02; users actively shopping)
- "monorepo linter" / "monorepo conventions enforcement"
- "language-agnostic linter" / "polyglot linter"
- "agent-friendly linter" / "AI-aware repository linter"
- "repository structure linter" / "filesystem linter"
- "ls-lint alternative"

#### P3.3 AI/LLM discovery (~1 day)

A coordinated story for every major way an LLM/agent finds documentation:

| File / endpoint | Purpose |
|---|---|
| **`/llms.txt`** | The [llmstxt.org](https://llmstxt.org/) standard. Single markdown file with H1 title + summary + H2 sections of bullet-list links to canonical content. LLMs ingest in one fetch instead of crawling. |
| **`/llms-full.txt`** | Companion: same content but with all linked docs inlined into one large markdown blob. For LLMs without browse-tool access. |
| **`/.well-known/security.txt`** (RFC 9116) | Standard vulnerability disclosure path. Important for a build-tool with supply-chain implications. |
| **`/.well-known/ai.txt`** | Spawning AI's emerging standard for opting in/out of AI training data. We *want* opt-in. |
| **`robots.txt` AI crawler rules** | Explicit allow/disallow for `GPTBot`, `ClaudeBot`, `anthropic-ai`, `CCBot`, `Google-Extended`, `PerplexityBot`, `Applebot-Extended`, `meta-externalagent`. We allow all. |
| **JSON-LD `SoftwareApplication`** | Schema.org structured data: name, version, license, install URL, supported OS. Both human SEO + agent ingestion. |
| **RSS/Atom feed for releases** (`/releases.atom`) | Both humans and agent monitoring poll feeds. |
| **Stable JSON endpoints**: `/api/rules.json`, `/api/rulesets.json`, `/api/versions.json` | Programmatic catalogue discovery. |

**Stretch (P5/post-launch): alint as an MCP server.** [Model Context
Protocol](https://modelcontextprotocol.io/) lets agents query tools directly.
An `alint` MCP server could expose `get_rule_doc(rule_kind)`,
`validate_config(yaml)`, `suggest_rules_for(repo_path)` — agent-native
integration. ~3-5 days of work.

### P4 — Launch (~2-3 days)

- **GitHub release for v0.9.14** (or hold for a v1.0 cut) — proper release notes, screenshots, migration guidance
- **Press kit** — `branding/` directory with logo SVGs, screenshots, GIFs, OG images
- **Pre-launch beta** — invite 5-10 people from the P2 case-study repos as beta testers (~1 week pre-public)
- **Launch posts drafted** — HN ("Show HN: alint, a fast linter for repo structure"), r/rust, Lobsters, dev.to. Each tailored to audience.
- **Launch day**: post → monitor Discussions/issues → respond fast for 24-48 hours
- **Day-after**: blog post or design-doc-style writeup of the v0.9.6→.10 silent-no-op bug class story (great content marketing)

### P5 — Post-launch (~ongoing)

- Privacy-respecting analytics on alint.org (Plausible)
- GitHub Sponsors button / `funding.yml`
- Star CTA banner in README (post-1k-stars)
- Newsletter / RSS for releases (RSS already in P3.3)
- Optional: `alint init` command that detects existing tooling and proposes a starter config
- **MCP server** (per P3.3 stretch)
- P2b case studies as ongoing content marketing — every polyglot case study becomes a blog post + dev.to article + social

---

## Other productionalization items

Worth doing but not blocking launch:

- **`deny_unknown_fields` uniformity audit** (v0.9.16+) — 13 of 60 rule
  Options structs don't carry `#[serde(deny_unknown_fields)]`
  (file_content_matches, file_content_forbidden, file_header,
  file_footer, file_max_lines, file_max_size, file_min_lines,
  file_min_size, file_shebang, filename_case, filename_regex,
  commented_out_code, markdown_paths_resolve). Without it those rules
  silently accept extra fields, which means the Phase 3 did-you-mean
  enricher can't fire for those kinds. v0.9.15 added the attr only to
  `structured_path.rs` (the one needed for pitfall #16); the rest is a
  separate audit since it's a behaviour change that could surface
  latent typos in adopters' configs.


- **`alint --version` includes commit SHA + build date** (verify current state)
- **Crash-report path** — when alint panics, print a pre-filled `https://github.com/asamarts/alint/issues/new` URL with context
- **Schema URL for editor autocomplete** — `# yaml-language-server: $schema=https://alint.org/schemas/v1/config.json` at the top of `.alint.yml`. Schema is already published; just needs docs.
- **Reproducer machinery** — `alint debug bundle` that captures config + a minimal failing repo into a tarball for bug reports
- **Public roadmap page** (separate from internal `docs/design/ROADMAP.md`) — single "what's next" page per major version
- **Telemetry-free guarantee** — `SECURITY.md` or `PRIVACY.md` explicitly states alint sends nothing over the network except `extends: https://...` URLs the user wrote (and that's SRI-pinned)
- **Governance doc** — even a one-page "this is currently a single-maintainer project; here's how decisions get made" sets expectations
- **Search on alint.org** — once SEO + llms.txt land, internal search (Pagefind or similar static-site search) for the rule catalogue
- **Versioned docs** — alint.org currently shows current docs; a `/docs/v0.9/` switcher would let users on older versions land on accurate pages (especially after major API changes like v0.9.10's Scope refactor)
- **Translation strategy** — punt for now; English-only at launch; revisit if traction

---

## Timeline summary

```
Week 1:    ✅ P1 hygiene + P2a pilot (5 of 20 repos, +12 pitfalls catalogued)
                + v0.9.15 P1+P2 (findings doc + examples-parse audit)
Week 2:    P2a-full Waves 1-3 (15 remaining repos in 3 batches of 5)
                + v0.9.15 Phase 3-4 (did-you-mean errors + domain-specific messages)
Week 3:    v0.9.15 Phase 5-6 (JSON Schema + validate-config subcommand) → ship v0.9.15
Week 4:    P3.1 hero + content + P3.2 SEO + P3.3 AI/LLM discovery
Week 5:    P4 launch prep + beta
Week 6:    Launch
Week 7+:   P2b (polyglot monorepos) — runs as evidence-driven content marketing
            + P5 post-launch infra (MCP server, sponsors, analytics)
```

**Total to launch:** ~5-6 weeks. **Total to fully realised state** (40 case
studies + post-launch infra including MCP server): ~10-12 weeks.

---

## First concrete steps

1. ✅ **This doc** — committed for tracking
2. ✅ **P1 in one sitting** — repo launch-presentable (committed `52e7494f`)
3. ✅ **P2a pilot** with 5 repos — methodology validated (committed `e7451b95` + `481b32db`)
4. ✅ **v0.9.15 Phase 1+2** — findings doc + examples-parse audit (committed `ba7802fa`)
5. ✅ **P2a-full Wave 1** — 5 parallel subagents (tokio, uv, ruff, clap, typescript) — surfaced pitfalls 13-15 + 6 new rule-kind candidates
6. ✅ **P2a aggregation (Wave 1)** — CONFIG-AUTHORING.md §13-15 added; rule-kind candidate table added to launch-prep.md
7. ✅ **P2a-full Wave 2** — 5 parallel subagents (next.js, pnpm, react, prettier, cpython) — surfaced pitfall #16 + 6 more rule-kind candidates + caught 6 silently-broken bool-match rules in committed configs
8. ✅ **P2a aggregation (Wave 2)** — CONFIG-AUTHORING.md §16 added; 6 broken rules fixed
9. ✅ **P2a-full Wave 3** — final 5 (golang/go, helm, arrow, pytorch, nodejs/node) — surfaced pitfalls #17 + #18 + 9 more rule-kind candidates + caught 1 silently-broken array-semantics rule in deno
10. ✅ **P2a aggregation (Wave 3 + final)** — CONFIG-AUTHORING.md §17-18 added; deno rule fixed; rule-kind table reorganised final; 5 positioning narratives tabled
11. **v0.9.15 Phase 3-7** — DX hardening with full pitfall catalogue (18 pitfalls; Phases 3-6 close the parse-time gap, Phase 7 closes the runtime-semantic gap via smoke-test fixtures)
12. **P3 marketing draft work** — runs *in parallel with* v0.9.15 Phases 3-7. Drafts only (not published) until v0.9.15 ships and v0.9.6 → v0.9.15 docs roll on alint.org. Single source of truth: `docs/marketing/STATE.md` (current marketing inventory + draft locations + per-draft status).
13. **v0.9.15 release**
14. **P3 marketing publish** — promote drafts to published; refresh alint.org with v0.9.15 evidence + 20 case studies + 18-pitfall narrative
15. **P4 launch**
16. **P5 post-launch** — concurrent with **P2b** (20 polyglot monorepos)

The plan is intentionally a living doc — every phase will surface adjustments.
Update this file as we learn.
