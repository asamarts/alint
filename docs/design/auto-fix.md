# Auto-fix: a systematic framework for mechanical remediation

Status: Draft (revised after an adversarial audit; see the changelog note at the end).
Decisions: [ADR-0017](../adr/0017-auto-fix-edit-model-and-applicability.md) (proposed) records the load-bearing decisions (the batched range-edit apply engine, the applicability model, and the fixer trust boundary).
Demand evidence: the structured-query family (25 kinds, the largest family) is 100% unfixable today; see the `format-coverage.md` arc and the 30-repo `examples/` corpus.

> This is an arc document (the scale of `distribution.md` / `format-coverage.md`, not a
> single rule-kind design). It is deliberately split into **Part I: research and analysis**
> (the universe of auto-fixable classes, the state of the art, and where alint sits) and
> **Part II: proposal and plan** (the architecture and a phased build). Each numbered phase
> in Part II carries its own surface, semantics, false-positive analysis, and test plan in
> the spirit of `TEMPLATE.md`; a phase graduates to its own `docs/design/vX.Y/` doc when it
> is scheduled. Companion detection-gap analysis: [`rule-coverage-gaps.md`](rule-coverage-gaps.md).

## Contents

- [Thesis](#thesis)
- [Part I: research and analysis](#part-i-research-and-analysis)
  - [1. Where alint is today](#1-where-alint-is-today)
  - [2. The universe of auto-fixable classes](#2-the-universe-of-auto-fixable-classes)
  - [3. The applicability model](#3-the-applicability-model)
  - [4. Prevalence and usefulness](#4-prevalence-and-usefulness)
- [Part II: proposal and plan](#part-ii-proposal-and-plan)
  - [5. Architecture](#5-architecture)
  - [6. The phased plan](#6-the-phased-plan)
  - [7. False-positive and safety surface](#7-false-positive-and-safety-surface)
  - [8. Invariants and the decision record](#8-invariants-and-the-decision-record)
  - [9. Open questions](#9-open-questions)
- [References and prior art](#references-and-prior-art)

## Thesis

Auto-fix should not be a grab-bag of per-rule special cases. Every mechanical fix is one
instance of a small, closed set of **edit primitives**, chosen by a **fix class** that maps
a violation shape to a known methodology, and gated by an **applicability tier** that says
whether it is safe to apply without a human. Get the primitives, the apply engine, and the
tiers right once, and each new fixer becomes a thin, provably-safe adapter rather than a
bespoke read-modify-write.

alint has the opposite today. It has four whole-file primitives, a single-pass sequential
apply loop with no conflict handling and no re-check, and no notion of fix safety. The
result: **16 of 94 distinct rule kinds are fixable (about 17%)**, the single largest rule
family (structured query, 25 kinds) is entirely unfixable, and the fixers that do exist
cannot express a located edit. This document defines the whole universe, shows that the
gap is structural rather than incidental, and proposes an engine plus a phased build that
closes it.

The framing is three orthogonal axes. They do not line up row-by-row; any primitive can
serve several classes, and any class can land in several tiers. They are listed separately
on purpose:

- **Edit primitives (how bytes change).** Today: `CreateFile`, `DeleteFile`, `RenameFile`
  (which already covers cross-directory moves), `SetContent` (whole-file). Proposed
  additions: `ReplaceRange` (a surgical byte-span splice) and `SetMode` (chmod).
- **Fix classes (violation shape to methodology).** Existence and presence-normalize;
  whole-file byte-normalize; located replace; format-preserving structured edit; ordering
  and canonicalization; metadata and permission; cross-file consistency and propagation.
- **Applicability tiers (is it safe to apply unattended).** Safe (applied by default),
  Unsafe (opt-in via `--unsafe-fixes`), Suggestion (surfaced, never auto-applied), Never
  (advisory or a tripwire; no fix proposed).

One batched apply engine runs over the primitives; every class is an adapter that emits
primitives with a declared tier. The engine, not the individual fixer, owns conflict
resolution, determinism, postcondition verification, and (from Phase 1) fixpoint iteration.

---

# Part I: research and analysis

## 1. Where alint is today

### 1.1 The numbers

As of v0.16.1 (`facts.json`): 105 rule kinds (94 distinct + 11 aliases), 13 families,
22 bundled rulesets, **12 fix ops**, 8 output formats. Only **16 distinct kinds (19 counting
aliases) attach a fixer**. Each fixable builder accepts exactly one op and rejects any other
at load (`fix.<op> is not compatible with <kind>`).

The 16 fixable kinds: `file_exists`, `file_absent`, `file_content_matches`, `file_header`,
`file_footer`, `filename_case`, `no_trailing_whitespace`, `final_newline`, `line_endings`,
`max_consecutive_blank_lines`, `no_bidi_controls`, `no_zero_width_chars`, `no_bom`,
`no_empty_files`, `no_submodules`, `no_symlinks`.

### 1.2 The current fix model (code map)

Fixes are **rule-attached, not violation-attached**. `Rule::fixer()`
(`crates/alint-core/src/rule.rs:390`); one `Fixer` per rule, applied to each violation the
rule emits. The `Fixer` trait (`rule.rs:553-579`) has three methods: `describe`; `apply`
(mutates the filesystem via `write_atomic`, `crates/alint-rules/src/io.rs:104`); and
`fix_edit` (the non-writing sibling, currently returning `Option<FixEdit>` per violation,
that the LSP turns into a `WorkspaceEdit`).

`FixEdit` (`rule.rs:541-550`) is the entire data model of an edit, and it is **whole-file
only**:

```rust
pub enum FixEdit {
    SetContent { path: PathBuf, content: Vec<u8> },  // replace full contents
    CreateFile { path: PathBuf, content: Vec<u8> },
    DeleteFile { path: PathBuf },
    RenameFile { from: PathBuf, to: PathBuf },        // same directory or not
}
```

There is no ranged/span variant, no permission-bit variant, and no structured-document
variant. `FixOutcome` is `Applied | Skipped`; the report tier adds `FixStatus::Unfixable`.
There is **no field anywhere that expresses fix safety.** The 12 ops split into path-only
(`file_create`, `file_remove`, `file_rename`) and content-editing (9 ops: `file_prepend`,
`file_append`, plus the 7 pure normalizers `file_trim_trailing_whitespace`,
`file_append_final_newline`, `file_normalize_line_endings`, `file_collapse_blank_lines`,
`file_strip_bidi`, `file_strip_zero_width`, `file_strip_bom`). Every one either creates a
whole file from a template, deletes/renames a whole file, or runs a **pure byte-to-byte
transform over the whole file**. None performs a located edit at a computed position.

### 1.3 The apply engine

`Engine::fix` (`crates/alint-core/src/engine.rs:909-1067`) is a **single pass**: iterate the
rule entries in config order, freshly evaluate each rule, and for every violation call
`fixer.apply`. It is sequential, not batched (each `apply` re-reads the file from disk via
`read_for_fix`, so two fixers on one file compose through disk round-trips); there is **no
fixpoint and no re-check** (convergence rests only on each fixer's own idempotency guard);
and there is **no conflict detection** (the whole-file model sidesteps it by never producing
two edits to one region). `alint fix` also **rejects `--baseline`** (`main.rs:179-186`), so
it acts on the unsuppressed violation set. The safety guards that already exist: `dry_run`,
`fix_size_limit` (enforced inside the writing `apply` path via `read_for_fix`, not the
non-writing `fix_edit` path, which is deliberately un-guarded), out-of-root path
confinement, a binary-content guard, atomic temp-then-rename writes, and skip-on-collision
for rename.

### 1.4 The gap is structural, and the maintainer already reasons about it

The catalog of unfixed kinds is not random. Reading the "why no fixer" notes across
`docs/rules.md`, the unfixed kinds already sort into five buckets that map one-to-one onto
the applicability model formalized later:

1. **Deferred / not-yet-built, low ambiguity:** `executable_bit`,
   `shebang_has_executable`, `executable_has_shebang` ("chmod auto-apply is deferred"),
   `ordered_block` (sort the marked lines), `indent_style` ("tab-width-aware reindentation
   is deferred"), `dir_exists` / `dir_absent` (no directory-level op exists).
2. **Deliberately check-only because a fix is destructive:** `file_content_forbidden`,
   `commented_out_code`, `git_no_denied_paths` ("git rm --cached is too destructive to
   automate"), `git_blame_age`, `no_merge_conflict_markers` (a fix cannot know which side to
   keep).
3. **Blocked by target ambiguity:** `file_starts_with` / `file_ends_with`,
   `markdown_paths_resolve`, `filename_regex` (a regex has no invertible canonical name),
   `no_illegal_windows_names` / `no_case_conflicts`.
4. **Integrity tripwires by design:** `file_hash`, `pair_hash`, `generated_file_fresh`
   (auto-updating the pin would defeat the check).
5. **Not mechanically fixable:** `json_schema_passes`, size / line caps, the git-commit
   history family, the cross-file relational family.

That is, in miniature, exactly the applicability taxonomy this document formalizes (bucket 1
becomes Safe/Unsafe once built; bucket 2 becomes Unsafe; bucket 3 becomes Suggestion;
buckets 4 and 5 stay Never). The framework is a natural fit, not an imposition. What is
missing is (a) the edit primitive to express located and structured changes, (b) an engine
that can batch and verify them safely, and (c) a first-class way to say "this fix exists but
is not safe to apply unattended."

## 2. The universe of auto-fixable classes

Surveying ESLint, Ruff, the formatter family (Prettier / gofmt / rustfmt / Black / dprint),
the format-preserving structured editors (`toml_edit`, `tomlkit`, `ruamel.yaml`,
`node-jsonc-parser`), the codemod family (comby, ast-grep, jscodeshift/recast, Semgrep,
OpenRewrite), the repo/config/dependency fixers closest to alint, and the archived
Repolinter, **seven mechanism classes** recur. Each is defined by its canonical algorithm,
its key safety concern, and its exemplars.

| # | Class | Canonical algorithm | Key safety concern | alint status | Where it lands |
|---|---|---|---|---|---|
| 1 | Whole-file canonical reprint | parse -> tree -> pretty-print through a Wadler-style layout algebra, discarding original layout | a parser/printer bug silently changes meaning (Black re-parses and diffs the AST to guard) | out of scope for the core (alint is not a formatter); reachable only via `command` | not proposed |
| 2 | Surgical span replacement | rule yields `{range, text}`; collect, sort by position, apply non-overlapping in one pass, skip conflicts, re-lint to a fixpoint | overlap resolution; minimal ranges; per-edit safe/unsafe tier | **absent** (no ranged primitive) | Phase 0 (engine) + Phase 1 |
| 3 | Format-preserving structured-data edit | parse into a lossless CST that keeps trivia and mutate one node; OR splice a minimal `{offset,length,content}` into the original bytes | naive parse->dump destroys comments/order; value serialization (quoting, type, entity-encoding) | **absent** (all 8 formats parse to a lossy detached tree) | Phase 2 (flagship) |
| 4 | Structural match-and-rewrite (codemod) | match a pattern with metavariable holes -> bind -> substitute into a rewrite template -> replace the matched span | over-matching; formatting survival | partial: `file_content_matches` can only append | Phase 1 (regex capture form) |
| 5 | Whitespace / trivia normalizers | per-line or per-file byte transform, idempotent by construction | mostly safe; EOL/BOM significant for some files; needs exclusions | **strong**: 7 of the 12 ops are pure normalizers | shipped; retrofit onto the new engine in Phase 0 |
| 6 | Content insertion / templating | presence-guard, then render from a template and insert at the correct anchor, in the file's comment syntax | never double-insert; correct comment style and placement | partial: `file_prepend` / `file_append` (fixed content, no anchor logic) | Phase 4 (license/SPDX headers) |
| 7 | Reference / version resolution and pinning | resolve a ref against an external source of truth (registry, git ref -> commit SHA), rewrite via a format-preserving edit | needs network + trust; semver/supply-chain correctness; human-gated | **absent**; tension with the telemetry-free-at-runtime invariant | deferred / special |

Two structural findings dominate the rest of the document.

**Finding A (class 2 is the missing substrate).** ESLint and Ruff converge on the same
engine: a fix is a `{range, text}` edit; collect every candidate, sort by position, apply
the non-overlapping ones in a single left-to-right pass while skipping any edit that overlaps
one already applied, then re-lint and repeat to a fixpoint (ESLint caps at 10 passes, Ruff
at 100 and bails loudly on non-convergence). alint has none of this because its whole-file
model never needs it. Adding a ranged primitive plus this engine is the enabler for classes
2, 3, and 4 at once.

**Finding B (class 3 is the flagship, and it is a span-splice problem).** Every alint
structured-query rule parses its file (JSON, YAML, TOML, XML, dotenv, properties, INI, HCL)
into one detached, lossy `serde_json::Value` (`crates/alint-core/src/structured_format.rs`).
Comments, whitespace, and even key order are discarded before a rule runs (`preserve_order`
is not enabled; the backing map is a `BTreeMap` and keys are alphabetized). Re-serializing
from that tree can never preserve formatting. The only viable technique is a **byte-span
surgical splice** against a span-capable or CST parser, and the JSONPath engine
(`serde_json_path`, whose `query()` runs over the detached value) can never itself yield a
span. So a write-back must **bridge** two representations: JSONPath selects the logical node,
a CST/spanned parser re-locates that node's byte range, and the fixer splices. This is a
two-part problem (locate the span, then serialize the replacement value), addressed in 5.3
and 5.4.

### 2.1 The intent view (violation shape to class)

The mechanism table is how bytes change. Authors think in violation shapes, so the same
universe indexed by intent (which is how the DSL will expose it):

- **Presence** (a file/dir must exist, must not exist, or is misnamed/mislocated): create /
  delete / rename / relocate (all via the existing `RenameFile`, which spans directories).
  alint covers create, delete, case-rename; gaps are arbitrary-target rename, relocate,
  directory create/remove, and cross-file partner creation.
- **Byte hygiene** (whitespace, newlines, EOL, BOM, Trojan-Source characters, blank-line
  runs): class 5. Strong; gaps are `indent_style` and `line_max_width` (reflow, genuinely
  unsafe).
- **Located content** (a forbidden token must go; a required token inserted or rewritten):
  classes 2 and 4. alint can only append; it cannot remove a match or rewrite a captured
  span. The general-purpose gap.
- **Structured value** (a value at a path must equal / be absent): class 3. Entirely
  unfixable today; the largest opportunity.
- **Ordering / canonicalization** (a marked block must stay sorted; entries deduplicated):
  `ordered_block` sort is the clean win; `unique_by` dedup and `.gitattributes` /
  `.gitignore` line insertion are adjacent, currently unbuilt.
- **Metadata / permission** (exec bit, symlink, submodule, portable name): `SetMode` plus
  delete/rename. alint deletes symlinks and submodules; chmod and portable-name repair are
  gaps.
- **Cross-file consistency** (a value or a whole file must match a canonical source across
  the tree): see 2.2. Unbuilt, and the highest-value repo-scale class.
- **VCS tree** (a committed artifact should be untracked; a path gitignored): untrack +
  gitignore insertion. High value; unbuilt; a spawning fixer (see 5.5).
- **Reference / version** (an Action tag should be a pinned SHA): class 7. Network-gated,
  special-cased.

Note that `*_path_matches` is listed as Never in 5.3, but the Phase 1 regex `replace` op
supplies a replacement template, so a user who provides one could make a `*_path_matches`
violation fixable; this is a deliberate deferral, not an impossibility.

### 2.2 Repo-scale fix classes the single-file view misses

The seven mechanism classes above are single-file-centric because that is where the
literature lives (code linters and formatters). A repository linter has four fix classes
that only exist because the unit is a tree, not a file. They compose the primitives above
but need engine support the single-file model does not (cross-file atomicity, a canonical
source, index invalidation):

- **Whole-file sync-from-canonical-source.** "Every crate's `LICENSE` must byte-equal the
  root `LICENSE`"; "every package's `.editorconfig` must match the canonical one." A byte
  copy from a reference file, distinct from class 1 (reprint) and class 6 (literal
  template). This is arguably the single most common mechanical repo fix, and it maps
  directly onto the existing `cross_file` / `cross_file_value_equals` "equals" relation.
- **Cross-file value propagation.** A version string that must agree across `package.json`,
  `Cargo.toml`, a `__version__`, a badge, and a tag: pick the source of truth, propagate to
  the N others via structured edits. Maps onto `unique_by` / the cross-file family, but the
  structured-value class (5.3, 5.4) is defined per-file, so propagation is an orchestration
  on top.
- **Regenerate-from-command.** `generated_file_fresh` is a Never tripwire when the fix would
  silence it by editing the pin, but "re-run the declared generator and commit its output"
  is a legitimate (spawn-gated) fix. Distinct because the corrected bytes come from running
  a command, not from a transform of the existing bytes.
- **Cross-file atomic create-and-register.** Create a new crate directory and add it to
  `workspace.members`; create a package and add it to a `CODEOWNERS` block. The fix is two
  edits to two files with an inter-edit dependency, and a half-applied create-without-
  register leaves the tree inconsistent, so it needs the multi-file transaction of 5.2.4.

## 3. The applicability model

The most refined safety model in the ecosystem is Ruff's three-tier `Applicability` (Safe /
Unsafe / DisplayOnly), safe-by-default, `--unsafe-fixes` opt-in, with per-rule
promote/demote. ESLint expresses the same split more coarsely as autofix versus suggestion.
alint should adopt a **four-state model**, and the tiers are defined strictly by
**application policy** (what the engine does with the fix), not by a bag of properties. The
properties (preserves the file's meaning, discards no human content, idempotent, output
re-parses) are the **inputs** a fixer author uses to pick the tier, not the definitions:

- **Safe.** Applied by `alint fix` with no flag. A fixer may declare Safe only when it sets
  exactly the declared or derived target, discards no human-authored content (comments,
  code), is idempotent, and (for content/structured edits) its output re-parses. Example:
  the seven hygiene normalizers; setting a structured value to an exact declared **scalar**
  via a format-preserving edit that re-parses.
- **Unsafe.** Applied only with `--unsafe-fixes`. Example: regex search-and-replace,
  structured key removal, chmod, git untrack, tab/space conversion, deleting a whole file.
- **Suggestion.** Never written by `alint fix`. Surfaced as a concrete proposed edit
  (path + range + content) in the `agent` / `json` / LSP output for a human or agent to
  apply. Example: line reflow, merge-conflict resolution, illegal-windows-name rename,
  structural (non-scalar, key-insertion, nested-creation) structured edits.
- **Never.** No fix proposed at all. Advisory or a tripwire. Example: `file_hash`,
  `generated_file_fresh` (unless the regenerate-from-command fix of 2.2 is explicitly
  configured), size/line caps, git history, most cross-file relations, `json_schema_passes`,
  duplicate-code.

Because the tiers are a policy, a fix can be property-safe (idempotent, re-parses) yet still
be a Suggestion because its **target** is ambiguous (which of two files to rename). "Preserves
meaning," borrowed from Ruff, is a code-behavior notion that does not translate to a value
edit, where changing the value is the entire point; for alint, the Safe test for a value
edit is "sets exactly the declared value, output re-parses, recoverable via VCS," not
"preserves meaning."

This maps one-to-one onto the maintainer's existing five buckets and gives alint three
properties it lacks: a machine-readable safety declaration on every fixer, a default that
only applies fixes that cannot surprise, and a first-class home for "proposed but not
automatic." The data model for Suggestions and their interaction with the existing agent
output is specified in 5.7.

**Trust:** the applicability tier a fixer runs at is not purely the author's to choose when
the rule arrives through `extends:`. Promotion toward Safe/Unsafe, and every content-mutating
or spawning op, is confined to the user's own top-level config (5.5). An inherited fixer may
be demoted but never promoted.

**Back-compatibility:** the 12 existing ops keep their current behavior and are classified
Safe on introduction, so `alint fix` is unchanged for existing configs. Whether `file_remove`
(deleting a whole file by default, with no `--unsafe-fixes`) should be reclassified Unsafe is
a safety-default question, not merely a semver one; see the open questions.

## 4. Prevalence and usefulness

Ranking the classes by expected value combines three signals: how much of the rule surface
the class unlocks, how often the underlying violation occurs in real repos (the 30-repo
`examples/` corpus and the README "where alint shines" evidence), and how much the ecosystem
invests in that class.

1. **Structured value edit (class 3): highest.** Unlocks the largest family (25 kinds across
   8 formats) and targets the exact pain the README leads with: manifests and workflows whose
   structural invariants no per-language tool checks (dotnet's ~2,300 XML manifests, uv's
   67-crate workspace conventions, GitHub Actions permissions). No mainstream repo linter
   does format-preserving config repair well; the biggest differentiator available.
2. **Located content replace (class 2/4): high and general.** One primitive makes
   `file_content_forbidden` fixable (remove a banned token: `console.log`, a debug pragma,
   an AI-affirmation phrase) and adds a general regex-capture rewrite. It is also the
   substrate the whole engine is built on.
3. **Cross-file consistency (2.2): high for monorepos.** Sync-from-canonical and version
   propagation are headline monorepo use cases and are what the `examples/` corpus repeatedly
   wants (uv, pnpm, tokio conventions enforced nowhere).
4. **Byte hygiene (class 5): the workhorse, keep it.** The fixes people run daily. The work
   is retrofitting onto the new engine with zero behavior change and closing `indent_style`.
5. **Content insertion / license headers (class 6): high for compliance repos.** The
   `compliance/reuse` and `compliance/apache-2` rulesets demand a per-file SPDX or license
   header; a comment-style-aware inserter turns a wall of violations into one `alint fix`.
6. **VCS untrack + gitignore: high for the hygiene story.** A tracked `target/`,
   `node_modules/`, or `.DS_Store` is one of the most common real findings (it is literally
   the README demo). Spawn-gated (5.5).
7. **Ordering / canonicalization: medium.** `ordered_block` sort and `unique_by` dedup are
   clean, deterministic wins.
8. **Metadata permission (chmod): medium.** `shebang_has_executable` (add +x) is unambiguous
   and Safe; the others are narrower.
9. **Reference/version pinning (class 7): high visibility, special-cased.** SHA-pinning a
   GitHub Action is a marquee security fix, but it requires resolving a tag over the network,
   colliding with alint's telemetry-free-at-runtime guarantee. Routed to an explicit opt-in
   or a future WASM plugin.

---

# Part II: proposal and plan

## 5. Architecture

The engine changes below underpin every new fixer. They are the content of ADR-0017.

### 5.1 Edit primitives

Extend `FixEdit` with the two primitives the current model cannot express, keeping the
existing four (`RenameFile` already covers cross-directory relocation, so no `MoveFile`
variant is added):

```rust
pub enum FixEdit {
    // existing, unchanged
    SetContent { path, content },
    CreateFile { path, content },
    DeleteFile { path },
    RenameFile { from, to },                                        // same directory or not
    // new
    ReplaceRange { path, range: Range<usize>, content: Vec<u8> },   // splice; insert = empty range, delete = empty content
    SetMode { path, mode: u32 },                                    // chmod (unix-gated)
}
```

`ReplaceRange` is the ESLint `{range, text}` primitive. Three payoffs: minimal diffs (the LSP
maps it straight to a `TextEdit` instead of replacing the whole document, which the current
`SetContent` path does with a full-document range at `crates/alint-lsp/src/lib.rs:779-782`);
mechanical overlap detection (two edits conflict iff their ranges intersect); and composition
(many edits to one file merge into one write). A structured splice can also be delivered as a
`SetContent` whose bytes differ only in the target span, so `ReplaceRange` is a
correctness-and-diff-quality optimization, not a hard prerequisite for shipping class 3; for
overlap and determinism purposes a whole-file `SetContent` is treated as a `ReplaceRange`
over `0..len`.

### 5.2 The batched apply engine

Replace the single-pass loop with a batched model, keeping the "serial filesystem mutation"
guarantee. In **Phase 0 the engine runs exactly one pass** (behavior-preserving); the
**fixpoint loop is introduced in Phase 1**, where the first fixers that can unblock one
another appear. One pass:

1. **Collect (read-only, not pure).** Each rule's fixer yields its edits as data, tagged with
   an `Applicability`. This reads and parses files, so it is read-only I/O, not pure; the
   engine reads each file once and shares the bytes across every fixer targeting that file
   (the analogue of the file-major `PerFileRule` coalescing, which `alint fix` does not use
   today). The `fix_size_limit` guard is **re-applied at this collect/read step**, because the
   `fix_edit` data path it generalizes is currently un-guarded by design (`rule.rs:571-574`);
   it does not "carry over" for free.
2. **Tier filter.** Drop edits below the run threshold (Safe by default; Safe+Unsafe with
   `--unsafe-fixes`; Suggestions never enter the apply set, only the report set of 5.7).
3. **Group by file, sort, skip overlaps.** Per file, sort by the total order of 5.2.2, walk
   left-to-right tracking the last applied end, skip any edit overlapping one already taken
   (deferred to the next fixpoint pass, once Phase 1 adds it). Adopt Ruff's isolation-group
   idea for edits that must not co-apply even when disjoint.
4. **Apply in memory and verify the postcondition.** For structured and regex edits, re-parse
   the buffer and confirm it still parses and the target now satisfies the rule. This is the
   Black-style equivalence check adapted to structured data: an edit that would produce an
   unparseable file is rejected and **demoted to Suggestion, memoized per (file, edit) for the
   run** so a stateless rule does not re-derive and re-demote it every pass.
5. **Write atomically**, once per file, via the existing `write_atomic`.
6. **Idempotence as a tested contract.** Every fixer gets a property test: apply twice, the
   second run is a no-op.

The `allow_out_of_root` confinement, binary guard, and dry-run guards carry over.

#### 5.2.1 Binding violations to edits

The current fixer is invoked once per `Violation` and receives only `&Violation` (path,
message, `baseline_key`). Structured-query violations carry **no byte span** (they never call
`with_location`), the rule emits **one violation per matched node** for `*_path_equals` but a
**single file-level violation for N matched nodes** for `*_path_absent`, and `Op::Equals`
holds an **arbitrary `serde_json::Value`**, not a scalar. A per-violation `fix_edit` therefore
cannot tell which of N nodes it is fixing. The fix is to generalize the edit-producing path to
**rule level**: a fixer exposes

```rust
fn collect_edits(&self, violations: &[Violation], file: &Path, bytes: &[u8], root: &Path)
    -> Vec<(FixEdit, Applicability)>;
```

so a fan-out rule computes the full match set once, re-runs its own query with a **located**
API (RFC 9535 normalized paths via `serde_json_path`'s `query_located`, which yields a
per-node location) and emits exactly one edit per node: N deletions for one `*_path_absent`
violation, one edit per failing node for `*_path_equals` (de-duplicated so M violations do not
produce M identical edit batches). Simple per-file fixers keep the one-liner `apply` /
`fix_edit` path; only rules that fan out over matches implement `collect_edits`. This is a
Phase 2 prerequisite, not a Phase 2 detail.

#### 5.2.2 Determinism and total edit order

Constitution invariant 1 requires byte-identical fixes across runs. Sorting "by start then
end" is not a total order when two edits share both offsets (two rules rewriting one span; two
whole-file retrofits both spanning `0..len`). The engine sorts by the total key
`(start, end, rule_index, violation_index)` with a stable sort, where `rule_index` is config
order and `violation_index` is the rule's deterministic emission order (JSONPath match order).
Ties therefore resolve by config order, which is already the determinism anchor elsewhere in
the engine.

#### 5.2.3 The fixpoint and index invalidation (Phase 1)

Applying one fix can unblock a previously-skipped conflicting fix or expose a new violation,
so from Phase 1 the engine loops collect -> apply -> re-check until a pass produces no edits or
a cap is hit (match Ruff; bail loudly on non-convergence). Two subtleties the single-pass Phase
0 avoids:

- **Index invalidation.** `CreateFile` / `DeleteFile` / `RenameFile` change the tree, but the
  `FileIndex` is built once. A pass that applied a path-mutating edit forces a **full
  deterministic re-walk** before the next pass (so cross-file and `requires_full_index()` rules
  see the new tree); a pass with only content edits re-checks just the touched files. A created
  file is not a "changed file," so path-mutating fixes and `--changed` interact (5.7).
- **Termination.** The per-fixer idempotence test catches **self**-oscillation only.
  **Cross-fixer** oscillation (fixer A reintroduces what fixer B removed) is caught solely by
  the iteration cap, which is why the cap exists and why non-convergence is a loud error, not a
  silent stop. 5.7 (isolation groups) and careful tier assignment reduce, but cannot eliminate,
  cross-fixer cycles.

#### 5.2.4 Multi-file transactions

A single logical fix can span files (create-and-register, sync-from-canonical to N targets,
version propagation). "Write once per file" gives no cross-file atomicity, so a fix that emits
edits to multiple files is applied **all-or-nothing**: stage every file's new bytes in memory,
verify every postcondition, and only then write them (each file still atomically). A failure on
any file aborts the whole group and writes nothing, so a half-applied create-without-register
cannot occur.

### 5.3 The structured bridge: locating the span

Class 3 needs, per format, a resolver from a JSONPath (or its normalized path) to the target
value's byte range. There is no shared abstraction to reuse, because the current shared
abstraction (`Format::parse -> serde_json::Value`) is exactly what throws spans away. The
feasibility verdicts, in build order by cost:

| Format | Verdict | Technique and dependency |
|---|---|---|
| HCL | free | parse with `hcl::edit` (already compiled in via `hcl-rs`, re-exported as `hcl::edit`); mutable CST, preserves layout |
| XML | free | `roxmltree` (already a dep) exposes `Node::range()` / `Attribute::range_value()` (behind its `positions` feature, on by default and not disabled); find the span, splice |
| dotenv | free | the parser is alint-owned and line-oriented; add per-value byte-offset tracking, splice |
| INI | free | same as dotenv (alint owns `ini.rs`) |
| properties | cheap | `java-properties` gives no spans; hand-roll a line/value-span locator (separators, backslash continuations, `\uXXXX`) |
| TOML | one dep | `toml_edit::DocumentMut` (present today only as a dev dependency via `trycmd`; promote to a real dep of `alint-rules`) |
| JSON | new dep | no in-tree JSON parser preserves formatting; add `jsonc-parser` (dprint), which records comment/value ranges, then splice |
| YAML | hard | no mature Rust library round-trips YAML comments; use a span-capable parser (`saphyr-parser`) to splice a single scalar (Unsafe until the re-parse postcondition passes); structural edits stay Suggestion |

Op-to-fixability, orthogonal to format: `*_path_equals` is fixable (set the node); `*_path_absent`
is fixable as a removal; `*_path_matches` and `json_schema_passes` are Never by default (no unique
target), though the Phase 1 `replace` template can make a `*_path_matches` fixable when the user
supplies one.

### 5.4 The structured bridge: serializing the value

Locating the span is only half the write; the other half is rendering the replacement value as
format-correct bytes, and it is where the real complexity lives. Each format needs a value
serializer that handles:

- **Quoting and escaping:** TOML bare vs quoted keys and strings; dotenv values with spaces;
  properties `\uXXXX` and separator escapes; XML entity-encoding of `& < >` in text vs
  attributes; JSON string escaping.
- **Type fidelity:** `equals: "true"` (a string) vs `equals: true` (a bool) must serialize
  differently; numbers, nulls, and booleans each have a canonical form per format.
- **`remove_value` surgery:** deleting a node means deleting its separator too (a trailing comma
  in JSON, the whole `key = value` line in TOML/dotenv/INI, the element in XML), which is
  format-specific.

Because `Op::Equals` holds an arbitrary `serde_json::Value`, three cases are explicitly **not**
Safe and route to Suggestion:

- **Object or array expected value** (deep-equal to a structure): the format-preserving
  synthesis of a multi-line structure at the right indentation is not a simple splice.
- **Zero-match `*_path_equals`** (the path is absent but should equal): there is nothing to
  replace; the fixer must synthesize `key: value` in the correct parent container, which for a
  missing intermediate (`$.a.b.c` where `.b` is absent) means **nested creation**.
- **Any edit whose re-parse postcondition the fixer cannot verify.**

Safe `set_value` is therefore restricted to a **scalar** expected value replacing an
**existing** scalar node, verified by re-parse. Everything else is Unsafe (removals) or
Suggestion (structures, insertion, nested creation).

### 5.5 Trust boundary for fixers

A fixer is a new trust surface, and the existing `SPAWNING_RULE_KINDS` allowlist does not cover
it: that gate checks the rule **kind** at load (`crates/alint-dsl/src/lib.rs:513`), so a
spawning or content-writing **fixer** attached to a non-spawning kind (for example a
`git_untrack` fix on `file_absent`, or a `replace` fix on `file_content_forbidden`) passes it
untouched. Without a new gate, an `extends:`-ed ruleset could ship a fix that silently rewrites
the user's files or shells out on a default `alint fix`, which is the file-writing and RCE
analogue of the spawning-kind hazard the repo already takes seriously. The rules:

- **Content-mutating fix ops** (`replace`, `set_value`, `remove_value`, `insert_header`, and
  any future op that rewrites existing bytes) and **spawning fix ops** (`git_untrack`, a
  `command`-backed fix, regenerate-from-command) are honored **only from the user's own
  top-level config**, never from an `extends:`-ed ruleset. This needs a new fix-level allowlist
  distinct from the kind-level `SPAWNING_RULE_KINDS`.
- **Applicability promotion is top-level-only.** An inherited fixer may be **demoted** (toward
  Suggestion) but never **promoted** (toward Safe/Unsafe). An inherited content-mutating fixer,
  if honored at all, defaults to Suggestion.
- `allow_out_of_root` remains top-level-only, unchanged.

### 5.6 DSL and CLI surface

- **New `fix:` ops.** `replace` (class 2/4, capture substitution); `set_value` and
  `remove_value` (class 3); `sort` and `dedup` (for `ordered_block` / `unique_by`); `chmod`;
  `git_untrack` (spawning, top-level-only); `sync_from` (whole-file copy from a canonical
  source, 2.2); `insert_header` (comment-style-aware). Each declares a default applicability.
- **Per-rule reclassification.** `fix: { op: ..., applicability: safe | unsafe | suggestion }`
  (Ruff's `extend-safe-fixes` model), subject to the promotion rule of 5.5.
- **`command` rule fixability.** The `command` plugin rule may carry a user-supplied fix
  command; it is a spawning fixer, so top-level-only and gated as in 5.5. Its name is distinct
  from the agent output's existing `fix_command` field (5.7).
- **CLI.** `alint fix` keeps applying Safe fixes by default; add `--unsafe-fixes`, `--diff`
  (print the patch without writing), and `--fix-only`. The `agent` and `json` outputs gain a
  per-fix `applicability` field and a proposed-edit payload for Suggestions (5.7).

### 5.7 Interaction surfaces

- **Baseline.** `alint fix` rejects `--baseline` today, so it acts on the unsuppressed set;
  after Phase 2 that would auto-rewrite a structured value a team deliberately grandfathered.
  The proposal makes `fix` **baseline-aware**: with a baseline, suppressed violations are
  skipped by the apply set and surfaced only as Suggestions. This is a behavior addition, called
  out as an open question.
- **`--changed`.** `Engine::fix` already honors `--changed`. The fixpoint can require writes to
  files outside the changed set (cross-file consistency, or a file a prior pass created); the
  proposal confines writes to the changed set plus files a fix in scope created, and treats a
  required out-of-scope write as a Suggestion rather than silently widening the blast radius.
- **LSP.** The LSP is not merely a Suggestion sink: a code action is fix application initiated
  by a human, so Suggestions **should** be offered there (with Unsafe/Safe distinguished in the
  action title). Multi-edit and multi-file fixes become a multi-document `WorkspaceEdit`. The
  LSP applies the edit and re-lints the buffer; it does not re-run the whole engine per
  keystroke, so the re-parse postcondition runs against the open buffer.
- **Agent output and the Suggestion data model.** The report types carry no tier today
  (`FixStatus` is `Applied | Skipped | Unfixable`) and the agent format emits `fix_available:
  bool` plus `fix_command: ["fix","--only",<id>]`. The proposal adds a `Suggested(edit)`
  outcome (or a tier field on `FixItem`), an `applicability` field on the agent violation, a
  `proposed_edit: {path, range, content}[]` payload for Suggestions, and gates `fix_command` to
  Safe/Unsafe (appending `--unsafe-fixes` for Unsafe). `has_unfixable_errors` is reconciled so a
  Suggested outcome is not counted as an unresolved error.

## 6. The phased plan

Each phase is independently shippable and ordered by value multiplied by feasibility. A phase
graduates into its own `docs/design/vX.Y/<name>.md` with the full seven-section treatment when
scheduled.

### Phase 0: the fix-engine foundation (no new user-facing fixers)

The substrate. Add `ReplaceRange` and `SetMode` to `FixEdit`; add `Applicability` to the fixer
contract (default Safe); add the rule-level `collect_edits` path alongside the existing
`apply` / `fix_edit`; replace the apply loop with **single-pass** collect / tier-filter / sort
(total order) / skip-overlap / verify / write; add `--unsafe-fixes`, `--diff`, `--fix-only`;
relocate the `fix_size_limit` guard onto the collect step. The **fixpoint is deliberately not in
Phase 0**, so the pass is a genuine no-op for existing configs. Retrofit all 12 existing ops onto
the engine with **zero behavior change for single-pass-convergent configs**, proven by
byte-identical fix-report snapshots including the adversarial same-file pairs (`file_header`
prepend plus `no_trailing_whitespace` trim; `final_newline` plus `max_consecutive_blank_lines`,
both touching EOF). Tier machinery ships but is not exercised end-to-end until Phase 1, so the
Phase 0 test plan does not over-claim tier coverage. Risk: low.

### Phase 1: located content replacement + the fixpoint (classes 2 and 4)

A `replace` fix op: a Rust regex plus a replacement template with capture substitution, emitting
`ReplaceRange` per match, wired to `file_content_forbidden` and `file_content_matches`. Unsafe by
default; Safe only when the replacement is provably a normalization. This phase also introduces
the **fixpoint loop and index invalidation** (5.2.3), since two `replace` rules can unblock each
other. Tests: firing, silent, idempotence, overlap between two `replace` rules on one file,
cross-fixer non-convergence hits the cap, multiline.

### Phase 2: structured value edits (the flagship, class 3)

Format-preserving `set_value` and `remove_value` via the locate bridge (5.3) and the value
serializer (5.4), driven by the rule-level `collect_edits` binding (5.2.1), wired to
`*_path_equals` (Safe only for a scalar replacing an existing scalar) and `*_path_absent`
(Unsafe removal). Non-scalar, zero-match insertion, and nested creation are Suggestions.
Sub-ordered strictly by dependency cost so value lands early: 2a (free) HCL, XML, dotenv, INI;
2b (one dep) TOML; 2c (cheap) properties; 2d (new dep) JSON; 2e (hard, scalar-only) YAML. Tests
per format: firing + silent + idempotence + a comment/order-preservation golden file + a
"produces invalid document -> demoted to Suggestion" case + a value-serialization matrix
(quoting, type fidelity, entity-encoding).

### Phase 3: metadata, VCS, and cross-file (classes 2.2 + metadata)

- **chmod (`SetMode`):** `shebang_has_executable` -> add +x (Safe); `executable_bit` -> set/clear
  (Unsafe); `executable_has_shebang` -> Suggestion.
- **VCS untrack:** a `git_untrack` fix (spawning, gated per 5.5) running `git rm --cached` plus an
  optional `.gitignore` line, Unsafe by default.
- **Sync-from-canonical (`sync_from`)** and cross-file partner creation, using the multi-file
  transaction of 5.2.4 for atomic create-and-register.
- **Directory create** (`dir_create`) for `dir_exists`; **relocate** for lockfile-not-at-root
  where the target is unambiguous (Suggestion where it is not).

### Phase 4: ordering, canonicalization, and the license-header inserter

- **`ordered_block` sort** and **`unique_by` dedup** (Safe, deterministic).
- **`indent_style`** leading-indent tab/space conversion (Safe for pure leading indentation only;
  Unsafe otherwise).
- **`.gitattributes` / `.gitignore` line insertion** (Safe, presence-guarded).
- **License / SPDX header inserter** (`insert_header`): comment-style-by-extension,
  shebang-and-xml-decl-aware, `.license` sidecar for uncommentable types. Presence-guarded, Safe.

### Deferred and special (not a numbered phase)

- **Reference/version pinning (class 7)** and **regenerate-from-command (2.2)**: both need to
  reach outside the tree (network, or a spawned generator), so they are top-level-only,
  explicitly-opted-in, spawn-gated fixes or future WASM plugins, never the default binary.
- **Whole-file reprint (class 1):** out of scope; alint is not a formatter.
- **`line_max_width` reflow; encoding transcode; JSON-Schema-guided fill:** unsafe or ambiguous;
  Suggestion at most, deferred behind demand.

## 7. False-positive and safety surface

This section is mandatory (TEMPLATE section 4) and applies across the phases.

- **Producing an invalid file.** The dominant risk for classes 2, 3, 4. Mitigation: the re-parse
  postcondition (5.2 step 4), memoized demotion to Suggestion for any edit that yields an
  unparseable file.
- **Discarding human content.** Mitigation: format-preserving splices only (never parse->dump);
  minimal ranges; the Unsafe tier for anything that removes content; structural/insertion edits
  kept to Suggestion.
- **Untrusted fixers from `extends:`.** Mitigation: the fix-level trust gate of 5.5 (content and
  spawning fix ops top-level-only; promotion top-level-only; inherited content fixers default to
  Suggestion). This is a new gate, distinct from the kind-level `SPAWNING_RULE_KINDS`.
- **Fixing a grandfathered violation.** Mitigation: baseline-aware `fix` (5.7) skips suppressed
  violations from the apply set.
- **Overlapping edits and non-convergence.** Mitigation: the total-order sort plus skip-overlap
  and isolation groups; the fixpoint cap with a loud error; the per-fixer idempotence test (which
  catches self-oscillation only, not cross-fixer cycles, 5.2.3).
- **Cross-file half-application.** Mitigation: the all-or-nothing multi-file transaction (5.2.4).
- **Ambiguous targets.** Mitigation: those stay Suggestion; alint proposes, a human or agent
  disposes.
- **Destructiveness and recoverability.** The practical undo is VCS. Keep the pre-commit "fix and
  fail nonzero" and `--diff` preview postures so machine edits are always reviewed.

## 8. Invariants and the decision record

The design upholds the constitution: determinism of fixes (invariant 1; the total edit order of
5.2.2 and a deterministic re-walk make the fixpoint reproducible); bounded fixes (invariant 4;
the `fix_size_limit` guard is relocated onto the new collect step); the trust boundary and path
confinement (invariants 5 and 6; note that a spawning **fixer** needs a **new** fix-level gate,
because the existing kind-level `SPAWNING_RULE_KINDS` does not cover a fixer attached to a
non-spawning kind); the coverage audits (invariants 7 and 8; every newly-fixable kind keeps its
schema entry and gains firing + silent scenarios, plus an idempotence test); and design-doc-first
plus ADR (invariants 12 and 13).

**ADR-0017** (proposed) records: (1) the ranged edit primitive plus the batched,
conflict-resolving, postcondition-verifying apply engine (single-pass in Phase 0, fixpoint from
Phase 1); (2) the four-state applicability model with safe-by-default and `--unsafe-fixes`
opt-in; and (3) the fixer trust boundary (content and spawning fix ops top-level-only, promotion
top-level-only).

**Not touched in the companion PR, to keep it green:** `ROADMAP.md` and the generated
`roadmap.json` (adding a `roadmap-public` marker requires regenerating `roadmap.json` under the
`gen-roadmap --check` gate). Proposed placement is a dedicated post-v0.16 cut, "Auto-fix
expansion"; wiring it in is a follow-up.

## 9. Open questions

1. **Reclassifying `file_remove`.** Deleting a whole file by default with no `--unsafe-fixes` is a
   poor safety default regardless of semver. Lean toward Unsafe with a one-release migration note;
   confirm before Phase 0 ships the tiers.
2. **Inherited fixers by default.** Should an `extends:`-ed ruleset's content-mutating fixer apply
   at all by default (as a Suggestion), or require the user to opt in per rule? 5.5 defaults it to
   Suggestion; the stricter option is "not honored unless re-declared top-level."
3. **`ReplaceRange` versus `SetContent` for class 3.** True ranged edits (best LSP diffs, real
   conflict detection) or `SetContent`-of-spliced-bytes? Leaning ranged, since Phases 1 and 3 need
   `ReplaceRange` regardless.
4. **YAML depth.** Is scalar-only YAML write-back (Unsafe, re-parse-guarded) enough for the real
   configs users care about, or is a heavier round-trip approach warranted later?
5. **Baseline-and-fix semantics.** Skip suppressed violations from the apply set (the proposal),
   refuse to run `fix` when a baseline exists, or surface suppressed findings as Suggestions only?
6. **Network-gated fixes.** Is a top-level-config-only, explicitly-opted-in network fix (SHA
   pinning) acceptable within the telemetry-free posture, or must it wait for the WASM sandbox?

## References and prior art

The state-of-the-art survey behind Part I, with primary sources.

- **ESLint `--fix`** (the range+text model, `SourceCodeFixer` sort/skip-overlap, the 10-pass
  fixpoint, `meta.fixable`, autofix vs suggestion).
- **Ruff `--fix`** (the `Applicability` Safe/Unsafe/DisplayOnly enum, `--unsafe-fixes`, isolation
  groups, the 100-iteration fixpoint, per-rule reclassification).
- **Formatters as idempotent reprinters** (Wadler/Oppen layout algebra, Black's AST-equivalence
  safety check, dprint's Wasm plugin host): Prettier, gofmt, rustfmt, Black, dprint.
- **Format-preserving structured editing** (CST/round-trip vs minimal `{offset,length,content}`
  splice): `toml_edit` (backs `cargo add`), `tomlkit` (Poetry), `ruamel.yaml` round-trip,
  Microsoft `node-jsonc-parser`, dprint `jsonc-parser`; the reserializing tools (`jq`, `yq`,
  `dasel`) as the anti-pattern.
- **Codemods** (match -> bind metavariables -> substitute -> replace-span): comby, ast-grep,
  jscodeshift/recast, Semgrep autofix, OpenRewrite (Lossless Semantic Tree).
- **Repo/config/dependency fixers** (read -> transform -> rewrite -> nonzero-on-change):
  pre-commit-hooks, eclint / editorconfig-checker, addlicense / licenseheaders / REUSE, ratchet /
  pin-github-action, Dependabot / Renovate, `npm audit fix`.
- **Repolinter** (todogroup/repolinter, archived 2026-02-06): the predecessor whose entire
  remediation surface was three blunt, auto-write-by-default ops (file-create, file-modify as
  prepend/append only, file-remove), with no diff preview and most rules unfixable. That is
  precisely the gap alint fills.

---

*Revision note: this draft was revised after an independent adversarial audit (technical +
design). Material changes from the first draft: the rule-level `collect_edits` violation-to-edit
binding (5.2.1); a total edit order (5.2.2); the fixpoint moved out of Phase 0 with an
index-invalidation story (5.2.3); multi-file transactions (5.2.4); the structured bridge split
into locate (5.3) and serialize (5.4) with Safe restricted to scalars; a fixer trust boundary
distinct from the kind-level `SPAWNING_RULE_KINDS` (5.5); baseline / `--changed` / LSP / agent
interaction surfaces and the Suggestion data model (5.7); the recount of the hygiene normalizers
to seven; and the removal of the undefined `MoveFile` primitive in favor of `RenameFile`.*
