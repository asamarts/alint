# Design doc: `KindMeta`, the per-kind capability inventory (W1)

Status: Accepted design (2026-10-14); all five questions in section 7 are decided. Not yet implemented.
Decisions: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md) (Phase 0)
and K1 to K5 in section 7; no new ADR (the trust gates' source of truth changes, not their policy).
Demand evidence: the inventory of all 95 kinds in [`kind-inventory.md`](kind-inventory.md), which
found 19 hand-maintained per-kind lists (its last table; 10 of them derive from `KindMeta`, section
3.2, and 3 are wrong or stale), 57 places where a configured field is silently ignored, and 4
reproducible bugs that per-kind metadata would have prevented (a fifth suspected case,
`no_submodules` under `--changed`, proved to be intended behaviour).

> Part of [v0.19](README.md) (work item W1). It is the source the trust gates, the audits, the
> fail-loud checks of W3, the plugin registry, and the general-linter migration map read from.

## 1. Problem

What each rule kind is and needs (how it dispatches, which selectors it honours, which
capabilities it uses, which fix operations it accepts) exists today only implicitly, spread over
builder functions, trait methods, and nine hand-maintained lists. The inventory shows the cost:

- **Silent drops.** A configured field that a kind does not read is accepted and ignored:
  `paths:` on 22 kinds, `fix:` on 24 kinds, `scope_filter` (`has_ancestor`, `changed_since`) on
  11 kinds, a `changed_since` on `dir_absent` that makes the rule match nothing, and nested rules
  that drop `fix`, `git_tracked_only`, and `respect_gitignore` without a message. The JSON Schema
  allows all of these on every kind, so editors do not flag them either.
- **Wrong or stale lists.** The Git-required kind list in the coverage audit names 3 of the 10
  kinds that only work inside Git. The documented-kinds test list misses 19 kinds. The spawn
  audit only sees `Command::new` in `alint-rules`, so the 10 kinds that run `git` through
  `alint_core::git` are invisible to it. A second alias table is generated from the docs and
  used by the CLI and LSP, in parallel to `register_alias`.
- **Bugs** (reproduced at b6ba88f4; fixed in the v0.18.3 patch per decision K2, section 6):
  1. A nested `git_blame_age` never fires unless some top-level rule also uses blame: the engine
     decides whether to build the blame cache from top-level rules only.
  2. Structured `*_path_*` kinds with literal `paths:` skip `scope_filter` in the rule-by-rule
     path, so `check` passes while `alint fix` edits a file the filter excluded.
  3. A nested `.alint.yml` re-roots every `select` as a path glob, which corrupts the regex
     `select` of `for_each_match` and `ordered_block`, and lets kinds whose real selectors are
     never re-rooted (the Git kinds, `pair_hash` `source`, `cross_file`, `registry_paths_resolve`,
     `file_graph`) into a nested config with a dummy `paths:`.
  4. Under `--changed`, `no_case_conflicts` and `max_files_per_directory` make whole-tree
     judgements without being full-index, so changed-mode runs miss real violations.
     (`no_submodules` also skips an unchanged `.gitmodules` under `--changed`, but that matches
     `file_absent` and the `--changed` contract, so it is not a bug.)

  Review of the v0.18.3 patch found ten more instances of the same classes, fixed in the same
  patch: a nested rule using `extends_template:` took its kind and selectors from a root template
  unconfined; nested `require:` blocks and several kind-specific path fields were not re-rooted;
  nested `fix`, `git_tracked_only`, and `respect_gitignore` were silently stripped; under
  `--changed`, scope matching and reference lookups used the changed files only (so a
  `has_ancestor` marker outside the diff never counted, and `markdown_paths_resolve` misreported
  targets), and a `json_schema_passes` schema-only change skipped the rule; and `file_exists`'s
  literal stat ignored `git_tracked_only`. Each is a capability or selector fact the records must
  express (section 3.1).

## 2. Surface area

No new user-facing configuration. The visible changes are:

- configured fields a kind does not honour become load errors (decision K1), reported by both
  `check` and `validate-config` with the kind and field named;
- `facts.json` gains a `kind_meta` section;
- the JSON Schema rejects those fields per kind (decision K4), so editors flag them;
- `alint rules show <kind>` and `explain` print a capability summary (decision K5).

## 3. Semantics

### 3.1 The record

`KindMeta` is a static description declared once per canonical kind; aliases share their
canonical kind's record. Proposed shape (names indicative):

| Field | Values (from the inventory) |
|---|---|
| `dispatch` | `PerFile` (51 kinds; since v0.18.3 one of them, `markdown_paths_resolve`, is also `FullIndex`, which makes the per-file pass evaluate every in-scope file under `--changed`), `FullIndex { path_scope: bool }` (20 since v0.18.3, was 18; `file_exists`, `file_absent`, `no_case_conflicts`, and `max_files_per_directory` expose a scope as their `--changed` skip predicate), `RuleMajorScoped` (14, was 16), `SingleShot` (10). No kind's dispatch depends on options. |
| `paths` | `Required`, `Optional`, `Forbidden` |
| `scope_filter` | `Full`, `AncestorOnly` (today's `dir_absent`), `Rejected { reason }` |
| `git_tracked_only` | `None`, `FileOnly`, `DirAware`; plus `respect_gitignore` and `root_only` flags |
| `selectors` | named fields with a type: `Glob`, `GlobList` (with `!` excludes), `BasenameGlob`, `LiteralPath`, `PathTemplate`, `Regex`, `DiffGlob`, `Extract` |
| `since` | `None`, `Optional` (falls back to HEAD), `Required` |
| `capabilities` | `SpawnUserCommand`, `Git(ops)` (tracked set, blame, diff, log, verify-commit), `HostKeyring`, `EnvRead`, `EnvExport`, `WallClock`, `OutOfRootRead`, `WritesDuringCheck`, `UnixOnly`; each either unconditional or conditional on a named option |
| `content` | `PathOnly`, `Metadata`, `Prefix(n)`, `WholeFile`, `OtherFiles`, `Blame`, `Delegated` |
| `formats` | the `Format`s the kind parses (structured kinds, `json_schema_passes`, extract-based kinds) |
| `dependencies` | repository files outside the kind's scope whose change alters its verdict, as `Rule::changed_dependencies` (v0.18.3) exposes per instance (today `json_schema_passes`'s `schema_path`) |
| `fix_ops` | the accepted fix operation names; empty means a `fix:` is rejected |
| `expect_matches` | supported or not (68 kinds support it) |
| `nested_rules` | whether the kind carries nested `require:` rules (`for_each_dir`, `for_each_file`, `every_matching_has`) |

Option-dependent behaviour is recorded as conditional capabilities, not as different records:
for example `file_exists` has `Git(tracked set)` conditional on `git_tracked_only`, and
`git_commit_message` has `EnvRead` conditional on a legacy `${VAR}` in `since:`. Conditions are
not only boolean options, so the condition type needs variants: an option being set, an option's
value matching a predicate (the legacy `${VAR}`), a fix op being configured (`file_absent`'s
`git_untrack` spawns Git), and a common field (every kind that honours `scope_filter` gains Git
through `changed_since`; that one is derived engine-wide rather than declared on 66 records).

Records are `&'static` for built-in kinds and owned (`Arc<KindMeta>`) for plugin kinds discovered
at runtime, and serializable for `facts.json` and the plugin handshake. The selector and option
presence facts (`since`, `git_tracked_only`, `selectors`) duplicate each kind's option schema, so a
parity gate checks the record against the schema (after K4 migrates the 20 hand-written schemas).

Fix operations get their own small record (`FixOpMeta`): the trust class (`Spawning`,
`ContentInjecting`, `FixedBehaviour`), from which `SPAWNING_FIX_OPS` and
`CONTENT_INJECTING_FIX_OPS` derive. The 26 operations split 2, 18, and 6 today.

### 3.2 What derives from it

| Derived item | Today | After W1 |
|---|---|---|
| `SPAWNING_RULE_KINDS` (trust gate) | hand list of 3 | kinds with any `SpawnUserCommand`, conditional or not (the gate runs on raw YAML before options resolve, so it gates by kind; deriving from unconditional spawning only would let a kind that spawns under an option escape the gate) |
| Git-required kinds (audit) | hand list of 3, should be 10 | kinds with unconditional `Git` |
| `SPAWNING_FIX_OPS`, `CONTENT_INJECTING_FIX_OPS` | hand lists | `FixOpMeta` trust class |
| nested-config re-rooting (`REGEX_SELECT_KINDS`, `UNCONFINABLE_KINDS`, `NESTED_RULE_KINDS`, and the kind-specific path fields in `alint-dsl/src/nested.rs`, added in v0.18.3) | three hand lists and a hand `match` | a per-selector `rerootable` flag on `selectors` (selector types alone cannot reproduce the list: `pair_hash.source` and `pair_changed_together.if_changed` are glob-typed but cannot be confined) |
| `FAIL_ONLY_KINDS` (docs export) | hand list | kinds with `HostKeyring` |
| nested-rule kind lists (when_iter audit, template expansion) | two hand lists | `nested_rules` |
| nested glob options (template expansion) | hand table | `selectors` typed `Glob`/`GlobList`/`DiffGlob` |
| `expect_matches` exemptions (options table) | hand list | `expect_matches` |
| option-less registration set | implicit | kinds with no option schema |
| documented-kinds test list | stale hand list | the registry |
| per-kind JSON Schema common fields | `rule_common` allows everything | `paths`, `scope_filter`, `fix`, `expect_matches` allowed only where the record says so |

### 3.3 Enforcement at load

The rule builder validates the configured spec against the record before building:

- a field the kind does not honour and that loads silently today (`paths`, `scope_filter` or one of
  its predicates, `fix` or any fix op on a kind with no fixer) is an error naming the kind and
  the field, with the supported alternative where one exists. (`expect_matches` and the kind
  options `git_tracked_only`, `respect_gitignore`, and `since` are already rejected at top level;
  `KindMeta` takes over those checks and extends them to nested rules.) Validation runs on the
  template-expanded rule and names the template or `extends:` source the field came from; an
  SRI-pinned remote ruleset that sets an ignored field becomes unloadable until its author fixes
  it, which the changelog states;
- `validate-config` runs the same validation, so manifest-predicate errors no longer wait for
  `check`;
- nested rules are validated against their own kind's record, as the nested pipeline delivers
  them: today `for_each_*` strips a nested `fix`, `git_tracked_only`, and `respect_gitignore`
  (`PARENT_FIELDS` in `config.rs`), so each of those becomes an error in a nested rule rather
  than being silently stripped. Nested specs are validated once at load, not per iteration.

### 3.4 Where the record lives (decision K3)

The DSL's trust gates run in `alint-dsl`, which depends only on `alint-core` and deliberately
works on raw YAML before any registry exists. Three options: (a) define every kind's record in a
central table in `alint-core`; (b) declare records next to each kind in `alint-rules` and
generate a constant trust table into `alint-dsl` with an `xtask` generator and a `--check` gate;
(c) declare records next to each kind, register them with the builder, and pass a kind catalog
into the DSL loader. Option (c) is the only one that also works for plugin kinds, which are not
known at compile time (DEC-14). **Decided: (c) (K3).**

The catalog API, to be fixed in the implementation design: `meta(kind) -> Option<&KindMeta>`
(resolving aliases), `canonical(kind)`, `kinds()`, and `fix_op(name) -> Option<&FixOpMeta>`,
injected through the existing `LoadOptions`. It fails closed: loading without a catalog must not
skip the trust gates, and a kind the catalog does not know in an `extends:` source is treated as
spawning (denied), never as non-spawning. Call sites: the CLI (`check`, `fix`, `validate-config`),
`init`, `suggest`, `export-agents-md`, the LSP, bundled rulesets loaded through `extends:`, and the
bench harness. `FixOpMeta` can be a static table in `alint-core`, next to `FixSpec`.

## 4. False-positive surface

"False positive" here means a configuration rejected that should load:

- **Configs that set an ignored field today.** Every such config currently has a field with no
  effect; after W1 it fails to load. This is the intended fail-loud change, but it is breaking:
  decision K1 makes it a hard error.
- **Bundled rulesets and examples.** Any bundled ruleset or documented example that sets an
  ignored field must be corrected in the same change; a gate loads every bundled ruleset and
  every documented example.
- **Templates and `extends:`.** A field set by an inherited template on a kind that ignores it
  would now error in the consumer's config; the error must name the template or source.
- **Conditional capabilities.** Recording a capability as unconditional when it depends on an
  option would over-gate (for example, treating `file_exists` as Git-required). The
  instance-conformance audit (section 5.3) checks conditional capabilities in both states.

## 5. Implementation notes

### 5.1 Location and types

`KindMeta`, `FixOpMeta`, and their enums live in `alint-core` (a new `kind_meta` module), so the
engine, DSL, rules, output, and plugin crates can all name them. Where each kind's values are
declared is decision K3.

### 5.2 Migration order

1. Add the types and `RuleRegistry::register` taking a record (aliases inherit; the second alias
   table generated from the docs is reconciled against the registry).
2. Declare the 95 records from the inventory, corrected for the v0.18.3 changes; the
   instance-conformance audit (section 5.3), not the markdown snapshot, checks them.
3. Inject the `KindCatalog` (K3) into every loader call site, then derive the trust lists and audit
   lists from it (including v0.18.3's two nested-config lists); keep the old hand lists for one
   release as cross-checks that must equal the derived ones, then delete them.
4. Fix any bundled ruleset, documented example, or scenario that sets an ignored field (a scan of
   the 22 bundled rulesets found none at top level), then turn on load-time enforcement
   (section 3.3) with the K1 policy, including context-aware nested validation.
5. Migrate the 20 hand-written option schemas, generate the per-kind schema restrictions (K4) and
   the `facts.json` section, and add the record/schema parity gate.
6. Add the capabilities summary to `rules show` and `explain` (K5).

### 5.3 Gates

- **Instance conformance.** For every rule instance built from the scenario corpus, the instance's
  trait answers (`as_per_file`, `requires_full_index`, `path_scope`, `git_tracked_mode`,
  `wants_git_blame`, `supports_expect_matches`, `fixer`) agree with its record, with each
  conditional capability exercised in both states. Capabilities the trait cannot reveal
  (`SpawnUserCommand`, `EnvRead`, `WallClock`, `OutOfRootRead`, `HostKeyring`) are checked through
  instrumented chokepoints in `alint_core::process` and `alint_core::git`; the direct
  `Command::new` calls in `command.rs` and `fixers/git_ops.rs` are routed through them first.
  "Honours `paths` / `scope_filter`" is checked behaviourally, by per-kind probes generated from
  the records (as the inventory did by hand), not by relying on the scenario corpus.
- **Spawn and Git audits** extend from grepping `alint-rules` to also covering `alint_core::git`
  and `alint_core::process` call sites, and must agree with the records.
- **Facts gate** covers the new `kind_meta` section; README counts stay derived.
- **Bundled-ruleset and examples gate** loads everything under enforcement.

### 5.4 Constitution and performance

No change to evaluation. Validation runs once per rule at load, so the deterministic performance
gate should show no change; the budget of DEC-13 applies.

## 6. Tests

- Unit tests per enum and for the derivations (each derived list equals the old hand list, or the
  corrected value where the hand list was wrong: the Git-required list grows from 3 to 10).
- A firing scenario per enforcement rule (ignored `paths`, `fix`, `scope_filter` predicate,
  nested `fix`, unsupported fix op) and a silent scenario per kind with a valid config (the
  existing silent scenarios).
- Regression scenarios for the four bugs of section 1, landing with their v0.18.3 fixes:
  1. nested `git_blame_age` fires without a top-level blame rule (v0.18.3 visits nested rules
     through `wants_git_blame()`; the conformance gate asserts it agrees with `Git(blame)`);
  2. `alint fix` never edits a file that `scope_filter` excludes (the literal-paths fast path
     applies the scope);
  3. nested-config re-rooting: regex `select` is not prefixed, kinds whose selectors cannot be
     re-rooted are rejected in nested configs, and confinement runs after template expansion
     (v0.18.3 ships these as hand lists; W1 derives them from the records);
  4. `no_case_conflicts` and `max_files_per_directory` become full-index and keep `path_scope` as
     the `--changed` skip predicate.

## 7. Open questions

Decided one at a time; each answer is recorded here with its date.

| ID | Question | Recommendation | Decision |
|---|---|---|---|
| K1 | Ignored fields: hard error in v0.19, or a warning for one release then an error? | hard error in v0.19 (a pre-1.0 minor; every such field has no effect today), with a migration note listing the field and kind | **2026-10-14: hard error in v0.19**, in both `check` and `validate-config`, naming the kind, the field, and the alternative; bundled rulesets and examples fixed in the same change |
| K2 | Ship the four bugs as a v0.18.3 patch first, or with v0.19? | patch bugs 2 and 4 in v0.18.3 (a fix that writes an excluded file, and changed-mode false negatives); bugs 1 and 3 with W1 and W3 | **2026-10-14: all four in a v0.18.3 patch**, ahead of W1, without waiting for `KindMeta`; W1 then re-expresses the fixes for bugs 1 and 3 through the records (blame from `Git(blame)`, re-rooting from typed selectors) |
| K3 | Where records are declared and how the DSL sees them | next to each kind in `alint-rules`, registered with the builder, and a kind catalog passed into the DSL loader (works for plugin kinds) | **2026-10-14: as recommended** (per-kind records, a `KindCatalog` injected into the DSL loader; DSL unit tests use a test catalog) |
| K4 | Generate per-kind JSON Schema restrictions in W1? | yes, so editors flag what the loader rejects | **2026-10-14: yes, in W1**, including migrating the 20 kinds whose option schemas are still hand-written, with a schema/runtime parity gate |
| K5 | Show capabilities in `alint rules show` and `explain`? | yes, a one-line capability summary; low cost | **2026-10-14: yes**, a capabilities line plus accepted selectors and fix ops, in human and JSON output (ADR-0009 envelope), under the ADR-0012 completeness gates |
