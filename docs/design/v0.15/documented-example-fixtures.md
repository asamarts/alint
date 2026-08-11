# Documented example fixtures: rule pages rendered from executed scenarios

Status: **Draft.** Target: v0.15 candidate. Decision of record:
[ADR-0014](../../adr/0014-rule-page-examples-are-executed-fixtures.md). This
doc holds the plan; the ADR holds the settled decision. Five design forks were
resolved with asamarts (see [Resolved decisions](#resolved-decisions)); the
implementation is phased and each phase is gated before the next. The mechanics
below were adversarially reviewed against the codebase over two rounds; the
corrections those rounds produced are folded in and flagged inline where they
overturn an intuition.

## Problem

alint's own reason to exist is preventing repo-shape drift, yet its
documentation drifts. Every rule kind gets a generated page at
`alint.org/docs/rules/<kind>` carrying a worked example - a `.alint.yml` snippet
and prose about what the rule flags - and those examples are **hand-written YAML
in `docs/rules.md`, verified only to parse.** Nothing runs them against a real
repository. An example can name a config that no longer loads, claim output the
rule no longer produces, or show a repo shape the rule never flags, and every
gate stays green.

Meanwhile a comprehensive, *executed* fixture suite already exists one directory
over - it is simply not what the docs render. The goal is to collapse the two:
make each page's example the exact scenario the integration suite runs, render
it from the fixture, and re-verify it at generation time, so the docs are
provably real and a new kind cannot ship without executed coverage.

## Current state (accurate inventory)

| Component | What it does today | Gap |
|---|---|---|
| `crates/alint-e2e/scenarios/` (~315 `.yml`) | Real fixtures: mock tree + `.alint.yml` + optional git, run through the actual `Engine`; every scenario's `when`/`expect` is asserted by the `scenarios.rs` harness. | Not the source the docs render. |
| `crates/alint-testkit/src/runner.rs` | `run_scenario` materialises `given.tree`, writes `.alint.yml` **into the tree**, drives `given.git`, runs `Engine::run`/`fix`. Returns **structured** `Report`/`FixReport`. | Structured, not the human CLI output a page shows - so docs-export must spawn the real binary, not reuse this. |
| `coverage_audit_pass_fail.rs` | Proves every registered kind has a firing + a silent scenario. **Keys the kind off `given.config` via `walk_rules` (mappings carrying both `id:` and `kind:`), not `tags:`.** | Seven kinds exempted via `NATIVE_FIRES_ALLOWLIST`. |
| `coverage_audit_doc_examples.rs` | Checks each ```` ```yaml ```` block in **four** doc files (`docs/rules.md`, `ARCHITECTURE.md`, `CONFIG-AUTHORING.md`, `rule-authoring.md`) **loads**. | Never runs it. Has count floors (`validated >= 20`, `probed >= 30`, per-file `n > 0`). |
| `docs_export.rs::enforce_example_gates` | **HARD today**: each kind's H3 body must contain a ```` ```yaml ```` block whose first `kind:` matches. Signature is a post-hoc text check over precomputed `missing`/`wrong_kind` lists. | Presence + kind only; the config is never loaded against a live tree. |
| `crates/alint-testkit/src/treespec/materialize.rs` (123 lines) | Writes `File(content)` + `Dir`. | No executable bit, no symlinks. |
| `init_git_for_scenario` (runner.rs) | `commits: [subject]` -> empty commits; `add`/`add_force` + `commit` -> one mass commit, **fixed** message. | No per-commit message, no `GIT_AUTHOR_DATE`, no commit with real file deltas. |
| `alint check` file walk (`walker.rs`) | `.hidden(false)`, excludes only `.git`; respects `.gitignore` by default. | **`.alint.yml`, `.gitignore`, `.alint.d/` are walked, rule-matchable files** (see [config-in-tree](#the-pinned-invocation-contract)). |

So ~80% of the machinery this feature needs already exists and is real. What is
missing is (a) a way to mark a scenario as a page's example, (b) a docs-export
render+verify path, (c) a gate that checks the example is a real executed
fixture, and (d) the primitives that retire the allowlist.

## Resolved decisions

| # | Fork | Decision |
|---|---|---|
| 1 | How to designate a page's example | A `docs: { title, case, kind, order }` opt-in block on `Scenario`, carrying an **explicit registry-validated `kind`**. Not a filename convention, not a manifest, **and not derived from `tags:`** (see [F1](#f1-kind-designation-must-be-explicit)). |
| 2 | What each page shows and gates | A `fail` example (rule fires) **and** a `pass` example (compliant repo). Edge cases stay tested; an author may promote one. |
| 3 | Where the shown output comes from | A **real `alint check` run at generation time**, under a pinned-invocation contract; `docs-export --check` re-runs it as the drift gate. No stored golden, no synthesised replica. |
| 4 | The seven native-fires kinds | **Extend the harness** (executable-`+x` file node, symlink node, per-commit message, `GIT_AUTHOR_DATE`, commits with file deltas) until `NATIVE_FIRES_ALLOWLIST` is empty. |
| 5 | How the 78-kind migration runs | Pilot on one family -> per-family workflow drafts, each verified by a real run and curated -> hard-flip the gate once all 78 are done, via an **atomic per-family swap**. |

## Schema: the `docs:` block

`Scenario` (`crates/alint-testkit/src/scenario.rs`) is `deny_unknown_fields`, so
the field is added explicitly and defaults to absent:

```rust
// on Scenario:
#[serde(default)]
pub docs: Option<DocsExample>,

/// Opt a scenario in as a rule page's rendered example. docs-export
/// renders documented scenarios onto alint.org/docs/rules/<kind>; the
/// gate requires every canonical kind to carry one `fail` and one `pass`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocsExample {
    /// Heading shown above the example, e.g. "Require a README".
    pub title: String,
    /// Which state this fixture demonstrates.
    pub case: DocsCase,
    /// The rule kind this scenario documents (canonical or alias
    /// spelling). Explicit and registry-validated by the gate - the
    /// kind is NOT inferred from `tags:` (see F1). The gate also asserts
    /// this equals the single top-level kind in `given.config`.
    pub kind: String,
    /// Tie-breaker when a kind documents more than the fail+pass pair.
    /// Lower sorts first. Defaults to 0.
    #[serde(default)]
    pub order: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocsCase {
    Fail, // the rule fires - "what this catches"
    Pass, // the rule stays silent - "what a compliant repo looks like"
}
```

### F1: kind designation must be explicit

The first draft claimed docs-export could read a scenario's kind from its
`tags:`, "the convention `coverage_audit_pass_fail.rs` uses." **That was wrong
and is corrected here.** No code in the repo derives a kind from tags:
`coverage_audit_pass_fail.rs` reads the kind from the `kind:` field inside
`given.config` (`walk_rules` collects every mapping carrying both `id:` and
`kind:`). Tags are a free-form taxonomy mixing step tags (`check`, `fix`), family
tags (`cross_file`, `compliance`), and OS tags (`unix-only`); they are consumed
only by the OS-skip logic and proptest. Empirically, of the 315 scenarios **102
carry no registered kind in their tags** and **29 carry two** - 42% of the
corpus is ambiguous or empty on a tag-derived kind.

So the `docs:` block carries an explicit `kind`, and the gate enforces three
things (see [Gate upgrade](#gate-upgrade)):

1. `docs.kind` is a registered kind (rejects typos);
2. `given.config` resolves to **exactly one top-level rule kind** via `walk_rules`
   semantics (rejects the multi-rule `interactions`/`scope_filter` scenarios and
   any `extends:`-only scenario);
3. `canonical(docs.kind)` equals that one kind (the label cannot lie about what
   the fixture exercises).

Canonicalisation must use the **`register_builtin`-derived** alias set (the
`xtask/src/facts.rs::rule_source_files` approach), **not** the hand-maintained
10-entry `ALIASES` const duplicated across six files - that const omits the 11th
alias (`cross_file_value_equals` -> `cross_file`). Extracting one shared
canonical-kind helper in `alint-rules` and deleting the six copies is folded into
Phase 1.

## Render pipeline

docs-export today reads the hand-written YAML block out of `docs/rules.md` and
emits it verbatim. The new path renders from the documented scenario instead:

1. **Discover.** Load every `scenarios/**/*.yml`, keep those with a `docs:`
   block, group by `canonical(docs.kind)`, and within a kind order by
   `(case, order)`.
2. **Render the repository.** Emit `given.tree` as an "Example repository" file
   tree, annotating executable and symlink nodes inline (`hook.sh (executable)`,
   `latest -> releases/v2`). `given.tree` never contains `.alint.yml` (the config
   is materialised separately, below), so the shown tree needs no filtering.
3. **Render the config.** Emit `given.config` as the "Configuration" block - this
   *replaces* the hand-written YAML in `docs/rules.md`, so the page's config is
   the fixture's config by construction. `emit_rule_page` must **suppress the
   body-YAML rendering and the `lead_example_with_kind` reordering** for a kind
   that renders from a scenario, so migrated pages do not double-render.
4. **Render the output.** Materialise `given.tree` into a tempdir, write
   `given.config` as a sibling file **outside the tree**, and spawn the built
   `alint` binary under the
   [pinned-invocation contract](#the-pinned-invocation-contract), capturing
   stdout as the "What alint reports" block.
5. **Emit** at the `emit_rule_page` injection point (after the options table,
   before "See also"), one subsection per documented case.

Two structural points the review surfaced:

- **The binary must be built *before* rendering.** docs-export's existing binary
  spawn (`build_release_binary`, `docs_export.rs:1934`) is called from
  `generate_cli_reference` (~line 233), which runs **after** `generate_rules_pages`
  (~line 171) - where both the output-capture and the upgraded gate now live. So
  "as it already does for `--help`" hides an ordering change: Phase 1 must **hoist
  `build_release_binary()` above `generate_rules_pages`** and thread the path into
  the emit path and the gate. (The `--rules-only` mode has no binary at all - the
  same root as the Phase-5 bridge conflict.)
- **The shared setup helper is the tree + git, not the config.** Extract a shared
  `materialize(tree, root)` (already exists) and a `setup_git(root, spec)` helper
  that `run_scenario` and docs-export both call. The runner writes `.alint.yml`
  *into* the tree (its model); docs-export deliberately writes the config
  *outside* the walked tree and passes `-c` (below), so it does not reuse the
  runner's in-tree config placement.

### The pinned-invocation contract

The captured output is committed and re-diffed by `docs-export --check`, so it
must be byte-identical across runs and machines. The naive "relative paths +
sorted findings" assumption is **insufficient**; the reviews found the config
file self-matches, remote extends break hermeticity, and six env/format surfaces
vary. docs-export must spawn `alint check` under this exact contract:

- **Config outside the walked tree, via `-c`.** `alint check` walks with
  `.hidden(false)` and excludes only `.git`, so a `.alint.yml` written into the
  tree is itself a **rule-matchable file** - a content rule self-matches (e.g. a
  `pattern: 'FIXME'` config fires on its own literal), which would break the
  `pass = 0 findings` gate for every content kind. Writing the config as a sibling
  and running `alint check -c ../config.alint.yml .` keeps it out of the walked
  set entirely (verified: the sibling-config run reports zero findings where the
  in-tree run flags `.alint.yml`). This is preferred over a new `--exclude` flag -
  it needs no product change.
- **Working directory = the tempdir tree; target = `.`** (not an absolute path).
  This keeps printed paths relative, stops the `command`-family spawn-failure
  message from leaking the random tempdir basename (`command.rs` prints
  `root.display()`), **and** prevents upward config-discovery from escaping into a
  parent repo's `.alint.yml` (`Path::new(".").parent()` terminates immediately).
- **Sanitised, fixed environment:** `LC_ALL=C`; `TERM` pinned to a fixed
  non-`dumb` value (locks Unicode glyphs); clear `CLICOLOR_FORCE`, `NO_COLOR`,
  `ALINT_FORCE_HYPERLINKS`, `ALINT_LOG`.
- **Explicit `--color=never`** (defeats `CLICOLOR_FORCE`/TTY, strips OSC-8
  hyperlinks) and **stdout captured through a pipe, never a PTY** (width -> 80).

Fixture-level care for the spawning and now-relative kinds:

- **`git_blame_age`** default message is `` "`{}` matched line is {} days old
  (>{} days)" `` (computed against `SystemTime::now()`), so it drifts every day
  *regardless of a pinned commit date*. Its `message:` config field **fully
  replaces** the default (`git_blame_age.rs:106`: `self.message ... unwrap_or`),
  and the only reachable template token is `{{ctx.match}}` - verified
  byte-identical across runs with a static message. The fixture sets a static
  `message:` omitting the age, commits the blamed file (blame no-ops on untracked
  files), and backdates via `GIT_AUTHOR_DATE` (the operative date; the rule reads
  blame author-time) far enough past `max_age_days` to fire on any date.
- **`command`, `command_idempotent`, `generated_file_fresh`** (the three
  `SPAWNING_RULE_KINDS`) embed the subprocess's raw stdout/stderr in the finding.
  Their documented fixtures must use a generator/command that resolves on PATH
  (no spawn-failure abspath) and emits byte-stable output (no `$$`, `date`,
  timestamps); prefer `generated_file_fresh` in stdout/diff mode with a
  deterministic generator, not the file-mutating mode.
- **`git_commit_message`** in *range* mode renders an abbreviated commit SHA
  (`%h`), which drifts; its documented fixture uses **HEAD-only** mode, where the
  SHA renders as the literal `"HEAD"`. The two diff rules
  (`changeset_requires_path`, `pair_changed_together`) print only the literal
  `since` string, so they are SHA-free.

Within-bucket finding order is deterministic today (buckets are a `BTreeMap` by
path; no rule uses `par_iter`; 30 repeated runs hash identically), but that is an
emergent property. Because `docs-export --check` re-runs each documented fixture,
per-fixture nondeterminism is already caught by the gate itself; Phase 1
additionally adds a **regression test** asserting stable ordering so a future
rule cannot silently regress it. An explicit final sort in `human.rs` is optional
hardening, deferred because it changes output ordering for all users.

## Gate upgrade

`enforce_example_gates` is **re-architected**, not merely tightened: its current
signature is a post-hoc text check over precomputed `missing`/`wrong_kind` lists,
whereas the new gate needs the built binary, the documented-scenario set, and a
materialise+spawn+assert loop - a different function shape that depends on the
hoisted binary path. It becomes **migration-aware from Phase 1** so it never reds
while a partially-migrated tree has some kinds rendered and some hand-written. For
every canonical kind (78) the gate requires an example, **accepting either source
during migration**:

- a linked documented `fail` scenario **and** a linked documented `pass`
  scenario (the migrated form), **or**
- a hand-written ```` ```yaml ```` block whose first `kind:` matches (today's
  form, for not-yet-migrated kinds).

For a documented (migrated) kind it additionally asserts:

- **Kind identity.** `docs.kind` is registered; `given.config` has **exactly one
  top-level rule** by `walk_rules` semantics (a mapping with both `id:` and
  `kind:`); `canonical(docs.kind)` equals that kind. Composite kinds
  (`for_each_dir`, `for_each_file`, `for_each_match`, `every_matching_has`) wrap a
  nested `require:`/`then:` rule of a *different* kind - that nested rule **must
  omit `id:`** so `walk_rules` counts one top-level rule; the gate error message
  states this rule explicitly (two existing corpus scenarios put `id:` on nested
  rules and could not be documented as-is).
- **Case correctness.** The documented scenario is also run by the `scenarios.rs`
  harness, whose `expect:` assertion guarantees a **real violation** (not a rule
  *error* masquerading as output - important because a misconfigured diff rule
  emits a range-resolution error that a bare "produced a finding" check would
  accept). docs-export's re-run additionally asserts the `fail` case exits
  non-zero and the `pass` case exits zero and produces no findings (now reliable,
  since the config lives outside the walked tree).
- **Config fidelity.** The config rendered on the page is byte-for-byte the
  scenario's `given.config`.
- **Hermeticity.** `given.config` (including any transitively resolved `extends:`)
  uses only inline rules or offline `alint://bundled/...` extends - **no
  `http(s)://`**, which would hit the network at gen time (`loader.rs` resolves
  `https://` via a blocking GET; an unreachable host exits 2 and reds the whole
  `--check`), whereas `alint://bundled` is `include_str!` and offline.

The Phase-4 hard-flip **removes the "or hand-written" branch**. Retiring the
old load-gate is *scoped*, not wholesale: `coverage_audit_doc_examples.rs` gates
four doc files, but the real-run gate only covers `docs/rules.md`, so Phase 4
drops **only** `docs/rules.md` from its `DOC_FILES` and keeps the module guarding
`ARCHITECTURE.md`, `CONFIG-AUTHORING.md`, and `rule-authoring.md`.

## Harness extension (retiring `NATIVE_FIRES_ALLOWLIST`)

Under decision 2 every page shows a real firing run, so the seven exempted kinds
need their firing state to be materialisable:

| Kind | Needs | Layer |
|---|---|---|
| `executable_bit` | an executable (`+x`) file | materialiser |
| `executable_has_shebang` | an executable (`+x`) file | materialiser |
| `no_symlinks` | a symlink node | materialiser |
| `git_commit_message` | a commit with a custom message (HEAD-only mode) | git harness |
| `git_blame_age` | a backdated commit (`GIT_AUTHOR_DATE`) + static `message:` | git harness |
| `changeset_requires_path` | **>=2 commits** (base + change), `since: HEAD~1` | git harness |
| `pair_changed_together` | **>=2 commits** with different `add:` sets, `since: HEAD~1` | git harness |

The three filesystem kinds need only the `+x` bit and symlink-ness (verified
against their implementations), so the materialiser exposes an executable-file
node, not general mode bits.

### Materialiser: executable and symlink nodes

`TreeNode` is a serde `untagged` enum - a YAML scalar is a `File`, a YAML mapping
is a `Dir`. Add two reserved-key nodes **tried before `Dir`**. They must be
**newtype variants wrapping named `deny_unknown_fields` structs** - an inline
struct variant carrying `deny_unknown_fields` is a hard compile error, and the
attribute is load-bearing (it makes a multi-key mapping fall through to `Dir`
instead of silently dropping keys):

```rust
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum TreeNode {
    File(String),
    Exec(ExecNode),        // { "$exec": "content" }  -> 0755 file
    Symlink(SymlinkNode),  // { "$symlink": "target" } -> symlink
    Dir(BTreeMap<String, TreeNode>),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecNode {
    #[serde(rename = "$exec")]
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SymlinkNode {
    #[serde(rename = "$symlink")]
    pub target: String,
}
```

This deserialises, `Serialize` round-trips, and normal dirs are unaffected
(verified against pinned `serde 1.0.228` / `serde_yaml_ng 0.10.0`). **One
collision is intrinsic and unavoidable:** a directory whose sole child is a file
literally named `$exec`/`$symlink` serialises byte-identically to the special
node, and no parser can distinguish them. So `$exec` and `$symlink` are **reserved
filenames** - a self-enforcing constraint, because any such key *is* a node by
construction, so a fixture simply cannot materialise a literal file with those
names (nothing needs to). There is nothing a grep could "detect" here (a legit
node and an illegitimate literal-file intent are the same bytes); the reservation
is documentation, optionally backed by a **positive** round-trip test asserting
every `$exec`/`$symlink` node materialises correctly - not a misuse detector.

Adding the variants makes `write_map`'s `match node` non-exhaustive - the compiler
forces the new arms, where the `0o755` write and `std::os::unix::fs::symlink`
land. **Only the filesystem-write arms are `#[cfg(unix)]`; the `TreeNode` variants
themselves compile and deserialise on all targets** (`scenarios.rs` parses before
the OS-skip, so a cfg-gated schema would panic on Windows). `TreeSpecIter`
recurses only on `Dir`, so the new nodes are yielded as leaves unchanged.

### Git harness: messages, dates, and file deltas

Extend `commits:` from `Vec<String>` to accept either the current bare-subject
form (back-compat) or a detailed form, via an untagged enum:

```rust
#[serde(untagged)]
pub enum CommitSpec {
    Subject(String),          // empty commit, subject only (today's shape)
    Detailed(DetailedCommit), // stage files, custom message, optional date
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetailedCommit {
    pub message: String,
    #[serde(default)] pub add: Vec<String>,     // stage these paths, then commit
    #[serde(default)] pub date: Option<String>, // GIT_AUTHOR_DATE + GIT_COMMITTER_DATE
}
```

The detailed form is a newtype over a named `deny_unknown_fields` struct for the
same reason as `TreeNode`'s nodes. A scalar is unambiguously `Subject`; a mapping
is `Detailed`. The `git()` helper gains an env-passing variant so `date` sets
`GIT_AUTHOR_DATE`/`GIT_COMMITTER_DATE`, and a `Detailed` with empty `add`
commits with `--allow-empty` (so a message-only commit still lands).

The review corrected two things here:

- **The two diff rules need >=2 commits, not one.** `changeset_requires_path` and
  `pair_changed_together` take a mandatory `since:` and diff `<since>...HEAD`
  (three-dot). A single (root) commit makes `HEAD~1` unresolvable, and the rule
  emits a **range-resolution error**, not its real finding - which a bare
  "produced a finding" gate would accept, shipping a shallow-clone error as the
  canonical example. Both the `fail` and `pass` fixtures therefore use a base
  commit (a bare-subject empty commit suffices) plus a change commit, with
  `since: HEAD~1`. The `scenarios.rs` `expect:` assertion is the backstop that
  catches a misconfigured fixture at test time.
- **The DSL expresses addition deltas only.** A `given.tree` gives each file one
  content, and `add:` stages already-materialised paths, so history can contain
  *additions* but never a content *modify* or *delete*. This fires all four target
  kinds, but it means `pair_changed_together`'s most natural example - "you
  modified the format struct but forgot to bump `FORMAT_VERSION`" - is not
  expressible; only the "*added* a new file, forgot the sibling" shape is. Any
  future git-diff rule needing a modify/delete delta needs a further primitive
  (per-commit content). Documented so authors do not attempt a modify-based
  fixture.

Once all seven fixtures fire natively, `NATIVE_FIRES_ALLOWLIST` is deleted and
`coverage_audit_pass_fail.rs` asserts it is empty.

## Phased plan

- **Phase 0 - ADR + design doc (this).** Decisions recorded; forks resolved.
- **Phase 1 - plumbing + pilot (existence family).** Add the `docs:` block (with
  explicit `kind`) and the `setup_git` helper; extract the shared canonical-kind
  helper and retire the six duplicated alias maps; **hoist `build_release_binary`
  above `generate_rules_pages`**; build the render pipeline (config-outside-tree
  via `-c`) and the re-architected, migration-aware gate; migrate the existence
  family end to end via an **atomic per-family swap** (one commit that adds the
  family's `docs:` scenarios, removes its hand-written YAML from `docs/rules.md`,
  and is covered by the gate's documented branch). Because docs-export gains a
  normal `alint-testkit` dependency (pulling `proptest` + `thiserror` into its
  build graph), **the same commit regenerates and commits `crate-graph.gen.c4`,
  `crate-graph.md`, and `DIAGRAMS.md`** (`gen-arch` + Node-22 `gen-mermaid`) -
  otherwise `gen_arch_check_passes_on_committed_tree` reds `cargo test` on every
  platform, and `gen-arch`/`gen-mermaid --check` red the Docs job. The render+verify
  path is **CLI-only** (in `ci/scripts/docs.sh`) and must **not** get a `#[test]`
  mirror, or `cross-platform.yml` would run it on Windows/macOS where the unix
  fixtures and the binary spawn break. It lands in a new `docs_export/` submodule
  (`docs_export.rs` is already at the dogfooded `rust-file-max-lines` limit).
- **Phase 2 - harness extension.** Land the executable/symlink `TreeNode` nodes
  (variants compile + deserialise on all targets; only the fs-write arms are
  `cfg(unix)`) and the git DSL (>=2-commit diff fixtures; `--allow-empty` for
  message-only commits); tag the three unix scenarios `unix-only`; add the
  positive `$exec`/`$symlink` round-trip test; migrate the seven native-fires
  kinds; assert the allowlist is empty. Pin `core.autocrlf=false` on the
  file-delta fixtures so a Windows `cargo test` sees the same diff.
- **Phase 3 - migrate the remaining families.** A per-family multi-agent workflow
  drafts a minimal `fail`+`pass` per family, each passing a real gen-time run and
  reviewed for quality before its atomic-swap commit. Any prose edit touching a
  kind's **first sentence** regenerates `kind_docs_gen.rs` + `categories_gen.rs`
  (LF-pinned) in the same commit; a migration must not reorder an H3's opening
  sentence. The **last family's swap also drops `docs/rules.md` from
  `coverage_audit_doc_examples`'s `DOC_FILES`** in the same commit, so the per-file
  `n > 0` floor never fires on an emptied file.
- **Phase 4 - hard-flip the gate.** Remove the gate's "or hand-written" branch;
  confirm no hand-written examples remain in `docs/rules.md`; the `validated`/
  `probed` floors are already lowered per-family, so only the final scoping
  remains.
- **Phase 5 - alint.org presentation.** An Astro component renders the blocks
  consistently. **Resolve the `--rules-only` bridge conflict** (it binds once the
  pilot injects real output): the docs-bundle bridge runs `docs-export --rules-only`
  to skip the release build, but the output block needs a real `alint check` spawn.
  Recommended: the bridge builds the binary + takes `alint-testkit` in its
  worktree (undoing the `--rules-only` speed optimisation but preserving the #82
  "deploy rule-page fixes without a release" contract for the output block); the
  alternative skips output-injection under `--rules-only` and ships that block only
  via a release-tag export. Tree + config blocks deploy via the bridge regardless.

## Determinism and cost

- **Determinism is a hard release invariant**, enforced by the pinned-invocation
  contract (config outside the tree, `.` target, fixed env, `--color=never`, pipe
  capture, stable order) plus the fixture-level handling for the spawning and
  now-relative kinds. Because `--check` re-runs each fixture, any residual
  per-fixture nondeterminism reds the build rather than shipping.
- **Build cost:** ~2 runs x 78 kinds = ~156 gen-time spawns, plus the release-build
  hoist. They run **sequentially** (docs-export has no threading; calling them
  parallelisable is aspirational - it needs new threads + per-spawn tempdirs), and
  the git-backed fixtures add real per-fixture `git` subprocess cost. Budget a
  low-tens-of-seconds addition to the 7-minute release `--check`. Note this also
  lands in the developer's local `ci/scripts/preflight.sh` (which runs `docs.sh`
  advisorily) and in the ci.yml Docs job. If it bites, feature-gating `proptest`
  in `alint-testkit` so docs-export takes a lighter dependency path is a follow-up.
- **LF pins:** the generated rule pages are built to a tempdir under `--check`
  (not committed), so they need no new pin, and decision 3 stores no golden. The
  regenerated-and-committed artifacts (`crate-graph.gen.c4`, `crate-graph.md`,
  `DIAGRAMS.md`, and on prose edits `kind_docs_gen.rs`/`categories_gen.rs`) are
  already LF-pinned.

## Crate-graph cascade and cross-platform

- **The `xtask -> alint-testkit` edge is a normal (runtime) dependency**, so it
  enters the workspace crate graph and makes `gen-arch` (`crate-graph.gen.c4`,
  `crate-graph.md`) and `gen-mermaid` (`DIAGRAMS.md`) stale. The sharp edge:
  `arch.rs`'s `gen_arch_check_passes_on_committed_tree` `#[test]` runs under
  `cargo test --workspace` on **every** platform (`cargo metadata` edges are
  platform-independent), so it reds Windows and macOS too until regenerated -
  hence the Phase-1 same-commit regeneration. (`gen-mermaid` needs Node 22 and
  silently skips on old Node, so a local preflight can false-green `DIAGRAMS.md`.)
- **`docs-export --check` and every `gen-* --check` run ubuntu-only** (in
  `ci/scripts/docs.sh`); `cross-platform.yml` runs only `cargo test --workspace`.
  So committed pages embedding unix-only fixture output are generated once on
  linux and LF-pinned - a Windows `--check` reporting them stale cannot happen.
  The real cross-platform hazard is Phase 2's schema (parse precedes OS-skip),
  handled by keeping the `TreeNode` variants target-independent.
- **Publish: no impact.** `alint-testkit` and `xtask` are both `publish = false`,
  and no published crate depends on testkit.

## Open sub-questions (for the pilot)

1. **Output-block length** - some kinds emit verbose output; decide full stdout vs
   a bounded, elided form (leaning: full, since a minimal fixture's output is
   short and real).
2. **`--rules-only` bridge** - confirm the recommended "build the binary in the
   bridge worktree" option against the docs-bundle build-time budget (Phase 5).
3. **`.gitignore` in a documented tree** - a `given.tree` may include a
   `.gitignore`, which the run respects (as a real repo would), so the shown tree
   can list files alint skips. Leaning: allow it but prefer trees without one
   unless the rule is about ignore semantics; the run keeps default
   `respect_gitignore` so the page reflects real behaviour.

(Resolved: kind keying - explicit `docs.kind` + single-top-level-kind config;
mode/symlink representation - reserved-key newtype variants; config self-match -
config kept outside the walked tree via `-c`.)

## Risks

- **The commit-delta DSL is the schedule risk.** It is the one genuinely new
  subsystem; Phase 2 is scoped to it alone.
- **Agent-drafted fixtures could be technically-passing but poor examples.** The
  real-run gate catches incorrectness; human curation per family catches quality.
- **A `docs:` scenario is doubly load-bearing** (a test and public documentation),
  so the single-kind, case-correctness, hermeticity, and config-fidelity gate
  assertions matter: a mislabeled or non-hermetic fixture would render a
  misleading page or red the build, not just weaken a test.
- **docs-bundle builds from the release tag**, so Phase 5's rendering needs the
  bridge decision resolved or the new blocks render only after a release.
