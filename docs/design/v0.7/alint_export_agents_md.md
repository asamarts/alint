# `alint export-agents-md` subcommand

Status: Design draft.

## Problem

Teams adopting alint in agent-heavy workflows end up
maintaining the same rules in two places:

1. **`.alint.yml`** — the rules alint enforces at commit
   time.
2. **`AGENTS.md` / `CLAUDE.md` / `.cursorrules`** — the
   directives the agent reads on every session, telling
   it what NOT to do.

Per Augment Code's research cited in
`PROPOSAL-AGENTS.md` §4: "67% of development teams are
maintaining duplicate configurations" between agent-context
files and their CI lint configs. The two drift, the agent
violates rules the lint catches, the user reaffirms the
rule in AGENTS.md, repeat.

`alint export-agents-md` makes alint the single source
of truth: the active rule set generates a section of
`AGENTS.md` containing one directive per rule. The agent
reads the rules from `AGENTS.md`; alint enforces them at
commit time. Edits to either propagate via running the
command.

## Schema

CLI subcommand. Invocation:

```
alint export-agents-md [options]

Options:
  --config <path>          Same as global --config (read rules from here).
  --output <path>          Where to write. Default: stdout. With --inline,
                           edits the file in place.
  --inline                 Splice into an existing file between
                           `<!-- alint:start -->` and `<!-- alint:end -->`
                           markers. Creates the section if absent.
  --section-title <text>   Heading for the generated section.
                           Default: "Lint rules enforced by alint".
  --include-info           Include `level: info` rules. Default: skip them
                           (info-level rules are nudges, not directives).
  --format <markdown|json> Output format. Default markdown.
```

## Output shape

### Markdown (default)

The generated section looks like:

```markdown
<!-- alint:start -->

## Lint rules enforced by alint

Generated from `.alint.yml` by `alint export-agents-md`.
Re-run after editing the lint config — these directives
must stay in sync with what alint blocks at commit time.

### Errors (commit will fail)

- **`agent-no-debugger-statements`**: `debugger;` /
  `breakpoint()` must not be committed — these halt
  execution at runtime. Remove before merge.
- **`oss-no-merge-conflict-markers`**: Merge-conflict
  markers must not be committed. See
  [policy](https://opensource.guide/legal/...).

### Warnings (review before merge)

- **`agent-no-scratch-docs-at-root`**: Scratch / planning
  documents should not be committed at the repo root.
  Move the content into a real doc (CHANGELOG, ADR,
  design doc, README) or delete it once the work is done.
- **`agent-no-console-log`**: `console.log` / `.debug`
  / `.trace` left in non-test source. Route through the
  project logger or remove before merge.
- ...

### Info (informational nudges, only shown with --include-info)

— (omitted by default; pass `--include-info` to include.)

<!-- alint:end -->
```

Rule-to-directive mapping:
- Section grouping: by severity (`error` / `warning` /
  `info`).
- Heading per group with a brief explanation of what
  that severity means at commit time.
- One bullet per rule: bold rule id + colon + the rule's
  `message` (with markdown rendered through). Trailing
  policy URL becomes a `[policy](...)` link if present.
- Generated section is sorted by `(severity desc, rule_id
  asc)` so diffs across runs are stable.

### JSON output

Parallel structure for agent consumption:

```json
{
  "schema_version": 1,
  "format": "agents-md",
  "generated_at": "2026-04-28T15:23:01Z",
  "directives": [
    {
      "rule_id": "agent-no-debugger-statements",
      "severity": "error",
      "directive": "`debugger;` / `breakpoint()` must not...",
      "policy_url": null
    },
    ...
  ]
}
```

## Inline mode

`--inline <file>` is the typical workflow. Editing rules:

1. User runs `alint export-agents-md --inline AGENTS.md`.
2. The command finds the markers `<!-- alint:start -->` /
   `<!-- alint:end -->` in the file.
3. Replaces everything between them with the freshly
   generated content.
4. Writes the file in place.

If the markers don't exist, the command appends a fresh
section at the end of the file (with markers) and warns
on stderr. Subsequent runs re-find the markers.

The markers are deliberately HTML comments so they
render invisibly in markdown. They're not alint-specific
to the casual reader — only the exact string matters.

## Semantics

1. Load the same `.alint.yml` and resolve `extends:`
   exactly like `alint check` does.
2. Filter rules by `level != off` (skipped rules are
   omitted) and the `--include-info` flag.
3. Render per the format selected.
4. Write to stdout or to the target file (with optional
   inline-marker splicing).

Exit codes:
- `0` on success.
- `2` on config error (same as `check`).
- `3` if `--inline` is requested but the file isn't
  writable.

## False-positive surface

