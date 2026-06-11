# Spec-Driven Development for alint: A Proposal

Status: Accepted (2026-06-10), recorded as ADR-0001. **All five workstreams
implemented and merged to `main` (2026-06-11):** Phase 0 governance (ADRs,
constitution, template); Phase 1 schema-from-types (`gen-schema --check`); Phase 2
schema-derived rule-options tables; Phase 3 the `facts.json` contract
(`gen-facts --check`); Phase 4 architecture-as-code (`gen-arch --check` — crate
graph + C4 model); Phase 5 pragmatic formal methods (proptest properties + a
verified Kani confinement proof). The only remainder is WS5 — alint.org rendering
*from* the shipped `facts.json` — which lives in the private site repo. Per-phase
"Progress" notes are inline below; ADR home is docs/adr/.

Phase 1 scope note: the option-bearing rule kinds with flat option structs derive
their `schemas/v1/config.json` branch from their Rust types (schemars); the deeply
nested kinds (cross_file, file_graph, registry_paths_resolve, for_each_*,
dir_contains/dir_only_contains, every_matching_has, generated_file_fresh,
import_gate, the structured-query family) stay hand-written passthrough by design,
plus two whose option type can't be faithfully derived (filename_case's custom
case-alias deserializer, json_schema_passes's runtime-validated format string).
Scope: cross-cutting (engine, DSL, schema, docs, CI, alint.org)
Related: [ARCHITECTURE.md](./ARCHITECTURE.md), [ROADMAP.md](./ROADMAP.md), [deterministic-perf-gating.md](./deterministic-perf-gating.md)

---

## TL;DR

This proposal moves alint to an explicit spec-driven model with four goals: capture
architectural decisions as ADRs, maintain formal specs and models (with verification
where it pays off), auto-generate architecture diagrams, and drive every drift between
spec, code, tests, docs, and marketing toward zero, as automatically as possible.

The central finding of the analysis is that **alint is already about 70 percent of the
way there.** It has a mature design-doc-first culture, a JSON Schema for its DSL, a
canonical `all_kinds.yaml`, declarative bundled rulesets, and a real multi-layer
drift-control system (the `coverage_audit_*` tests, `check-version-pins.sh`,
`xtask docs-export`). This is an **extension and consolidation**, not a greenfield
transformation, and it is deliberately calibrated to a one-to-two-person team.

The organizing idea is a single rule, drawn from how the industry actually succeeds and
fails at this (see Appendix B):

> **Put machine-checkable contracts on the highest rung (generate everything downstream
> from one source, fail CI on any diff). Keep prose specs (design docs, ADRs) as
> point-in-time scaffolding. Never try to make English prose a regenerable source of
> truth.**

The keystone change is small and high-leverage: the DSL JSON Schema is currently 2180
lines of **hand-written JSON** that can silently disagree with the Rust types that
actually parse configs. Derive it from the types instead. Then generate the rule
reference, the CLI reference, and a `facts.json` manifest from code, and gate all of it
with a regenerate-and-diff check. alint already ships the rule kind that enforces exactly
this kind of gate (`generated_file_fresh`), so **alint can enforce its own spec-driven
discipline on itself** - which is both the most automated option and a genuine marketing
asset for an anti-drift linter.

Heavy deductive verifiers (Verus, Creusot, Prusti) are explicitly out of scope: they
target systems/crypto code at roughly 5:1 proof-to-code ratios and are not maintainable
for application logic by a small team. Formal methods are confined to a pragmatic tier
(Kani for panic/overflow freedom on the pure core, Miri on the test suite, and at most
one Rust-native Stateright model for the single genuinely subtle ordering invariant).

---

## 1. Where alint stands today (honest baseline)

### 1.1 What already exists (and is good)

**A real spec-driven culture.** Every behavior-bearing rule kind ships with a design doc
under `docs/design/vX.Y/` following an informal seven-section convention (Problem,
Surface area, Semantics, False-positive surface, Implementation notes, Tests, Open
questions). The workflow is "design-doc draft commit, then atomic implementation commit
that flips the doc's Status to Implemented." This is spec-first development done well.

**Contracts that already function as specs:**

- `schemas/v1/config.json` - JSON Schema (draft 2020-12) for the DSL, embedded into
  `alint-dsl` via `include_str!` and validated against representative configs.
- `crates/alint-dsl/tests/fixtures/all_kinds.yaml` - the canonical enumeration of every
  rule kind with its options; the source of truth that `coverage_audit_readme_claims`
  counts against.
- `crates/alint-dsl/rulesets/v1/**/*.yml` - the bundled rulesets, which are themselves
  declarative governance specs.
