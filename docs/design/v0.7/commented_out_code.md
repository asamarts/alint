# `commented_out_code` rule kind

Status: Design draft.

## Problem

Coding agents (and humans, less often) leave commented-out
code blocks behind during refactors and exploration:

```ts
function applyDiscount(price, code) {
  // const oldRate = lookupOldRate(code);
  // if (oldRate > 0.5) return price * 0.5;
  // log("legacy code path:", oldRate);
  return price * lookupRate(code);
}
```

These blocks carry no runtime value, accumulate over time,
and tell future readers "the author thought this might
matter again" — usually wrong. They're especially common in
agent-driven workflows because LLM agents tend to comment
rather than delete when iterating.

Existing alint primitives don't catch this:
- `file_content_forbidden` could ban `// foo` lines, but
  any prose comment trips it too.
- `agent-no-affirmation-prose` and the model-TODO regex
  catch specific phrasings, not the general pattern.

The v0.6 field test (`PROPOSAL-AGENTS.md` §2 item 8 / §3
"comment cruft") flagged this as a class alint should
address but couldn't with existing rule kinds.

## Schema

```yaml
- id: no-commented-code
  kind: commented_out_code
  paths:
    include: ["src/**/*.{ts,tsx,js,jsx,rs,py,go,java,c,cpp}"]
    exclude:
      - "**/*test*/**"
      - "**/fixtures/**"
  language: auto              # auto | rust | typescript | python | … (optional, default auto)
  min_lines: 3                # how many consecutive comment lines to flag (default 3)
  threshold: 0.5              # token-density floor — 0.0 to 1.0 (default 0.5)
  level: warning
  message: >-
    Block of commented-out code; remove it or convert to a
    runtime-checked branch.
```

Field semantics:

- `language` — picks the comment-marker set. `auto` infers
  from filename extension (`.rs` → `//` + `/* */`; `.py` →
  `#`). Explicit setting overrides the auto-detection (useful
  for embedded DSLs like Liquid or Mustache).
- `min_lines` — block size threshold. A 1-line comment is
  almost always prose; 3+ consecutive comment lines
  starts looking like dead code. Default 3.
- `threshold` — token-density score (see "Heuristic" below).
  Default 0.5 means "at least half the non-whitespace
  characters in the block look code-shaped." Tunable per
  language because syntax-heavy languages (Rust, C++) need a
  higher floor than syntax-light ones (Python).

## Semantics

Walks each file in scope. For each block of `min_lines+`
consecutive comment lines, computes a token-density score.
If the score ≥ `threshold`, emit one violation pointing at
the first line of the block.

Comment markers per language:
- `//` and `/* … */` — rust, ts, js, go, java, c, c++,
  swift, kotlin, scala, dart
- `#` — python, ruby, shell, yaml, toml
- `--` — sql, lua, haskell
- `;` — lisp, scheme
- `<!-- … -->` — html, markdown (probably out of scope —
  too prose-heavy to lint usefully)

### Heuristic — what counts as "code-shaped"

Token-density score for a comment block is:

```
density = (
    count(identifier-like tokens)
  + 2 × count(operators / brackets / semicolons)
) / non-whitespace-character-count
```

Code-shaped tokens (positive signal):
- Identifier-like: `[A-Za-z_][A-Za-z0-9_]*` longer than 2 chars
- Operators / structural: `( ) { } [ ] ; = => <- -> + - * / & | ^ ! < > ?`
- Numeric literals
- Quoted strings (`"…"`, `'…'`, `` `…` ``)

Prose-shaped tokens (negative or neutral):
- Single-letter words (`a`, `I`, `e`)
- Words ending in `,` `.` `!` `?` followed by a space
- Common English glue: `the`, `a`, `is`, `to`, `for`, …
  (small fixed set, not a stopword corpus)

The 2× weight on operators reflects that `{}` / `;` /
arrows are stronger code signals than identifiers (which
also appear in technical prose).

A 5-line `//` comment in `.rs` containing
`let x = compute(y); // already-known-good path` would
score above 0.5 because of the `=`, `;`, `(`, `)` tokens.
A 5-line `#` doc comment in `.py` saying "This module
parses RFC 9535 JSONPath..." scores well below because
the tokens are mostly alphabetic words.

## False-positive surface