This isn't a check; there's no FP rate per se. The risks
are:
- **Generated section diverges from human-edited
  AGENTS.md prose.** Mitigation: the markers wall off
  the generated region; the user owns everything outside.
- **Bidirectional sync** is tempting (parse AGENTS.md,
  reconcile with rules) but cuts wrong — humans edit
  prose, the rule set is the source of truth. Out of
  scope for v0.7.
- **Markdown rendering quirks** — a rule message
  containing `` ` `` or `*` or `_` could render
  incorrectly when wrapped in a bold rule id. Mitigation:
  HTML-escape the message body; do NOT escape rule ids
  (they're known-safe kebab-case).

## Implementation notes

**Crate location:** new module
`crates/alint/src/export_agents_md/`. The actual
generation logic could live in `crates/alint-output/`
(it's a formatter that consumes the rule set just like
`Format::Markdown` consumes a `Report`), but the
subcommand orchestration belongs in the CLI crate.

**Reuse:**
- Rule loading via the existing `alint_dsl::Loader`
  pipeline.
- Inline-marker splicing as a small standalone
  helper in `alint::export_agents_md::splice_markers`
  (it's a 30-line two-pointer scan; not worth a
  separate crate).

**Sketch:**

```rust
pub fn run(opts: ExportAgentsMdOptions) -> Result<i32> {
    let cfg = load_config(&opts.config)?;
    let directives: Vec<Directive> = cfg.rules.iter()
        .filter(|r| r.level != Level::Off)
        .filter(|r| opts.include_info || r.level != Level::Info)
        .map(Directive::from_rule)
        .collect();

    let body = match opts.format {
        Format::Markdown => render_markdown(&directives, &opts),
        Format::Json     => render_json(&directives),
    };

    match (&opts.output, opts.inline) {
        (None, false)            => { stdout().write_all(body.as_bytes())?; Ok(0) }
        (Some(path), false)      => { fs::write(path, body)?; Ok(0) }
        (Some(path), true)       => splice_markers(path, &body),
        (None, true)             => bail!("--inline requires --output"),
    }
}
```

**Test surface:**

- Markdown rendering for a canonical rule set
  (snapshot test).
- JSON rendering byte-equivalent for the same input.
- Severity grouping + sort stability across runs.
- Inline-marker splicing:
  - file with existing markers → replaces between
  - file without markers → appends with markers + stderr
    warning
  - file with malformed markers (start without end) →
    error
  - file with multiple marker pairs → error (ambiguous)
- `--include-info` toggles info section.
- Policy URLs render as markdown links when present.

**Complexity estimate:** ~3 days. Mostly formatting work
+ careful inline-splicing logic.

## Tests

- Snapshot tests of the generated markdown / json for
  representative rule sets (oss-baseline, agent-hygiene,
  the dogfood `.alint.yml` from this repo).
- Splice-marker behaviour (4 cases above).
- HTML-escape correctness in messages.
- Stable sort across runs.
- Empty rule set → minimal valid output (no sections
  if no rules at the displayed severity).

## Open questions

1. **Round-trip identity** — running `export-agents-md
   --inline` twice should produce a no-op (same file
   bytes). Plan: trim trailing whitespace, normalise
   line endings to `\n` regardless of host OS, write
   files with a final newline.

2. **What if the user has hand-edited the generated
   section** between runs? — `--inline` overwrites
   silently. Document that the section between
   markers is alint-managed; manual edits there will
   be lost. Consider a `--check` mode that compares
   current file to the would-be-generated content
   and exits non-zero on drift (useful for CI gating
   "AGENTS.md is in sync"). Defer to v0.7.x point
   release.

3. **Multiple agent-context files** — should the
   command write to AGENTS.md, CLAUDE.md, AND
   .cursorrules in one invocation? Plan: no — pass
   `--inline <file>` once per file you want updated.
   Most teams symlink them anyway.

4. **Severity-to-section mapping** — should `error`
   rules render as "MUST" directives and `warning`
   as "SHOULD" (RFC 2119 style)? Lean no for v0.7 —
   the rule's `message` is already the directive
   text and rewording adds maintenance overhead.
   Document the convention so users know they can
   write messages in directive voice.

5. **Bundled rulesets in the export** — when a config
   extends `oss-baseline@v1`, all 11 of its rules
   show up in the export. That can balloon the
   generated section. Options:
   - Group by ruleset: "From `oss-baseline@v1`: …
     (11 rules)". Use a `<details>` block to make
     each group collapsible.
   - Show only top-level rules; collapse extended
     rules into `Inherits: oss-baseline@v1` line.

   Lean: collapse via `<details>` per ruleset by
   default; flag `--expand-extended` to inline.
   Decide before implementation; the answer affects
   the rendering structure.