- `crates/alint-testkit/TREE_SPEC.md` - a small declarative DSL for test-fixture trees.

**A multi-layer drift-control system already in production.** This is the most important
existing asset and the proof that the team already believes in this model:

| Layer | Mechanism | Pins |
| --- | --- | --- |
| README/about claims | `coverage_audit_readme_claims` | counts (kinds, families, rulesets, fixers, formats, subcommands) derived from code/fixtures |
| Schema vs registry | `coverage_audit_schema_drift` | every registered kind has a schema dispatch entry |
| Rule pass/fail coverage | `coverage_audit_pass_fail` | every kind has a firing and a silent e2e scenario |
| Site frontmatter | `coverage_audit_site_docs_frontmatter` | every site page has parseable YAML frontmatter |
| Bench trajectory | `coverage_audit_benchmarks_trajectory` | latest release surfaces on top |
| Version pins | `check-version-pins.sh` | install snippets pin the workspace version |
| Dep floors | `check-workspace-dep-floors.sh` | internal version pins do not exceed workspace version |
| Perf | `det-perf-gate.sh` (Valgrind) | instruction/cycle regressions vs merge-base |
| CLI docs | `cli_reference_subcmds_match_command_enum` | docs subcommand list matches the `Command` enum |

**The dogfooding ethos.** alint already lints itself (`.alint.yml` + `ci/scripts/dogfood.sh`),
including a `bundled-ruleset-has-uri-header` rule and an `install-snippets-match-workspace-version`
command rule. The instinct to "enforce our own discipline with our own tool" is the exact
instinct this proposal builds on.

### 1.2 The gaps (what actually drifts)

The drift-surface analysis (Section 4) found these concrete weaknesses, ranked by risk:

1. **The schema is hand-maintained.** `schemas/v1/config.json` is 2180 lines of
   hand-written JSON kept in sync with the Rust types by human discipline plus a
   *byte-copy* into `crates/alint-dsl/schemas/v1/config.json`. There is no test that the
   schema's per-kind option keys match the actual serde struct fields. A field rename in a
   rule (the kind of rename that happened in the v0.10 Phase 1 work: `registry:` to
   `source:`, `in:` to `target:`) propagates only by hand across the struct, the schema,
   the docs, the examples, and the bundled rulesets. **This is the single largest
   untracked drift class.**

2. **alint.org marketing surfaces are not audited from this repo.** Version strings, the
   JSON-LD `softwareVersion`, the "Latest release" line, and any feature/count claims in
   blog or landing copy live in a separate repo and can advertise a stale reality
   undetected.

3. **Per-kind reference prose is hand-written.** `docs/rules.md` and `docs/site/reference/`
   describe each rule and its options in prose that no audit checks against the code.

4. **Two-list hazards.** `xtask`'s `CLI_REFERENCE_SUBCMDS`, the in-crate schema byte-copy,
   and the README ruleset prose-list are each a second copy that can fall behind their
   source even where a count audit passes.

5. **No formal architecture diagrams, no ADRs.** Architecture intent lives only in prose
   (`ARCHITECTURE.md`) and in the maintainer's head. There is no decision log, despite
   alint *shipping a ruleset that lints ADRs in other repos.*

### 1.3 What alint already ships that we can turn on itself

This is the lever that makes "maximally automated" realistic. alint's own rule kinds are,
almost exactly, the anti-drift primitives the industry reaches for:

- **`generated_file_fresh`** runs a declared generator and diffs its output against the
  committed file. This *is* the rust-analyzer / Ruff "regenerate and `git diff --exit-code`"
  codegen gate, as a first-class rule. Every generated artifact in this proposal can be
  guarded by it.
- **`command_idempotent`** runs a checker in `--check` mode and parses its offender list -
  the generic form of the same gate.
- **`cross_file_value_equals`** / **`file_graph`** / **`registry_paths_resolve`** can assert
  that a value in one file matches another, that references resolve, and that declared sets
  stay consistent - i.e. they can pin cross-file invariants between spec and code.
- **The `docs/adr@v1` bundled ruleset** already enforces MADR ADR hygiene. alint's own
  `docs/adr/` can be linted by alint's own shipped ruleset with zero new code.

The recurring theme below: **prefer an alint rule over a bespoke CI script** wherever the
two are equivalent, because it is more automated, it is self-documenting, and every such
use is a real-world test of alint.

---

## 2. The organizing principle: the spec maturity ladder

Martin Fowler's team classifies spec-driven approaches on a three-rung ladder, and it
dissolves most of the confusion in this space:

1. **Spec-first** - specs written before code, then discarded as scaffolding.
2. **Spec-anchored** - specs persist; downstream artifacts are regenerated from them.
3. **Spec-as-source** - humans edit only the spec; the rest is generated and marked
   "DO NOT EDIT."

