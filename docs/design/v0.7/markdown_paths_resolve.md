# `markdown_paths_resolve` rule kind

Status: Implemented in v0.7.1 (`crates/alint-rules/src/markdown_paths_resolve.rs`).

## Problem

Markdown files (READMEs, AGENTS.md / CLAUDE.md, runbooks,
ADRs, contributor guides) reference workspace paths in
backticks: ``See `src/api/users.ts` for the route handler.``
Those paths drift as the codebase evolves. The
`agent-context-no-stale-paths` rule shipped in v0.6 surfaces
*candidate* drift via a regex but can't tell which paths
actually broke. The agent then has to verify each one
manually — exactly the kind of mechanical work alint should
do for them.

`markdown_paths_resolve` does the actual existence check:
walk the matched markdown files, find backticked tokens that
look like workspace-relative paths, and emit a violation for
each one that doesn't resolve to a real file or directory.

## Schema

```yaml
- id: agents-md-paths-resolve
  kind: markdown_paths_resolve
  paths:
    - AGENTS.md
    - CLAUDE.md
    - .cursorrules
    - GEMINI.md
    - "docs/**/*.md"
  prefixes:                  # which path-shapes to validate
    - src/
    - crates/
    - packages/
    - apps/
    - services/
    - docs/
    - bench/
    - schemas/
    - "./"                  # explicit relative paths
  ignore_template_vars: true # skip backtick'd paths containing `{{` / `${` / `<…>`
  level: warning
  message: >-
    Backticked path `{{ctx.match}}` in {{ctx.file}} doesn't
    resolve. Update the reference or remove it — context
    files commonly outlive the code they describe.
```

Field semantics:

- `prefixes` — required. Whitelist of path-shape prefixes
  to validate. A backticked token must start with one of
  these to be considered a path candidate. This avoids
  false positives on every backticked word: `` `npm` ``
  isn't a path; `` `src/foo.ts` `` is. **No defaults** —
  the rule requires the user to declare which paths to
  validate, both because every project's layout differs
  and because letting the rule guess inflates the FP
  rate.
- `ignore_template_vars` — when true (default), skip
  any backticked token containing `{{ … }}` (Mustache /
  Jinja / alint-vars), `${ … }` (shell / JS template),
  or `<…>` (placeholder convention). These are template
  values, not real paths.
- `paths` — same scope semantics as other rules.

`{{ctx.match}}` and `{{ctx.file}}` — message placeholders
for the offending path string and the markdown file it
appeared in.

## Semantics

For each markdown file in scope:

1. **Strip code blocks first.** Anything between ` ``` `
   fences (with or without a language tag) and any line
   indented by 4+ spaces is excluded. Code samples
   demonstrate paths; they're not claims about the
   current tree.
2. **Strip inline code that looks like code samples**
   following ``` ` ``` . Heuristic: if the backticked
   token contains spaces or matches a non-path shape
   (`function(args)`, `cmd --flag`), skip.
3. **Find candidate paths.** Backticked tokens that
   start with one of the configured `prefixes`.
4. **Apply `ignore_template_vars` filtering.**
5. **Resolve each candidate.** Path is interpreted
   relative to the repository root (the `Context::root`
   alint engine already provides). Existence check uses
   the file index already built by the walker — no
   extra I/O per match.
6. **Emit a violation per unresolved path** at the
   line / column of the opening backtick. Multiple
   misses on the same line each produce their own
   violation.

Resolution rules:
- Trailing punctuation (`.`, `,`, `:`, `;`, `?`, `!`)
  inside the backticks is stripped before lookup, since
  English prose often ends a sentence with a backticked
  path: `` see `src/foo.ts`. ``
- Trailing slashes are tolerated (treat `src/foo/` and
  `src/foo` as the same lookup; both succeed if the
  directory exists).
- Glob characters (`*`, `?`) inside the path mark it as
  a glob pattern; resolve via the existing globset over
  the file index. Pass if at least one file matches.

## False-positive surface

- **Code samples in inline backticks** —
  `` Example: `src/foo.ts` was where utils used to
  live. `` referencing a deleted file deliberately, as
  a historical narrative. Mitigation: severity `info`
  / `warning` only. Users add `paths.exclude` for
  retrospective narrative docs (CHANGELOG entries,
  ADRs).
- **Templated path with literal-looking tokens** —
  `` `src/{user_id}.json` `` where `{user_id}` is a
  runtime placeholder. Mitigation: angle-bracket
  convention `<…>` covers many cases;
  `ignore_template_vars` defaults true.
- **Case sensitivity drift** — a markdown file says
  `` `src/UserAPI.ts` `` but the actual file is
  `src/userApi.ts`. On Linux the resolution fails; on
  macOS-default it succeeds. Mitigation: always
  case-sensitive; the file index already builds that
  way. Document the choice — case mismatch *is* a real
  drift even if some checkouts hide it.
- **Backticked file extensions** — `` `.tsx` `` /
  `` `.config.js` `` are extensions, not paths. The
  required `prefixes` field eliminates these by
  construction.

## Implementation notes

**Crate location:**
`crates/alint-rules/src/markdown_paths_resolve.rs`. New
file.

**Markdown parsing:** use a minimal hand-rolled scanner
rather than pulling in `pulldown-cmark` for one rule.
We need to identify code-fence boundaries and inline
backticks, both of which are well-defined CommonMark
constructs we can match without a full parser. A 50-line
state machine handles it (saw the same approach in
xtask docs-export's leading-comment parser). If we add
more markdown-aware rules in v0.8+, revisit and pull in
`pulldown-cmark` then.

**Lookup performance:** the rule depends on the file
index for existence checks. Each lookup is O(1) via the
existing `FileIndex::contains_path` (already used by
`pair` and similar). No re-walks per match.

**Sketch:**

```rust
pub struct MarkdownPathsResolveRule {
    spec: RuleSpec,
    scope: Scope,
    prefixes: Vec<String>,
    ignore_template_vars: bool,
}

impl Rule for MarkdownPathsResolveRule {
    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut out = Vec::new();
        for path in scope_matched_files(ctx, &self.scope) {
            let bytes = read_text_or_skip(&path)?;
            for cand in scan_markdown_paths(&bytes, &self.prefixes) {
                if self.ignore_template_vars && has_template_vars(&cand.token) { continue; }
                let lookup = cand.token.trim_end_matches(|c: char| ".,:;?!".contains(c))
                    .trim_end_matches('/');
                if !ctx.index.contains_path(Path::new(lookup))
                    && !ctx.index.glob_matches_any(Path::new(lookup)) {
                    out.push(violation_at(path, cand.line, cand.column, &cand.token));
                }
            }
        }
        Ok(out)
    }
}
```

**Test surface:**

- Unit: scanner correctly skips fenced code blocks (` ```yaml `,
  ` ```rust `, generic ` ``` `).
- Unit: scanner correctly skips 4-space-indented blocks.
- Unit: scanner finds inline backticked candidates only
  matching `prefixes`.
- Unit: trailing punctuation / trailing slash handling.
- Unit: glob patterns resolve correctly.
- Unit: template-var detection (`{{ }}`, `${}`, `<>`).
- Integration: synthetic markdown file with 5 mixed paths
  (3 valid, 2 broken); rule fires twice with correct
  line/column.

**E2E:** one fixture under
`crates/alint-e2e/scenarios/check/markdown-paths-resolve/`
exercising AGENTS.md with valid + broken backticked paths.

**Complexity estimate:** ~2 days. Markdown scanning
takes most of it; the existence check is trivial against
the file index.

## Tests

- Code-fence boundaries (must skip ` ``` ` blocks
  including with language tags).
- Inline backtick boundaries (don't trip on
  ``…``double-backticked``…``).
- Prefix filtering (only validate paths starting with
  declared prefixes).
- Resolution honours trailing punctuation + trailing
  slash.
- Glob candidates resolve via the file index.
- Template variable detection.
- Case-sensitive resolution on Linux/macOS/Windows.

## Open questions

1. **Default `prefixes`?** — we could ship a sensible
   default like `[src/, crates/, packages/, docs/]`. Lean
   no-default (require the user to declare). Rationale:
   every project layout differs, and a missing prefix is
   a silent miss while a wrong-default trips false
   positives. Make the user think once.
2. **Section anchors and URL-style references** —
   `` `src/foo.ts:42` `` (with a line number) or
   `` `src/foo.ts#L42` `` (GitHub-style). Plan: strip
   `:N` and `#L…` suffixes before lookup; treat them as
   anchor metadata, not part of the path. Document.
3. **Markdown link targets** (`[text](path/to/foo.md)`)
   — resolve those too? They're a different syntactic
   shape and a separate FP surface (links to external
   URLs, anchors). Lean defer to a sibling rule
   `markdown_links_resolve` in v0.8 if there's demand.
4. **`fix:` support?** — auto-fixing a stale path
   requires guessing the new location, which is unsafe.
   Check-only.
