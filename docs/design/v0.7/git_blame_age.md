# `git_blame_age` rule kind

Status: Implemented. See `crates/alint-rules/src/git_blame_age.rs`
+ `BlameCache` in `crates/alint-core/src/git.rs`.

Resolved open questions (from the original design draft, listed
at the bottom of this doc):

1. **Time format input** — days only for v0.7. `max_age_days:
   <integer>` is the only knob. Humanised input (`6mo`, `1y`)
   deferred until asked for.
2. **Bundled-ruleset adoption** — held for a v0.7.x point
   release once the rule has settled in user configs.
3. **"Author time" vs. "commit time"** — author time wins.
   The porcelain parser pulls `author-time` per source line.
4. **`fix:` support** — none. The build path explicitly
   rejects a `fix:` block.
5. **Performance on large monorepos** — `--changed` mode
   pairs naturally; documented in `docs/rules.md` and
   the CHANGELOG. No new optimisations beyond the per-file
   `BlameCache` (memoises both successes and failures).

## Problem

Some matching content matters less when fresh and more
when stale. The canonical case is `TODO` / `FIXME`
markers: a TODO added yesterday is a normal in-flight
note; a TODO that has sat in tree for 18 months is
abandoned debt. The same logic applies to `XXX` / `HACK`
markers, agent-attributed TODOs (`TODO(claude:)` covered
by `agent-no-model-todos`), and `@deprecated` JSDoc tags
that nobody followed up on.

Existing alint primitives can find the TODO marker but
can't ask "how old is this line?" The user has to choose
between:
- `level: warning` on every TODO — too noisy.
- `level: off` — accepts unbounded debt accumulation.

`git_blame_age` closes the gap: same regex match as
`file_content_forbidden`, but only the matched lines that
are *older than N days* fire as violations.

## Schema

```yaml
- id: stale-todos
  kind: git_blame_age
  paths:
    include: ["**/*.{rs,ts,tsx,js,jsx,py,go,java,kt,rb}"]
    exclude:
      - "**/*test*/**"
      - "**/fixtures/**"
  pattern: '\b(TODO|FIXME|XXX|HACK)\b'
  max_age_days: 180
  level: warning
  message: >-
    `{{ctx.match}}` marker has been here for over 180 days.
    Resolve, convert to a tracked issue, or remove.
```

Field semantics:

- `pattern` — regex applied to each line's content. Same
  flavour as `file_content_forbidden`'s pattern.
- `max_age_days` — minimum line age (in days) for a
  matching line to fire as a violation. Lines younger
  than this pass silently. Required field.
- `paths` — same scope semantics as other content rules.

`{{ctx.match}}` in messages — new placeholder containing
the captured marker text (`TODO`, `FIXME`, …). Falls back
to the full match when no capture group is present.
Optional, but useful for the message to be specific.

## Semantics

For each file in scope, run `git blame --line-porcelain
<file>` once. Parse out the per-line author-time. For
each line whose content matches `pattern` AND whose
author-time is older than `now - max_age_days`, emit a
violation at that line.

Outside a git repo (no `.git` parent, or the file is
untracked): the rule silently no-ops. Same convention as
`git_no_denied_paths` / `git_commit_message`. This means
fresh checkouts where files exist but aren't yet
committed don't trigger spurious violations.

`git blame` is invoked via the existing
`alint-core::git` shell-out helper. Cache the parsed
blame output per file so repeated rules over the same
file don't re-run blame. Cache lives in `Context` /
engine state, not a global.

## False-positive surface

- **Re-formatted lines reset the blame age.** `cargo fmt`
  / `prettier` rewrites every touched line, which in git
  blame attributes the line to the formatting commit,
  not the original author. A 5-year-old TODO passing
  through a recent format pass looks young.

  Mitigation: document this explicitly in the rule's
  message. Note that ignore-revs files
  (`.git-blame-ignore-revs`) are honoured by `git
  blame` automatically when present, so teams that
  maintain one for their formatter sweeps get the
  correct behaviour for free.

- **Imported / vendored code** carries the import
  commit's timestamp, which makes everything look
  young. Mitigation: standard `paths.exclude` patterns
  for `vendor/`, `third_party/`, etc.

