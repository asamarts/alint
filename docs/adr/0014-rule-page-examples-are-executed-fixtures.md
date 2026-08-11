---
status: accepted
date: 2026-08-10
decision-makers: asamarts
---

# 0014. Rule-page examples are executed scenario fixtures

## Status

Accepted. Companion design doc:
[`docs/design/v0.15/documented-example-fixtures.md`](../design/v0.15/documented-example-fixtures.md).
This extends the "docs are a tested contract" line ADR-0012 opened for
`explain`/`list` output to the generated per-kind rule pages on alint.org, and
builds on the scenario harness (`crates/alint-testkit`) and the docs-export
pipeline (`xtask/src/docs_export.rs`). The decision below is the intended
end-state; the implementation is phased (see the design doc) and each phase is
guarded by a gate before the next begins.

## Context

Every rule kind gets a generated documentation page under
`alint.org/docs/rules/<kind>` (built by `xtask docs-export` from `docs/rules.md`
family sections). Each page carries a worked example: a `.alint.yml` snippet and
prose describing what the rule flags. Today those examples are **hand-authored
YAML in `docs/rules.md`, disjoint from anything the test suite actually runs.**
Two independent bodies of work touch examples, and they do not meet:

- **The e2e scenario suite** (`crates/alint-e2e/scenarios/`, ~315 files) is
  real: `crates/alint-testkit/src/runner.rs` materialises a mock repo tree,
  writes the `.alint.yml`, optionally drives git, and runs the actual `Engine`.
  `coverage_audit_pass_fail.rs` already proves **every** registered kind has a
  firing scenario and a silent (passing) scenario - except the seven in
  `NATIVE_FIRES_ALLOWLIST`, whose firing case needs a filesystem or git
  primitive the testkit cannot yet materialise (mode bits, symlinks, backdated
  or message-bearing commits, commits with real file deltas).
- **The docs example gates** are shallow. `coverage_audit_doc_examples.rs`
  checks only that each page's YAML *parses*; it never runs it.
  `docs_export.rs::enforce_example_gates` checks only that each kind's H3 has a
  fenced YAML block whose `kind:` matches. Nothing asserts the example
  corresponds to a real repository, produces the output the prose claims, or
  even that the config is valid against a live tree.

So the linter that exists to prevent drift ships documentation whose examples
are verified only to be *syntactically well-formed*. An example can name a
config that no longer loads, describe output the rule no longer produces, or
show a repo shape the rule never actually flags, and every gate stays green. The
real, executed fixtures that *would* catch this already exist one directory over
- they are simply not the thing the docs render.

The user requirement is explicit: every rule's page must show a real
example repository and the exact tested config, every kind must carry
comprehensive end-to-end coverage against a real mock repo, and the examples on
the page must be *the same artifacts the integration tests execute* - strictly
and automatically enforced, so a new kind cannot ship without it.

## Decision

We will **make each rule page's examples the exact scenario fixtures the e2e
harness executes**, rendered from the scenario and re-verified at generation
time. Concretely:

1. **Designation via a `docs:` block on the scenario.** `Scenario`
   (`crates/alint-testkit/src/scenario.rs`) gains an opt-in
   `docs: { title, case, kind, order }` field (`case` is `fail` | `pass`). A
   scenario carrying it is a *documented* scenario; docs-export renders exactly
   those. This is one source of truth (the fixture is the example), lets an
   author curate a clean minimal example distinct from edge-case fixtures, and
   needs no second manifest to drift against. The block carries an **explicit,
   registry-validated `kind`** rather than inferring it from the scenario's
   `tags:` - tags are a free-form taxonomy that 42% of the corpus does not
   usefully key a kind on (102 of 315 scenarios carry no kind tag, 29 carry two)
   - and the gate constrains a documented scenario's `given.config` to a single
   canonical kind equal to `docs.kind`, so the label cannot lie about what the
   fixture exercises. Filename-convention and separate-manifest designation are
   rejected (see Considered Options).

2. **Every kind's page shows a firing (`fail`) example and a compliant (`pass`)
   example.** Both states, always - "what the rule catches" and "what a clean
   repo looks like." Edge-case scenarios stay tested but unshown; an author may
   promote a notable one to a third `docs:` entry. Fail-only and
   mandatory-three-cases are rejected.

