# What it means for alint to be a general linter

Status: Draft (analysis and architectural proposal).
Decisions: none yet. Any public DSL version, new execution model, or plugin ABI must graduate
to its own design document and ADR before implementation.
Demand evidence: the 100-repository coverage study, the shipped 95-kind catalogue, the
recurring abstractions in `scope_filter`, `Extract`, `cross_file`, `for_each_*`, facts, and
fixers, and the companion [`rule-coverage-gaps.md`](rule-coverage-gaps.md) analysis.

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

alint is already broad: 95 canonical kinds across 13 families, eight structured formats,
cross-file relations, Git-aware rules, baselines, fixes, nested configs, remote rulesets,
eight report formats, and LSP support. Its main limitation is not a shortage of kinds. It is
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
  fixpoint. Native path edits are root-confined and content rewrites have a configurable size cap;
  spawning operations are explicitly privileged because their side effects cannot be confined by
  that algebra.
- Baselines turn findings into stable identities and support incremental adoption without
  making a rule stop protecting new code.
- `extends`, templates, nested configs, bundled rulesets, SRI, and trust gates make policy
  reusable across teams while distinguishing trusted top-level choices from inherited data.

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

The engine shares the walk and coalesces reads for `PerFileRule`, but it cannot generally
see or optimize the middle stages because the `Rule` trait exposes only `evaluate`. Similar
operations consequently have multiple public spellings and multiple implementations.

### 3.3 Concrete signs that an internal algebra already exists

The recurring concepts are visible in the source:

- **Selection** appears as `paths`, global `ignore`, `respect_gitignore`, kind-specific
  `git_tracked_only`, `scope_filter`, `--changed`, `changed_since`, `primary`, `select`, and
  the selector fields of the `for_each_*` kinds.
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
index. `scope_filter` applies to per-file rules and is deliberately rejected by cross-file
rules. Git-tracked selection is kind-specific. CLI changed-mode and rule-level
`changed_since` overlap but have different purposes.

This creates three problems:

1. Users must learn which selection spelling each kind accepts.
2. The same logical set cannot be named and reused by several rules.
3. Cross-file rules need the full repository **as evaluation context**, but that should not
   prevent them from applying an explicit selector to the rows they compare. Full-index
   correctness and rule target selection are different concepts.

### 4.3 Extracted data is string-only, lossy, and private

The shared extractor is valuable but intentionally narrow:

- structured extraction returns only string-valued JSONPath matches;
- non-string nodes are dropped;
- computed-looking values are often skipped by consumers;
- duplicate keys and source spans can disappear during parse-to-`serde_json::Value`;
- path, key, and entry enumeration need custom code;
- each consumer owns its extracted set rather than publishing a reusable relation;
- cardinality and skip behavior vary by caller.

A generic constraint system built directly on this shape would amplify silent false
negatives. Provenance, types, explicit cardinality, and explicit error/skip policy are
prerequisites, not later polish.

### 4.4 Facts and conditions are not a general data model

Facts are scalar `Bool`, `Int`, or `String` values evaluated once. They are appropriate for
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
numbers and booleans are converted to strings, missing placeholders remain literal, chained
templates are forbidden, and templates declare no parameter schema. This is safe and simple
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

- `when: env.*` reads the host environment;
- `custom` facts, `command`, `generated_file_fresh`, `command_idempotent`, and spawning fix ops
  launch processes;
- Git-aware rules inspect repository metadata beyond walked files;
- `git_blame_age` currently compares commit timestamps with `SystemTime::now()`;
- HTTPS `extends` resolves remote content before evaluation, albeit with caching, limits, and
  optional SRI.

The trust gates cover the most dangerous operations, but the evaluation plan and report do
not describe the resulting reproducibility class. In particular, the implicit wall-clock read
means the strict snapshot-determinism definition in section 2.3 is a target invariant, not an
accurate description of every current kind.

