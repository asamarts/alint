# Auto-fix: a systematic framework for mechanical remediation

Status: Accepted (ratified 2026-09-09; revised five times after independent adversarial audits, see the changelog note at the end). Execution is tracked in the companion [`auto-fix-implementation-plan.md`](auto-fix-implementation-plan.md).
Decisions: [ADR-0017](../adr/0017-auto-fix-edit-model-and-applicability.md) (accepted) records the load-bearing decisions (the batched range-edit apply engine, the applicability model, and the fixer trust boundary).
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
  - [9. Resolved decisions and open questions](#9-resolved-decisions-and-open-questions)
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

One apply engine runs over the primitives; every class is an adapter that emits primitives
with a declared tier. The engine has **two composition regimes** (5.2): whole-file transforms
compose in config order, and located edits go through a batched, verifying pass. The engine,
not the individual fixer, owns conflict resolution, determinism, postcondition verification,
and (from Phase 1) fixpoint iteration.

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

### 1.4 The gap is structural, not incidental

The catalog of unfixed kinds is not random. Several unfixed kinds carry an explicit rationale
in `docs/rules.md`; the rest fit the taxonomy on inspection. Grouped, they sort into five
buckets that map onto the applicability model formalized later:

1. **Deferred / not-yet-built, low ambiguity:** `executable_bit` (which notes "chmod auto-apply
   is deferred"), `shebang_has_executable`, `executable_has_shebang`, `ordered_block` (sort the
   marked lines), `indent_style` (deferred as "language-specific"), `dir_exists` / `dir_absent`
   (no directory-level op exists).
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
buckets 4 and 5 stay Never). That the existing rationales line up with the taxonomy is a good
sign the model is not an imposition. What is missing is (a) the edit primitive to express
located and structured changes, (b) an engine that can batch and verify them safely, and (c) a
first-class way to say "this fix exists but is not safe to apply unattended."

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
  `ordered_block` sort and line dedup are the clean wins; `.gitattributes` / `.gitignore` line
  insertion is adjacent, currently unbuilt; a `unique_by` collision has no unique automatic fix
  (which duplicate survives is ambiguous) and stays a Suggestion.
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
  code), is **idempotent** (the lens GetPut law, 5.8), passes the localized-equivalence
  acceptance test where it edits content (5.2 step 4), and **strictly reduces the violation
  multiset** without introducing violations of other rules (the Dershowitz-Manna termination
  contract, 5.8, which is what lets a Safe-only fixpoint terminate without relying on the cap).
  Example: the seven hygiene normalizers; setting a structured value to an exact declared
  **scalar** via a format-preserving edit. For a structured fix, Safe means the write-back is a
  **well-behaved lens** on that input (GetPut and PutGet hold at apply time, 5.8).
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

This maps cleanly onto the maintainer's existing five buckets (5-to-4, per 1.4) and gives alint three
properties it lacks: a machine-readable safety declaration on every fixer, a default that
only applies fixes that cannot surprise, and a first-class home for "proposed but not
automatic." The data model for Suggestions and their interaction with the existing agent
output is specified in 5.7.

**Trust:** the applicability tier a fixer runs at is not purely the author's to choose when
the rule arrives through `extends:`. Promotion toward Safe/Unsafe, and every content-mutating
or spawning op, is confined to the user's own top-level config (5.5). An inherited fixer may
be demoted but never promoted.

**Back-compatibility and `file_remove`:** the content and path-normalizing ops keep their current
behavior and are classified Safe on introduction. The one intentional change is `file_remove`
(used by `file_absent`, `no_empty_files`, `no_submodules`, `no_symlinks`, and the bundled
`hygiene/no-tracked-artifacts` ruleset): it is **reclassified Unsafe by default**, because deleting
a whole file irreversibly is a poor default for a bare `alint fix`. A one-release deprecation
warning ships first, and a user can **promote it back to Safe on a specific rule** via
`fix: { file_remove: { applicability: safe } }` in their own top-level config (per-rule promotion
is top-level-only, 5.5).

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
7. **Ordering / canonicalization: medium.** `ordered_block` sort and line dedup are clean,
   deterministic wins; a `unique_by` collision has no Safe fix (which duplicate file survives is
   ambiguous) and stays a Suggestion.
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
mechanical overlap detection (two located edits conflict iff their ranges intersect); and
composition (many located edits to one file merge into one write). Normalizers can be lowered
from a whole-file `SetContent` to minimal `ReplaceRange`s with an O(ND) Myers diff (Myers
1986), which tightens both the LSP diff and the overlap footprint.