The field's evidence (Appendix B) is blunt about the failure mode: pushing **prose** to
the high rungs fails, because reality changes faster than prose specs do, and a stale
prose spec misleads confidently. Even Google admits it does not keep design docs current.

So the rule for alint:

| Artifact class | Target rung | Why |
| --- | --- | --- |
| DSL schema, CLI interface, rule/option reference, counts, `facts.json` | **Spec-as-source** (generate + gate) | Machine-checkable; drift is detectable and fixable automatically |
| Architecture *structure* (crate/module graphs) | **Spec-as-source** (code-extracted) | Derivable from `cargo metadata`; cannot drift |
| Architecture *intent* (C4 context/container) | **Spec-anchored** (hand-modeled, CI-rendered, consistency-gated) | Low churn; a small gate keeps it honest |
| Design docs, ADRs, the constitution | **Spec-first** (point-in-time scaffolding) | Prose; valuable as decision record, not as regenerable truth |

This single table is the spine of the whole proposal. Everything below is an application
of it.

---

## 3. The five workstreams

### WS1 - Contracts as the single source of truth (the anti-drift core)

This is the highest-ROI workstream and the prerequisite for closing most of the drift
surface. Ordered by dependency.

**1a. Derive the schema from the Rust types (the keystone).**
Adopt `schemars` (v1.x, actively maintained, honors serde attributes) so the JSON Schema
is generated from the config structs instead of hand-written. The guarantee is structural:
the schema cannot disagree with what serde actually parses. Concretely:

- Add `#[derive(JsonSchema)]` to the config/`RuleSpec`/per-kind option structs.
- Add an `xtask gen-schema` step that writes `schemas/v1/config.json` and the in-crate copy
  from `schema_for!`.
- Retire the hand-maintained 2180-line file and the byte-copy hazard in one move.
- This closes drift sources #1 and #4 from Section 1.2 at the root, and would have caught
  the dashed-key JSONPath bug recorded in project history.

Caveat to validate during implementation: the current schema may encode constraints
(per-kind `oneOf` dispatch, descriptions, `additionalProperties: false`) that need
`schemars` attributes or a small post-processing pass to reproduce. Budget a spike to
confirm fidelity before deleting the hand-written file; keep the existing
`coverage_audit_schema_drift` test as the safety net during migration.

**1b. Generate the rule/option reference from rule metadata.**
This is the Ruff model (a near-perfect analogue: a linter with a large rule and option
surface whose `cargo dev generate-all` emits the schema, the config docs, and the rules
pages from rule structs). Widen the existing `xtask docs-export` so that per-kind reference
content (options, defaults, one canonical example) is generated from the same metadata that
feeds the schema, not hand-written in `docs/rules.md`. Mark generated sections with a
`DO NOT EDIT - generated by xtask` banner.

**1c. Generate the CLI reference from clap.**
Use `clap_mangen` (maintained, part of the clap project; its own docs recommend the xtask
pattern alint already uses) to emit man pages, plus a small in-repo Markdown emitter for the
CLI docs. Do not depend on `clap-markdown` (archived). This retires the hand-maintained
`CLI_REFERENCE_SUBCMDS` two-list hazard.

**1d. Make documentation executable (snapshots + CLI conformance).**
- Adopt `insta` to snapshot generated output (`--help` per subcommand, the rules table, the
  emitted `facts.json`). Drift becomes a reviewable failing snapshot.
- Adopt `trycmd` to run every documented `alint ...` invocation (including examples embedded
  in `README.md` and the site) as a conformance test against the real binary. A stale example
  becomes a failing test.
- Schema-validate every example config (repo `examples/`, docs, the v0.12 corpus, bundled
  rulesets) against the generated schema in CI. This is provider/consumer contract testing
  adapted to a DSL.

**1e. Emit `facts.json` (the contract for everything downstream, including alint.org).**
Add an `alint facts --json` (or an `xtask` emitter) that prints a machine-readable manifest:
version, rule-kind count, bundled-ruleset count, supported languages, the rule catalogue,
output formats, and any comparison-table cells the marketing site asserts. This single file
becomes the contract that the README badges, the docs, and the website all consume, rather
than restating numbers in prose. (See WS5 for the alint.org side.)

**1f. The regenerate-and-gate mechanism - dogfooded.**
Every generated artifact (schema, rule reference, CLI reference, `facts.json`) is committed
and guarded by a regenerate-then-diff check. Implement the gate as an alint rule using
`generated_file_fresh` in `.alint.yml` rather than a bespoke shell loop, so the gate is
itself an alint feature under test. Keep `git diff --exit-code` as the CI backstop. Prefer
content-diff over any mtime-based staleness check (mtime is unreliable on fresh CI clones).