3. **The shown output is captured from a real `alint check` run at generation
   time.** docs-export materialises the documented scenario's `given.tree` into a
   tempdir, writes `given.config` as a sibling **outside** that tree (so the config
   file is not itself walked and self-matched), builds the `alint` binary - the
   build hoisted ahead of page rendering, where today it runs later for `--help` -
   and spawns `alint check -c <config> .` from inside the tree, capturing stdout as
   the "what alint reports" block. The `docs-export --check` gate re-runs it, so
   any drift fails the build. The run is byte-deterministic only under a
   **pinned-invocation contract** (config outside the tree; `.` target; fixed
   `TERM`/`LC_ALL`; `--color=never`; pipe capture), and a few kinds need
   fixture-level care - `git_blame_age` (a wall-clock-relative default message,
   overridable via `message:`), the three spawning kinds (`command`,
   `command_idempotent`, `generated_file_fresh`, which embed subprocess output),
   and `git_commit_message` (HEAD-only mode, to avoid a drifting abbreviated SHA) -
   all specified in the design doc. The rendered `fail` case is a *real violation*
   rather than a rule error because the same scenario is asserted by the runner
   harness's `expect:`. There is no stored golden and no hand-rendered replica;
   synthesising the block from `expect:` is rejected because it re-introduces a
   hand-maintained renderer that can diverge from the real CLI.

4. **The example gate becomes a real-fixture gate.** `enforce_example_gates` is
   re-architected from "the H3 has a matching-kind YAML block" (a post-hoc text
   check) into a materialise+spawn+assert loop: every canonical kind resolves to a
   linked documented `fail` **and** `pass` scenario; `docs.kind` is registered and
   equals the scenario's single top-level rule kind (by the `walk_rules` id+kind
   rule - a composite kind's nested rule must omit `id:`); the config is hermetic
   (inline or offline `alint://bundled` extends, no `http(s)://`) and rendered
   byte-for-byte; and a live re-run reproduces the output, with the `fail` case
   exiting non-zero and the `pass` case clean. A kind with no documented pair fails
   the gate. It is migration-aware from Phase 1 and hard-flips at Phase 4, retiring
   only `docs/rules.md` from the old load-gate's scope (which still guards three
   other doc files).

5. **The testkit grows the primitives that zero `NATIVE_FIRES_ALLOWLIST`.** The
   materialiser gains an executable (`+x`) file node and symlinks; the git
   harness gains per-commit messages, `GIT_AUTHOR_DATE` overrides, and commits
   that stage real file deltas - the primitives the allowlist's own doc-comment
   names as its retirement path (an executable-bit node is all the two executable
   kinds check; nothing needs arbitrary chmod). The two changeset rules need a
   two-commit history (`since: HEAD~1`; a single commit hard-errors on the diff
   range), and the DSL expresses addition deltas only - enough to fire every
   target kind, though a modify-based example is out of scope. All seven kinds
   then express their firing case as ordinary scenarios shown on their pages; the
   allowlist goes to empty. No page is special-cased and no firing example is left
   unverified.

