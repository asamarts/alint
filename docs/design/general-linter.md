# What it means for alint to be a general linter

Status: Accepted direction (2026-10-14). Revised after an adversarial review against `main` at
v0.18.2 (section 15); the program-level decisions are taken (section 13.1) and recorded in
[ADR-0019](../adr/0019-general-linter-program-and-plugin-architecture.md). Each phase still
graduates to its own design document before implementation.
Decisions: [ADR-0019](../adr/0019-general-linter-program-and-plugin-architecture.md). Section 13.2
lists the questions each phase's design decides, and section 13.3 lists the existing ADRs and
constitution invariants this program amends or supersedes. ADRs are immutable (constitution 13), so each amendment is a new superseding ADR.
Any public DSL version, new execution model, or plugin ABI must graduate to its own design
document and ADR before implementation.
Demand evidence: the 111-repository v0.12 coverage study and its architecture synthesis
([`v0.12/architecture_synthesis.md`](v0.12/architecture_synthesis.md)), the shipped catalogue
(counts in the generated [`facts.json`](../../facts.json)), the recurring abstractions in
`scope_filter`, `Extract`, `cross_file`, `for_each_*`, facts, and fixers, and the companion
[`rule-coverage-gaps.md`](rule-coverage-gaps.md) analysis. Section 12.3 notes where that
evidence is thin.

> **How this document is used.** This is an arc document, like [`auto-fix.md`](auto-fix.md):
> it sets the direction and the invariants for a multi-release program. It does not follow
> the seven-section [`TEMPLATE.md`](TEMPLATE.md) itself. Each phase in section 11 graduates to
> its own `docs/design/vX.Y/<name>.md` in that template (including the mandatory
> false-positive section) before its code is written, and the gap-to-substrate matrix in
> section 11.8 is the canonical ordering of the work across both documents.

> This document defines the product claim **"A general linter"** precisely, audits how close
> the current architecture is to that claim, and proposes a path to make alint more general
> without turning it into an unbounded programming language, a build system, or a universal
> static analyzer. It is an architectural proposal, not a commitment to the illustrative YAML.

## Contents

