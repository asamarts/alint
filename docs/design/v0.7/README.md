# v0.7 — Design pass

Status: Working drafts, written 2026-04-28 after the v0.6 cut
shipped. Each file in this directory is a per-feature design
that should be reviewed and revised before implementation
starts.

## What v0.7 ships

Three new rule kinds and two new subcommands. All of them
were sketched in `PROPOSAL-AGENTS.md` (local-only, see
`.gitignore`) as "Tier 2" / "Tier 3" follow-ups to v0.6's
bundled-ruleset cut. Where v0.6 was config-only — every
feature composed from existing primitives — v0.7 actually
extends the engine with new rule kinds and new CLI surface,
so each one needs deliberate design before code.

| File | Feature |
|---|---|
| [`commented_out_code.md`](./commented_out_code.md) | New rule kind: heuristic detector for blocks of commented-out code. |
| [`git_blame_age.md`](./git_blame_age.md) | New rule kind: catch matching lines older than N days (stale TODOs, etc.). |
| [`markdown_paths_resolve.md`](./markdown_paths_resolve.md) | New rule kind: validate backticked workspace paths in markdown files. |
| [`alint_suggest.md`](./alint_suggest.md) | New subcommand: scan a repo for known antipatterns and propose rules. |
| [`alint_export_agents_md.md`](./alint_export_agents_md.md) | New subcommand: render the active rule set as an `AGENTS.md` directive section. |

## Cross-cutting decisions

A few questions touch multiple features and benefit from
being settled once.

### gix vs. `git` shell-out

Both `git_blame_age` (line ages) and `alint suggest` (repo
ecosystem detection) need git operations. Two options:

- **Add `gix` as a workspace dep.** Pure-Rust git, no
  external `git` binary required, used by `gitoxide`-family
  tools. Already on the deps roadmap per PROPOSAL.md §5.3.
- **Shell out to `git`.** No new dep; uses whatever git the
  user has. Currently how `alint-core::git` /
  `git_no_denied_paths` etc. work.

Recommendation: **shell out to `git`** for v0.7. We already
do this in `alint-core::git`; staying consistent is cheaper
than introducing a new dep for two features. Revisit when
LSP lands (v0.8) — long-running processes benefit more from
in-process git.

### Heuristic vs. precise

`commented_out_code` and `markdown_paths_resolve` both have
heuristic surfaces. The bundled-ruleset experience (v0.6
field test) showed false-positive rate dominates user
perception — a 30% FP rate is treated as broken even when
the rule catches real issues 70% of the time.

Default policy for v0.7 heuristic rules:
- **Severity floor `info` / `warning`, never `error` by
  default.** Users escalate per-rule once their workflow
  proves the FP rate is acceptable for their codebase.
- **Generous `paths.exclude` defaults**, matching the
  shape of the v0.6 agent-hygiene tuning.
- **Document the FP profile in the rule's help text.**
  Don't let the user discover it from a CI failure.

### Naming convention for new rule kinds

All v0.7 rule kinds use `snake_case` (matching the existing
catalogue). Aliases only when there's a clear short-form
that's already idiomatic — `commented_out_code` has none,
`git_blame_age` has none, `markdown_paths_resolve` could
take `md_paths_resolve` but it's not a common shorthand.
Skip aliases unless asked.

### Schema versioning

All v0.7 rule kinds are additive — they parse new YAML
shapes that v0.5/v0.6 configs simply don't use. No schema
version bump needed; `version: 1` covers them.

## Out of scope for v0.7

Explicitly held back to keep the cut tight:

- **WASM plugin tier** — still v0.9.
- **LSP server** — still v0.8.
- **`commented_out_code` with full AST awareness** — v0.7
  ships a regex-and-token-density heuristic; precise AST
  detection would need per-language parsers and is out of
  proportion for a structural linter.
- **`alint suggest --apply`** — for v0.7 the subcommand
  prints proposals to stdout. Auto-applying them edits
  user config and is a separate decision (probably v0.8
  after the LSP lands and fix-application UX is mature).
- **`alint export-agents-md --bidirectional`** — auto-syncing
  AGENTS.md changes back into the rule config is tempting
  but cuts the wrong way (humans edit AGENTS.md for
  prose; the rule set is the source of truth). One-way
  generation only.

## Implementation order

Roughly easiest-first:

1. `markdown_paths_resolve` — cleanest design surface,
   well-bounded, no git dependency.
2. `commented_out_code` — needs careful heuristic tuning
   but no new external integrations.
3. `git_blame_age` — adds the first git-blame dependency
   to the rules layer.
4. `alint suggest` — depends on the rule kinds above
   being available so its proposals can name them.
5. `alint export-agents-md` — last because it pulls in
   formatting decisions that benefit from being made
   when all the other v0.7 rules are concrete.

## How to use these docs

Each design doc has the same shape:

1. **Problem** — what user pain this addresses, sourced
   from the v0.6 field test or PROPOSAL-AGENTS.md.
2. **Schema** — the YAML the user writes.
3. **Semantics** — what the engine does on each match.
4. **False-positive surface** — what could go wrong and
   the planned mitigations.
5. **Implementation notes** — crate location, dependencies,
   complexity estimate.
6. **Tests** — what to cover.
7. **Open questions** — decisions to make before
   implementation.

When implementation starts, the doc gets a `Status:
implemented in <commit>` header line and any open
questions get resolved in the doc itself, not lost in
commit messages.