*Decision (2026-06-11): the gates shipped as `gen-{schema,facts,arch} --check` (run in CI's
`docs` job via `ci/scripts/docs.sh`) plus a `gen_*_check_passes_on_committed_tree` cargo
test per generator — content-diff, not mtime. We did NOT dogfood them via
`generated_file_fresh` because it doesn't fit: that kind diffs a command's stdout against a
single target, but our generators write files (and `gen-schema` writes two — root + the
in-crate copy), have a first-class `--check` mode, and would have to shell `cargo` from
inside `alint check .` — making the dogfood non-self-contained and slow on every push. The
`--check` gates are themselves under cargo test, so the mechanism is still "an alint
generator under test," just not routed through an alint rule. The earlier CI-gate bypass
(artifact-only PRs skipping the `docs` job) was closed in the same review by routing
`facts.json` + `docs/design/architecture/**` through the `docs` change-class.*

**WS1 net effect:** the schema, the rule reference, the CLI reference, and the public facts
all become generated, snapshot-pinned, and gated. Drift sources #1, #3, #4 are closed; #2 is
set up for WS5.

---

### WS2 - Architecture Decision Records

**Format and home.** Adopt MADR 4.0.0 (dual MIT/CC0; vendor the bare template into the repo).
Store ADRs in `docs/adr/` rather than MADR's default `docs/decisions/`, for one decisive
reason: **alint's own shipped `docs/adr@v1` ruleset defaults to `docs/adr/*.md`,** so the
project's ADRs are linted by alint's own ruleset out of the box, with zero path overrides.
This is the credibility play - the tool that lints ADRs must keep exemplary ADRs.

**Mechanics.** Files `NNNN-title.md`; YAML front-matter `status: {proposed | accepted |
deprecated | superseded by ADR-NNNN}`; immutable records (supersede, do not rewrite);
PR/commit links in "More Information." A generated index. Optionally surface them via
`log4brains` (ships a GitHub Pages action) later; the markdown files alone are sufficient to
start.

**Backfill the load-bearing decisions** that currently live only in prose or memory, for
example: DSL is YAML; the rule-kind taxonomy and the three dispatch classes; the
path-confinement / `allow_out_of_root` security model; `SPAWNING_RULE_KINDS` as the
code-execution trust boundary; LSP as a separate crate; determinism as a hard guarantee;
the design-doc-first workflow itself; the Valgrind-based perf gate. Each is one short ADR.

**Relationship to design docs.** Design docs stay where they are and keep their role
(detailed, version-scoped feature specs). ADRs are the *durable, cross-cutting decision log*
that design docs reference. ARCHITECTURE.md Section 9 (or a new section) links to the ADR
index.

---

### WS3 - Architecture diagrams (hand-modeled intent + code-extracted structure)

The anti-drift answer here is a split, not a single tool.

**Code-extracted (cannot drift, regenerated every CI run):**

- `cargo-depgraph` -> the 10-crate workspace dependency graph (SVG).
- `cargo-modules` -> per-crate module structure and internal dependency graphs (SVG), plus
  its `--acyclic` flag as a **CI gate** that fails on any new module cycle.
- These are committed and guarded by the same regenerate-and-diff gate as WS1 (again via
  `generated_file_fresh`).

**Hand-modeled but CI-rendered (C4 intent):**

- One Structurizr `workspace.dsl` (Apache-2.0) as the source of truth for C4 levels 1-3:
  System Context (alint + developer/CI actors + linted repo + registries + alint.org),
  Container (the CLI binary, the `alint-lsp` server, the alint.org site + docs-bundle
  pipeline), and a Component view of the CLI (the crates as components). The ten crates are
  components, not containers. Skip C4 level 4 (code) per C4's own guidance.
- Export to Mermaid (renders natively in GitHub markdown and PRs) and to SVG (for alint.org),
  in CI.
- A small `xtask` consistency gate diffs the crate names declared in `workspace.dsl` against
  `cargo metadata` workspace members and fails if a crate is added or removed without
  updating the model. This is what keeps the hand-modeled layer honest.

**Important currency caveat:** the standalone `structurizr/cli` repo was archived 2026-02-04
and consolidated into a single `structurizr.war` tool (still Apache-2.0, data-compatible).
Pin tooling to the consolidated artifact, not the archived install path. Confirm the exact
invocation at implementation time.