- [1. Executive conclusion](#1-executive-conclusion)
- [2. A precise meaning of general](#2-a-precise-meaning-of-general)
- [3. The current generality envelope](#3-the-current-generality-envelope)
- [4. Where the current model stops generalizing](#4-where-the-current-model-stops-generalizing)
- [5. Linting and lint-adjacent uses not fully covered](#5-linting-and-lint-adjacent-uses-not-fully-covered)
- [6. The target conceptual model](#6-the-target-conceptual-model)
- [7. DSL proposal](#7-dsl-proposal)
- [8. Diagnostics, exceptions, and remediation](#8-diagnostics-exceptions-and-remediation)
- [9. Execution and extension architecture](#9-execution-and-extension-architecture)
- [10. What alint should not generalize](#10-what-alint-should-not-generalize)
- [11. Delivery sequence](#11-delivery-sequence)
- [12. Validation and success criteria](#12-validation-and-success-criteria)
- [13. Open decisions](#13-open-decisions)
- [14. Research lessons and references](#14-research-lessons-and-references)
- [15. Review log](#15-review-log)

## 1. Executive conclusion

"General" should describe alint's **composition model**, not claim that alint understands
every artifact or can decide every property.

The defensible product definition is:

> **alint is a general linter for finite, inspectable project state.** It selects resources
> from a reproducible project snapshot, recognizes structure in those resources, asserts
> local or relational constraints, and emits explainable findings and bounded remediations.

That definition is broader than "repository file convention checker" and narrower than
"general-purpose policy engine":

- It includes file trees, structured documents, repository metadata, local Git state and
  history, relations between files, and eventually explicitly supplied snapshot data.
- It includes checks over one file, a set of files, extracted records, graph edges, and a
  change set.
- It requires deterministic, bounded, inspectable evaluation. Network answers, the wall
  clock, package resolution, compilation, runtime behavior, and deep language semantics do
  not silently become part of the declarative core.
- It permits semantic tools at an explicit capability boundary through `command` today and
  a typed, sandboxed plugin interface later. Their results can participate in the same
  finding model without pretending that their analysis is native or hermetic.

alint is already broad. As of v0.18.2 it has 95 canonical kinds across 13 families, eight
structured formats, cross-file relations, Git-aware rules, baselines, 26 fix operations,
nested configs, remote rulesets, eight report formats, and LSP support (the generated
`facts.json` is the authority for these counts; this document does not track them). Its main limitation is not a shortage of kinds. It is
that most behavior is packaged as a new bespoke `kind`, while the reusable operations under
those kinds are only partially exposed.

The highest-leverage direction is therefore:

1. Make **selection, decoding, extraction, normalization, assertion, reporting, and
   remediation** explicit internal stages.
2. Give extracted values typed provenance and make reusable, named relations first-class.
3. Add a bounded `constraint` surface for common quantifiers and integrity constraints.
4. Keep today's specialized kinds as readable, documented shorthand and optimized native
   kernels. Do not force ordinary users to write relational policy for `final_newline`.
5. Improve the policy-authoring loop with fixture tests, evaluation traces, relation
   previews, and strict cardinality/error semantics before exposing more expressive syntax.

This would let one substrate cover version consistency, key parity, reference integrity,
allowlist hygiene, grouped counts, manifest-derived scope, and many future rule requests.
It also avoids a catalogue that grows by one Rust module, schema branch, documentation page,
and bespoke evaluator for every variation of "extract these records and assert this relation."

## 2. A precise meaning of general

### 2.1 Generality has several independent axes

A linter is not general merely because it accepts many filename extensions. Its generality
can be evaluated on at least nine axes:

| Axis | General question | alint today |
|---|---|---|
| Resource | What state can be inspected? | Working-tree files, directories, metadata, selected local Git state/history, environment values, and trusted command output |
| Selection | How are targets chosen? | Globs, ignores, Git-ignore state, some tracked/change filters, ancestor manifests, manifest-derived path sets, and several kind-specific selectors |
| Recognition | How do bytes become facts? | Bytes/text/regex, eight structured formats, JSONPath, lines, whole files, Markdown links, and several specialized scanners |
| Binding | Can matched parts be named and reused? | Regex captures and iteration context in selected kinds; extracted values are mostly private to one rule |
| Assertion | What relationships can be required? | A large closed kind catalogue, scalar `when`, set/equality relations, bounded counts, and one graph kernel |
| Context | Can a rule depend on repository or change context? | Scalar facts, variables, environment values, current branch, tracked paths, blame, diffs, and commits, but through separate surfaces |
| Finding | How precisely can failure be explained? | Rule, severity, message, optional policy URL, one optional path and point location, notes, fixability, baseline identity, and concrete proposed edits where available |
| Remediation | How can a violation be changed? | 26 typed fix operations, applicability tiers, dry-run/diff, composed per-file atomic writes, and fixpoint evaluation |
| Composition | Can policies be reused and extended? | Bundled/local/remote `extends`, SRI, field merging, nested configs, string-substitution templates, and trust gates |

This table matters because the axes should remain separable. Supporting another parser does
not require a more powerful expression language. Adding multi-location findings does not
require network access. A sandboxed plugin need not weaken the deterministic core.

### 2.2 General does not mean universal

There are three useful levels of claim:

1. **Catalogue generality:** many built-in checks cover many ecosystems. alint has this now,
   but it scales linearly with maintainer effort and does not make user-specific policy easy.
2. **Compositional generality:** users combine a small vocabulary of selectors, extractors,
   constraints, and findings to describe new repository policies. This is the recommended
   target.
3. **Computational generality:** users can execute arbitrary algorithms. `command` already
   supplies this at a trusted edge; a WASM component can improve the contract. It should not
   define the safety, predictability, or usability of the declarative core.

The product should pursue level 2 and keep level 3 explicit. A Turing-complete YAML language
would be more computationally general and a worse linter: harder to validate, optimize,
explain, secure, and make deterministic.

### 2.3 The semantic envelope

The declarative core should accept only evaluations with all of these properties:

- **Finite:** every source and quantifier has a bounded input relation.
- **Snapshot-based:** a run identifies the state it inspected. The default is the working
  tree; future Git-index or Git-tree views would be equally explicit.
- **Deterministic:** the same policy, snapshot, declared inputs, and tool version produce the
  same ordered findings and proposed edits.
- **Capability-declared:** filesystem escape, environment access, process spawning, network
  access, and time are absent by default and visible when enabled.
- **Explainable:** a finding can identify the selected resource, extracted evidence, failed
  assertion, and any related locations.
- **Resource-bounded:** reads, parse depth, row counts, joins, graph traversal, plugin fuel,
  and output have enforced limits.
- **Fail-loudly:** unknown fields, missing bindings, type mismatches, cardinality errors,
  unsupported adapters, and truncated evaluation cannot silently become a pass.

These are stronger and more useful boundaries than "only files" or "no ASTs." A concrete
syntax tree obtained from a bounded parser can fit the envelope; a package-version answer
fetched from a registry cannot fit unless the response is supplied as an explicit input.

## 3. The current generality envelope

### 3.1 What is already genuinely general

Several current designs are reusable foundations, not isolated features:

- The `FileIndex` makes the repository a common finite model instead of making every rule
  walk the filesystem independently.
- `PathsSpec`, `Scope`, global ignores, Git-ignore handling, and `scope_filter` express a
  substantial selection algebra.
- `ExtractSpec` already normalizes eight structured formats, line lists, regex captures, and
  whole-file content behind one interface.
- `cross_file` separates extraction, normalization, and relations such as equality, subset,
  superset, set equality, identity, resolution, and registration.
- `for_each_dir`, `for_each_file`, `for_each_match`, and `every_matching_has` introduce
  quantification and nested assertions.
- Facts plus the bounded `when` grammar provide conditional policy without arbitrary code.
- The rule registry and common `RuleSpec` support a shared lifecycle across very different
  evaluators.
- The native fix algebra is typed, tiered, previewable, composition-aware, and checked to a
  fixpoint (bounded at ten passes; a fix that has not settled is reported, not looped). Native
  path edits are root-confined by default (only a top-level, never-inherited
  `allow_out_of_root` grant lifts that), content rewrites have a configurable size cap, and
  spawning operations are explicitly privileged because their side effects cannot be confined
  by that algebra.
- Baselines turn findings into stable identities and support incremental adoption without
  making a rule stop protecting new code.
- `extends`, templates, nested configs, bundled rulesets, mandatory SRI pinning for remote
  rulesets, and trust gates make policy reusable across teams while distinguishing trusted
  top-level choices from inherited data. Since v0.18.0 the gates judge the rule that actually
  runs, and untrusted remote rulesets lose `ignore:` and environment access.

The rule-coverage study correctly identifies the strongest existing abstraction: much of
repository linting is schema-on-read over a finite database, followed by familiar integrity
constraints. That insight should become an engine model rather than remain private to a few
cross-file kinds.

### 3.2 The current effective pipeline

Today, a rule usually implements several stages inside one Rust type:

```text
repository walk
    -> rule-specific target matching
    -> rule-specific reads and parsing
    -> rule-specific extraction and normalization
    -> rule-specific assertion
    -> Violation { path?, line?, column?, message }
    -> optional kind-compatible Fixer
```

The engine shares the walk and coalesces reads for `PerFileRule`. The `Rule` trait exposes
scheduling metadata (path scope, full-index need, Git-tracked mode, blame need, fixer, scope
assertions), but extraction, normalization, and assertion are opaque inside
`evaluate` / `evaluate_file`, so the engine cannot see, share, or optimize those stages. Similar
operations consequently have multiple public spellings and multiple implementations.

### 3.3 Concrete signs that an internal algebra already exists

The recurring concepts are visible in the source:

- **Selection** appears as `paths`, global `ignore`, `respect_gitignore`, kind-specific
  `git_tracked_only`, `scope_filter`, `--changed`, `changed_since`, `primary`, `select`, the
  selector fields of the `for_each_*` kinds, and the change selectors of the Git kinds
  (`since:`, `if_changed` / `then_changed`, `when_changed`).
- **Extraction** appears in facts, the shared `ExtractSpec`, structured-query kinds,
  Markdown scanners, import scanners, `for_each_match`, manifest-derived scope, and Git
  parsers.
- **Cardinality** appears as `expect_matches`, `expect_nonempty`, exactly-one requirements in
  `cross_file`, existence kinds, thresholds, and selector-specific empty-set behavior.
- **Relations** appear as pair existence, equality, set relations, registration, uniqueness,
  graph edges, ordered blocks, and generated-file freshness.
- **Iteration bindings** appear as regex captures, path templates, `iter.path`,
  `iter.basename`, and `iter.has_file`.
- **Exceptions** appear as path exclusions, allow lists on selected kinds, baselines, and
  bespoke tolerance options such as `allow_missing_target`.

The repetition is not evidence that these features were mistakes. Each was a practical,
tested step toward the latent model. It is evidence that the next step should consolidate
the model before adding many more surface variations.

### 3.4 Conclusions already reached in v0.12

This proposal continues the v0.12 architecture synthesis rather than starting fresh, and it
must stay consistent with what that work decided:

- **"Compose, don't accrete"** ([`v0.12/architecture_synthesis.md`](v0.12/architecture_synthesis.md)):
  cross-file capability is a composition of five shared axes (extract, normalize, relation,
  quantifier/scope, graph). This document's stage model is the same idea taken one level
  down, into the engine.
- **"Relation: essentially complete. Don't add."**
  ([`v0.12/post_build_coverage_analysis.md`](v0.12/post_build_coverage_analysis.md) section 6):
  that verdict was about the comparison operators over extracted *value lists*, and it still
  holds. What this proposal adds is not new comparison operators. It is the parts the v0.12
  model cannot express: enumerating object keys, grouping rows, counting, and reusing one
  extraction across rules.
- **The `file_set` source was dropped for lack of demand**
  ([`v0.12/cross_file.md`](v0.12/cross_file.md)): not one of the 111 repositories asked for
  glob-against-glob path-set parity. Named selectors (section 7.2) must therefore be justified
  by reuse and consistency, not by that use case.
- **A named `path_sets:` block was deferred, not rejected**
  ([ADR-0010](../adr/0010-manifest-derived-rule-scope.md), considered options): it is the
  direct ancestor of named selectors, and question Q1 in section 13.2 decides the two together.

## 4. Where the current model stops generalizing

### 4.1 Rule kind is doing too many jobs

`kind` currently chooses all of these at once:

- target domain and dispatch class;
- supported selectors and scope behavior;
- parser or scanner;
- extracted value shape;
- assertion semantics and empty-set behavior;
- diagnostic location and message construction;
- eligible fixes;
- Git, metadata, or command capabilities.

That makes a kind easy to use but expensive to extend. A small variation often needs a new
kind even when most of its mechanics already exist. Conversely, a maximally configurable
mega-kind would be difficult to document and would produce worse errors. The solution is a
shared internal algebra plus a carefully bounded public generic surface, not either extreme.

### 4.2 Selection is fragmented and dispatch-shaped

`paths` is universal-looking but not universal in semantics. Some kinds enumerate files,
some select directories, some use `source`/`targets`/`primary`, and some inspect the full
index. `scope_filter` has three behaviours, not two: it applies to per-file rules; it is
rejected at build time by the cross-file and existence kinds and by fixed-target kinds such
as `no_submodules`; and it is **accepted and silently ignored** by kinds that never read it
(the `git_commit_*` family, `git_no_denied_paths`, `generated_file_fresh`,
`command_idempotent`). That last behaviour is a live silent drop (section 4.12). Git-tracked
selection is kind-specific by decision: [ADR-0008](../adr/0008-git-tracked-only-is-a-kind-option.md)
rejected a universal option because kinds that do not honour it fail quietly. CLI changed-mode
and rule-level `changed_since` overlap but have different purposes.

This creates three problems:

1. Users must learn which selection spelling each kind accepts.
2. The same logical set cannot be named and reused by several rules.
3. Cross-file rules need the full repository **as evaluation context**, but that should not
   prevent them from applying an explicit selector to the rows they compare. Full-index
   correctness and rule target selection are different concepts. Today they are fused by
   constitution invariant 2 and [ADR-0003](../adr/0003-rule-engine-dispatch-and-determinism.md):
   a full-index rule must have no path scope, which is what keeps `--changed` from skipping
   it. Separating the two therefore amends that invariant (section 9.1 and question Q2).

### 4.3 Extracted data is string-only, lossy, and private

The shared extractor is valuable but intentionally narrow:

- structured extraction returns only string-valued JSONPath matches;
- non-string nodes are dropped;
- computed-looking values are often skipped by consumers;
- duplicate keys and source spans can disappear during parse-to-`serde_json::Value`: JSON,
  YAML, dotenv, and properties keep the last duplicate, TOML and HCL reject the file, and XML
  and INI collect duplicates into an array that the string-only extractor then drops entirely.
  The spanned parsers exist (the v0.17 fix bridge uses `jsonc-parser`, `toml_edit`, and
  hand-written YAML and INI locators; `adotenv` keeps every `.env` entry with spans since
  v0.18.2), but extraction flattens them away;
- path, key, and entry enumeration need custom code;
- each consumer owns its extracted set rather than publishing a reusable relation;
- cardinality and skip behavior vary by caller: `cross_file` and `registry_paths_resolve`
  report computed-looking values as notes, while `file_graph` and manifest-derived scope drop
  them silently, and a manifest that fails to parse yields an empty member set (an include
  rule warns only through `expect_nonempty`; an exclude rule silently falls back to full
  scope).

A generic constraint system built directly on this shape would amplify silent false
negatives. Provenance, types, explicit cardinality, and explicit error/skip policy are
prerequisites, not later polish.

### 4.4 Facts and conditions are not a general data model

Facts are scalar `Bool`, `Int`, or `String` values, evaluated once per `check` run (and again
on each `fix` pass). They are appropriate for
gating a rule but cannot represent a set of packages, dependency rows, path-to-owner
mappings, or graph edges. `when` consequently answers "should this rule run?" but not "for
which bound records does this assertion fail?"

The expression language should remain bounded. The missing feature is not arbitrary
functions or arithmetic. It is a first-class finite collection model with structural
quantifiers supplied by the host.

### 4.5 Iteration is powerful but split across kinds

The four iteration-style families bind different things and expose different nested syntax.
They cannot uniformly say:

- for each parsed object entry;
- for each group of records sharing a key;
- for every source record, require a matching target record;
- report records that have no counterpart;
- require all groups to have the same key set;
- count distinct values per group;
- ensure every exception entry matched a real would-be violation.

These are finite quantifiers over extracted relations. Encoding each as a new `for_each_*`
kind would repeat the current pattern.

### 4.6 Findings lose relational evidence

`Violation` has one optional path and one point location. A cross-file mismatch naturally
has at least two locations: the canonical declaration and the drifted declaration. A missing
reference has the declaration site plus the absent target. A uniqueness failure has every
colliding record. Today the message must serialize much of that evidence into prose.

This limits:

- human explanations and editor navigation;
- SARIF `relatedLocations` support;
- stable structural fingerprints;
- precise generic fixes;
- machine consumers that need to distinguish expected, actual, and related evidence.

### 4.7 Templates are textual rather than typed

Templates remove repetition, but instance variables are effectively string substitution:
numbers and booleans are converted to strings, list, map, and null values are dropped without
an error (so their placeholders stay literal), missing placeholders remain literal, chained
templates are forbidden, and templates declare no parameter schema. Since v0.18.0,
`{{vars.*}}` is also forbidden in `kind:` and in fix `applicability:`, because a
trust-relevant field must be literal for the gates to judge it. This is safe and simple
for path/message variation, but weak as the composition layer for reusable generalized
policies.

The next model needs typed parameters, required/default values, unused-parameter checks, and
reference validation. It does not need template inheritance or arbitrary macro expansion.

### 4.8 Extension points divide into shell-or-core

The `command` kind is computationally general per selected file but process-spawning,
trust-sensitive, difficult to cache,
and constrained by an exit-code/text protocol. Built-in Rust kinds get rich access but must
ship with alint. The proposed WASM tier is the right middle, but its most important design
question is not merely sandboxing. It needs a stable typed interface for selectors,
resources, findings, locations, and edits so plugins compose with the same engine rather
than becoming opaque commands in another format.

### 4.9 The input view is implicit

Most evaluation reads the working tree. `--changed` narrows paths using Git, while some Git
rules inspect commits and the LSP overlays editor content in a specialized path. A genuinely
general project-state linter should eventually name the inspected view:

- working tree;
- Git index (staged snapshot);
- a committed Git tree;
- working tree plus in-memory editor overlays.

This would make pre-commit checks independent of unrelated unstaged changes and make LSP and
CLI semantics converge. It is a substantial engine project, not a CLI flag to bolt onto the
current walker.

### 4.10 Purity and external inputs are not modeled explicitly

Most built-in checks are pure over the repository snapshot, but the complete product already
has several other dependency classes:

- `when: env.*` and config-wide `{{env.X}}` interpolation read the host environment (untrusted
  remote rulesets have been refused both since v0.18.0);
- `custom` facts, `command`, `generated_file_fresh`, `command_idempotent`, and spawning fix ops
  launch processes;
- Git-aware rules inspect repository metadata beyond walked files;
- `git_commit_gpg_signed` depends on the host's GPG keyring through `git verify-commit`;
- `git_blame_age` currently compares commit timestamps with `SystemTime::now()`;
- HTTPS `extends` resolves remote content before evaluation, with mandatory SRI pinning
  (`#sha256-...`), an SRI-keyed cache, fetch timeouts, and a body-size cap.

The trust gates cover the most dangerous operations, but the evaluation plan and report do
not describe the resulting reproducibility class. In particular, the implicit wall-clock read
means the strict snapshot-determinism definition in section 2.3 is a target invariant, not an
accurate description of every current kind.

Failure is also not typed uniformly at these boundaries. A failed, timed-out, non-zero, or
non-UTF-8 `custom` fact becomes an empty string; an unavailable or detached `git_branch` does the
same, and the Git-aware rule kinds (`pair_changed_together`, `changeset_requires_path`, the
commit-range kinds) quietly do nothing when `git` or the repository is unavailable. That
behavior is compatible with v1's scalar gate model, but a generalized plan must not confuse
“provider failed,” “value is absent,” and “provider successfully returned an empty string.”

Each plan node should therefore declare one of: hermetic snapshot input, local Git input,
declared host input, or privileged effect. Environment variables must be enumerated in the
plan; time-sensitive policy should receive an explicit `--as-of` value (with a documented
CLI/CI source, decision DEC-11); process and future network nodes remain
top-level-capability-gated. Constitution invariant 1 (byte-identical output for the same input)
is already broken by `git_blame_age`'s wall-clock read and the GPG keyring dependency, so this
is a correction to make, not only a target: either the invariant gains a declared-host-input
exception or those kinds take explicit inputs, and either way through a superseding ADR. Caches, policy
tests, snapshot compatibility, and `explain --trace` can then use the real dependency set
instead of assuming all rules are equivalent.

### 4.11 Multi-file remediation is ordered, not transactional

Current fixes deliberately preserve config order, compose content edits per file, flush each
file atomically, and re-evaluate to a fixpoint. A fix that changes several files is not one
filesystem transaction: a later write or external command can fail after earlier edits landed.
That is honest and practical, but generic relational fixes will make multi-resource plans more
common and increase the cost of an implicit partial result.

The generalized edit plan should validate all derivations, confinement, expected source hashes,
overlaps, and applicability before its first write. It must then report per-edit outcomes and
remain idempotently resumable if application stops partway through. It should not promise
cross-file atomicity unless a future implementation can actually provide rollback with correct
filesystem and process semantics.

This is not a new idea: [`auto-fix.md`](auto-fix.md) section 5.2.4 and
[ADR-0017](../adr/0017-auto-fix-edit-model-and-applicability.md) designed all-or-nothing
application through verification, and it was explicitly deferred during v0.17 in favour of per-file atomic writes plus convergent
re-runs (see [`v0.17/auto-fix-completion-plan.md`](v0.17/auto-fix-completion-plan.md)). The generalized edit
plan should pick up that deferred design, not restart it, and only in the phase that first
enables generic multi-resource fixes (section 11).

### 4.12 Current fail-open and leak sites

The review that revised this document checked the "fail-loudly" envelope of section 2.3 against
the v0.18.2 code. These sites violate it today. They are concrete Phase 1a work (section 11.3),
and several can be fixed on their own before any IR exists:

| Site | Current behaviour | Consequence |
|---|---|---|
| Rule evaluation error | an `Err` from `evaluate` / `evaluate_file` becomes `Violation::new("rule error: ...")` at the rule's own `level`, with no path | a `level: warning` rule that cannot evaluate exits 0 without `--fail-on-warning`; the pathless violation fingerprints on its message, so `alint baseline` can grandfather a broken rule permanently |
| Notes in CI formats | notes reach `json`, the LSP, and `human` (a count on stderr; the list only with `--show-notes`); `sarif`, `github`, `gitlab`, `junit`, `markdown`, and `agent` drop them | anything routed to "note" (computed values skipped, incomplete analysis) is invisible in GitHub code scanning and PR comments |
| Extracted values in output | structured-query messages render the actual value ("expected X, got Y"), and the same value enters the `baseline_key` and the advisory message stored in the baseline file | `dotenv_path_equals` on `.env` can echo a secret into terminal, JSON, SARIF uploads, PR comments, and a committed `.alint-baseline.json` |
| `scope_filter` on kinds that ignore it | accepted and silently ignored (section 4.2); `include_manifest_paths` on such a rule passes `validate-config` and fails only at `check` time | a user believes a Git or spawning rule is scoped when it is not |
| Manifest-derived scope parse failure | empty member set; include rules warn only via `expect_nonempty`, exclude rules silently fall back to full scope | silent scope change on a malformed manifest |
| Git unavailable | Git-aware rule kinds return no findings | a CI image without `git`, or a shallow export, passes every change-set rule |
| Non-string extraction results | dropped by the string-only extractor | a typed value (number, boolean, array) at a queried path cannot fail a `cross_file` relation |

Two related facts belong next to these. JSON Schema `pattern` keywords in `json_schema_passes`
run on `fancy-regex` (pulled in by `jsonschema`), so they support backreferences and
lookaround, bounded by fancy-regex's default backtrack limit of 1,000,000 steps; hitting the
limit surfaces as an ordinary schema-validation finding, indistinguishable from a real schema
failure. And the
rule-error row interacts with exit codes: today 0 is pass, 1 is violations, 2 is config or
usage error, 3 is internal error. Decision DEC-2 chooses how incomplete analysis maps onto them.

## 5. Linting and lint-adjacent uses not fully covered

The detailed missing-rule catalogue remains in
[`rule-coverage-gaps.md`](rule-coverage-gaps.md). The following are broader behavior classes
that cut across many individual kinds.

### 5.1 Relational integrity over discovered records

Highest priority. Examples include:

- all package manifests use one version for a dependency;
- locale files expose the same key set;
- every declared output has a source and every generated source is registered;
- identifiers are unique across a family of documents;
- every owner/reference/registry entry resolves;
- two groups are equal, disjoint, subsets, or have matching cardinality;
- exactly one record in a group has a distinguished role;
- graph edges satisfy a boundary or reachability property.

Some are expressible by `cross_file`, `registry_paths_resolve`, `unique_by`, or
`file_graph` when their input shape matches. The general gap is applying a small constraint
vocabulary to user-defined, dynamically extracted relations.

### 5.2 Embedded and virtual documents

Repositories contain parseable regions inside other files: YAML front matter in Markdown,
fenced configuration examples, XML properties embedded in project files, notebook cells,
and generated regions delimited by markers. Today each needs a specialized scanner or a
whole-file regex.

A bounded adapter should be able to emit virtual resources with parent provenance. This is
also how an LSP buffer or a parser plugin should enter the engine. Archive expansion,
unbounded recursive embedding, and executing notebook cells remain out of scope.

### 5.3 Change-set and transition policy

Path-level co-change already has native kernels: `pair_changed_together` (if X changed, Y must
change) and `changeset_requires_path` (the diff must add a matching file) cover "the changelog
must change" and "a registry and its members change together" at the level of paths. What they
do not cover is content-level and rename-aware transition assertions:

- a public declaration changed, so a changelog or migration file must change;
- an exception set may shrink but not grow without an approval marker;
- a generated registry and its members must change together;
- a file was renamed rather than added/removed independently, using explicit Git rename data or
  a documented similarity threshold rather than pretending a rename is intrinsic filesystem
  state;
- a version advanced, so every release artifact must advance consistently;
- a policy became stricter or looser between two ruleset revisions.

This needs a first-class finite **change relation** (`status`, old path/value, new
path/value), not more string predicates over `git diff --name-only`. It must be evaluated
against an explicit base and snapshot.

### 5.4 Aggregates, budgets, and ratchets

Fixed thresholds exist, and baselines gate new findings, but alint does not expose a general
way to assert:

- at most N records per group;
- exactly one matching declaration;
- equal counts between groups;
- no increase relative to a supplied baseline snapshot;
- maximum ratio or percentage, with explicit rounding;
- top-N evidence for a budget breach.

Counts and distinct counts over finite relations are safe additions. Trend dashboards and
time-series storage are not linter responsibilities; a comparison must receive both finite
snapshots explicitly. Threshold ratchets against a baseline are a recorded v1 limitation of
baseline mode ([`baseline.md`](baseline.md) section 7, its question 8; ADR-0006), so "no increase relative
to a baseline" reopens that decision and needs its own superseding design.

### 5.5 Exception and waiver hygiene

The baseline solves legacy-debt adoption. It is not a full exception model. Real repositories
also have reviewed allowlists and per-policy exceptions that should be:

- narrow and tied to the rule or record they exempt;
- accompanied by owner/reason metadata where required;
- rejected when unused or stale;
- optionally time-limited against an explicit `--as-of` input, never an implicit wall clock;
- visible in reports rather than silently filtered from evaluation.

This should **not** be a generic `suppress:` escape hatch in inherited config. Two separate
existing rules apply. [ADR-0006](../adr/0006-baseline-suppression.md) rejected config-embedded
suppressions even in top-level config, because they mix hand-edited policy with a
machine-regenerated debt snapshot and tempt per-line entries that drift. Separately, ADR-0006
makes the baseline a trusted top-level input that `extends:` and nested configs can never
supply. The safer abstraction is an explicit exception relation owned by the top-level policy,
joined against would-be findings, with unmatched exception rows themselves producing findings.
That stale check is the answer to ADR-0006's drift objection, but it is still a reversal of a
recorded decision and needs a superseding ADR (question Q3). Existing rule-specific allowlists
can migrate to this model internally.

Two further constraints apply. ADR-0006 makes stale baseline entries warn by default and fail
only under `--strict-baseline`; the exception model must choose its own default deliberately
rather than inherit "stale is a finding" by accident. And "owned by the top-level policy" does
not stop a pull request from adding its own exception to an in-repository file. Until change
relations exist (section 5.3), the mitigation is ownership (CODEOWNERS on the exception file) or
keeping exceptions inside the top-level config file.

### 5.6 Policy conformance and policy-as-source-of-truth

alint often restates policy already declared in `.editorconfig`, `.gitattributes`, workspace
manifests, release metadata, CODEOWNERS, or dependency-update config. The next general step is
to consume these files as policy inputs and apply their declarations, rather than duplicate
them as literals in `.alint.yml`.

This is different from arbitrary configuration execution. Each supported policy adapter
needs documented semantics, provenance, conflict handling, and a fail-loudly mode. The
`editorconfig_conforms` proposal in the gap study is the flagship case.

It also crosses a recorded boundary. [ADR-0010](../adr/0010-manifest-derived-rule-scope.md)
states that "manifest VALUES may gate WHICH files a rule sees, never WHAT the rule decides
about a file". An `.editorconfig` value such as `indent_size = 4` decides exactly that. The
distinction worth making is between a *build manifest* read to infer intent (ADR-0010's
subject) and a *policy-source file* whose documented purpose is to declare the policy being
checked. Decision DEC-12 chooses whether policy-source files get their own ADR or
`editorconfig_conforms` is restricted to parametrizing existing kinds' options.

### 5.7 Syntax-aware but language-light linting

Regex remains the right default for text. Some useful checks need syntax structure but not
types, control flow, compilation, or package resolution: matching an import node, a call
shape, a front-matter field, or a Dockerfile instruction.

Tree-sitter-style query adapters or a syntax-aware WASM plugin can emit captures with spans.
alint should not bundle a growing grammar zoo in the core binary. Grammar identity, version,
hash, resource limits, parse-error handling, and query semantics must be explicit. Semantic
AST/type/dataflow analysis remains delegated to language tools.

The documents disagreed on this line before the 2026-10-14 revision: the README says alint is
"not a code or AST linter", the gap study marked anything needing an AST as out of scope, and
this section proposed syntax adapters. Decision DEC-10 settles it: **by default the core ships a
reasonable set of data and document format adapters (the eight config formats, CommonMark through
`pulldown-cmark`, and planned tabular and front-matter adapters) and no programming-language
syntax, and every format, built-in included, sits behind one public adapter contract, so other
repositories, users, and developers can add and enable further adapters, syntax adapters
included (section 9.4).** Type, control-flow, and dataflow analysis stay out of scope in the
core in every case. Under that line, Markdown structure (headings, list nesting, image alt text)
is in scope today, because the shipped CommonMark parser already produces it, and the README's
"not a code or AST linter" remains true of the core.

### 5.8 Policy authoring, testing, and debugging

The repository has excellent internal firing/silent scenarios, but downstream policy authors
cannot define executable fixture tests for their own rulesets. A general DSL without a test
surface would make policy more expressive and less trustworthy.

Needed lint-adjacent behavior:

- `alint test` over tiny fixture repositories with expected findings and edits;
- `alint explain <id> --trace` showing gates, selectors, cardinalities, and failed assertions;
- relation/extraction previews with provenance and truncation markers;
- a policy impact command comparing findings under two configs;
- static checks for unused selectors, relations, template parameters, and exception entries;
- deterministic profiling by stage, not only by opaque rule.

These authoring capabilities should precede the public general constraint DSL.

### 5.9 Snapshot and editor operation

First-class snapshot views would support:

- staged-only pre-commit linting;
- checking a commit or pull-request merge base without checking it out;
- consistent cross-file LSP evaluation with unsaved buffers;
- cache keys based on policy plus content identity;
- accurate impact analysis between two snapshots.

Watch mode, daemonization, and incremental recomputation are optimizations built on this
model. They should follow a correct dependency graph rather than introduce a second set of
evaluation semantics.

### 5.10 Aggregation of external tools

It is tempting to make a "general linter" mean one command that runs and merges every other
linter. alint should remain an evaluator, not become a build orchestrator. The existing
report-layer integration decision in ADR-0018 is the right boundary:

- explicit `command` or plugin rules may delegate a particular assertion;
- machine report formats let a CI wrapper merge results;
- alint should not own arbitrary task graphs, tool installation, dependency resolution, or
  every host linter's cache and exit semantics.

## 6. The target conceptual model

### 6.1 A typed finite dataflow

The target engine model is:

```text
Snapshot
  -> Resources
  -> Selectors
  -> Adapters / decoders
  -> Extractors
  -> Typed relations with provenance
  -> Constraints / specialized kernels
  -> Findings with primary + related evidence
  -> Optional bounded edit proposals
```

Each stage has one job:

- **Snapshot** identifies the complete state under evaluation.
- **Resource** represents a file, directory, symlink, Git object, change, virtual document,
  or explicitly supplied plugin record.
- **Selector** chooses resources without discarding the full evaluation context.
- **Adapter** recognizes a format and yields a navigable document or capture stream.
- **Extractor** emits typed rows.
- **Relation** is a named, finite, ordered multiset of rows carrying provenance.
- **Constraint** evaluates a declared property over one or more relations.
- **Finding** records why the property failed and where its evidence lives.
- **Remediation** proposes edits derived from explicit source provenance.

This is an internal architecture first. Existing kinds can compile to parts of it where that
improves consistency, while specialized rules can stay native when they have distinct
algorithms. `file_graph.acyclic`, byte-level Unicode checks, and Git signature verification
are examples of useful kernels, not failures of abstraction.

### 6.2 The relation and provenance model

A row should support at least these value types:

- `null`, boolean, integer, string, bytes;
- normalized repository `path`;
- lists and records with a statically known shape;
- tagged domain scalars where semantics justify them, initially `semver` and perhaps SPDX
  identifiers, but not a proliferation of nominal types.

Every emitted value should retain:

- resource identity and snapshot identity;
- byte range and line/column range when known;
- structured pointer or capture name when known;
- adapter/extractor identity;
- original value plus normalized value;
- derivation links when a transform or path mapping produced it.

Relations are multisets by default. Converting to a set must be explicit because duplicate
rows are often the violation being sought. Ordering is canonical for deterministic output
but is not semantic unless a constraint declares that order matters. Missing, explicit `null`,
and an extraction error are distinct states; none may be collapsed into another by a convenient
coercion.

Three details must be fixed before any of this is public:

- **Numbers keep their source lexeme.** The list above has integers but no decimals, and the
  formats disagree: YAML reads `version: 1.10` as the float 1.1, while TOML and JSON have
  their own number grammars. A `number` value therefore carries its original lexeme, and
  comparison uses the lexeme unless the policy declares a numeric transform. Otherwise the
  model would commit the silent coercion section 4.3 warns about.
- **Paths are compared byte-exactly after normalization.** Path values in relations are
  forward-slash, root-relative, with `./`, `..`, and trailing slashes resolved the way
  manifest-derived scope already resolves them. Case folding is opt-in per join, never
  dependent on the host operating system. A symlink is its own resource type and is never
  followed for relation membership. The cross-platform test class (case-insensitive file
  systems, Windows separators) applies to every join.
- **Canonical order is defined, not implied.** Rows, related locations, and groups sort by
  (normalized path bytes, byte offset, structural pointer, label). Every truncation ("top N",
  "and N more") cuts after that sort, so it is deterministic.

Provenance is also a memory question at a million files. The default should be eager only for
(resource id, structural pointer); byte and line ranges are resolved lazily, for failing rows
only, through the span resolvers the v0.17 fix bridge already has for every span-capable
format (`structured_fix::resolve_value_span`). Section 9.6 sets the budget this must meet.

### 6.3 Cardinality and absence are types of outcome

Current consumers differ on whether missing files, no matches, non-string values, parse
errors, non-literal values, and ambiguous multiple values are silent, tolerated, warnings,
or violations. A general layer must make these choices visible.

Every extractor should declare or infer cardinality:

- `one`;
- `zero_or_one`;
- `one_or_more`;
- `many`.

And every fallible stage should have a small, common policy vocabulary:

- `error`: configuration/evaluation cannot establish a valid verdict and the command returns its
  operational/configuration-error exit class, not a policy-violation exit;
- `finding`: emit a policy finding at the source;
- `note`: surface incomplete analysis without failing;
- `skip`: permitted only when explicitly authored and visible in an explanation trace.

There must be no default where a parse failure or type mismatch becomes an empty relation and
therefore a passing universal assertion.

**Current behaviour and migration.** Today none of these outcomes exists as a type. A rule that
cannot evaluate emits an ordinary violation at its own severity (section 4.12), and v1 exit codes
are 0 (pass), 1 (violations), 2 (config or usage error), and 3 (internal error). Introducing the
vocabulary above therefore changes observable pass/fail behaviour, which is why it ships first,
in Phase 1a, as a deliberate and announced minor-version change (decision DEC-2), before anything
builds on it. The rule for section 12.1's stability criterion is: pass/fail stays stable except
where v1 produced a false pass. Two constraints follow from current mechanics: an analysis
failure must never be baselinable (today a pathless "rule error" can be grandfathered), and its
failing effect must not depend on the rule's `level`.

### 6.4 Constraints, not general code

The first generic assertion vocabulary should be deliberately small:

- cardinality: `empty`, `nonempty`, `count`, `count_distinct`, `exactly_one`;
- row predicates: equality/inequality, regex, membership, comparison, path resolution;
- keys: `unique`, functional dependency (`key -> values`);
- references: inclusion/foreign-key existence, optional reverse/orphan check;
- sets: equals, subset, superset, disjoint;
- grouping: apply a constraint per declared key;
- ordering: sorted, contiguous, before/after;
- graph hooks: no dangling nodes, forbidden edges, acyclic, and reachability through an
  explicit specialized graph kernel.

The engine should not infer constraints, prove implication between policies, execute recursive
user rules, or accept arbitrary joins. Joins should be equality joins over declared fields,
with row and output limits. Cross products require an explicit operator and a stricter cap rather
than emerging accidentally from an omitted key. Negation must be stratified over already finite
relations. This preserves a predictable cost model and avoids order-dependent results.

### 6.5 Specialized kinds remain the primary ergonomic surface

The generic layer should not replace:

- familiar one-line kinds such as `file_exists`, `final_newline`, or `filename_case`;
- purpose-built kinds with domain-specific error messages and safer fixers;
- optimized scanners for Unicode, encodings, metadata, Markdown, or Git;
- graph algorithms that are clearer as named operations.

Instead, specialized kinds become one or more of:

- sugar compiled to the shared plan;
- native constraints over shared relations;
- native adapters feeding the general relation model;
- optimized kernels with the same finding/provenance contract.

This keeps common policy discoverable while making the uncommon combinations possible.

## 7. DSL proposal

### 7.1 Design rules for any v2 surface

The public syntax should follow these rules:

1. Common cases stay shorter than their generic equivalents.
2. A referenced selector/relation/field is statically plan-checked before walking the repository;
   JSON Schema alone is not expected to resolve cross-references.
3. Types and cardinality are validated before evaluation where possible.
4. No value is silently stringified, dropped, or coerced across unrelated types.
5. Quantification is visible in YAML structure, not hidden in an expression.
6. Expressions operate on one bound row/group and remain non-Turing-complete.
7. Evaluation cost can be estimated from the plan and bounded.
8. Every generic finding has deterministic evidence and a stable identity.
9. Remote rulesets cannot acquire capabilities by indirection through templates, relations,
   adapters, or fixes. Today trust is enforced by per-rule markers that survive merging
   (provenance and environment markers in `alint-dsl`'s trust module). Named top-level objects
   are a new indirection that those markers do not cover, so every named selector, relation,
   input, and module carries its own provenance; references resolve by namespace; a local name
   can never be shadowed by an inherited one; and the planner checks capability monotonicity
   (a rule never gains a capability through what it references).
10. v1 configs remain supported. A v2 surface must not reinterpret existing keys.
11. Trust-relevant fields stay literal. The v0.18 rule that `kind:` and fix `applicability:`
    cannot come from `{{vars.*}}` extends to every v2 field the gates judge (severity is not
    trust-relevant, but kinds, capabilities, inputs, and exception sources are).
12. Interpolated globs are escaped. A parameter or variable spliced into a glob is literal
    text, following the v0.18 glob-escaping decision; a parameter that is meant to be a
    pattern must be typed as one.

### 7.2 Named selectors

Illustrative syntax, not yet a schema:

```yaml
version: 2

inputs:
  base-sha:
    env: ALINT_BASE_SHA
    type: git_ref

selectors:
  package-manifests:
    resources: files
    paths: "packages/*/package.json"
    where:
      tracked: true

  rust-sources:
    resources: files
    paths: "**/*.rs"
    where:
      has_ancestor: Cargo.toml
      changed_since: { input: base-sha }
```

A selector has one target domain and AND-composed predicates. Selectors can be referenced by
per-file or cross-file rules; a cross-file rule still receives the full snapshot as context.
Global ignores and root confinement remain outside user-overridable selector logic.

The initial predicate set should consolidate existing behavior only: path globs, resource
type, tracked status, changed status, ancestor manifest, and manifest-derived membership.
Content predicates belong after decoding, not in the resource selector.

Two existing decisions constrain this sketch. A universal `tracked` predicate is what
[ADR-0008](../adr/0008-git-tracked-only-is-a-kind-option.md) rejected, because kinds that do not
honour a universal option fail quietly. A v2 selector predicate is acceptable only if the engine
implements it for every resource domain (and rejects it where it cannot), never as an option a
kind may ignore; otherwise it supersedes ADR-0008 with the same flaw. And `inputs:` is a second
pathway to the host environment beside `when: env.*` and `{{env.X}}`, both already denied to
untrusted remote rulesets. It is therefore declarable only in the top-level config.

### 7.3 Named relations

The key new construct is a named extraction plan. For example, dependency consistency could
be described conceptually as:

```yaml
relations:
  dependency-pins:
    from: { selector: package-manifests }
    decode: json
    bind:
      package: { json: "$.name", cardinality: one }
    emit_each:
      at: { json_entries: "$.dependencies" }
      fields:
        package: { bound: package }
        dependency: { entry: key }
        version: { entry: value }
```

`json_entries` is intentionally shown as a proposed extractor rather than pretending
JSONPath returns object keys. `emit_each` makes it structural that `dependency` and `version`
come from the same entry; the compiled plan does not rely on coincidental array position.

Path-derived fields should be equally explicit:

```yaml
relations:
  locale-keys:
    from:
      files: "locales/{locale}.json"
    decode: json
    emit_each:
      at: { json_entries: "$" }
      fields:
        locale: { path_capture: locale }
        key: { entry: key }
        value: { entry: value }
```

The exact syntax requires a dedicated design pass. The semantic requirements are more
important than these names: row binding, typed values, cardinality, provenance, limits, and
fail-loudly parse behavior.

### 7.4 The generic constraint kind

The companion gap analysis proposes a `constraint` kind. With named relations, it can remain
small and readable:

```yaml
rules:
  - id: dependency-versions-agree
    kind: constraint
    input: dependency-pins
    transform:
      version: [trim, semver-range-canonical]
    assert:
      functional_dependency:
        key: [dependency]
        determines: [version]
    report:
      at: version
      message: "{{row.dependency}} has inconsistent version requirements"
    level: error

  - id: locale-key-parity
    kind: constraint
    input: locale-keys
    assert:
      sets_equal:
        group_by: locale
        value: key
    report:
      at: key
      message: "translation key sets differ between locales"
    level: error
```

The first public release should be **check-only** and should cover the integrity-constraint
core: unique keys, references, scalar equality, set relations, cardinality, and grouping.
Generic fixes should wait for the provenance and conflict model to prove safe.

### 7.5 Bounded expressions

`when` should remain a gate. A future expression environment can generalize the namespaces
without becoming the main query language:

- top-level: `facts`, `vars`, declared environment inputs, and snapshot metadata;
- selector context: `resource` path/type/metadata;
- row context: `row.FIELD`;
- group context: declared aggregate values only;
- finding/report context: fields selected by the constraint plan.

Structural YAML operators should perform iteration, grouping, joins, and aggregation. The
expression layer should perform boolean predicates and small deterministic transforms over
already-bound values. This follows CEL's useful host-language boundary without requiring
alint to adopt CEL syntax or an external runtime.

### 7.6 Typed templates and policy modules

A v2 template should declare parameters:

```yaml
templates:
  - id: package-required-file
    params:
      manifest: { type: filename }
      filename: { type: filename }
      severity: { type: level, default: warning }
    rule:
      kind: for_each_file
      select: "**/{{params.manifest}}"
      require:
        kind: file_exists
        paths: ["{dir}/{{params.filename}}"]
      level: { param: severity }
```

This example remains illustrative. `filename` parameters splice into globs as escaped literal
text (rule 12 in section 7.1), and `level` is a whole-node typed substitution rather than string
interpolation. The necessary behavior is:

- missing required, unknown, and unused parameters fail validation;
- whole-node substitution preserves booleans, integers, lists, paths, and enums;
- interpolation inside a larger string is allowed only for scalar stringable types;
- references are namespaced when policies are imported (today templates and rules merge by flat
  id through `extends:`, per ADR-0002 and the v0.5 composition design, so namespacing is a
  change to that merge model, not only new syntax);
- templates remain acyclic and capability-monotone;
- a module can ship fixture tests and metadata alongside rules/templates/relations.

Do not add template inheritance. Reusable named selectors and relations remove much of the
pressure that would otherwise motivate it.

### 7.7 Policy tests

A first-class test format should operate on fixture repositories, not mock internal APIs:

```yaml
test_version: 1
cases:
  - name: matching dependency pins pass
    fixture: fixtures/pins-pass
    expect: { findings: [] }

  - name: drift points to both declarations
    fixture: fixtures/pins-drift
    expect:
      findings:
        - rule: dependency-versions-agree
          count: 1
          primary: packages/b/package.json
          related: [packages/a/package.json]
```

`alint test` should support expected findings, notes, related locations, fix diffs, exit
class, and deterministic snapshots. Remote execution stays disabled; command/plugin tests
must explicitly declare and provide those capabilities. This is a dedicated policy-test schema,
not a `.alint.yml` config with an overloaded `version` field; fixture paths remain confined to
the test package by default.

**Build on the existing harness rather than inventing a second format.** alint already has a
deny-unknown-fields scenario format in `crates/alint-testkit` (`given.tree` / `given.config`,
a `when: [check, fix, check]` step list, `expect` per step, `expect_tree`, and the ADR-0014
rule-page marker), and over 500 scenarios use it. The sketch above is a different format for
the same job. The recommended path (decision DEC-6) is to promote that scenario schema, plus an
optional `given.fixture: <dir>` for on-disk fixtures, as the public policy-test format with a
published JSON Schema and its own version field. Two mechanics follow:

- `alint-testkit` is `publish = false` and documented as not a stable API, and a published
  crate cannot depend on an unpublished one. The runner either moves into a published crate or
  the testkit is published with a stability statement.
- The design doc must specify fixture confinement (resolved under the test file's directory,
  `..` rejected the same way `extends:` targets are confined), `extends:` inside tests (bundled
  and local only, `https://` refused), cases that need a Git repository (the harness already
  builds one by spawning `git`), and message matching (exact, regex, or omitted).

Module authors then also get what ADR-0014 gives rule pages: the same fixture can render as a
documented example.

### 7.8 Versioning across `extends`, schemas, and tooling

A new DSL surface touches more than the parser. Each of these needs an answer in the Phase 3
design document (except the `requires:` field, question Q11, which lands earlier in Phase 1b),
with the defaults recommended here:

- **Version mixing across `extends:`.** The loader currently drops an extended file's `version:`
  without checking it, so a v1 consumer extending a v2 ruleset would fail with an opaque
  "unknown field" error. Each extended source's version is validated before it is dropped. A
  v2 consumer may extend v1 sources; a v1 consumer extending a v2 source is a hard error that
  names the required version. A v1-compatible `requires: { alint: ">=0.N" }` ruleset field
  should land before Phase 3, so rulesets that use new kinds can say so (question Q11).
- **JSON Schema and editors.** Configs point editors at `schemas/v1/config.json` on `main`. A v2
  surface ships `schemas/v2/config.json`; the v1 schema is frozen, and the published `$id`
  stops pointing at a moving branch.
- **Commands.** `validate-config` validates both versions; `init` and `suggest` keep emitting v1
  until v2 is stable; `list`, `explain`, and `export-agents-md` render named selectors and
  relations, under the ADR-0012 completeness gates.
- **Generated documentation.** Rule pages are generated per kind from `docs/rules.md`, and
  ADR-0011 summaries come from the first sentence. A `constraint` kind with a dozen operators
  needs per-operator pages, and the facts pipeline must decide whether operators count toward
  the headline (constitution invariant 9 makes those counts gated facts).
- **Relation to v1.0.** The roadmap's v1.0 milestone commits "DSL schema committed; semver on
  `version: 1`" and a frozen `alint-core` API, while Phase 2 reshapes public `alint-core` types.
  Decision DEC-9 orders the two: `version: 2` ships before v1.0, so v1.0 follows Phase 3 and
  freezes both versions, the reshaped `alint-core` API, and plugin contract 1.0 together.

## 8. Diagnostics, exceptions, and remediation

### 8.1 A richer finding record

Before generic constraints ship, replace the conceptual single-point violation with a
finding that can carry:

- stable rule id, kind, severity, category, and policy URL;
- message plus optional structured help/note;
- one primary location with a byte/line range;
- zero or more related locations, each with a label and relationship;
- structured evidence values, bounded and redaction-aware;
- analysis status (`complete`, `truncated`, or `skipped` with reason);
- stable structural identity for baselines and external systems;
- zero or more typed edit proposals with applicability.

Existing output formats can project this richer record down. SARIF already emits `fixes[]`
from proposed edits and can add `relatedLocations`; terminal output can show a concise primary
message with labeled secondary lines; formats that cannot express the structure must not alter
pass/fail.

The record must be specified against what already ships:

| Concern | Today | Requirement |
|---|---|---|
| JSON report schema | `schemas/v1/check-report.json` sets `additionalProperties: false` on violation and rule-result objects, while the agent format documents field additions as non-breaking | decision DEC-4: relax v1 one release ahead of new fields, or version the report |
| Per-format projection | `github` and `gitlab` annotations hold one location; `junit` has none; `markdown` is a PR comment; the LSP maps one range | each format's projection is specified and gated by ADR-0012 positive-completeness tests, not by the weaker "cross-formatter tests" alone; LSP `relatedInformation` only when the client advertises it |
| Primary location | one optional path | it decides the baseline path, the GitHub annotation file, and SARIF `locations[0]`: the rule's declared `report.at`, else the non-canonical side, else the first in canonical order; existing kinds keep today's anchor |
| Baseline identity | ADR-0006 and `baseline.md`: one fingerprint definition; cross-file findings key on the sorted set of involved paths; baseline `SCHEMA_VERSION` 1 and other versions are rejected | decision DEC-5: fingerprints of existing kinds are frozen and gated by a golden file recomputed over the scenario corpus; related locations never enter the fingerprint; new kinds set `baseline_key` from sorted identity fields |
| Proposed edits | computed only for fix-carrying formats | stay lazily attached; a plain `check` does not derive edits for every finding (the located-fixer correlation rules still apply) |
| Notes | dropped by six of eight formats (section 4.12) | SARIF `level: note` results, plus `toolExecutionNotifications` for truncated analysis; a summary line in `github`, `gitlab`, and `markdown` |
| Evidence values | rendered verbatim in messages and baseline files | decision DEC-3: redaction by default for secret-bearing formats, opt-in `sensitive:` elsewhere, fingerprints hash values instead of storing them, and a gate proving a planted secret never appears in any format's bytes |
| Repository-controlled text | `sanitize_terminal` and Markdown escaping cover messages | the same sanitizers cover every new label, evidence string, and related-location field |
| Evidence size | messages are capped by short rendering | at most a fixed number of related locations per finding (for example 20, then "and N more") and a byte cap on structured evidence |

alint is not a secret scanner, and it must not become a secret leaker: the redaction row is a
Phase 1a deliverable, not polish. Messages stay English-only; message templates over row fields
are stored as a message id plus arguments, so a future localization never changes a
fingerprint.

### 8.2 Exceptions are evaluated data

An exception should not erase a candidate before the engine can account for it. The safe
pipeline is:

```text
would-be findings + top-level exception relation
  reconcile on declared identity fields
    -> intersection: finding is marked excepted and does not fail
    -> finding-only anti-join: live finding, fails normally
    -> exception-only anti-join: stale-exception finding
```

This gives allowlists a reverse check and makes stale debt removable. Baselines remain a
separate mechanism for snapshotting existing findings. Path exclusions remain policy scope,
not exceptions. The three concepts should not be conflated. Section 5.5 lists the ADR-0006
decisions this reverses or must align with (question Q3).

Expiry needs an explicit reference date supplied by CLI/config/CI. The default evaluator
must not read the current time. Remote or nested configs must not choose top-level exception
sources, paralleling the baseline trust boundary.

### 8.3 Generic fixes must be conservative

The current fix design has strong properties worth preserving. A generic constraint does
not automatically imply a unique fix:

- equality does not say which side is authoritative;
- a missing set member does not say whether to add it or remove it elsewhere;
- a duplicate does not say which record is correct;
- a broken reference does not say whether the target or declaration is wrong.

The first constraint release should report only ("report-only" here means no fixes; it is a
different guardrail from "check, never infer" in the gap study, which forbids deriving or
minimizing constraints). Later fixes can be enabled when the policy declares direction and the
engine has source spans. They should reuse the shipped v0.17 operations rather than invent
parallel ones:

- `canonical: relation-row-selector` for one source of truth, generalizing the shipped
  `sync_from` (which already fixes `cross_file equals` drift from a declared source, Unsafe by
  default);
- `on_missing: insert` with a format-aware insertion strategy, bounded by
  [ADR-0017](../adr/0017-auto-fix-edit-model-and-applicability.md) Decision 2: only a scalar
  replacing a scalar can be Safe, and key insertion is a Suggestion;
- explicit applicability, conflict detection, and a unique target provenance check.

If any derivation is ambiguous, the result is a suggestion or no fix, never a guessed edit.
Command fixes retain their stricter trust boundary.

## 9. Execution and extension architecture

### 9.1 Compile policy to a plan

The DSL loader should eventually produce a typed plan rather than a vector of opaque rules.
The planner can then:

- validate names, types, cardinalities, capabilities, and cycles;
- deduplicate selectors, reads, parses, and extraction plans;
- choose per-file, relation-major, or full-index execution;
- estimate and enforce row/join/graph limits;
- determine invalidation dependencies for LSP/watch mode;
- expose the exact plan through `explain --trace`;
- preserve deterministic ordering at every boundary.

Existing `Rule` implementations can coexist during migration. An adapter can wrap a native
kind as a plan node, and high-value shared paths can move incrementally.

Two engine invariants constrain the planner:

- **Full-index context versus target selection.** Constitution invariant 2 and ADR-0003 fix three
  dispatch classes and require a full-index rule to have no path scope; that is how
  `--changed` mode avoids skipping cross-file rules. A selector on a cross-file rule must
  therefore be modelled as a filter over the rows it compares, never as a path scope, and
  `--changed` must still run the rule when any row of its context changed, not only a selected
  row; otherwise a pull request that edits only the canonical side of a drift relation would
  pass. This amends invariant 2 and needs a superseding ADR (question Q2).
- **Read coalescing survives relation-major execution.** Per-file rules share one read per file
  (ADR-0003). A relation-major schedule that reads a file once per relation would repeat the
  v0.18.0 content-scan regression class. The planner keeps file-major read coalescing for every
  extractor over a per-file selector, and the deterministic benchmark gains a scenario with at
  least three relations over one glob.

### 9.2 Cache parsed resources and relations, not verdicts only

The current file-major path coalesces reads. The next reusable unit is a decoded resource or
relation keyed by:

- snapshot/resource content identity;
- adapter name and version;
- extractor specification;
- normalization specification;
- declared environment/capability inputs.

This avoids parsing one manifest repeatedly for facts, scope filters, structured rules, and
constraints. Cache entries must include failure/truncation outcomes so a cached incomplete
parse cannot turn into an empty successful relation.

### 9.3 Snapshot abstraction

Introduce a read-only `Snapshot` interface internally before adding user flags. It should
provide normalized resource enumeration, bounded reads, metadata, and optional Git/change
relations. Implementations can then be:

- filesystem working tree (current default);
- overlay snapshot for LSP buffers;
- Git index;
- Git tree at a revision.

Rules that need real filesystem behavior or spawn a command must declare that restriction
and cannot run against an incompatible snapshot. Fixes apply only to a mutable working-tree
snapshot.

### 9.4 Plugin architecture

Decided 2026-10-14 (DEC-10 and DEC-14). Third parties can extend alint with format adapters,
checkers, and fixers, written as Rust crates (the canonical way) or in any other language, and
run either as native code or sandboxed. The research behind the choices below is summarized in
section 9.4.7; the full design (type mapping, wire identity and evolution, protocol, trust,
estimates, work breakdown, and open questions) is [`plugin-architecture.md`](plugin-architecture.md).

#### 9.4.1 One contract, three roles

A single versioned plugin contract defines three roles, each a pure function from inputs the host
supplies to typed results:

1. **Adapter:** turns a selected resource (bytes plus metadata) into typed rows with spans, after
   which native constraints and kinds operate normally. Every built-in format (the eight config
   formats, CommonMark, and later tabular and front-matter decoding) is an adapter on this same
   contract (DEC-10), so a third-party adapter is a peer of the built-in ones rather than a
   special case.
2. **Analyzer (checker):** turns resources or rows into findings with primary and related
   locations, for algorithms that do not fit the relation model.
3. **Fixer:** returns typed edit proposals with an applicability tier. Plugins never write files:
   the host derives, confines, verifies, tier-gates, and applies every edit through the existing
   fix engine, so ADR-0017's guarantees and the untrusted-remote demotions apply unchanged.

The contract also carries plugin metadata and the compatible contract range, a configuration
schema the host validates options against, declared capabilities, and resource limits. Only
contract types cross the boundary, never internal engine layouts (the failure mode in which a
host's internal tree layout leaked into its plugin ABI and broke every plugin on each change).
Version 1 of the contract uses a restricted, pure subset of WIT: no resources or handles, no
streams or futures, no maps, and no recursive types.

#### 9.4.2 Three hosts, one internal interface

| Host | Who writes it | Isolation and trust | Transport |
|---|---|---|---|
| **Native Rust** | Rust crates depending on the semver-stable `alint-plugin-api` crate | full trust, decided when the binary is built | direct trait calls |
| **Out-of-process** | any language, through a generated SDK or the raw protocol | full trust native code, integrity-checked | protobuf frames over stdio (section 9.4.4) |
| **WASM component** | any language with a component toolchain (Rust, C, C++, C#, Go, and others through `componentize-*`) | sandboxed and pure: no filesystem, network, clock, randomness, or environment imports; fuel or epoch limits and memory limits | wasmtime, behind a cargo feature |

All three feed one internal `Adapter` / `Analyzer` / `Fixer` trait set, which the built-in kinds
and formats also implement as they move onto the Phase 2 IR. The plugin API is therefore
exercised by alint itself and cannot drift from what built-ins can do. Registration replaces
the hard-coded `builtin_registry()` call sites with an injectable registry; plugin kinds are
namespaced (`<plugin>/<kind>`), carry their option schema for `validate-config` and editors, and
declare their capabilities through the same `KindMeta` inventory as built-ins (section 9.7), so
the trust gates and audits see them.

#### 9.4.3 Native Rust plugins: compiled in, never dynamically loaded

Rust has no stable ABI, so loading Rust dynamic libraries is unsound across compiler versions and
builds (one major Rust game engine deprecated its dynamic-plugin loader as unsound for exactly
this reason). Native plugins are instead compiled into a custom binary, the model of
`golangci-lint custom` and `xcaddy`:

- `alint build` reads a manifest naming the alint version and the plugin crates (registry, git,
  or path sources), generates a throwaway crate that registers them, and runs
  `cargo build --release`, producing the user's own alint binary.
- The same registration is available to embedders as a library call (running alint with an
  extended registry).
- Native plugins are fully trusted: choosing to compile a crate is the grant.

This supersedes `ARCHITECTURE.md`'s earlier "native Rust plugins are out of scope", whose
objection was to dynamic loading, not to compiled-in plugins.

#### 9.4.4 Out-of-process plugins: protobuf frames over stdio

The out-of-process protocol follows the persistent-worker and process-plugin designs that have
worked across languages (Bazel workers, protoc plugins, dprint process plugins, Nushell plugins)
rather than gRPC. gRPC would add an HTTP/2 and async-runtime stack to the host, needs sockets or
ports (awkward on Windows), and buys nothing that request ids over stdio do not provide.

- **Framing and encoding.** Length-prefixed protobuf messages on stdin/stdout; stderr is free for
  plugin logs. A plugin may negotiate the proto3-JSON encoding of the same schema for debugging
  and for languages where adding protobuf is a hurdle.
- **Handshake.** The plugin announces its encoding, then both sides exchange a hello carrying the
  protocol version, the supported contract range, and feature flags. Unknown features are
  ignored and no side uses a feature the other did not confirm, so minor contract versions do not
  force lock-step upgrades.
- **Lifecycle.** One persistent process per plugin per run (and per LSP session), multiplexed
  request ids, batches of many files per request, best-effort cancellation, per-request
  timeouts, kill on timeout or EOF, crash isolation, and the bounded-output and process-group
  machinery the `command` kind already has.
- **SDKs.** A Rust SDK that runs the same plugin crate either compiled in or as a process, then
  generated Python, Go, and TypeScript SDKs from the emitted `.proto`.

#### 9.4.5 One source of truth: WIT, compiled by a separate project

The three bindings evolve under different rules. In the component model a published record,
variant, enum, or flags type cannot gain a member without breaking compatibility (wasmtime checks
these types for exact equality); protobuf tolerates new fields by stable field number; Rust
tolerates them with `#[non_exhaustive]`. The strictest system has to be the source, because
projecting it to the more permissive ones loses nothing, while the reverse is lossy (protobuf to
WIT yields all-optional fields and open enums; Rust can express recursion and generics WIT
cannot). So:

- **WIT is the hand-written source of truth** for the contract, as a versioned package.
- **A new, separate open-source project, a WIT-first contract compiler, generates the other
  bindings:** plain Rust types and traits for `alint-plugin-api` (with no wasmtime dependency),
  a `.proto` schema whose field numbers come from a committed, append-only lockfile, and later
  JSON Schema. alint is its first consumer, and its scope is limited to what alint needs until
  alint's contract v1 ships. No existing tool generates protobuf or plain Rust from WIT; the
  closest ecosystem work goes the other way (protobuf to WIT), and the WIT annotations proposal
  (component-model issue 695) may later let field numbers live in the WIT source, which the
  lockfile is designed to be replaced by.
- **Gates.** Generated outputs are committed and checked (`--check`, LF-pinned); the lockfile is
  monotonic against the merge base; `buf breaking` runs on the emitted schema at the WIRE_JSON
  level; and a WIT compatibility check rejects any change to a released type.
- **Evolution policy.** A published contract version is frozen. Changes ship as a new minor
  version with additive, defaulted members; the previous WIT stays in the tree, and the host keeps
  one adapter per supported version, converting each into the latest plain types. A major version
  is a new package, negotiated in the handshake.

#### 9.4.6 Trust and distribution

- **Process plugins** are spawn-capable: declarable only in the top-level config (or through
  `trusted_extends:`), never by an ordinary `extends:`, and every executable is pinned by
  SHA-256.
- **WASM plugins** may arrive through a remote `extends:` when SRI-pinned, because they are
  sandboxed and receive no capabilities. A plugin inherited through `extends:` receives no
  environment-derived values, consistent with the 2026-10 audit; the roadmap's earlier sketch of
  host-side `{{env.X}}` interpolation into plugin configuration applies only to plugins declared
  in the top-level config.
- **An `alint.lock`** records per-platform hashes for every platform at once (so a lockfile
  written on one operating system does not fail CI on another), with optional cosign or
  attestation verification reusing the release-signing tooling of ADR-0015.
- **Capability grants** extend `trusted_extends:`, the one existing top-level grant mechanism,
  rather than add a parallel one; ADR-0004 gains the plugin rules in a superseding ADR.
- **No public registry** is promised until one adapter plugin and one analyzer plugin have been
  validated end to end.

#### 9.4.7 Prior art consulted

HashiCorp go-plugin and tflint (gRPC over a local socket, signed checksums), dprint (sandboxed
WASM plus checksum-pinned process plugins), Nushell (stdio with negotiated encoding, a
version-and-features handshake, and the cost of minor-version lock-step), Bazel persistent
workers and protoc plugins (protobuf over stdio), golangci-lint and xcaddy (custom builds), Zed
(one WIT copy and host adapter per contract version), Typst (pure WASM plugins), sqlc and Buf
check plugins (one protobuf contract over WASM and processes), Terraform's plugin protocol
(side-by-side major versions and lock files), wasmCloud and wRPC (WIT as a transport-independent
IDL), and the Bevy and SWC cautionary cases (unsound dynamic loading; internal layouts in an
ABI).

### 9.5 Cost model and limits

Generality without limits invites accidental quadratic behavior. The plan needs defaults
and hard caps for:

- files and bytes decoded by one adapter;
- parse depth/nodes and virtual-document recursion;
- rows emitted by one extractor and total rows per run;
- group cardinality and distinct-value count;
- equality-join inputs and output rows;
- graph nodes/edges and traversal work;
- regex size and existing engine-level regex limits;
- findings/evidence/related locations retained;
- plugin memory, fuel, output, and elapsed time.

Hitting a limit is an incomplete-analysis error or explicit note according to policy. It is
never a silent pass.

### 9.6 Performance budget

"Prove performance on the million-file benchmark" is not a budget, and the existing gate cannot
enforce one on its own. The deterministic gate compares each pull request against its base
(+2% instructions, +5% estimated cycles), is advisory in CI, and is blind to I/O (the v0.14 S2
read-path regression showed flat instruction counts can hide a real wall-clock loss). A
ten-PR migration at +1.9% each would pass every check and land about 20% slower. The program
therefore measures against a **frozen pre-IR reference (the v0.18.2 tag)**, cumulatively, with
these budgets (decision DEC-13):

| Measure | Budget against v0.18.2 | Where |
|---|---|---|
| Instruction count, deterministic scenarios S1-S3 | at most +3% cumulative | the gungraun gate, enforced (not advisory) for IR pull requests |
| Wall-clock median, 30 runs, S1-S4 x {full, changed}, 100k and 1M files | at most +3% | the canonical bench host with caches dropped |
| `read` / `stat` syscall count | no increase | strace counts on the same scenarios, because the instruction gate is I/O-blind |
| Peak RSS at 1M files | at most +10% | the bench harness |

New capabilities (relations, constraints) are budgeted separately as new scenarios; the
budgets above protect configurations that do not use them.

### 9.7 Capability inventory as a source of truth

Phase 0's per-kind capability inventory needs a defined source. Today `RuleRegistry` stores only
builders and aliases, dispatch traits are instance methods that can depend on options, and the
trust lists are hand-maintained (`SPAWNING_RULE_KINDS` in the DSL trust module, the Git-required
kind list in a coverage audit). The inventory should be a static `KindMeta` declared at
registration: resource domain, dispatch class, accepted selectors, capabilities (spawn, Git,
environment, time, host keyring), formats, and fix operations. It is emitted into `facts.json`,
a coverage audit asserts that every scenario-built rule instance behaves within its declared
`KindMeta`, and the spawn and Git lists are derived from it. That turns Phase 0 from
documentation into the single source for trust decisions, and it is the migration map for
Phase 2. It would also have caught the `scope_filter` silent-ignore row of section 4.12, since a
kind that declares no `scope_filter` support would reject the field.

## 10. What alint should not generalize

These are this proposal's recommendations; each becomes binding only through the ADRs of
section 13.3. The following directions would weaken the product even if they increase
expressiveness:

- **No general-purpose language embedded in YAML.** No loops, recursion, user functions,
  I/O, dynamic imports, or mutation in the declarative core.
- **No hidden network or current-time dependency.** Fetching a remote ruleset during config
  resolution is already explicit and integrity-checked. Rule evaluation itself remains
  offline unless a top-level capability explicitly invokes an edge plugin.
- **No package manager or build-system reimplementation.** Dependency resolution, lockfile
  semantics, compilation, generated-code execution, and test results belong to their tools.
- **No bundled parser for every programming language.** Syntax adapters are demand-driven
  plugins or carefully selected optional components, not core-binary gravity.
- **No automatic fix inference from an arbitrary failed constraint.** Direction and edit
  semantics must be authored.
- **No single mega-kind replacing the catalogue.** Named specialized kinds remain the best
  documentation, onboarding, optimized implementation, and false-positive boundary for
  common checks.
- **No silent coercion for convenience.** Stringifying structured values or treating parser
  failures as no matches produces false greens.
- **No inherited authority.** A remote ruleset may describe pure policy but cannot grant
  itself process, network, environment, outside-root, exception, or destructive-fix power.
- **No finding aggregator masquerading as evaluation.** External report merging remains an
  integration concern unless a delegated rule has an explicit, validated contract.
- **No claim of complete lint coverage.** Code semantics, SAST, secrets, semantic IaC,
  external governance state, and runtime behavior remain named boundaries.

## 11. Delivery sequence

This is intentionally staged. Publishing the generic YAML first would freeze the weakest
part of the design before provenance, testing, and planning are ready. Each phase has a proposed
target release, a size, the substrates it delivers, and an exit criterion. DEC-1 adopted these
targets; the roadmap records Phases 0 and 1a under v0.19, and later targets stay provisional
until each phase's design document.

### 11.1 Phase summary

| Phase | Proposed target | Size | Exit criterion |
|---|---|---|---|
| 0. Terminology and capability inventory | v0.19 | S | `KindMeta` declared for every kind, emitted into `facts.json`, the spawn and Git lists derived from it, and the audit gate green |
| 1a. Fail-loud and finding foundations | v0.19 | L | every row of section 4.12 closed or explicitly recorded as accepted, each with a gate asserting its invariant |
| 1b. Authoring loop | v0.20 | M | `alint test` public with a published schema; a rule-level `explain --trace`; the bundled rulesets carry their own tests |
| 2. Internal relation IR (go / no-go) | v0.21 | L | the IR pilot ships as a v1 kind, one existing kind is ported with zero shadow diffs over one release, and the section 9.6 budgets hold |
| 3. Selectors, report-only constraints, and `version: 2` | v0.22 or later | L | the section 12.1 measures met on the Phase 3 conformance policies (locale key parity, dependency version consistency, toolchain-pin consistency; stale exceptions are measured at Phase 4); the version-boundary ADR accepted |
| 4. Typed modules and exception data | unscheduled | M | decided after Phase 3 by measured reuse pressure |
| 5. Snapshot views | unscheduled | L | the LSP overlay part first; Git index and tree views only with demand |
| 6. Plugin hosts (parallel track, DEC-14) | contract with Phase 2 (v0.21); native Rust and `alint build` next (about v0.22); process host and Rust SDK (about v0.23); WASM host and other-language SDKs after | L (about 33-53 engineer-weeks in all, including the contract compiler; a first real plugin after about 12-18) | one adapter and one analyzer plugin validated end to end before any registry |
| WIT contract compiler (separate project, DEC-14d) | starts now, in parallel | MVP about 12-18 weeks; 1.0 about 26-45 | generates `alint-plugin-api` and the `.proto` with lockfile gates; alint is the first consumer |
| v1.0 | after Phase 3 | n/a | freezes `version: 1`, `version: 2` (DEC-9), the `alint-core` API, and plugin contract 1.0 together |

**Stop rule.** If the Phase 2 pilot does not make the ported kind simpler and more consistent,
or if after Phase 3 the share of new demand that is expressible by configuration (section 12.3)
does not move, the program stops there. Phases 0 to 2 have standalone value either way.

### 11.2 Phase 0: terminology and capability inventory

- Keep the bounded definition from section 1 synchronized across contributor-facing architecture
  and product documentation. `ARCHITECTURE.md` was corrected alongside the first version of this
  proposal; the roadmap's Positioning section was aligned in the 2026-10-14 revision.
- Keep the short public tagline "A general linter," but describe the domain as finite,
  inspectable project state in longer copy.
- Add the static `KindMeta` inventory of section 9.7: resource domain, dispatch, selectors,
  adapters, capabilities, finding shape, and fixes. It becomes the migration map, a drift gate,
  and the source of the trust lists.

### 11.3 Phase 1a: fail-loud and finding foundations

The v0.19 work breakdown, with acceptance criteria per item, is
[`v0.19/README.md`](v0.19/README.md).

- Separate analysis failure from violations, with the exit semantics chosen in decision DEC-2.
  Analysis failures are never baselinable and do not depend on `level`.
- Render notes in every format (section 8.1).
- Redact evidence values and add the leak gate (decision DEC-3).
- Reject `scope_filter` and manifest predicates on kinds that do not honour them, at
  `validate-config` time (driven by `KindMeta`).
- Make the remaining section 4.12 rows explicit: manifest parse failure, Git unavailable, and
  non-string extraction results each become an analysis failure or an explicit, visible
  outcome.
- Add primary ranges and labeled related locations to the finding model, with the JSON report
  decision (DEC-4), per-format projections, and the frozen-fingerprint golden gate (DEC-5).
- Add the `--as-of` input (decision DEC-11) and move `git_blame_age` onto it.

Edit-plan preconditions and partial-application reporting move out of Phase 1. Generic fixes
are deferred until after the report-only constraint release, so that work belongs to the phase
that first enables generic multi-resource fixes, building on the deferred auto-fix
section 5.2.4 design (section 4.11). Phase 1 keeps today's per-file atomicity and reports it.

### 11.4 Phase 1b: authoring loop

- Promote the testkit scenario format to `alint test` for downstream rulesets and fixtures
  (section 7.7, decision DEC-6).
- Add a rule-level `explain --trace`: which gates ran, which selectors matched how many
  resources, extracted counts and cardinalities, and which assertion failed. The plan-level
  trace of section 9.1 waits for the Phase 2 IR. Trace output is experimental (human output,
  or JSON under ADR-0009's `schema_version` envelope marked experimental) until v2 is stable,
  so users cannot freeze IR internals by scripting against it (question Q14).
- Add an extraction preview with provenance and truncation markers.

Phase 1 as a whole has standalone value and reduces the risk of every later phase.

### 11.5 Phase 2: internal relation IR (go / no-go)

- Implement internal resource, selector, adapter, relation, and constraint types, with lazy spans
  (section 6.2) and file-major read coalescing (section 9.1).
- **Pilot on a new kind, not a port.** The first user of the IR is a new v1 kind with no
  existing users, `key_parity` (a reference file plus a glob of peers, covering locale files and
  `.env.example`), so the IR delivers user value with zero migration risk (decision DEC-7).
- **Port existing kinds only under differential shadow evaluation.** Today's parity evidence is
  thin for this purpose: the coverage audit requires one firing and one silent scenario per
  kind, the property tests cover panics and fix idempotence, and the ADR-0014 rule-page fixtures
  are one pass/fail pair per kind, which exercises one documented shape rather than the extract
  and relation matrix. A port keeps the old implementation and runs both on every
  scenario, on the 110 committed v0.12 corpus configurations (111 repositories were studied)
  at their pinned commits, and on a
  property-test generator over extract specs, asserting byte-identical findings, messages, and
  baseline fingerprints. The old implementation is deleted only after one release with zero
  differences. The first port is `cross_file_value_equals` (an alias over `cross_file`, so its
  surface is small), then the shared `ExtractSpec` and manifest-derived scope.
- Keep `file_graph` and other specialized kernels native but make them consume and emit shared
  relation and finding records where useful.
- Meet the section 9.6 budgets against v0.18.2.

This is the go/no-go proof. If several real kinds do not become simpler and more consistent,
do not expose a generic DSL.

### 11.6 Phase 3: selectors and report-only constraints

- Design and ADR the version boundary (decision DEC-9) and the cross-file selector amendment of
  constitution invariant 2 (question Q2).
- Expose named selectors and relations (question Q1).
- Ship the report-only `constraint` kind with cardinality, unique/key, reference, equality,
  set, group, and bounded-count assertions.
- Implement the first high-value policies from the gap study as conformance cases: locale
  key parity (already a kind from Phase 2, re-expressed), dependency version consistency, and
  toolchain-pin consistency.
- Ship specialized named kinds or bundled templates for common cases so most users need not
  author raw relations (decision DEC-8).

Stale-exception detection moved to Phase 4, because exception relations are designed there.

### 11.7 Phases 4 to 6

- **Phase 4, typed modules and exception data.** Typed template parameters and namespaced module
  references; modules that ship tests and minimum-engine and capability metadata; explicit
  top-level exception relations with reverse (stale) checks and optional `--as-of` expiry
  (question Q3); policy-impact comparison between configs. Scheduled only if Phase 3 shows
  named-relation reuse.
- **Phase 5, snapshot views.** The LSP overlay snapshot first, because it fixes a real
  inconsistency (cross-file findings refresh only on save). Git index and Git tree views wait for
  demand: the pre-commit framework already stashes unstaged changes, which covers the most
  common staged-only use. Dependency-aware LSP and watch invalidation follow the compiled plan.
  Process-spawning and mutable-fix rules stay restricted to compatible snapshots (question Q9).
- **Phase 6, plugin hosts (a parallel track, not the last phase).** Per DEC-14e, the plugin
  contract is designed together with the Phase 2 IR and published as an unstable `0.x` contract,
  with built-in kinds and formats on the same traits. The hosts then follow in order: native
  Rust plugins and `alint build`; the out-of-process host and Rust SDK (Linux and macOS first,
  then Windows hardening); the WASM host; and the Python, Go, and TypeScript SDKs. The WIT
  contract compiler is built in parallel as its own project. Contract 1.0 freezes at v1.0 with
  the DSL. Validate one adapter plugin (a syntax-query adapter) and one analyzer plugin before
  promising a registry (section 9.4).

### 11.8 Gap-to-substrate matrix

This is the canonical ordering of every detection gap in [`rule-coverage-gaps.md`](rule-coverage-gaps.md)
(IDs as in its section 4) against the phases above. Its purpose is to prevent throwaway work: a gap in the "after the IR"
bucket built today as a standalone evaluator would be replaced by Phase 3 and leave two
spellings frozen.

| Bucket | Gaps | Why |
|---|---|---|
| Ship now as native kinds or bundled rulesets (no IR needed, nothing thrown away) | A1 `editorconfig_conforms` (after decision DEC-12), A2 `gitattributes_valid`, A3 `ignore_consistency` (coverage and duplicate halves), B2 `well_formed`, B4 path casing and mutual exclusion, B5 `yaml_footguns`, B6 self-declared schemas, the Markdown structure kinds (E2 alt text, E3 required headings, E4 reference definitions, E5 duplicate headings, E7 fragment resolution, E8 list structure), E6 `frontmatter_*`, F1 `license_detectable`, F2 the SPDX table with `spdx_identifier_valid`, F4 `manifest_license_valid`, G1 `delimited_columns`, H1 `pinned_references`, H2 `dockerfile_structural`, I1 `max_path_length`, I2 `lfs_pointer_valid`, I3 `file_encoding`, I4 `symlinks_valid`, I5 normalization collisions, J1 `codeowners_valid` syntax checks, J2 the governance-schema pack, J4 `numeric_sequence`, and the C1 version-SSOT, C2 toolchain-pin (exact pins), and Keep-a-Changelog bundled rulesets | native kernels or adapters; under section 6.5 they stay native after the IR exists |
| Needs local Git tag access | C6 `git_tag_*` | a small new Git input, independent of the IR |
| Needs a Phase 1 substrate first | B1 `no_duplicate_keys`, B3 `structured_key_sort` | the duplicate-aware spanned extraction is part of Phase 1a's provenance work (dotenv is ready through `adotenv`; JSON through the `jsonc-parser` AST, and YAML, INI, and properties through hand-written locators, have span resolvers in the fix bridge; TOML and HCL already reject duplicates at parse), so it is built once there, not inside a kind |
| Needs `--as-of` first | J3 `field_date_valid`, F5 header and copyright-year consistency, exception expiry | the Phase 1a time input |
| Build on the IR (Phase 2 or later), not before | D1 `key_parity` for nested formats (the Phase 2 pilot), C3 `dependency_version_consistency`, D2 `placeholder_parity`, F3 `reuse_license_completeness`, C5 `dependabot_ecosystem_drift`, the dead-pattern halves of A3 `ignore_consistency` and J1 `codeowners_valid`, B7 in-file reference integrity | relational; as standalone evaluators they would duplicate the Phase 3 constraint |
| Standalone library now, reused by the IR | C4 `semver_range` | lands first as a standalone library (a canonicalizing `normalize:` band plus range-satisfaction and intersection checks), then becomes the `semver` domain scalar of section 6.2 |

Flat-format key parity (`.env` against `.env.example`, `.properties`) is already expressible today
with `cross_file` `set_equals` over a `regex:` extract, and should be documented as a recipe now.

## 12. Validation and success criteria

### 12.1 Product criteria

The proposal succeeds if, measurably:

- each named conformance policy (locale key parity, dependency version consistency,
  toolchain-pin consistency, stale exceptions) is expressible without Rust, in a documented
  configuration of bounded length, with its firing and silent fixtures shipped as `alint test`
  cases;
- for every specialized kind given a generic spelling, the specialized spelling stays shorter,
  and the documentation shows both;
- incomplete analysis cannot produce a false pass: mutation tests on the error, truncation, and
  type-mismatch paths kill every "empty success" mutant;
- a finding navigates to all material evidence in SARIF and the LSP, verified by the ADR-0012
  completeness gates;
- policy packs are testable without cloning a large real repository;
- the trust boundary is no weaker than v1, verified by the trust tests of section 12.2;
- v1 configs keep their pass/fail semantics, except where v1 produced a false pass (section 6.3),
  and every such change is listed in the changelog.

### 12.2 Engineering criteria

Before public DSL release, require:

- differential shadow evaluation with zero differences for every v1 kind moved onto the shared
  plan (section 11.5);
- firing and silent end-to-end cases for every new assertion operator;
- property tests for relation ordering, grouping, joins, cardinality, and normalization;
- mutation tests proving parse/type/truncation errors cannot become empty-success;
- ADR-0012 positive-completeness tests for primary and related locations, notes, and redaction in
  every format;
- the frozen-fingerprint golden gate, plus baseline identity tests under row reordering and
  benign source movement;
- trust tests for every way a remote/nested policy can reference selectors, modules,
  adapters, exceptions, inputs, or fixes;
- bounded-resource tests for adversarial documents and relation explosions;
- the section 9.6 budgets on small, 100k-file, and million-file repositories;
- LSP/CLI parity tests for any snapshot-supported subset;
- JSON Schema/runtime parity and generated documentation gates.

### 12.3 A useful coverage metric

Do not measure generality by the number of kinds alone. The v0.12 study already fixed a unit
and a taxonomy ([`v0.12/case_study_100_repos.md`](v0.12/case_study_100_repos.md)): each finding
is A (expressible today), B (a gap needing new capability), C (a tuning candidate: expressible
but awkward), or D (a deliberate non-goal), and
`coverage_today = A / (A + B + C)` over 111 pinned repositories. This program splits bucket B by
the kind of work it needs:

- B-config: expressible by configuration of a generic constraint;
- B-adapter: needs a new reusable adapter or operator;
- B-kernel: needs a specialized native kernel;
- B-delegate: delegated to a plugin or `command`.

The pinned corpus is re-scored at each phase exit. The success signal is that B1's share of new
static repository-state demand grows while semantic and external-state requests land in
B-delegate or D. A growing kind count can still be healthy when kinds are ergonomic wrappers over stable
substrates.

**The demand evidence is thinner than the first version of this document implied.** The v0.12
locked backlog contains no locale, i18n, key-parity, dependency-version-consistency, or
duplicate-key findings. The flagship Phase 3 policies rest on outside tools (syncpack, manypkg,
i18n-tasks, compare-locales, yamllint) and on the 30-repository examples set, whose analyses
are v0.9.17 snapshots. Before Phase 3 is scheduled, the corpus should gain an i18n and
JavaScript-monorepo stratum so the claim can be measured rather than assumed.

## 13. Open decisions

Section 13.1 lists the program-level decisions, taken on 2026-10-14, because they shape Phases 0
to 2 or reconcile this program with recorded decisions. Section 13.2 lists questions that need their phase's
design work first; each carries a recommendation and is decided in that phase's design
document. Section 13.3 lists the records this program would amend.

### 13.1 Decisions taken (2026-10-14)

The maintainer decided each of these on 2026-10-14, one at a time, after the review. They are
recorded in [ADR-0019](../adr/0019-general-linter-program-and-plugin-architecture.md). Where an
outcome differs from the review's recommendation, the outcome column says so.

| ID | Decision | Outcome |
|---|---|---|
| DEC-1 | Adopt the phased program and its targets (section 11.1), and track it in the roadmap | **Adopted as proposed.** Phases 0 and 1a target v0.19; later targets stay provisional until each phase's design document |
| DEC-2 | How analysis failure maps onto exit codes | **A failure caused by repository content is a finding that always counts as error**, whatever the rule's `level`, and is never baselinable. Configuration failures exit 2, internal errors stay 3, and there is no new exit code |
| DEC-3 | Redaction default for evidence values | **Redact values from dotenv and properties by default, plus an opt-in `sensitive:` marker elsewhere.** Raw values never reach baseline files. The baseline fingerprint is already a hash of the identity, so redaction changes only the advisory message; the Phase 1a design must still address that an unsalted hash of a low-entropy value can be brute-forced |
| DEC-4 | How the JSON report evolves | **Relax v1's `additionalProperties: false` one release before adding fields**, announced in the changelog. A v2 report is reserved for removals and renames. Consequence: related locations reach SARIF and the LSP in v0.19 (both formats already allow them) and the JSON report one release later |
| DEC-5 | Baseline identity under the richer finding model | **Freeze existing kinds' fingerprints behind a golden gate** recomputed over the scenario corpus. Related locations never enter a fingerprint, new kinds set `baseline_key` from sorted identity fields, and the baseline schema stays at version 1 |
| DEC-6 | The public policy-test format | **Promote the testkit scenario schema**, with `given.fixture:`, its own version field and a published JSON Schema; the runner moves into a published crate |
| DEC-7 | The IR proof strategy | **Pilot on the new `key_parity` kind, then port existing kinds only under differential shadow evaluation** with zero differences for one release |
| DEC-8 | Named kinds versus generic constraints for the relational gaps | **Named v1 kinds implemented on the IR**; no standalone evaluators for those gaps before Phase 2; the generic `constraint` is the escape hatch |
| DEC-9 | The DSL version boundary and its order relative to v1.0 | **`version: 2` ships before v1.0** (not the review's post-1.0 recommendation). v1.0 therefore follows Phase 3 and freezes both `version: 1` and `version: 2`, together with the plugin contract 1.0 |
| DEC-10 | The syntax-analysis and format-extension line | **By default the core ships a reasonable set of data and document format adapters** (the eight config formats, CommonMark, and planned tabular and front-matter adapters) and no programming-language syntax. **Every format, built-in included, sits behind one public adapter contract**, so further adapters (including syntax adapters) can be written, distributed, and enabled by other repositories, users, and developers. The adapter contract is designed with the Phase 2 IR, not deferred to the plugin phase |
| DEC-11 | Time-dependent rules and constitution invariant 1 | **A global `--as-of` input** (CLI flag and `ALINT_AS_OF`); without it, time-dependent kinds use the HEAD commit time; outside a Git repository they report an analysis failure unless `--as-of` is given; recorded in a superseding ADR |
| DEC-12 | Policy-source files versus ADR-0010 | **A new ADR defining policy-source files** (files whose documented purpose is to declare the checked policy) as distinct from intent-inferring manifests; findings cite the policy line; `editorconfig_conforms` is the first user |
| DEC-13 | The performance reference and enforcement | **Cumulative budgets against the frozen v0.18.2 tag** (section 9.6), enforced for IR pull requests |
| DEC-14 | The plugin plan of record | Decided in six parts (section 9.4): **(a)** one versioned plugin contract with adapter, analyzer, and fixer roles, served by three hosts (native Rust compiled in, out-of-process in any language, sandboxed WASM components), all feeding one internal trait set that built-in kinds and formats also use; **(b)** out-of-process plugins speak protobuf frames over stdio, with a negotiated proto3-JSON encoding, a version and feature handshake, persistent processes, multiplexed batched requests, cancellation, and timeouts (no gRPC); **(c)** native Rust plugins are compiled into a custom binary by an `alint build` command, never loaded as dynamic libraries; **(d)** WIT is the single source of truth for the contract, compiled by a new, separate open-source project (a WIT-first contract compiler) with alint as its first consumer; **(e)** the contract is designed with the Phase 2 IR, and the hosts follow in order (native, process, WASM, other-language SDKs); **(f)** tiered trust with a lockfile |

### 13.2 Questions decided in their phase's design document

| ID | Question | Phase | Recommendation |
|---|---|---|---|
| Q1 | Are selectors and relations top-level, rule-local, or both, and how do they relate to ADR-0010's deferred `path_sets:` block? | 3 | both, with local definitions anonymous and top-level ones namespaced; `path_sets:` becomes the selector feature, not a second concept |
| Q2 | How do selectors on full-index rules interact with `--changed`? | 3 | selectors filter compared rows, never path scope; the rule runs when any context row changed |
| Q3 | Do exception relations supersede ADR-0006's rejection of config-embedded suppressions, and what is the stale default? | 4 | yes, by a superseding ADR justified by the stale check; stale exceptions warn by default and fail under the existing strict flag; the first release keeps exceptions in the top-level config file |
| Q4 | Which value types are core? | 2 | primitives with lexeme-preserving numbers, path, lists, and records; semantic scalars (`semver`, SPDX) only with several consumers |
| Q5 | How are object keys and duplicate entries represented across all eight formats? | 1a | as ordered entry rows with spans, never as a JSON object |
| Q6 | What is the smallest join vocabulary that covers the corpus without accidental quadratic plans? | 3 | equality joins on declared fields only, with explicit caps; no implicit cross products |
| Q7 | Are relation rows user-visible in JSON output? | 1b | only through the experimental preview command |
| Q8 | Which existing kinds compile to generic constraints, and which stay native? | 2 | decided per kind from the shadow-port results; graph, Unicode, and Git kernels stay native |
| Q9 | Can snapshot views support every pure rule? | 5 | no; the `KindMeta` inventory declares snapshot compatibility and incompatible rules are rejected per view |
| Q10 | Exact shape of the adapter, analyzer, and fixer worlds in contract 0.x (the roles themselves are decided by DEC-14a) | 2 | separate worlds, even if one artifact implements several; inputs pushed by the host, no host callbacks in 0.x |
| Q16 | Name and repository of the WIT contract compiler, and its first-release scope | now | **Decided 2026-10-14:** `witforge` (`asamarts/witforge`), private first, public with 0.1 once alint's contract `0.x` consumes it; MVP emits Rust types and traits plus `.proto` with the lockfile, JSON Schema next |
| Q17 | The plugin manifest and `alint build` manifest formats, and how plugin kinds are namespaced in `.alint.yml` | 6 | `<plugin>/<kind>` kind names; YAML for `plugins:` in config; TOML for the build manifest |
| Q11 | How do versions mix across `extends:`, and when does `requires:` land? | 1b | v2 consumers may extend v1 sources, never the reverse; `requires:` lands in v1 before Phase 3 |
| Q12 | How are named objects namespaced and protected from shadowing? | 3 | provenance on every named object, namespace-qualified references, no shadowing of local names, plan-time capability monotonicity |
| Q13 | How are paths compared in joins? | 2 | byte-exact on normalized paths, opt-in case folding, never host-dependent |
| Q14 | What is the stability tier of `explain --trace` and previews? | 1b | experimental until v2 is stable |
| Q15 | Is the `when` expression syntax evolved in place or replaced for v2? | 3 | preserve v1 `when`; extend a typed evaluator internally before choosing syntax |

### 13.3 Records this program would amend or supersede

| Record | What changes | When |
|---|---|---|
| Constitution invariant 1 (determinism) | an explicit time input and a declared-host-input class (DEC-11) | Phase 1a |
| Constitution invariant 2 and ADR-0003 (dispatch) | full-index context separated from target selection (Q2) | Phase 3 |
| ADR-0002 (YAML, flat merge by id) | namespaced modules and a `version: 2` envelope (DEC-9, Q12) | Phases 3 and 4 |
| ADR-0004 (trust boundary) | plugin and named-object grants (DEC-14f, Q12) | Phases 3 and 6 |
| `ARCHITECTURE.md` "Plugin model" (native Rust plugins out of scope) | compiled-in native plugins, process plugins, and WASM components (DEC-14) | updated 2026-10-14 |
| ADR-0006 (baseline) | exception relations and the stale default (Q3); fingerprint freeze stated explicitly (DEC-5) | Phases 1a and 4 |
| ADR-0008 (`git_tracked_only` is a kind option) | a selector-level `tracked` predicate implemented for every domain | Phase 3 |
| ADR-0009, ADR-0012, ADR-0013 (explain and output projections) | an evaluating trace and new finding fields under the completeness gates | Phases 1a and 1b |
| ADR-0010 (manifest values never decide verdicts) | policy-source files (DEC-12) | before `editorconfig_conforms` |
| ADR-0017 (fix model) | generic edit plans built on `sync_from` and the deferred section 5.2.4 transaction | after Phase 3 |
| Roadmap backlog and v1.0 milestone | plugin plan of record (DEC-14) and DSL ordering (DEC-9) | on acceptance of DEC-1 |

## 14. Research lessons and references

The proposal borrows design lessons, not syntax wholesale:

- [Open Policy Agent's Rego documentation](https://www.openpolicyagent.org/docs/policy-language)
  demonstrates declarative rules, variable binding, sets, comprehensions, and policy over
  structured data. It also demonstrates the complexity alint should avoid for ordinary
  repository checks. alint needs a smaller integrity-constraint fragment with first-class
  file provenance.
- [OPA policy testing](https://www.openpolicyagent.org/docs/policy-testing) is strong evidence
  that a reusable policy language needs its own fixture/test loop, not only engine unit tests.
- [CEL](https://cel.dev/) is a useful boundary model: a non-Turing-complete expression is
  embedded by a host and accesses only host-provided data. alint should follow that bounded
  principle even if it retains its own syntax.
- [CUE validation](https://cuelang.org/docs/concept/how-cue-enables-data-validation/)
  demonstrates the compositional value of constraints and unification. alint differs by
  centering source locations, repository selection, findings, and fixes rather than producing
  a unified configuration value.
- [JSON Schema 2020-12 Core](https://json-schema.org/draft/2020-12/json-schema-core) separates
  assertions, applicators, and annotations and standardizes instance locations in detailed
  output. That separation supports alint's proposed constraint/evidence distinction.
- [Tree-sitter query syntax](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html)
  and [query predicates](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/3-predicates-and-directives.html)
  show a mature selector/capture model over concrete syntax trees, including error and
  missing nodes. This is a good adapter boundary, not a reason to make syntax parsing the
  core DSL.
- [ast-grep's YAML rule model](https://ast-grep.github.io/reference/yaml) separates finding,
  constraints, transforms, fixes, diagnostics, and file globs. Its
  [rewrite model](https://ast-grep.github.io/guide/rewrite-code) reinforces the need to bind
  edits to captured source nodes rather than infer them from a boolean failure.
- [ESLint's extension model](https://eslint.org/docs/latest/extend/) distinguishes rules,
  parsers, processors, formatters, plugins, and shared configurations. alint needs similarly
  explicit extension roles, adapted to a repository-wide rather than file-only unit of work.
- [ESLint bulk suppressions](https://eslint.org/docs/latest/use/suppressions) reinforce the
  existing baseline design and, especially, the value of detecting and pruning stale
  suppressions. alint should generalize stale-entry checking to explicit exception data.
- [SARIF 2.1.0](https://docs.oasis-open.org/sarif/sarif/v2.1.0/os/sarif-v2.1.0-os.html)
  supports related locations and structured fixes. alint's internal finding model should be
  at least rich enough to preserve those relationships instead of encoding them only in a
  message.
- [The WebAssembly Component Model's WIT overview](https://component-model.bytecodealliance.org/design/wit.html)
  shows why the future plugin boundary should be a typed interface contract rather than a
  guest-specific serialization convention.

Within this repository, the direct predecessors are the v0.12 records:
[`v0.12/architecture_synthesis.md`](v0.12/architecture_synthesis.md) (the five composable axes),
[`v0.12/post_build_coverage_analysis.md`](v0.12/post_build_coverage_analysis.md) (where the
relation axis was declared complete), [`v0.12/cross_file.md`](v0.12/cross_file.md) (the dropped
`file_set` source), and [`v0.12/case_study_100_repos.md`](v0.12/case_study_100_repos.md) (the
coverage taxonomy section 12.3 extends).

The formal recognition/assertion framework, database-integrity analogy, coverage map, and
additional primary references are in
[`rule-coverage-gaps.md`](rule-coverage-gaps.md). The non-negotiable engine properties are in
[`constitution.md`](constitution.md), dispatch and determinism in
[`ADR-0003`](../adr/0003-rule-engine-dispatch-and-determinism.md), trust and confinement in
[`ADR-0004`](../adr/0004-extends-trust-boundary-and-path-confinement.md), baseline semantics
in [`ADR-0006`](../adr/0006-baseline-suppression.md), fix semantics in
[`ADR-0017`](../adr/0017-auto-fix-edit-model-and-applicability.md), and host-linter integration
in [`ADR-0018`](../adr/0018-eslint-integration-boundary.md).

## 15. Review log

**2026-10-14, adversarial review against `main` at v0.18.2.** Four independent passes checked
this document and [`rule-coverage-gaps.md`](rule-coverage-gaps.md) against the code, against
each other, and against the roadmap, constitution, and ADRs, then critiqued the plan as an
executable program. The revision made these changes:

- **Factual corrections.** Remote `extends:` SRI is mandatory, not optional; native path edits
  are root-confined by default, not unconditionally; `scope_filter` has a third, silently
  ignored behaviour; the `Rule` trait exposes scheduling metadata; the environment is also
  read by `{{env.X}}`; the GPG keyring is a host input; Git-aware kinds no-op without Git;
  facts re-evaluate per fix pass; template list, map, and null values are dropped; SARIF
  already emits fixes; ADR-0006's rejection of `suppress:` was about drift, not trust.
- **New analysis.** Section 4.12 tabulates the fail-open and leak sites found in the code, the
  most serious being rule errors that become ordinary, level-dependent, baselinable violations
  and extracted values (possibly secrets) echoed into reports and baseline files.
- **Reconciliation with recorded decisions.** The v0.12 conclusions (section 3.4), constitution
  invariants 1 and 2, and ADRs 0002, 0003, 0004, 0006, 0008, 0010, 0012, 0013, and 0017 are now
  cited where this program touches them, and section 13.3 lists every record it would amend.
- **Filled gaps.** The finding-model compatibility table (section 8.1), lexeme-preserving
  numbers, path semantics, and canonical ordering (section 6.2), the policy-test plan built on
  the existing harness (section 7.7), DSL versioning across `extends:` and tooling (section
  7.8), the plugin reconciliation (section 9.4), a performance budget (section 9.6), the
  capability inventory as a source of truth (section 9.7), phase targets with exit criteria
  and a stop rule (section 11.1), the gap-to-substrate matrix (section 11.8), measurable
  success criteria tied to the v0.12 taxonomy (section 12), and the honest demand caveat.
- **Re-sequencing.** Phase 1 split into fail-loud foundations (1a) and the authoring loop (1b);
  the IR pilots on a new kind before any port, and ports run under differential shadow
  evaluation; edit-plan preconditions moved to the phase that first enables generic fixes;
  stale-exception detection moved from Phase 3 to Phase 4; Phase 5 starts with the LSP overlay.
- **Overstated finding withdrawn.** One pass flagged JSON Schema `pattern` matching as an
  unbounded backtracking surface; `fancy-regex`'s default limit of 1,000,000 steps applies, so
  section 4.12 records only that a limit hit is indistinguishable from a schema failure.