- **Squash-merged PRs** collapse to one commit, so the
  squash date wins over the actual edit date.
  Mitigation: none — accept that the rule's age
  estimate is bounded by the project's git history
  granularity.

- **Generated files** show the regen timestamp.
  Mitigation: standard exclude patterns.

## Implementation notes

**Crate location:** `crates/alint-rules/src/git_blame_age.rs`.
New file. Implements `Rule` trait.

**Dependencies:** none new. `alint-core::git` already
shells out to `git`; we add a `blame_lines(path)` helper
that returns `Vec<BlameLine { line: usize, author_time:
SystemTime, content: String }>`.

**Engine integration:** the rule needs the same
git-availability gate as `git_no_denied_paths`.
`wants_git_tracked()` already exists; add
`wants_git_blame()` mirror so the engine can short-
circuit when no rule needs it (avoid `git --version`
probing on every run).

**Sketch:**

```rust
pub struct GitBlameAgeRule {
    spec: RuleSpec,
    scope: Scope,
    pattern: Regex,
    max_age: Duration,
}

impl Rule for GitBlameAgeRule {
    fn wants_git_blame(&self) -> bool { true }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let Some(blame_cache) = ctx.git_blame else {
            return Ok(Vec::new());  // silent no-op outside git
        };
        let now = SystemTime::now();
        let mut out = Vec::new();
        for path in scope_matched_files(ctx, &self.scope) {
            let blame = blame_cache.get_or_compute(&path)?;
            for line in &blame {
                if !self.pattern.is_match(&line.content) { continue; }
                let age = now.duration_since(line.author_time).unwrap_or_default();
                if age > self.max_age {
                    out.push(violation_at(path, line.line_number, …));
                }
            }
        }
        Ok(out)
    }
}
```

**Test surface:**

- Unit: blame parser correctly handles the porcelain
  format (real-world output samples).
- Unit: rule fires only on lines older than threshold.
- Unit: silent no-op outside a git repo.
- Integration: synthetic git history with two commits a
  year apart; pattern matches in both; only the older
  fires.
- Integration: respect `.git-blame-ignore-revs` when
  present (git handles this; we just need a fixture
  that exercises the case to make sure we're not
  bypassing it).

**E2E:** one fixture under
`crates/alint-e2e/scenarios/check/git/git_blame_age_*`
that stands up a multi-commit git repo via the
`alint-testkit::Given` git block.

**Complexity estimate:** ~2 days. Most of the work is
parsing porcelain blame output reliably and threading
the cache through `Context`. The rule logic itself is
small.

## Tests

- Parser correctness on porcelain-format git blame
  output.
- Rule fires on synthetic history (commits at known
  dates, threshold either side).
- Silent no-op outside git, on untracked files.
- `.git-blame-ignore-revs` correctly skipped in age
  calculation (test fixture: file edited by a "format
  sweep" commit listed in ignore-revs; original commit
  age wins).
- Cache: rule shouldn't re-blame the same file when
  multiple rules use it.

## Open questions

1. **Time format input** — should `max_age_days` be the
   only knob, or should we accept `max_age: 6mo` /
   `max_age: 1y`? Lean simple — days only for v0.7,
   add humanized format if asked. (`humantime` crate is
   already a transitive dep.)

2. **Bundled-ruleset adoption** — should v0.7 update
   `agent-hygiene@v1` to include a `stale-model-todos`
   rule using `git_blame_age`? Probably yes, but as a
   v0.7.x point release once the rule kind has settled.

3. **"Author time" vs. "commit time"** — git tracks
   both. Author time is when the change was originally
   authored; commit time is when it landed in the
   current branch. Author time is what users intuitively
   mean by "age." Plan: use author time. Document the
   choice.

4. **`fix:` support?** — automatically removing TODO
   markers is destructive; pinning the line content as
   "do nothing" doesn't help. Plan: no fix block;
   check-only.

5. **Performance on large monorepos** — `git blame` per
   file is O(file_size × commits_touching_file). On a
   100k-file monorepo, blaming every file in scope is
   prohibitive. Mitigation: the `--changed` flag (v0.5.0)
   already restricts the file set in incremental mode;
   this rule benefits naturally. For full-tree runs,
   document the cost and recommend pairing with
   `--changed` in CI.