**arc42:** cherry-pick, do not adopt wholesale. Fold the high-value, low-churn sections
(Introduction/Goals, Constraints, Context/Scope, Building Block View, Crosscutting Concepts,
and Architecture Decisions -> links to `docs/adr/`) into ARCHITECTURE.md. The full 12-section
template would itself become drift-prone.

---

### WS4 - Formal specification and verification (the pragmatic tier)

The honest framing: most of alint is a deterministic batch pipeline, not a concurrent
protocol, so the ROI of heavy formal methods is low and concentrated. Adopt in this order.

**Adopt now (high value, low cost):**

- **Grammar/schema as the syntax spec** - WS1's generated schema already is the config
  contract. (A formal PEG/EBNF grammar via `pest`/`lalrpop` is optional and only relevant if
  the DSL ever grows beyond YAML; not recommended now.)
- **`insta` snapshots + `proptest` properties** as the behavior spec (proptest is already a
  dependency; lean in harder). Properties are drift-proof partial specs that run in CI.
- **Kani** (AWS bounded model checker) as a separate CI job, proving panic-freedom, no
  arithmetic overflow, no out-of-bounds, and no unexpected `unwrap` on the **pure, bounded**
  core: config merge/precedence, glob/path normalization, the dispatch-ordering helpers,
  count arithmetic. No code annotations; harnesses look like proptest harnesses. Run nightly
  to manage model-checking time.
- **Miri** on the existing test suite (nightly job) to catch undefined behavior if any
  `unsafe` exists. Near-zero effort.

**Adopt selectively (medium):**

- **`contracts` crate** (`#[requires]`/`#[ensures]`) on a few load-bearing invariants
  (config merge, cross-file dispatch ordering, the read cap) as runtime/test assertions and
  living documentation. Migrate toward the native compiler contracts (MCP-759) as they
  stabilize, since those are designed to feed verifiers later.
- **One Stateright model** (Rust-native model checker; spec and impl in one language, runs in
  `cargo test`) for the single genuinely subtle invariant: that cross-file dispatch produces
  the same result set regardless of file-visit order, and/or that the LSP's incremental
  cache never serves a stale cross-file result. Prefer Stateright over TLA+/Quint precisely
  because it avoids a second language and toolchain. Drive conformance by generated tests, not
  by post-hoc trace validation (the field shows trace-validation is the expensive, brittle
  end of anti-drift).

**Explicitly skip (low ROI / research-grade):** Verus, Creusot, Prusti, Aeneas, hax (built
for systems/crypto at ~5:1 proof-to-code; the Rust std-lib effort reached under 4 percent
coverage with experts). TLA+/Alloy/P as maintained dependencies. Loom/Shuttle unless alint
grows hand-rolled lock-free concurrency. These are "watch," not "adopt."

---

### WS5 - Close the alint.org drift loop

The website is just another consumer of the `facts.json` contract from WS1e.

- **Repo side:** ship `facts.json` as a release artifact (and at a stable URL via the
  docs-bundle pipeline that alint.org already pulls).
- **Site side:** render counts, version, supported-language lists, and comparison tables from
  the synced `facts.json` at build time (Astro fetches build-time data and ingests JSON as a
  typed source), instead of hardcoding them in prose. Live README/site badges can use the
  shields.io endpoint contract pointed at `facts.json`.
- **A content test in the site repo** fetches the product's published metadata (crates/npm
  version, shipped `facts.json`) and fails the site build if the rendered claims disagree.
- **Version-pin parity:** extend the release checklist (and ideally a cross-repo CI check) so
  alint.org's version surfaces cannot lag a release undetected.

The narrative bonus: "alint's own marketing claims are enforced by alint" is a credible,
on-brand story for an anti-drift linter, and worth saying out loud on the site.

---

### WS0 (cross-cutting) - lightweight governance

Two small, prose-rung artifacts that formalize tribal knowledge without creating regen risk:

- **`docs/design/constitution.md`** - alint's immutable invariants, stated once and reviewed
  on every change: every spawning rule kind must be in `SPAWNING_RULE_KINDS`; every rule kind
  needs a firing and a silent e2e scenario; all whole-file analysis reads go through
  `read_capped`; output is deterministic; cross-file kinds declare `requires_full_index()`
  and no path scope. The project currently re-learns these each cycle; a constitution makes
  them explicit. Several are already machine-enforced by `coverage_audit_*` and should link to
  their enforcing test.
- **A fixed design-doc template** (`docs/design/TEMPLATE.md`) codifying the existing
  seven-section convention so it stops being informal, plus a one-line "which ADR(s) does this
  touch" pointer.

---

## 4. Drift surface to fix map

Consolidated from the drift-surface analysis. "Today" is the current control; "Proposal" is
the target rung.