Two edit shapes must not be conflated, and conflating them was a round-1 error this revision
corrects. A **whole-file transform** (the seven normalizers, or a create / remove / rename path op) is not
an edit at a span; the engine applies each in **config order** (exactly today's per-op behavior),
and Phase 0 replays that same order. It does *not* rely on these transforms commuting (they do
not: 5.8 gives a counterexample); reproducing today's config order is all the Phase 0 no-op
needs. A **located edit** (`ReplaceRange` from `replace` / `set_value` / `remove_value`) targets a
computed span and can genuinely overlap another, so it is the range/overlap machinery's job. A
whole-file `SetContent` is therefore **not** modeled as a `0..len` range for overlap purposes.
The two shapes do not co-apply to one file in a single pass: a file a whole-file transform touches
in a pass takes no located edits that pass, and located edits re-collect against the new bytes on
the next fixpoint pass (5.2.3), so every located byte offset stays valid. A single `*_path_absent`
violation expands to N node deletions (5.2.1). `ReplaceRange` is the **chosen** representation for
that fan-out rather than a strict necessity: a rule-level `collect_edits` (5.2.1) could instead
splice all N nodes internally and return one whole-file `SetContent`, which would conflict with
nothing, so the flagship does not *require* the ranged primitive. The engine adopts `ReplaceRange`
anyway because it gives the whole located-edit family - including the Phase 1 regex `replace`, where
two rules genuinely overlap one span - a single substrate with engine-level overlap detection and
minimal LSP diffs, instead of pushing that bookkeeping into each fixer. The leaner
single-`SetContent` alternative is recorded and rejected in ADR-0017 (resolved; 5.2.1).

### 5.2 The batched apply engine

Replace the single-pass loop with a batched model, keeping the serial-mutation guarantee. (Today
`Engine::fix` is already fully serial, evaluation included: a plain `for entry in &self.entries`
loop, not the rayon `PerFileRule` path `check` uses; so "parallel eval, serial fix" describes
`check`, and `fix` gains parallel edit *collection* only if later profiling warrants it.) The
engine has two composition regimes (5.1): **whole-file transforms** compose
functionally in config order, and **located edits** go through the collect / sort /
skip-overlap batch below. **Phase 0 ships only whole-file ops** (the existing 12), so it
composes them in config order in memory (one write per file), reproducing today's
disk-round-trip result exactly. The **located-edit batch, the overlap-skip, and the fixpoint
are built in Phase 0 but first exercised in Phase 1**, when located edits (and fixers that can
unblock one another) arrive. One located-edit pass:

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
4. **Apply in memory and verify the acceptance test.** The Safe acceptance test is
   **translation validation** (Pnueli et al. 1998), in two parts: (i) a **decidable re-parse**
   proving the output is still in the format language (syntactic; 5.8), and (ii) a **localized
   equivalence** proving the parsed tree equals the original everywhere except the targeted
   path and the target now holds the intended value (semantic; this is the lens PutGet law,
   5.8). Re-parse alone is insufficient: a splice can parse yet be wrong (a value that parses
   as the wrong type, a removal that deletes the wrong separator). An edit that fails either
   part is rejected and **demoted to Suggestion, memoized per (file, edit) for the run** so a
   stateless rule does not re-derive and re-demote it every pass.
5. **Write atomically**, once per file, via the existing `write_atomic`.
6. **Idempotence as a tested contract for Safe fixers.** Every Safe fixer gets a property
   test: apply twice, the second run is a no-op (the lens GetPut law, 5.8). A configurable
   Unsafe `replace` (for example `a` to `aa`) is not idempotent by construction; it relies on
   the bounded fixpoint and the loud non-convergence cap, not on idempotence.

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

so a fan-out rule computes the full match set once and re-runs its own query with a **located**
API (`serde_json_path`'s `query_located`, which yields each match's RFC 9535 **normalized
path**). A normalized path is a *logical* node identity, not a byte offset: it both
disambiguates the N-node binding and is the navigation key handed to the per-format bridge of
5.3, which recovers the actual byte span to splice. The rule then emits one edit per node: N
deletions for one `*_path_absent` violation, one edit per *failing* node for `*_path_equals`
(de-duplicated so M violations do not produce M identical edit batches). Simple per-file fixers
keep the one-liner `apply` / `fix_edit` path; only rules that fan out over matches implement
`collect_edits`. This is a Phase 2 prerequisite, not a Phase 2 detail.

#### 5.2.2 Determinism and total edit order

Constitution invariant 1 requires byte-identical fixes across runs. This is **reproducibility,
not canonicity**: the located-edit relation is not confluent (5.8), so the result is the unique
output of a fixed strategy, not an order-independent normal form. The strategy is a total order
on located edits. Sorting "by start then end" is not total when two edits share both offsets
(two rules rewriting one span), so the engine sorts by the total key
`(start, end, rule_index, violation_index)` with a stable sort, where `rule_index` is config
order and `violation_index` is the rule's deterministic emission order (JSONPath match order).
Ties resolve by config order, the determinism anchor elsewhere in the engine. Across
`nested_configs`, `rule_index` orders the root config's rules first, then each nested `.alint.yml`
in the engine's deterministic nested-discovery order, so the total order stays well-defined once
nested rules are lifted into the flat list. The applied subset is always pairwise byte-disjoint,
and disjoint edits commute (5.8), which is what makes skip-overlap sound.

#### 5.2.3 The fixpoint and index invalidation (Phase 1)

Applying one fix can unblock a previously-skipped conflicting fix or expose a new violation,
so from Phase 1 the engine loops collect -> apply -> re-check until a pass produces no edits or
a cap is hit (match Ruff; bail loudly on non-convergence). Two subtleties the single-pass Phase
0 avoids:

- **Index invalidation.** `CreateFile` / `DeleteFile` / `RenameFile` change the tree, but the
  `FileIndex` is built once. A pass that applied a path-mutating edit forces a **full
  deterministic re-walk** before the next pass (so cross-file and `requires_full_index()` rules
  see the new tree); a pass with only content edits re-checks just the touched files. This
  **amends ARCHITECTURE's "the walk runs exactly once per invocation" invariant for the `fix`
  path only** (`check` is unchanged), so ARCHITECTURE.md must be updated when the fixpoint lands
  (5.6). It also needs the walk configuration the current `Engine::fix` lacks (it takes a
  pre-built index and stores no `WalkOptions`): either thread `WalkOptions` into the fix engine,
  or hoist the fixpoint loop up to the `cmd_fix` layer that already holds them. A created file is
  not a "changed file," so path-mutating fixes and `--changed` interact (5.7).
- **Termination.** The per-fixer idempotence test catches **self**-oscillation only.
  **Cross-fixer** oscillation (fixer A reintroduces what fixer B removed) is caught solely by
  the iteration cap, which is why the cap exists and why non-convergence is a loud error, not a
  silent stop. The isolation groups of 5.2 (step 3) and careful tier assignment reduce, but
  cannot eliminate, cross-fixer cycles; a Safe-only fixpoint, by contrast, is guaranteed to
  terminate (5.8).

#### 5.2.4 Multi-file transactions

A single logical fix can span files (create-and-register, sync-from-canonical to N targets,
version propagation). The apply is **staged**: compute every file's new bytes in memory, verify
every postcondition **against the staged set** (a cross-file postcondition, such as "the created
crate now resolves in `workspace.members`", is checked over the staged buffers, not a single
file's), and only then write. This is genuinely all-or-nothing *through the verify phase*: any
collect or verify failure discards the whole group and writes nothing. The write phase is honest
about its limit: `write_atomic` is per-file with no cross-file journal, so a failure *during* the
writes (ENOSPC on file 3 of 5, after files 1-2's atomic renames already committed) leaves a
partially-applied group. The proposal shrinks that window (write all temp files first, then
rename them, since a rename failure is far rarer than a write failure) and reports a partial
apply loudly with the list of files that changed, rather than claiming a transactionality the
filesystem does not provide. A single fixer that errors during collect or apply skips *that
edit* and continues, matching today's per-violation `FixStatus::Skipped("fix error: ...")`
(`engine.rs:1006-1065`); it never aborts the run.

### 5.3 The structured bridge: locating the span

Class 3 needs, per format, a resolver from a normalized path (5.2.1) to the target value's byte
range. There is no shared abstraction to reuse: the current `Format::parse -> serde_json::Value`
is a **non-injective** parse, so no serializer inverts it (5.8), which is precisely why the
spans are gone and a re-serialize cannot preserve format. Two mechanisms recover the span: a
**round-tripping CST** (mutate a lossless tree, then re-serialize, so untouched nodes reproduce
their source verbatim) or a **spanned splice** (locate the node's byte range, splice the
original bytes). The verdicts below say which each format gets; the YAML "scalar-only"
restriction is a *consequence* of having only a spanned scalar parser there, not an ad hoc
caution. In build order by cost:

| Format | Verdict | Technique and dependency |
|---|---|---|
| HCL | free | parse with `hcl::edit` (already compiled in via `hcl-rs`, re-exported as `hcl::edit`); mutable CST, preserves layout |
| XML | free | `roxmltree` (already a dep) exposes `Node::range()` / `Attribute::range_value()` (behind its `positions` feature, on by default and not disabled); find the span, splice |
| dotenv | free | the parser is alint-owned and line-oriented; add per-value byte-offset tracking, splice |
| INI | free | same as dotenv (alint owns `ini.rs`) |
| properties | cheap | `java-properties` gives no spans; hand-roll a line/value-span locator (separators, backslash continuations, `\uXXXX`) |
| TOML | zero or one dep | the `toml` 1.1 crate alint already depends on ships `serde_spanned` (span-capable), so a spanned splice may need no new dep; or promote `toml_edit` (a full round-trip CST, present today only as a `trycmd` dev-dep) |
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
**existing** scalar node, verified by the acceptance test (5.2 step 4). Everything else is
Unsafe (removals) or Suggestion (structures, insertion, nested creation). The lens view (5.8)
makes this principled rather than arbitrary: on the zero-match creation case the lens `get` is
undefined, so its GetPut law is vacuous and PutPut unconstrained, which is exactly the "no
well-behaved fix here" signal that routes creation to Suggestion.

### 5.5 Trust boundary for fixers

A fixer is a new trust surface, and the existing `SPAWNING_RULE_KINDS` allowlist does not cover
it: that gate checks the rule **kind** (its `.contains(&kind)` sites are at
`crates/alint-dsl/src/lib.rs:271` / `545` / `579`), so a spawning or content-injecting **fixer**
attached to a non-spawning kind (a `git_untrack` fix on `file_absent`, or a `replace` fix on
`file_content_forbidden`) passes it untouched. Without a new gate, a remote `extends:`-ed ruleset
could ship a fix that silently injects bytes into the user's files or shells out on a default
`alint fix`, the file-writing and RCE analogue of the spawning-kind hazard. The gate keys on
**what the fixer can do** and **where it came from**:

- **Fixed-behavior fixers** (the seven hygiene normalizers, rename-to-case, `file_remove`, `chmod`,
  `dir_create`) carry no ruleset-supplied bytes and do not spawn, so there is no injection surface;
  they are honored at their own tier from any source. This is what keeps the bundled rulesets'
  trailing-whitespace / final-newline fixes auto-applying (they arrive via
  `extends: alint://bundled/...`); a destructive one like `file_remove` is already gated by its
  Unsafe tier, independent of source.
- **Content-injecting fixers** (`replace`, `set_value`, `sync_from`, `insert_header`, and
  `file_create` / `file_prepend` / `file_append` with inline content) are honored at their tier
  from the user's own top-level config, local-path `extends:`, a nested `.alint.yml` (all the
  user's own tree), and first-party **bundled** rulesets. From a **remote-URL `extends:`** they
  are **demoted to Suggestion** by default (they can propose an edit, never auto-write), because
  the content is authored by a third party. A top-level `trusted_extends:` allowlist opts
  specific remote URLs (a company's internal ruleset host) into honoring their content fixers at
  tier.
- **Spawning fixers** (`git_untrack`, a `command`-backed fix, regenerate-from-command) are
  **refused at load** from any non-top-level source (bundled included), matching the
  top-level-only posture of `SPAWNING_RULE_KINDS`.
- **Applicability promotion** toward Safe/Unsafe is top-level-only: an inherited fixer may be
  demoted but never promoted. `allow_out_of_root` remains top-level-only, unchanged.

**This gate is new plumbing, not a reuse of an existing mechanism (correcting an earlier draft).**
Today the loader `merge()`s every source's rules into one id-keyed list with **no source tag**,
and `RuleSpec` / `RuleEntry` carry no origin field; `allow_out_of_root` is a top-level policy
matched by id/kind, and inherited-rule safety is enforced by **rejecting dangerous keys at load**
(`reject_command_rules_in`, `reject_custom_facts_in` in `loader.rs` abort the whole load with an
error that names the offending source, rather than dropping one rule and continuing), so no
provenance needs to survive the merge. Two consequences: (1) the spawning-fixer **refusal** fits
that existing reject-at-load pattern directly, scanning each `extends:`-ed rule's `fix:` block; but
(2) the content-fixer **demotion** *keeps* the rule, so its four-way source (top-level /
local-path / bundled / remote-URL) must be newly threaded through `merge()` onto `RuleSpec` /
`RuleEntry` and carried to fix time, and `trusted_extends:` is a new top-level config key. This is
security-load-bearing work to build, not a mechanism to inherit.

### 5.6 DSL, CLI, and downstream surface

- **New `fix:` ops and their config surface.** Each new op declares a default applicability and
  binds to a specific set of host rule kinds; the existing "`fix.<op> is not compatible with
  <kind>`" load-time gate (enforced by each fixable builder, e.g. `no_empty_files.rs`,
  `file_header.rs`) is extended to the new pairings. Most ops read their parameters from the **host
  rule** so a fix restates no data the rule already carries; the table gives, per op, the host
  kind(s), what it reads from the rule, and the few genuinely new fields of the op object:

  | Op | Class | Host kind(s) | Reads from the rule | New op field(s) | Default tier |
  |---|---|---|---|---|---|
  | `replace` | 2/4 | `file_content_forbidden`, `file_content_matches`, `*_path_matches` | the search regex (`pattern:`; `matches:` on `*_path_matches`) | `replacement` (template, capture substitution) | Unsafe |
  | `set_value` | 3 | `*_path_equals` only | target `path:` and value (`equals:`) | none | Safe (scalar into an existing scalar), else Suggestion |
  | `remove_value` | 3 | `*_path_absent` | target `path:` (the node to delete) | none | Unsafe |
  | `sort` | 6 | `ordered_block` | `comparator` / `start` / `end` / `select` / `unique` | none | Safe |
  | `dedup` | 6 | `ordered_block` only | the marked-block bounds | none | Safe |
  | `chmod` | metadata | `executable_bit`, `shebang_has_executable`, `executable_has_shebang` | the desired bit (`require:`) | none (mode derived from `require:`) | Unsafe (Safe for shebang add-+x) |
  | `git_untrack` | VCS | `file_absent` (and `no_committed_binaries` once built) | the violating path | `gitignore` (also append a `.gitignore` line; default true) | Unsafe, spawning (top-level-only, 5.5) |
  | `sync_from` | 2.2 | `cross_file` (`identical` / `equals`) | the canonical `source:` file | none | Unsafe |
  | `insert_header` | 6 | `file_header` | (nothing: `pattern:` is a regex, unusable as literal bytes) | `text` (literal header) + `comment_style` (`line` / `block` / `auto`) | Safe (presence-guarded) |
  | `dir_create` | presence | `dir_exists` | the missing directory path | none | Safe |

  Three bindings need an explicit design call, because the obvious reading collides with an existing
  mechanism:
  - **`dedup` is `ordered_block`-only.** Deduping a marked line-block is a clean Safe transform. The
    superficially similar `unique_by` violation is cross-*file* (N files share a key), where "dedup"
    would mean *deleting* all but one source file with no principled rule for which survives; that is
    a destructive, ambiguous-target fix and routes to **Suggestion** (a human picks the survivor),
    never a Safe `dedup`. One op name must not span both.
  - **`insert_header` does not replace the existing `file_header` fix.** `file_header` is already
    fixable today via `file_prepend` (`file_header.rs:136-149` builds a `FilePrependFixer` from
    `content` / `content_from`), which inserts verbatim bytes. `insert_header` is a *distinct*,
    comment-style-aware op that renders the header in the file's comment syntax by extension; a rule
    carries one or the other (one op per `fix:` block), and plain `file_prepend` stays valid for an
    already-formatted literal block.
  - **`sync_from` takes its source from the host rule, not a new field.** It reads the canonical file
    from `cross_file`'s existing `source:`, so it introduces no `content_from:`-style field (which
    would duplicate the `file_create` / `file_append` content source). `cross_file` parses no `fix:`
    block today, so this is new fix-block plumbing on that kind.
- **Per-rule reclassification.** The schema encodes exactly one op per `fix:` block (the op is
  the key; each branch is `additionalProperties: false`), so applicability is a field *of the
  op's object*, not a sibling key: `fix: { file_remove: { applicability: safe } }`. The
  (currently empty-marker) op structs gain an optional `applicability: safe | unsafe | suggestion`
  (Ruff's `extend-safe-fixes` model), subject to the promotion rule of 5.5. This is how a user
  promotes `file_remove` back to Safe on a chosen rule.
- **Trust config.** A top-level `trusted_extends:` list (URL prefixes) opts specific remote
  `extends:` sources into having their content-injecting fixers honored at tier rather than
  demoted to Suggestion (5.5), for teams that trust their own internal ruleset host.
- **`command` rule fixability.** The `command` plugin rule may carry a user-supplied fix
  command; it is a spawning fixer, so top-level-only and gated as in 5.5. Its name is distinct
  from the agent output's existing `fix_command` field (5.7).
- **CLI and preview modes.** `alint fix` applies Safe fixes by default. `--unsafe-fixes` widens
  the set to include Unsafe; Suggestions never enter the apply set and render as a separate
  "proposed, not applied" block in every mode.

  | Mode | Writes? | Prints | Exit code |
  |---|---|---|---|
  | `fix` | yes | applied / skipped / suggested summary | nonzero if an error-level violation is unresolved (5.7) |
  | `fix --dry-run` (existing) | no | the same summary, "would" phrasing | same predicate, on the simulated result |
  | `fix --diff` (new) | no | a unified diff of the would-apply edits | same predicate |
  | `fix --fix-only` (new) | yes | applied only; residual-violation report suppressed | zero unless a fix errored |

  `fix --dry-run` doubles as the CI gate (write nothing, fail if fixes are pending), so no
  separate `fix --check` is added.
- **Downstream artifacts (per phase).** alint gates its own generated/derived surfaces, so each
  phase that adds an op or a status also updates, in the same PR: the hand-written `$defs/fix`
  branch in `schemas/v1/config.json` (`FixSpec` has no schemars derive, so `gen-schema` only
  re-syncs the byte-identical in-crate copy `crates/alint-dsl/schemas/v1/config.json`); the
  hand-written `schemas/v1/fix-report.json` for the new `suggested` status, guarded by the
  `crates/alint-output` round-trip test rather than `gen-schema` (constitution invariants 7 and 11);
  the `facts.json` `auto_fix_ops` count, which is code-derived (it counts `*Fixer` structs) and gated
  by `readme_auto_fix_ops_count_matches_fixers` (invariants 9 and 11);
  `README.md`'s headline counts; the per-op reference in `docs/rules.md`; and `ARCHITECTURE.md`'s
  fix-operations table and execution step 9 (which the batched engine supersedes, and which is
  already stale: it omits `file_footer`). `ROADMAP.md` / `roadmap.json` are wired when the arc is
  scheduled as v0.17 (section 9).

### 5.7 Interaction surfaces

- **Baseline.** `alint fix` rejects `--baseline` today, so it acts on the unsuppressed set;
  after Phase 2 that would auto-rewrite a structured value a team deliberately grandfathered.
  `fix` becomes **baseline-aware**: with a baseline, suppressed (grandfathered) violations are
  skipped by the apply set and surfaced only as Suggestions, mirroring `check --baseline`; only
  new violations are fixed. (Resolved; was an open question.)
- **`--changed`.** `Engine::fix` already honors `--changed`. The fixpoint can require writes to
  files outside the changed set (cross-file consistency, or a file a prior pass created); the
  proposal confines writes to the changed set plus files a fix in scope created, and treats a
  required out-of-scope write as a Suggestion rather than silently widening the blast radius.
- **LSP.** The LSP is not merely a Suggestion sink: a code action is fix application initiated
  by a human, so Suggestions **should** be offered there (with Unsafe/Safe distinguished in the
  action title). Multi-edit and multi-file fixes become a multi-document `WorkspaceEdit`. The
  LSP applies the edit and re-lints the buffer; it does not re-run the whole engine per
  keystroke, so the re-parse postcondition runs against the open buffer. Because a code action
  is computed against one buffer version and applied against another, edits carry a pinned
  document version (`OptionalVersionedTextDocumentIdentifier`) and are rejected on version
  drift; this is the one place true concurrency appears, and version-pinning, not operational
  transformation, is the right tool (5.8). `SetMode` (chmod) has no `WorkspaceEdit` representation
  (LSP has no permission-bit op), so it surfaces as a non-LSP Suggestion, not a code action; the
  new `FixEdit` and `FixStatus` variants also force mechanical match-arm updates across every
  formatter and the LSP `fix_edit_to_workspace_edit` mapping.
- **Platform.** `SetMode` (chmod) edits are skipped with a note on non-unix (Windows has no
  equivalent), consistent with `executable_bit`'s evaluate path already being `#[cfg(unix)]`.
  Symlink and `file_remove` fixes stay cross-platform, since `no_symlinks` can fire on Windows; a
  `SetMode` edit simply never enters the apply set there.
- **Output formats and the Suggestion data model.** Two report types must not be conflated: the
  check-side `Report` (what `check` emits; today carries only `is_fixable: bool` per violation) and
  the fix-side `FixReport` (what `alint fix` emits). The **automatic path is fix-side**: the
  `FixReport` gains a **`FixStatus::Suggested(edit)`** variant, and `has_unresolved`
  (`crates/alint-core/src/report.rs:106-110`, which today counts only `Skipped | Unfixable` and
  drives the nonzero `fix` exit) is **extended to count `Suggested` as unresolved**, so an
  error-level Suggestion still stands after `alint fix` and still drives a nonzero exit (the "fix
  and fail nonzero" posture of section 7 is preserved). Carrying a fix in a **check-side finding
  format** is a separate, deliberate feature: `alint check --format sarif` emits SARIF 2.1.0
  `result.fixes[]` (an `artifactChange` -> `replacement` of a `deletedRegion` + `insertedContent`,
  which `ReplaceRange { range, content }` maps onto almost 1:1) for **every tier** (Safe, Unsafe,
  Suggestion), tagging the tier in each fix's `description`. SARIF fixes are advisory (the consumer
  chooses to apply), so surfacing all tiers is safe and is the point of emitting SARIF at all;
  `agent` and `json` gain the same `proposed_edit: {path, range, content}[]` + `applicability`.
  Because computing an edit means running `collect_edits` during `check` (today `check` computes no
  edit bytes, only `is_fixable`), it runs **only when a fix-carrying format is selected**
  (`--format sarif` / `agent`, or `--format json --include-fixes`); the default `human` / `github`
  / `gitlab` / `junit` check path computes nothing and pays nothing, protecting the sub-second
  floor. `alint fix` still rejects the finding-only formats, so fixes ride the `check` path;
  `github`-annotations, `gitlab`, `junit`, and `markdown` carry only the fixable flag (no native
  edit slot).

### 5.8 Formal model: the guarantees the engine can and cannot make

Several tempting guarantees do not hold, and the doc must not claim them. Each paragraph gives
the model, what it buys, and the honest limit.

**The fix relation as an abstract rewriting system.** The located-edit engine is an abstract
rewriting system: a state is a repository, a fixer application is a rewrite step. It is **not
confluent**, because two fixers can target overlapping spans (an unjoined critical pair) and a
content edit can reintroduce another rule's violation, so different orders can reach different
results and the relation is not terminating in general. The total order of 5.2.2 therefore makes
the engine a **deterministic strategy over a non-confluent system**, which buys
**reproducibility** (a fixed order gives one result every run), **not canonicity** (that result
is not an order-independent normal form). The one positive law that holds is the analogue of
**orthogonality**: edits with disjoint byte ranges commute, so applying a pairwise-disjoint set is
order-independent (the property proved for non-overlapping / left-linear systems by Rosen 1973,
and later termed orthogonality), which is exactly why applying a pairwise-disjoint subset and
skipping overlaps is sound. The whole-file normalizers do **not** have this property among
themselves: they compose single-pass and do not all commute (for example `final_newline` appends a
bare `\n`, so on interior-CRLF input it does not commute with `line_endings: crlf`; a trailing
zero-width character shields preceding spaces from the trailing-whitespace trim). Phase 0 does not
rely on their commuting; it reproduces today's single-pass config-order composition exactly, which
is a no-op whether or not that order is canonical. (Newman 1942; Baader and Nipkow 1998; Rosen
1973.)

**Termination of the Safe tier.** Uniform termination of a rewriting system is undecidable (Huet
and Lankford 1978), which is why the fixpoint carries a hard cap and bails loudly. But the Safe
tier can terminate by construction: require every Safe fixer to **strictly shrink the set of
outstanding violations** (resolve at least one, introduce none of any rule). The count then falls
by at least one each non-final pass, so a Safe-only fixpoint reaches a fixed point in at most `|V|`
passes with no reliance on the cap. (The full Dershowitz-Manna multiset ordering, 1979, is only
needed if the contract is relaxed to "replace a violation with strictly-lower-ranked ones"; under
the strict "introduce none" contract a plain cardinality argument suffices.) The cap guards only
Unsafe and user-promoted fixers, which carry no such guarantee (an arbitrary regex `replace` can
loop). This makes "a Safe fixer" a checkable contract.

**Not a least fixed point (a caveat, not a theorem).** It is tempting to call the loop a
least-fixed-point computation (Kleene / Knaster-Tarski). In general it is not: the apply operator
is non-monotone (an Unsafe fix can add violations) and the state space is not a complete lattice.
Even under the Safe remove-only contract the reading is only partial: the loop is a
strictly-**descending** iteration from the top (all of `V`) on the finite violation-inclusion
lattice, so it reaches **a** fixed point in at most `|V|` steps (the lattice height), but this is a
greatest-fixed-point-flavored descent, not the ascending-from-bottom Kleene least fixed point, and
without confluence that fixed point need not be unique. The failure of monotonicity for the general
operator is precisely *why* the cap exists; the cap is not an implementation detail of an
otherwise-guaranteed convergence.

**The structured write-back is a lens.** A structured read (`*_path_equals`, the query) and its
write-back (`set_value`, the splice) form an asymmetric **lens** (Foster et al. 2007; the
database view-update problem, Bancilhon and Spyratos 1981): `get(source) = value`,
`put(value, source) = source'`. The **lens laws are the correctness specification and the test
oracle**, and they line up with the acceptance checks:

- **GetPut** (`put(get(s), s) = s`): writing back the value already present is a no-op. This is
  format-preservation stated precisely: byte-exact stability that forbids touching any byte the
  edit did not mean to change (idempotence alone permits gratuitous byte changes on a first
  application). GetPut and PutGet together in fact imply the fixer is idempotent.
- **PutGet** (`get(put(v, s)) = v`): after the fix, the query returns the intended value. This is
  the localized-equivalence half of the acceptance test (5.2 step 4).
- **PutPut** (the "very-well-behaved" law) does **not** hold in general for format-preserving
  edits; treat its failure as a **demotion signal**, never a guarantee.

A **Safe structured fix is defined as a well-behaved lens on that input** (GetPut and PutGet hold,
checked at apply time). The degenerate cases justify the Suggestion routing of 5.4 rather than
merely cautioning it: on a zero-match path there is no `get`, so GetPut is vacuous and nothing
constrains the write, which is the formal statement of "there is no well-behaved fix here."
(Foster, Greenwald, Moore, Pierce, Schmitt 2007; Boomerang, Bohannon et al. 2008.)

**Why re-serialization is lossy, and what re-parse guarantees.** `Format::parse` is a
**non-injective** function (many byte strings, differing in whitespace, comments, and key order,
map to one `serde_json::Value`), so it has no **left inverse**: no serializer `Q` recovers the
original bytes (`Q . parse = id`). (It does have right inverses: an ordinary serializer `Q'`
satisfies `parse . Q' = id` on the value; what is impossible is recovering the discarded bytes.)
That is the formal reason the value tree cannot preserve format and a lossless
CST or a spanned splice is required (Roslyn red-green trees; rust-analyzer's Rowan; the `cstree`
crate). The re-parse in the acceptance test is a **decidable membership check** in the format
language, a real guarantee that no file outside the language is ever written, but it is
**syntactic only**: "re-parses" does not imply "correct" (a splice can produce a valid document
with the wrong value), which is why the acceptance test pairs it with the PutGet check.

**The Safe tier is translation validation, not verified transformation.** The right posture is
**translation validation** (Pnueli, Siegel, Singerman 1998): rather than prove each fixer correct
for all inputs (CompCert-grade, Leroy 2009, a cost `formal-methods.md` explicitly declines for a
batch linter), validate **each run** by checking a localized equivalence between input and
output, exactly as Black's `--safe` re-parses its own output and diffs the AST (adapted here to
"equal everywhere except the declared change-set"). The guarantee is therefore **per-run
validation on unverified fixers**, backed by property tests, not a proof that every fixer is
semantics-preserving.

**What the engine does not guarantee** (the honest ledger): the fix result is not independent of
rule order (only reproducible for a fixed order); the loop is not a Kleene least fixed point (even
under the Safe contract it is a strictly-descending iteration to a fixed point, not an ascending
least one); the write-back is well-behaved but
not very-well-behaved (no PutPut); "re-parses" is syntactic, not semantic; the Safe tier is
validated per run, not proven; and no operational transformation or CRDT is needed, because a
single-writer total order is stronger than eventual convergence (OT is a future LSP-only concern,
5.7).

### 5.9 Performance and the perf-gate

alint's identity is speed (a benchmarked sub-second floor at ~100k files), so the fix engine must
not erode it, and the proposal commits to keeping it on the gate rather than asserting it.

- **Cost shape.** A located-edit pass is one sort (`O(e log e)` in the edits) plus a linear
  splice; the fixpoint runs at most `k` passes (the cap). A content-only pass re-checks just the
  touched files; a pass that applied a path-mutating edit costs a **full re-walk + re-check** (one
  `check` pass) before the next iteration, which is the dominant term and the reason path-mutating
  fixers are rare and the cap is small. A Safe-only run converges in at most `|V|` passes (5.8),
  in practice one or two.
- **The default paths stay free.** `alint check` without a fix-carrying format computes no edits
  (5.7); `alint fix` runs serially after a single evaluation and is not on the interactive hot
  path. Only a fix-carrying `check` (a CI SARIF/agent run) or a multi-pass `fix` pays extra, and
  only in proportion to the fixable violations present.
- **Gate it, do not trust it.** `fix` is **not** on the deterministic perf-gate today
  (`crates/alint-bench/benches/det_check.rs` gates `check` scenarios only; the one `fix` bench is
  criterion-only, dry-run, and capped at 1k files). Phase 0 adds a gated `fix` scenario and a
  fixpoint-convergence scenario to the Valgrind/Ir gate, so a fix-path regression trips CI the way
  a check-path one does.

## 6. The phased plan

Each phase is independently shippable and ordered by value multiplied by feasibility. A phase
graduates into its own `docs/design/vX.Y/<name>.md` with the full seven-section treatment when
scheduled.

### Phase 0: the fix-engine foundation (no new user-facing fixers)

The substrate. Add `ReplaceRange` and `SetMode` to `FixEdit`; add `Applicability` to the fixer
contract (default Safe); add the rule-level `collect_edits` path alongside the existing
`apply` / `fix_edit`; build the located-edit machinery (collect / tier-filter / total-order sort
/ skip-overlap / verify / write) and the fixpoint; add `--unsafe-fixes`, `--diff`, `--fix-only`;
relocate the `fix_size_limit` guard onto the collect step. Because Phase 0 ships **only whole-file
ops**, the engine applies them by **functional composition in config order** (5.1, 5.2), which
reproduces today's disk-round-trip result exactly, so Phase 0 is a **genuine no-op** for existing
configs, including the adversarial same-file pairs (`no_trailing_whitespace` trim plus
`final_newline`; `file_header` prepend plus `max_consecutive_blank_lines`, both touching EOF)
that would each get only one fix under a naive `0..len` overlap-skip. The located-edit batch, the
overlap-skip, and the fixpoint are built here but first *exercised* in Phase 1, so the Phase 0
test plan is byte-identical snapshots of the whole-file composition **plus a per-fixer
`apply`-vs-`fix_edit` byte-parity property test** (the batched engine composes the `fix_edit` data
path, while today's `alint fix` uses `apply`, and those paths can drift; the codebase already
carries such a guard, e.g. `bom_fix_edit_binary_guard_mirrors_apply`) - not a claim of tier or
located-edit coverage. Risk: low.

### Phase 1: located content replacement + the fixpoint (classes 2 and 4)

A `replace` fix op: a Rust regex plus a replacement template with capture substitution, emitting
`ReplaceRange` per match, wired to `file_content_forbidden` and `file_content_matches`. Unsafe by
default; Safe only when the replacement is provably a normalization. This phase also **first
exercises the fixpoint loop and index invalidation** (5.2.3, both built in Phase 0), since two
`replace` rules can unblock each other. Tests: firing, silent, idempotence, overlap between two `replace` rules on one file,
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

### Phase 3: metadata, VCS, and the repo-scale cross-file classes (2.2)

- **chmod (`SetMode`):** `shebang_has_executable` -> add +x (Safe); `executable_bit` -> set/clear
  (Unsafe); `executable_has_shebang` -> Suggestion.
- **VCS untrack:** a `git_untrack` fix (spawning, gated per 5.5) running `git rm --cached` plus an
  optional `.gitignore` line, Unsafe by default.
- **Sync-from-canonical (`sync_from`)** and cross-file partner creation, using the multi-file
  transaction of 5.2.4 for atomic create-and-register.
- **Directory create** (`dir_create`) for `dir_exists`; **relocate** for lockfile-not-at-root
  where the target is unambiguous (Suggestion where it is not).

### Phase 4: ordering, canonicalization, and the license-header inserter

- **`ordered_block` sort** and **`ordered_block` dedup** (Safe, deterministic). A `unique_by`
  collision has no Safe fix - deleting all but one of N files that share a key is destructive and
  the survivor is ambiguous - so it surfaces as a Suggestion (5.6).
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
- **Cross-file half-application.** Mitigation: the multi-file transaction (5.2.4), all-or-nothing
  through verify and best-effort (no cross-file journal) through the per-file writes.
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

**ADR-0017** (accepted) records: (1) the ranged edit primitive plus the batched,
conflict-resolving, postcondition-verifying apply engine (whole-file ops composed in config
order in Phase 0; the located-edit batch and the fixpoint built in Phase 0 but first exercised in
Phase 1); (2) the four-state
applicability model with safe-by-default and `--unsafe-fixes` opt-in, including the
Dershowitz-Manna termination contract for Safe fixers and the well-behaved-lens acceptance test
for structured fixes (5.8); and (3) the fixer trust boundary (spawning fix ops refused from
`extends:`, content fix ops demoted to Suggestion, promotion top-level-only).

**Verification plan.** The formal model of 5.8 maps onto the repo's existing verification tiers
(`docs/design/formal-methods.md`): proptest for the algebraic laws (GetPut, PutGet,
disjoint-edit commutativity, overlap-skip disjointness, total-order well-formedness, localized
semantic preservation, and Safe-tier termination in at most `|V|` passes); one bounded Kani proof
for the load-bearing combinatorial invariant (the splice primitive is byte-correct, or the
overlap detector yields a pairwise-disjoint applied set), the natural sequel to the existing
`confine_steps_is_sound` path-confinement proof; and debug_assert runtime contracts (the applied
set was disjoint; PutGet holds on the buffer after a Safe structured edit; a multi-file abort
committed nothing).

**Not touched in the companion PR, to keep it green:** `ROADMAP.md` and the generated
`roadmap.json` (adding a `roadmap-public` marker requires regenerating `roadmap.json` under the
`gen-roadmap --check` gate). Proposed placement is a dedicated post-v0.16 cut, "Auto-fix
expansion"; wiring it in is a follow-up.

## 9. Resolved decisions and open questions

Resolved after review (folded into the sections above):

- **`file_remove` default:** reclassified **Unsafe** with a one-release migration and a per-rule
  Safe override (3, 5.6).
- **Inherited fixers:** gated by capability and provenance: fixed-behavior fixers are honored from
  any source (so bundled hygiene keeps auto-applying), remote-URL content-injecting fixers are
  **demoted to Suggestion** with a `trusted_extends:` opt-in, and spawning fixers are refused from
  any non-top-level source (5.5).
- **`ReplaceRange` vs `SetContent`:** **ranged**. The multi-node structured fan-out does not
  strictly require it (a rule-level `collect_edits` could emit one whole-file `SetContent`), but a
  ranged primitive gives the whole located-edit family one substrate with engine-level overlap
  detection and minimal LSP diffs; the leaner single-`SetContent` alternative is recorded and
  rejected in ADR-0017 (5.1, 5.2.1).
- **Baseline and `fix`:** **baseline-aware**; skip suppressed violations, surface them as
  Suggestions, fix only new ones (5.7).
- **Network-gated fixes:** the core stays network-free; SHA-pinning is a separate, top-level-only,
  explicitly-opted-in fix or a future WASM plugin, never a bare `alint fix` (6, Deferred).
- **Versioning:** the arc slots as **v0.17**; it introduces the tiers and a deprecation warning,
  then flips `file_remove` to Unsafe about two minors later (a warned, pre-1.0 MINOR change) (5.6,
  6).
- **Fixes in finding output:** `check --format sarif` emits SARIF `fixes[]` for all tiers, and
  `agent` / `json --include-fixes` carry a `proposed_edit`; the edit is computed during `check`
  **only** when such a format is selected, so the default check path is unchanged (5.7).

Still open:

1. **YAML edit depth.** Is scalar-only YAML write-back (Unsafe, re-parse-guarded) enough for the
   real configs users care about, or is a heavier round-trip approach warranted later? Decide with
   corpus evidence before scheduling sub-phase 2e.

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

The theoretical foundations behind section 5.8:

- **Abstract rewriting** (confluence, termination, critical pairs, strategies): Newman 1942;
  Baader and Nipkow, *Term Rewriting and All That*, 1998; Rosen, "Tree-manipulating systems and
  Church-Rosser theorems," JACM 1973; Dershowitz and Manna, "Proving termination with multiset
  orderings," CACM 1979; Huet and Lankford 1978 (undecidability of termination).
- **Bidirectional transformations / lenses**: Foster, Greenwald, Moore, Pierce, Schmitt,
  "Combinators for bidirectional tree transformations," ACM TOPLAS 2007; Bancilhon and Spyratos,
  "Update semantics of relational views," ACM TODS 1981; Bohannon, Foster, Pierce, Pilkiewicz,
  Schmitt, "Boomerang," POPL 2008.
- **Minimal edits**: Myers, "An O(ND) difference algorithm," Algorithmica 1986; Zhang and Shasha,
  tree edit distance, SIAM J. Comput. 1989.
- **Lossless syntax trees**: Roslyn red-green trees; rust-analyzer's Rowan; the `cstree` crate.
- **Semantics preservation**: Pnueli, Siegel, Singerman, "Translation validation," TACAS 1998;
  Leroy, "Formal verification of a realistic compiler" (CompCert), CACM 2009; Black's `--safe`
  AST-equivalence check.

The completeness-theory grounding for the detection surface (finite model theory, the Chomsky
hierarchy, relational dependency theory) lives in the companion
[`rule-coverage-gaps.md`](rule-coverage-gaps.md).

---

*Revision note: revised five times after independent adversarial audits. Round 1 (technical +
design) added the rule-level `collect_edits` binding (5.2.1), a total edit order (5.2.2),
multi-file transactions (5.2.4), the locate/serialize split (5.3, 5.4), the fixer trust boundary
(5.5), the interaction surfaces (5.7), the recount to seven normalizers, and dropped the undefined
`MoveFile`. Round 2 (a second audit plus a mathematical-foundations pass) corrected the Phase 0
"zero behavior change" claim (whole-file transforms compose in config order; located edits and the
fixpoint are Phase 1) and the `Suggested`-outcome exit code, and added the formal model (5.8).
Round 3 (a third audit, a theory-correctness referee pass, and a resolved-decisions pass with the
maintainer) corrected a false "the normalizers are confluent" claim (they are not; Phase 0 relies
only on replaying today's config order, 5.1 and 5.8) and a swapped inverse term (left, not right,
inverse, 5.8), sharpened the fixed-point and counting framings, specified whole-file-vs-located
sequencing (5.1, 5.2.3) and the `Suggested` exit-code plumbing (5.7), refined the trust boundary to
gate content-injecting fixers by provenance with a `trusted_extends:` opt-in while leaving
fixed-behavior fixers untouched (5.5), and recorded the maintainer's decisions: `file_remove`
Unsafe-by-default with a per-rule override, baseline-aware `fix`, and a network-free core (3, 9).
Round 4 (an implementability-plus-cross-repo-consistency audit and a completeness gap-hunt)
corrected the trust boundary's false claim that per-source provenance already exists in the loader
(it must be built; 5.5), noted the fixpoint re-walk amends ARCHITECTURE's "walk once" invariant and
listed ARCHITECTURE / schema / `facts.json` / README / `docs/rules.md` as downstream artifacts each
phase must update (5.6), fixed the per-rule `applicability` schema shape, made the multi-file
failure model honest (all-or-nothing through verify, best-effort through the per-file writes,
5.2.4), and added SARIF `fixes[]` for all tiers on `check --format sarif`, a
performance-and-perf-gate section (5.9), a preview-mode table, and Windows / `nested_configs`
handling. The maintainer resolved: the v0.17 warned-migration slot, all-tier SARIF fixes, and
computing check-side edits only when a fix-carrying format is selected. Round 5 (a holistic
coherence and drift audit, a coverage-doc re-audit with DSL worked-examples, and an
over-engineering critique) specified 5.6's per-op config surface (host kinds, inputs-read-from-rule
versus new op fields) for every new op and resolved three collisions the worked-examples exposed
(`dedup` is `ordered_block`-only, since deduping `unique_by` would delete source files;
`insert_header` is distinct from the existing `file_prepend`-on-`file_header` fix; `sync_from` reads
`cross_file`'s `source:` rather than a new content field), defined the referenced-but-undefined
`dir_create` op, fixed a schema-shape contradiction (the section 3 `file_remove` promotion example
used the invalid sibling-key form) and a run of cross-reference drift (the section 9 TOC anchor, a
dangling "open question 3", the round count, the Thesis two-regime framing, and the multi-file
"all-or-nothing" qualifier), and made the `ReplaceRange` justification honest (a chosen substrate,
not a strict necessity, since a single `collect_edits` could emit one `SetContent`). The maintainer
resolved to keep the full-engine-first plan of section 6 as written, with the leaner "defer the
located-edit engine" alternative recorded and rejected in ADR-0017.*