Failure is also not typed uniformly at these boundaries. A failed, timed-out, non-zero, or
non-UTF-8 `custom` fact becomes an empty string; an unavailable or detached `git_branch` does the
same. That behavior is compatible with v1's scalar gate model, but a generalized plan must not
confuse “provider failed,” “value is absent,” and “provider successfully returned an empty
string.”

Each plan node should therefore declare one of: hermetic snapshot input, local Git input,
declared host input, or privileged effect. Environment variables must be enumerated in the
plan; time-sensitive policy should receive an explicit `as_of` value (with a documented CLI/CI
source); process and future network nodes remain top-level-capability-gated. Caches, policy
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

Current state and changed-path selection do not cover all transition assertions:

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
snapshots explicitly.

### 5.5 Exception and waiver hygiene

The baseline solves legacy-debt adoption. It is not a full exception model. Real repositories
also have reviewed allowlists and per-policy exceptions that should be:

- narrow and tied to the rule or record they exempt;
- accompanied by owner/reason metadata where required;
- rejected when unused or stale;
- optionally time-limited against an explicit `--as-of` input, never an implicit wall clock;
- visible in reports rather than silently filtered from evaluation.

This should **not** be a generic `suppress:` escape hatch in inherited config. ADR-0006
correctly rejects that trust model. The safer abstraction is an explicit exception relation
owned by the top-level policy, joined against would-be findings, with unmatched exception
rows themselves producing findings. Existing rule-specific allowlists can migrate to this
model internally.

### 5.6 Policy conformance and policy-as-source-of-truth

alint often restates policy already declared in `.editorconfig`, `.gitattributes`, workspace
manifests, release metadata, CODEOWNERS, or dependency-update config. The next general step is
to consume these files as policy inputs and apply their declarations, rather than duplicate
them as literals in `.alint.yml`.

This is different from arbitrary configuration execution. Each supported policy adapter
needs documented semantics, provenance, conflict handling, and a fail-loudly mode. The
`editorconfig_conforms` proposal in the gap study is the flagship case.

### 5.7 Syntax-aware but language-light linting

Regex remains the right default for text. Some useful checks need syntax structure but not
types, control flow, compilation, or package resolution: matching an import node, a call
shape, a front-matter field, or a Dockerfile instruction.

Tree-sitter-style query adapters or a syntax-aware WASM plugin can emit captures with spans.
alint should not bundle a growing grammar zoo in the core binary. Grammar identity, version,
hash, resource limits, parse-error handling, and query semantics must be explicit. Semantic
AST/type/dataflow analysis remains delegated to language tools.

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
   adapters, or fixes.
10. v1 configs remain supported. A v2 surface must not reinterpret existing keys.

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
      manifest: { type: path }
      filename: { type: string }
      severity: { type: level, default: warning }
    rule:
      kind: for_each_file
      select: "**/{{params.manifest}}"
      require:
        kind: file_exists
        paths: ["{dir}/{{params.filename}}"]
      level: "{{params.severity}}"