| Fact / artifact | Today | Risk | Proposal |
| --- | --- | --- | --- |
| Schema vs parser (option keys) | hand-written JSON + byte-copy; no field-level audit | High (undetected) | WS1a generate from types (`schemars`) |
| alint.org version + claims | manual, cross-repo, unaudited | High | WS5 `facts.json` + site content test |
| Per-kind reference prose | hand-written `docs/rules.md`, `docs/site/reference/` | High | WS1b generate from metadata |
| `CLI_REFERENCE_SUBCMDS` two-list | partial test (count only) | Medium | WS1c generate from clap |
| In-crate schema byte-copy | manual copy | Medium | WS1a single generated emit |
| Ruleset doc cell values | count audited, cells not | Medium | WS1b generate ruleset pages from YAML |
| Rule-kind / family / ruleset counts | `coverage_audit_readme_claims` | Low (audited) | WS1e fold into `facts.json`; keep audit |
| Version pins (install snippets) | `check-version-pins.sh` | Low | keep; surface via `facts.json` |
| Architecture intent | prose only | Medium (no record) | WS2 ADRs + WS3 C4 model |
| Crate/module structure | undocumented | Low | WS3 code-extracted graphs + acyclic gate |
| Invariants (spawning, read cap, determinism) | scattered, partly tested | Medium | WS0 constitution linking to tests; WS4 Kani/contracts |

Principle applied throughout: promote each row from "manual + hope" to "duplicate + CI
audit," and from "audit" to "single source + generate," as far up as is practical. The
`coverage_audit_*` counters stay as a safety net but are explicitly treated as a way-station,
not a destination.

---

## 5. Dogfooding: alint enforces alint

A short section because it is the multiplier that makes the rest "maximally automated," and
because it is a differentiator worth being deliberate about.

Wherever this proposal needs a CI gate, prefer expressing it as an alint rule in `.alint.yml`:

- Generated-artifact freshness (schema, rule reference, CLI reference, `facts.json`,
  diagrams) -> `generated_file_fresh`.
- ADR hygiene in `docs/adr/` -> the shipped `docs/adr@v1` ruleset.
- Cross-file consistency (for example, `facts.json` count vs the live registry) ->
  `cross_file_value_equals` / `file_graph` / `registry_paths_resolve`.
- The constitution's structural invariants -> existing `coverage_audit_*` tests, referenced
  from the constitution.