6. **Migration is piloted, then fanned out, then hard-flipped.** Phase 1 lands
   the plumbing and proves it on one family (existence). Phase 2 extends the
   harness. Phase 3 migrates all 78 kinds via a per-family multi-agent workflow
   whose drafts must pass a real gen-time run and are curated for quality before
   commit. The gate is migration-aware from Phase 1 (it accepts either a
   documented scenario or today's hand-written example per kind), each family
   moving in one **atomic swap commit**, and flips to hard - dropping the
   hand-written branch - only once all 78 are migrated (Phase 4), so the drift
   guard is never half-enforced and the old presence check never fires on a
   just-removed example. A soft-launch that gates new kinds immediately while a
   "temporary" backfill allowlist drains is rejected.

This changes no rule's runtime behaviour and no `.alint.yml` semantics. It adds
a scenario field, a docs-export render+verify path, a stricter gate, and testkit
primitives; the observable product surface is unchanged except for richer,
provably-real rule pages.

## Consequences

Easier:

- **The docs cannot lie.** Every rendered example is a fixture the suite runs
  and the build re-runs; a stale config or a changed message breaks the build,
  not a user's trust. This is ADR-0012's tested-contract guarantee extended from
  `explain`/`list` to the public rule pages.
- **One source of truth.** The example *is* the test. Authoring a rule's docs
  and authoring its e2e coverage become the same act.
- **New kinds carry coverage by construction.** The gate makes a documented
  `fail`+`pass` pair a merge requirement, so the "add a rule, forget the
  example/tests" gap closes structurally rather than by review vigilance.
- **The allowlist disappears.** Seven kinds stop being second-class; the
  new primitives (an executable-bit node, symlinks, richer git history) are
  reusable by any future rule.

Harder, and accepted:

- **docs-export gains materialise + spawn, and ~156 gen-time runs.** Two runs
  per kind land on the 7-minute release-build `--check`. They run **sequentially**
  (docs-export has no threading today; calling them parallelisable is aspirational),
  and the git-backed fixtures add real per-fixture `git` subprocess cost - budget a
  low-tens-of-seconds addition. The release path grows a real dependency on a
  working built binary and on a **deterministic-output contract** the `--check`
  gate enforces (the naive "relative paths, sorted findings" assumption is
  insufficient - see the design doc's pinned-invocation contract). docs-export
  takes a normal `alint-testkit` dependency it did not have, pulling `proptest`
  and `thiserror` into its build graph.
- **The scenario schema grows a `docs:` block**, and ~156 scenarios must opt in
  and be curated to minimal, idiomatic examples - user-facing content, not just
  passing fixtures.
- **The harness extension is real work**, the commits-with-file-deltas DSL most
  of all, and it touches the untagged `TreeNode` representation. The reserved-key
  executable/symlink nodes carry an **unavoidable collision** (a directory whose
  sole child is a file literally named `$exec`/`$symlink` is byte-identical to the
  special node), so `$exec` and `$symlink` become reserved filenames enforced by a
  corpus guard - see the design doc. It is bounded and reusable, but it is not free.
- **A raised contribution bar.** Every new kind now *requires* a documented,
  executed `fail`+`pass` pair to merge. That is the point, but it is friction on
  the smallest rule addition, and the gate's error messages must make the
  requirement obvious or it becomes a stumbling block.
- **The new dependency edge cascades into the crate-graph artifacts.** Because
  `xtask -> alint-testkit` is a normal edge, the landing commit must regenerate
  `crate-graph.gen.c4`, `crate-graph.md`, and `DIAGRAMS.md` (`gen-arch` + Node-22
  `gen-mermaid`), or `gen_arch_check_passes_on_committed_tree` reds `cargo test`
  on *every* platform (including the Windows/macOS `cross-platform` matrix).
  Accepted as a one-time regeneration.
- **The render+verify path is ubuntu-only and must stay CLI-only.**
  `docs-export --check` runs only on ubuntu, so committed pages embedding
  unix-only fixture output are safe; but the verify path must not gain a `#[test]`
  mirror, or `cargo test --workspace` would run it on Windows/macOS where the
  unix fixtures and the binary spawn break.

## Considered Options

- **Designation - `docs:` block (chosen)** vs a `<kind>_pass.yml`/`_fail.yml`
  filename convention vs a separate `rules-examples.toml` manifest. The
  convention cannot distinguish a curated example from an edge-case fixture and
  is ambiguous for kinds with several pass/fail files; the manifest is a second
  source of truth to keep in sync - the exact drift class this project exists to
  prevent.
- **Shown cases - fail + pass (chosen)** vs fail-only vs mandatory fail+pass+edge.
  Fail-only omits the "what compliant looks like" half that adopters need;
  mandatory-edge forces contrived edges onto kinds that have none.
- **Output source - real gen-time run (chosen)** vs a captured golden file per
  scenario vs synthesising from `expect:`. The golden adds ~156 artifacts and a
  separate staleness gate for no gain over re-running; synthesising re-creates a
  hand-maintained output renderer that can drift from the real CLI - the precise
  failure this feature exists to remove.
- **Native-fires seven - extend the harness to zero the allowlist (chosen)** vs
  special-casing their pages vs a hybrid that defers the commit-delta DSL.
  Special-casing leaves seven unverified pages permanently; the hybrid leaves
  two, and "temporary" allowlists linger (this feature's own allowlist is the
  evidence).
- **Migration - pilot then workflow-draft-and-curate then hard-flip (chosen)**
  vs fully serial authoring vs a soft-launch with a draining backfill allowlist.
  Serial is the slowest path for naturally parallel work; the soft-launch ships
  enforcement soonest but leaves most pages hand-written meanwhile and risks a
  permanent "temporary" allowlist.

## More Information

- Design doc, with the schema, render pipeline, gate upgrade, harness extension,
  native-fires mapping, phased plan, and open sub-questions:
  [`docs/design/v0.15/documented-example-fixtures.md`](../design/v0.15/documented-example-fixtures.md).
- Builds on ADR-0012 (output-completeness as a tested contract) - same
  philosophy, applied to rule pages instead of `explain`/`list`.
- Complements ADR-0011 (per-kind explanation prose): 0011 gave each page a
  generated *summary*; this gives it a generated, executed *example*.
- Related: ADR-0009 (rule discovery: CLI config vs catalog) frames the rule-page
  surface these examples land on.
- Key anchors: `crates/alint-testkit/src/scenario.rs` (schema),
  `crates/alint-testkit/src/runner.rs` (real runner, structured reports),
  `crates/alint-testkit/src/treespec/materialize.rs` (no mode/symlink today),
  `crates/alint-e2e/tests/coverage_audit_pass_fail.rs` (`NATIVE_FIRES_ALLOWLIST`),
  `crates/alint-e2e/tests/coverage_audit_doc_examples.rs` (loads-only),
  `xtask/src/docs_export.rs` (`enforce_example_gates`, `emit_rule_page`).