```

This example remains illustrative. The necessary behavior is:

- missing required, unknown, and unused parameters fail validation;
- whole-node substitution preserves booleans, integers, lists, paths, and enums;
- interpolation inside a larger string is allowed only for scalar stringable types;
- references are namespaced when policies are imported;
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

Existing output formats can project this richer record down. SARIF can preserve related
locations and fixes; terminal output can show a concise primary message with labeled
secondary lines; formats that cannot express the structure must not alter pass/fail.

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
not exceptions. The three concepts should not be conflated.

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

The first constraint release should report only. Later fixes can be enabled when the policy
declares direction and the engine has source spans, for example:

- `canonical: relation-row-selector` for one source of truth;
- `on_missing: insert` with a format-aware insertion strategy;
- `on_drift: set_from canonical.FIELD`;
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

### 9.4 Plugin interface

The future WASM interface should use typed records, ideally a WIT component contract, for:

- plugin metadata and compatible ABI range;
- requested capabilities and resource limits;
- selected resource metadata and bounded content;
- configuration validated against a plugin-provided schema;
- emitted rows or findings with source ranges;
- optional edits with applicability.

Prefer two plugin roles over one unconstrained callback:

1. **Adapter/extractor plugin:** turns a selected resource into typed rows, after which native
   constraints operate normally.
2. **Analyzer plugin:** returns findings for algorithms that do not fit the relation model.

Capabilities are granted by the user's trusted top-level config. No network, inherited
environment, host filesystem traversal, process spawn, or wall clock is available by
default. SRI authenticates bytes but does not confer trust or authority.

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

## 10. What alint should not generalize

The following directions would weaken the product even if they increase expressiveness:

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
part of the design before provenance, testing, and planning are ready.

### Phase 0: terminology and inventory

- Keep the bounded definition from section 1 synchronized across contributor-facing architecture
  and product documentation. `ARCHITECTURE.md` was corrected alongside this proposal.
- Keep the short public tagline "A general linter," but describe the domain as finite,
  inspectable project state in longer copy.
- Create a generated capability inventory per kind: resource domain, dispatch, selectors,
  adapters, capabilities, finding shape, and fixes. This becomes the migration map and a
  drift gate.

### Phase 1: authoring and finding foundations

- Add primary ranges and labeled related evidence to the core finding model and output
  projections.
- Make extraction outcomes carry provenance, types, cardinality, and explicit incomplete
  analysis.
- Add `alint test` for downstream rulesets and fixtures.
- Add `explain --trace` and an extraction/relation preview suitable for debugging.
- Tighten silent-drop sites before making them reusable.
- Define edit-plan preconditions and partial-application reporting before generic constraints can
  propose multi-resource remediation.

This phase has standalone value and reduces the risk of every later phase.

### Phase 2: internal plan and relation IR

- Implement internal resource, selector, adapter, relation, and constraint traits/types.
- Port the shared `ExtractSpec`, manifest-derived scope, `cross_file`,
  `registry_paths_resolve`, and one `for_each_*` path to the IR behind unchanged v1 syntax.
- Prove parse/read deduplication, deterministic ordering, error semantics, and performance on
  the million-file benchmark.
- Keep `file_graph` and other specialized kernels native but make them consume/emit shared
  relation/finding records where useful.

This is the go/no-go proof. If several real kinds do not become simpler and more consistent,
do not expose a generic DSL.

### Phase 3: additive selectors and read-only constraints

- Design and ADR the version boundary: additive v1 keys versus `version: 2`. The default
  recommendation is v2 for the new top-level typed namespace while continuing v1 support.
- Expose named selectors and relations.
- Ship the check-only `constraint` kind with cardinality, unique/key, reference, equality,
  set, group, and bounded-count assertions.
- Implement the first high-value policies from the gap study as conformance cases: locale
  key parity, dependency version consistency, and stale exception detection.
- Ship specialized wrappers or bundled templates for common cases so most users need not
  author raw relations.

### Phase 4: typed modules and exception data

- Add typed template parameters and namespaced module references.
- Let rule modules ship tests and minimum-engine/capability metadata.
- Design explicit top-level exception relations, including reverse/stale checks and
  optional explicit-as-of expiry.
- Add policy-impact comparison between configs.

### Phase 5: snapshot views and incremental evaluation

- Introduce worktree, overlay, index, and Git-tree snapshot implementations.
- Add staged-only and revision checking once parity tests cover all applicable rule classes.
- Build dependency-aware LSP/watch invalidation from the compiled plan.
- Keep process-spawning and mutable-fix rules restricted to compatible snapshots.

### Phase 6: typed plugins and syntax adapters

- Freeze the WIT ABI only after the native relation/finding model has stabilized.
- Ship adapter and analyzer roles with capability manifests, fuel/memory limits, SRI, and
  test fixtures.
- Validate one syntax-query adapter and one non-syntax domain adapter before promising a
  registry ecosystem.

## 12. Validation and success criteria

### 12.1 Product criteria

The proposal succeeds if:

- a user can express the recurring extract/group/compare policies without adding Rust;
- the common built-in kinds remain simpler than the generic spelling;
- incomplete analysis is visible and cannot produce a false pass;
- a finding explains the failed relation and navigates to all material evidence;
- policy packs are testable without cloning a large real repository;
- the trust boundary is no weaker than v1;
- v1 configs and output pass/fail semantics remain stable.

### 12.2 Engineering criteria

Before public DSL release, require:

- semantic parity tests for every v1 kind moved onto the shared plan;
- firing and silent end-to-end cases for every new assertion operator;
- property tests for relation ordering, grouping, joins, cardinality, and normalization;
- mutation tests proving parse/type/truncation errors cannot become empty-success;
- cross-formatter tests for primary and related locations;
- baseline identity tests under row reordering and benign source movement;
- trust tests for every way a remote/nested policy can reference selectors, modules,
  adapters, exceptions, or fixes;
- bounded-resource tests for adversarial documents and relation explosions;
- benchmark comparison for small, 100k-file, and million-file repositories;
- LSP/CLI parity tests for any snapshot-supported subset;
- JSON Schema/runtime parity and generated documentation gates.

### 12.3 A useful coverage metric

Do not measure generality by the number of kinds alone. Track new policy demand by outcome:

- expressible by an existing specialized kind;
- expressible by configuration of a generic constraint;
- needs a new reusable adapter/operator;
- needs a specialized native kernel;
- delegated to a plugin/command;
- out of scope.

The desired trend is that most new **static repository-state** requests land in the first two
buckets, while semantic/external-state requests are cleanly delegated or declined. A growing
kind count can still be healthy when kinds are ergonomic wrappers over stable substrates.

## 13. Open decisions

These questions require separate design work, not answers smuggled into implementation:

1. Does the generalized surface require `version: 2`, or can named selectors/relations be
   added to v1 without constraining the eventual language? Recommendation: use v2 unless the
   internal IR proves the surface can remain small and purely additive.
2. Are relations top-level reusable objects, rule-local objects, or both? Recommendation:
   support both, with local definitions anonymous and top-level definitions namespaced.
3. Is the expression syntax evolved in place or replaced for v2? Recommendation: preserve
   v1 `when`; extend a typed evaluator internally before choosing syntax.
4. Which value types are core? Recommendation: primitive values, path, lists, and records
   first; add semantic scalars only with multiple demonstrated consumers.
5. How are object keys and duplicate entries represented across all eight formats without
   forcing them into a misleading JSON object model?
6. What is the smallest join vocabulary that covers the corpus without enabling accidental
   quadratic plans?
7. Should relation rows be user-visible in JSON output, or only through a bounded debug
   command?
8. How does baseline identity treat a relational finding with several equally important
   locations?
9. What exception metadata is mandatory, and how is an explicit reference date supplied in
   local development and CI?
10. Which existing kinds should compile to generic constraints, and which should stay native
    permanently?
11. Can snapshot views support every pure rule, or should the registry expose a compatibility
    matrix and reject incompatible rules per view?
12. Does a plugin emit rows, findings, or both in ABI v1? Recommendation: define separate
    adapter and analyzer worlds even if one artifact implements both.

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

The formal recognition/assertion framework, database-integrity analogy, coverage map, and
additional primary references are in
[`rule-coverage-gaps.md`](rule-coverage-gaps.md). The non-negotiable engine properties are in
[`constitution.md`](constitution.md), dispatch and determinism in
[`ADR-0003`](../adr/0003-rule-engine-dispatch-and-determinism.md), trust and confinement in
[`ADR-0004`](../adr/0004-extends-trust-boundary-and-path-confinement.md), baseline semantics
in [`ADR-0006`](../adr/0006-baseline-suppression.md), fix semantics in
[`ADR-0017`](../adr/0017-auto-fix-edit-model-and-applicability.md), and host-linter integration
in [`ADR-0018`](../adr/0018-eslint-integration-boundary.md).