Every such use is simultaneously: more automated than a bespoke script, self-documenting, a
real-world test of alint on a polyglot repo, and a true marketing claim ("alint's own
spec/docs/marketing drift is prevented by alint"). This is the cleanest possible alignment of
the four user goals.

---

## 6. Phased rollout

Sequenced by dependency and ROI, using the project's established one-commit-per-phase
convention (each phase: design note or ADR, then atomic implementation; forward
`Next: Phase N` pointer). Phases 1-3 deliver most of the anti-drift value.

**Phase 0 - Governance scaffolding (cheap, no code).**
Constitution, design-doc template, MADR template into `docs/adr/`, and the first few backfill
ADRs. Wire alint's `docs/adr@v1` ruleset into `.alint.yml`. ADR-0001 records "adopt
spec-driven development" (this document, distilled).

**Phase 1 - The keystone: schema from types.**
WS1a. Spike fidelity, add `schemars` derives, add `xtask gen-schema`, retire the hand-written
schema and the byte-copy, guard with `generated_file_fresh`. Keep `coverage_audit_schema_drift`
as the net. Add the schema-validate-all-examples gate (WS1d, partial).

**Phase 2 - Generate the reference + executable docs.**
WS1b/1c/1d: rule reference and CLI reference generated and gated; `insta` snapshots for
`--help` and the rules table; `trycmd` over documented invocations and README examples.

*Progress (2026-06-10): each generated rule page now carries a schema-derived `## Options`
table - name, type, required, default, description - sourced from the type-derived
`$defs/rule_<kind>` branch, so option docs flow Rust type -> schema -> alint.org with no
hand-maintained intermediate (the Ruff model). Aliases resolve to their canonical branch;
enums render as `one of ...`; whole-repo kinds advertise no `paths`. Locked by
`committed_schema_every_branch_renders_a_clean_table` in `xtask`. CLI reference and README
invocations are already covered by the existing `trycmd` suite; `insta` snapshots for the
rendered tables remain to add.*

**Phase 3 - The facts contract + alint.org loop.**
WS1e `facts.json`; WS5 site renders from it with a content test and version-pin parity.
Closes the highest-remaining-risk external drift.

*Progress (2026-06-11): WS1e shipped. A committed, gated `facts.json` (`xtask gen-facts`
[`--check`], mirroring `gen-schema`) carries the version, the six headline counts -
including the `families` count the build-time `manifest.json` omitted - and catalogue lists
(rule kinds, families, bundled rulesets, output formats, subcommands, fact predicates).
Every field derives from the same canonical source `coverage_audit_readme_claims` pins the
README to; a test binds `facts.json`'s counts to the README's claimed numbers. Shipped into
the docs bundle next to `manifest.json` for alint.org to consume. The live `manifest.json`
shape is deliberately untouched. Spec: `docs/design/facts-json.md`. WS5 (site renders from
`facts.json` + cross-repo content/version-pin test) remains, in the private alint.org repo.*

**Phase 4 - Architecture as code.**
WS3: `cargo-depgraph` / `cargo-modules` extracted graphs + acyclic gate; the Structurizr
`workspace.dsl` with Mermaid/SVG export and the crate-name consistency gate; arc42
cherry-pick into ARCHITECTURE.md.

*Progress (2026-06-11): shipped, Mermaid-native (no Graphviz/Java/Structurizr in CI).
`xtask gen-arch [--check]` extracts the workspace crate dependency graph from `cargo
metadata` into a committed `docs/design/architecture/crate-graph.md` (Mermaid graph +
crate-by-tier table), content-diff gated like `gen-schema`/`gen-facts`. A hand-modeled C4
`workspace.dsl` (Structurizr) is kept honest by a crate-name consistency gate (model crates
== `cargo metadata` members); an acyclic gate and an `alint-core`-is-a-dependency-sink
layering invariant are unit tests. `ARCHITECTURE.md` gains a Building Block View pointer
(arc42 cherry-pick) linking the graph, the C4 model, and `docs/adr/`. Spec:
`docs/design/architecture-as-code.md`. Deferred (heavy tooling): `cargo-modules` per-crate
internal graphs + `--acyclic` CI flag, and Structurizr `.war` SVG export.*

**Phase 5 - Pragmatic formal methods.**
WS4: Kani job on the pure core; Miri nightly; `contracts` on a few invariants; evaluate a
single Stateright model for cross-file dispatch determinism / LSP cache. This phase is
genuinely optional and can trail the rest.

*Progress (2026-06-11): shipped, scoped to where it pays. (1) **proptest** properties as the
always-on behaviour spec: `normalize_confined` confinement + idempotence + model-agreement,
and the `cross_file` normalisers' idempotence + clean-band shape. (2) A **verified Kani
proof** of the path-confinement security policy (`confine_steps_is_sound`) — a bounded proof
that an absolute component always escapes and a surviving path's depth never exceeds its
`Normal` count, run on a weekly/dispatch CI job (`.github/workflows/kani.yml`, off the PR
path). (3) A **`debug_assert!`** confinement contract. Miri (deferred: `forbid(unsafe_code)`
makes it near-zero ROI), the `contracts` crate (deferred: `debug_assert!` suffices; migrate
to MCP-759 native contracts later), and Stateright (deferred: dispatch is sequential; revisit
for an LSP incremental cache) are documented with rationale. Research-grade verifiers skipped.
Spec: `docs/design/formal-methods.md`.*

Each phase is independently shippable and leaves the tree green. The whole program is mostly
non-user-facing (engine internals, CI, docs pipeline), so it can interleave with feature work
rather than blocking a release; the natural home is an engineering-foundations track
(candidate: v0.13) rather than a user-facing minor.

---

## 7. What we will deliberately NOT do

Stated explicitly so the proposal cannot be read as "adopt everything."

- **No heavyweight deductive verification** (Verus/Creusot/Prusti/Aeneas/hax). Real and
  active, but built for systems/crypto with prohibitive proof-to-code ratios. Not maintainable
  for linter business logic by a small team.
- **No prose-as-regenerable-source.** Design docs, ADRs, and narrative docs stay
  point-in-time. We do not try to regenerate English from a model; that repeats Model-Driven
  Development's failures and stale prose specs mislead confidently.
- **No second spec language as a dependency** (TLA+/Alloy/Quint/P) unless a specific subtle
  invariant demands it, and even then Stateright (Rust-native) is preferred.
- **No archived or fragile tooling.** `clap-markdown` (archived) -> use `clap_mangen`. Avoid
  mtime-based staleness checks (unreliable on fresh CI clones) -> use content-diff. Pin
  Structurizr to the consolidated `structurizr.war`, not the archived CLI.
- **No big-bang rewrite of the existing drift system.** The `coverage_audit_*` tests stay and
  are extended; they are the proof of concept, not technical debt.

---

## 8. Decisions needed from you

1. **Deliverable shape:** is a single living design doc here (this file, evolving) the right
   home, or do you want this split into per-workstream design docs under
   `docs/design/spec-driven/` plus ADRs? (This draft assumes the former until Phase 0.)
2. **ADR home:** `docs/adr/` (zero-friction dogfooding of the shipped ruleset, recommended) vs
   MADR's canonical `docs/decisions/` (needs a three-line path override in `.alint.yml`).
3. **Scope and sequencing:** approve the full program, or start with Phases 0-3 (the
   anti-drift core) and defer WS3/WS4? My recommendation: commit to 0-3, treat 4-5 as
   opt-in.
4. **`facts.json` cross-repo wiring:** are you willing to add a CI step in the (separate,
   private) alint.org repo that consumes `facts.json` and fails on drift? WS5's strongest
   guarantee depends on a check living on the site side.
5. **Formal-methods appetite:** is the single Stateright model (Phase 5) worth a spike, or
   should WS4 stop at Kani + Miri + `contracts`?

---

## Appendix A - Tool inventory

| Tool | Role | License | Maturity (as researched 2026) | Notes |
| --- | --- | --- | --- | --- |
| `schemars` | JSON Schema from Rust types | MIT | v1.x, active (latest early 2026) | Keystone; honors serde attributes |
| `clap_mangen` | CLI man/reference from clap | MIT/Apache | v0.3.x, maintained in clap project | Recommends xtask pattern |
| `insta` | Snapshot testing | Apache-2.0 | v1.4x, very active | Pin generated output |
| `trycmd` | CLI/README conformance | MIT/Apache | maintained | Runs docs as tests |
| `proptest` | Property testing | MIT/Apache | already a dependency | Lean in harder |
| Kani | Bounded model checking | MIT/Apache (AWS) | v0.6x, active; GitHub Action | Pure-core panic/overflow proofs |
| Miri | UB detection | MIT/Apache | mainstream, very active | Run on existing tests |
| `contracts` | Runtime pre/postconditions | MPL-2.0 | maintained | Bridge to native MCP-759 contracts |
| Stateright | Rust-native model checker | MIT | active; liveness experimental | One model, if any |
| MADR 4.0.0 | ADR template | MIT/CC0 | released 2024-09-17 | Dogfood `docs/adr@v1` |
| log4brains | ADR site (optional) | Apache-2.0 | v1.1.0 (2024-12) | Defer unless wanted |
| Structurizr | C4 model + export | Apache-2.0 | CLI archived 2026-02-04 -> `structurizr.war` | Pin consolidated artifact |
| `cargo-modules` | Module graphs + acyclic gate | MPL-2.0 | maintained | Code-extracted, CI gate |
| `cargo-depgraph` | Workspace crate graph | MIT/Apache | maintained | Code-extracted |

All licenses above are compatible with alint's Apache-2.0/MIT posture; run them through
`cargo-deny` as usual before adoption.

## Appendix B - Key sources

Spec-driven development and anti-drift:
- Martin Fowler, "Understanding SDD: Kiro, spec-kit, and Tessl" (the three-rung ladder).
- GitHub Spec Kit; AWS Kiro (requirements/design/tasks).
- Tom Preston-Werner, "Readme Driven Development" (2010).
- "Design Docs at Google" (humans do not keep docs current).
- Isoform, "The Limits of Spec-Driven Development"; Augment, "What SDD gets wrong."
- Ruff contributing docs (`generate-all`: schema + docs + rules from metadata).
- rust-analyzer PR #19315 (fail CI if codegen modifies tracked files); `cargo-xtask`.
- `schemars`, `clap_mangen`, `insta`, `trycmd`, Astro build-time data, shields.io endpoint.

ADRs and architecture diagrams:
- MADR (adr.github.io/madr, v4.0.0); log4brains; joelparkerhenderson/architecture-decision-record.
- C4 model (c4model.com): containers are deployables, not libraries; auto-generate the code level.
- Structurizr DSL + export + EOL/consolidation notice; structurizr-site-generatr.
- GitHub Mermaid rendering; D2/PlantUML/Graphviz trade-offs.
- `cargo-modules` (`--acyclic`), `cargo-depgraph`; arc42 (cherry-pick, esp. Section 9 -> ADRs).

Formal methods for Rust:
- Kani (model-checking; Firecracker, verify-rust-std); Miri (POPL 2026, 100k+ crates).
- Stateright (Rust-native model checker); Quint (modern TLA+) as the alternative if needed.
- Verus/Creusot/Prusti/Flux surveys; std-lib verification coverage reality check.
- MongoDB conformance-checking blog (trace-validation cost vs test-generation).

(Full URLs are retained in the research notes that produced this proposal and can be inlined
into the relevant ADRs as those decisions are recorded.)