The pattern that's hardest to disambiguate:
- **License headers** — multi-line `//` blocks at file
  start with prose-and-URL content. Mitigation: skip the
  first 30 lines of any file (configurable
  `skip_leading_lines: 30` default).
- **ASCII art / banners** — sometimes commented blocks
  contain box-drawing characters and structure. Density
  may score high. Mitigation: detect runs of identical
  non-alpha characters (`────`, `====`, `####`) and
  weight them as prose.
- **MDX / docstrings** — `/** … */` Javadoc, Rust `///`
  inner-doc, Python `"""…"""` triple-quoted. **These are
  documentation, not commented-out code.** Mitigation:
  per-language detection of doc-comment markers and skip
  blocks that match them.
- **Disabled-via-comment for legitimate reasons** — code
  the user explicitly disabled with a TODO (`// TODO:
  re-enable after migration`). Mitigation: leave to
  user — that's exactly the case the rule means to catch
  and a TODO comment alone shouldn't suppress the
  finding.
- **Bilingual codebases** with code samples in comments
  (a Python tutorial in a Python file). Mitigation: not
  worth special-casing — recommend `paths.exclude` of
  the affected directory.

Plan: ship with `skip_leading_lines: 30` default + per-
language doc-comment exclusion + the `paths.exclude`
escape hatch. Severity floor is `warning`, not `error`,
because the FP rate is non-trivial.

## Implementation notes

**Crate location:** `crates/alint-rules/src/commented_out_code.rs`.
New file. Implements `Rule` trait. No new external deps.

**Algorithm sketch:**

```rust
pub struct CommentedOutCodeRule {
    spec: RuleSpec,
    scope: Scope,
    options: Options,
    detector: BlockDetector,  // language-specific
}

impl Rule for CommentedOutCodeRule {
    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut out = Vec::new();
        for path in scope_matched_files(ctx, &self.scope) {
            let bytes = read_text_or_skip(&path)?;
            let blocks = self.detector.find_comment_blocks(&bytes);
            for block in blocks {
                if block.lines.len() < self.options.min_lines { continue; }
                if block.start_line < self.options.skip_leading_lines { continue; }
                if self.detector.is_doc_comment(&block) { continue; }
                let density = score_density(&block);
                if density >= self.options.threshold {
                    out.push(violation_at(path, block.start_line, …));
                }
            }
        }
        Ok(out)
    }
}
```

**Test surface:**

`crates/alint-rules/tests/commented_out_code.rs` — table-
driven tests with synthetic blocks for each language:
- positive cases (real commented-out code in rust / ts /
  python)
- negative cases (license headers, doc comments, prose
  comments, ASCII banners)
- edge cases (1-line block, exactly-min_lines block,
  block at file start, block at file end, mixed-marker
  block like `//` followed by `#`)

Plus per-language detector unit tests.

**E2E:** one `crates/alint-e2e/scenarios/check/commented-out-code/`
fixture exercising a small repo with one TP + one FP per
language.

**Complexity estimate:** ~3 days. The algorithm is simple;
the tuning is what eats time. Plan one day for the
detector, one day for the heuristic + per-language
weights, one day for tests + tuning against real repos.

## Tests

- Unit: scoring function on table of canonical blocks.
- Unit: detector correctly identifies block boundaries
  per language.
- Unit: skips license headers when within
  `skip_leading_lines`.
- Unit: skips doc comments (`///`, `/** */`, `"""`).
- Integration: rule fires on a synthetic violation, doesn't
  fire on `cargo install` license headers, doesn't fire on
  rustdoc.
- Field-test: run against the same 12 local + 3 OSS clones
  from the v0.6 field test and tune defaults to keep FP
  rate ≤ 5%.

## Open questions

1. **Markdown / HTML support** — worth shipping in v0.7 or
   defer? Lean defer — markdown comments are rarely
   "commented-out code"; they're hidden prose.
2. **Per-language threshold defaults** — should Rust's
   default be 0.5 same as Python's? Or higher because
   Rust code is more punctuation-dense? Field-test says
   start uniform, tune if needed.
3. **Multi-line strings inside `/* */`** — Rust block
   comments can nest; some languages treat `/*…*/` as
   no-line-info. Plan: count any line entirely contained
   in a block comment as part of the block, even if the
   block was opened on a previous line.
4. **`fix:` support?** — auto-removing commented-out code
   is destructive (the user might intentionally have
   left it). Plan: no fix block; check-only. Add later
   if requested.
