# Per-file-rule dispatch flip

Status: Design draft.

## Problem

The engine today runs every rule under a rule-major outer
loop (`engine.rs:159`):

```rust
let results: Vec<RuleResult> = self.entries
    .par_iter()
    .filter_map(|entry| {
        ...
        run_entry(entry, ctx, &when_env, &fact_values)
    })
    .collect();
```

Each rule iterates `ctx.index.files()`, filters by its scope,
and — for content rules — calls `std::fs::read(&full)` per
matching file. When N content rules share the same scope
(e.g. `oss-baseline@v1`'s `no_trailing_whitespace` +
`final_newline` + `line_endings` + `line_max_width` all gated
on `**/*.md`), the same file is read N times. At the v0.8.4
`single_file_rules.rs` 10k-tree baseline this is ~85% of
total `engine.run` wall time on warm-cache (read syscall +
allocator); the rule-evaluation work is itself fast.

The fix is **flip the loop**: file-major outer, rule-major
inner. For per-file rules, walk the file index once; per file,
read once; per file, dispatch every per-file rule whose scope
matches. Cross-file rules — those that override
`requires_full_index() = true` (`pair`, `for_each_dir`,
`for_each_file`, `every_matching_has`, `unique_by`,
`dir_contains`, `dir_only_contains`, `file_exists`,
`file_absent`, `dir_exists`, `dir_absent`) — keep today's
rule-major path because their verdicts span the whole tree
by definition.

Coalesces the read cost across N rules sharing one file from
N reads to 1.

## Surface area

A new sub-trait next to `Rule`:

```rust
// crates/alint-core/src/rule.rs

pub trait PerFileRule: Send + Sync + std::fmt::Debug {
    /// The rule's scope. The engine checks scope.matches(path)
    /// before calling evaluate_file; a rule that always returns
    /// Scope::match_all is in scope for every file.
    fn path_scope(&self) -> &crate::scope::Scope;

    /// Evaluate one file. The bytes slice is the engine's
    /// already-read file content — rules MUST NOT call fs::read
    /// themselves. Per-file rules that need only a prefix /
    /// suffix should declare it via the optional
    /// max_bytes_needed() hint and consume bytes[..max] /
    /// bytes[bytes.len()-max..]; the engine still hands them the
    /// full slice but a future revision could honour the hint.
    fn evaluate_file(
        &self,
        ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>>;

    /// Optional lower bound on bytes the rule needs to evaluate.
    /// Default 0 means "I need the whole file." Used as a hint;
    /// the engine isn't required to honour it in v0.9.3 (the
    /// engine's I/O strategy is "read once, fully" for now).
    fn max_bytes_needed(&self) -> Option<usize> {
        None
    }
}

pub trait Rule: Send + Sync + std::fmt::Debug {
    // ... existing methods ...

    /// Opt into the file-major dispatch path. Per-file rules
    /// override this to return Some(self); cross-file rules
    /// (and any rule with requires_full_index == true) leave it
    /// as None and keep evaluating under the rule-major loop.
    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        None
    }
}
```

The `Rule::evaluate` method **stays** — it's still the entry
point for cross-file rules and remains the fallback for rules
that don't migrate. A per-file rule's `evaluate` becomes a thin
wrapper that the engine calls only in `--changed` mode for
fallthrough or in tests; in the hot path, `as_per_file()`
short-circuits to `evaluate_file`.

The engine partitions entries at run-time:

```rust
// engine.rs

let (per_file_entries, cross_file_entries): (Vec<_>, Vec<_>) =
    self.entries.iter().partition(|e| e.rule.as_per_file().is_some());
```

The cross-file partition runs through today's
`par_iter().filter_map(run_entry)` unchanged. The per-file
partition runs through:

```rust
let per_file_results: Vec<RuleResult> = ctx.index.files()
    .par_bridge()
    .map(|entry| {
        // 1. Decide which per-file rules apply to this file.
        let applicable: Vec<&dyn PerFileRule> = per_file_entries.iter()
            .filter(|e| {
                let rule = e.rule.as_per_file().unwrap();
                rule.path_scope().matches(&entry.path)
            })
            .map(|e| e.rule.as_per_file().unwrap())
            .collect();
        if applicable.is_empty() { return Vec::new(); }

        // 2. Read once.
        let abs = ctx.root.join(&entry.path);
        let Ok(bytes) = std::fs::read(&abs) else { return Vec::new(); };

        // 3. Dispatch.
        applicable.into_iter()
            .map(|rule| evaluate_per_file_with_when(rule, ctx, &entry.path, &bytes))
            .collect()
    })
    .flatten()
    .collect();
```

The aggregation step then merges per-file results back into
per-rule `RuleResult`s (so the report still groups violations
by rule, not by file — output formatters expect per-rule
grouping).

## Semantics

Rule semantics are unchanged. A `no_trailing_whitespace` rule
that fires on line 12 of `src/foo.rs` at v0.8.2 fires on line 12
of `src/foo.rs` at v0.9.3 with the same message text, the same
column, the same level. The dispatch flip is invisible at the
violation level.

Three engine-level mechanics change:

1. **Read coalescing.** Files are read once per evaluation pass
   (or zero times if no per-file rule's scope matches the
   file). The matching count is cached during the per-file
   loop's filter step.
2. **Per-rule violation grouping in the report.** The file-
   major loop produces violations interleaved across rules;
   the engine post-processes them into per-rule
   `RuleResult`s (sort by `rule_id`, `dedup_consecutive`-style
   grouping, or accumulate via a `HashMap<Arc<str>, Vec<Violation>>`).
3. **`--changed` mode interaction.** Per-file rules already
   run against the changed-only filtered context (`engine.rs::pick_ctx`).
   That logic moves to the file-major loop: filter
   `ctx.index.files()` to the changed set before iterating.
   Behaviour identical.

## Behavioural invariants

- **Violation output is byte-identical** to v0.8.2 (and to
  v0.9.2) for any input tree. The per-rule output ordering must
  match the rule-major ordering — this is what the report-
  aggregation step guarantees.
- **`Rule` trait remains add-only.** Adding `as_per_file()`
  with a default `None` preserves backwards compatibility for
  any rule (built-in or out-of-tree) that doesn't migrate.
  Pre-v0.9.3 rules simply stay on the rule-major path. WASM
  plugins (v0.11) inherit this — the trait surface they bind
  against is the same.
- **`when:` evaluation order unchanged.** Per-file rules that
  carry a `when:` expression have the expression evaluated
  once per evaluation pass (not once per file), since `when`
  references facts that are constant across files. The
  rule-major path already does this; the file-major path
  pre-filters the per-file rule list before entering the
  file loop.
- **`fix` subcommand stays sequential.** Fixers race the
  filesystem; today's `Engine::fix` uses a sequential `for`
  loop deliberately. v0.9.3 keeps that — fix mode still uses
  rule-major dispatch with per-file reads, because the
  parallelism win for `fix` is dominated by sequential
  filesystem mutation.

## False-positive surface

(Bugs the new shape can introduce.)

- **Aggregation drops violations.** The trickiest part of the
  flip: producing the same `RuleResult` shape as before. A
  rule that produced 3 violations across 3 files at v0.8.2
  now produces them across 3 file-major iterations; the
  aggregation step has to find them all and emit one
  `RuleResult { rule_id: ..., violations: [...] }` with all 3.
  Mitigation: a unit test that runs the engine with both
  rule-major and file-major paths against the same fixture
  and asserts identical `Report` output (down to violation
  ordering inside each `RuleResult`). The `cross_formatter.rs`
  invariant test is also a backstop.
- **A migrated rule's `evaluate_file` returns more violations
  than its `evaluate` did.** If the migration accidentally
  drops the rule's own scope check (because the engine pre-
  filtered), no new false positives — the engine's filter is
  the same `Scope::matches` the rule used. But: if the rule
  did *additional* filtering inside `evaluate` (e.g.
  `git_tracked_only` checks), that filtering must move to
  `evaluate_file`. Mitigation: per-rule migration PR includes
  a fixture that exercises the rule's auxiliary filters.
- **Rule with `wants_git_tracked` or `wants_git_blame`
  doesn't migrate cleanly.** These rules consult
  `ctx.git_tracked` / `ctx.git_blame` per file. Migration is
  mechanical (the new trait carries the same `&Context`), but
  must be confirmed on the v0.5.9 git rules
  (`git_no_denied_paths`, `git_commit_message`,
  `git_blame_age`). Two of those — `git_no_denied_paths` and
  `git_commit_message` — operate at the repo level (one
  evaluation per run, not per file); they stay on the rule-
  major path. `git_blame_age` is per-file and migrates.
- **Empty file index trips a Rayon zero-iter regression.**
  `par_bridge` over an empty iterator should be free; if
  Rayon has a fixed-overhead startup cost, the file-major
  loop pays it even for empty trees. Test against
  `Engine::new(...).run(root, &empty_index)` — should be
  ~no-op.
- **`as_per_file()` returns `Some` but the rule's
  `requires_full_index()` is also `true`.** Caller bug; the
  engine resolves it by partitioning on `as_per_file().is_some()`
  *after* checking `requires_full_index()`. A rule that says
  both is treated as cross-file (full-index wins). Document
  the precedence; add a debug-assertion in dev builds that
  catches the contradiction.

## Implementation notes

**Crate locations:**

- `crates/alint-core/src/rule.rs` — new `PerFileRule` trait;
  `Rule::as_per_file()` default method.
- `crates/alint-core/src/engine.rs` — partition logic; new
  file-major loop; aggregation into per-rule `RuleResult`s.
- `crates/alint-rules/src/` — per-rule migration. Each per-
  file rule grows a `PerFileRule` impl alongside its existing
  `Rule` impl; `as_per_file()` returns `Some(self)`. The old
  `evaluate` body becomes a thin wrapper that walks the
  index, reads each file, and calls `evaluate_file` — only
  invoked from rule-major fallback paths (the `fix` flow,
  some test harnesses).

**Migrating rules — the candidate list:**

Per-file rules that read file content (and benefit from read
coalescing):

```
file_content_matches      file_header
file_content_forbidden    file_footer
file_max_lines            file_min_lines
file_max_size             file_min_size
file_starts_with          file_ends_with
file_hash                 file_is_ascii
file_is_text              file_shebang
file_header               commented_out_code
markdown_paths_resolve    no_bom
no_bidi_controls          no_zero_width_chars
no_merge_conflict_markers no_trailing_whitespace
final_newline             line_endings
line_max_width            indent_style
max_consecutive_blank_lines  json_path_*
yaml_path_*               toml_path_*
json_schema_passes        executable_has_shebang
shebang_has_executable    file_max_size
```

Per-file rules that do **not** read content (and migrate
trivially because the read step is a no-op):

```
filename_case  filename_regex  no_case_conflicts
no_illegal_windows_names  no_symlinks  executable_bit
```

Cross-file rules that **stay** on the rule-major path:

```
pair  for_each_dir  for_each_file  every_matching_has
unique_by  dir_contains  dir_only_contains
file_exists  file_absent  dir_exists  dir_absent
git_no_denied_paths  git_commit_message  command
```

(`command` is per-file by `paths:` scope but invokes an
external binary per file — same shape as a content rule, but
the read isn't ours; we still partition it into the per-file
group, and the read step is the binary invocation.)

**Migration shape per rule** (`no_trailing_whitespace` as the
template):

```rust
impl PerFileRule for NoTrailingWhitespaceRule {
    fn path_scope(&self) -> &Scope { &self.scope }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        if let Some(line_no) = first_offending_line(bytes) {
            let msg = self.message.clone()
                .map(Cow::Owned)
                .unwrap_or_else(|| Cow::Owned(
                    format!("trailing whitespace on line {line_no}")
                ));
            Ok(vec![
                Violation::new(msg)
                    .with_path(Arc::from(path))
                    .with_location(line_no, 1)
            ])
        } else {
            Ok(Vec::new())
        }
    }
}

impl Rule for NoTrailingWhitespaceRule {
    fn as_per_file(&self) -> Option<&dyn PerFileRule> { Some(self) }
    // ... existing evaluate / fixer / id / level / policy_url ...
}
```

The per-file rule's `evaluate_file` is exactly the inner body
of the rule's old `evaluate` — minus the file walking and the
`fs::read`. The rule's `evaluate` becomes a wrapper:

```rust
fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
    let mut out = Vec::new();
    for entry in ctx.index.files() {
        if !self.scope.matches(&entry.path) { continue; }
        let abs = ctx.root.join(&entry.path);
        let Ok(bytes) = std::fs::read(&abs) else { continue; };
        out.extend(self.evaluate_file(ctx, &entry.path, &bytes)?);
    }
    Ok(out)
}
```

This wrapper is what the `fix` subcommand runs (sequential)
and what fallback / test harnesses run. The hot path skips it
entirely.

**Aggregation step** (engine.rs):

```rust
// Per-file results come back as Vec<(Arc<str>, Violation)>;
// fold into per-rule RuleResults preserving rule order.
let mut by_rule: HashMap<Arc<str>, Vec<Violation>> = HashMap::new();
for (rule_id, violation) in per_file_results {
    by_rule.entry(rule_id).or_default().push(violation);
}
// Re-emit in rule-registration order.
for entry in &per_file_entries {
    let id = entry.rule.id_arc();  // returns Arc<str>
    if let Some(violations) = by_rule.remove(&id) {
        results.push(RuleResult {
            rule_id: id,
            level: entry.rule.level(),
            policy_url: entry.rule.policy_url().map(Arc::from),
            violations,
            is_fixable: entry.rule.fixer().is_some(),
        });
    }
    // No entry → rule produced no violations → omit, same as today.
}
```

The `id_arc()` method is a small addition to `Rule` —
`fn id_arc(&self) -> Arc<str>`, defaulted to
`Arc::from(self.id())`. Rules can override to return a
pre-built Arc and amortise the cost. (Coupled to v0.9.2's
`RuleResult::rule_id: Arc<str>` migration.)

**Complexity estimate:** ~5 days. Day 1: new trait + engine
partition + aggregation (~150 lines net new in core). Day 2:
migrate the line-oriented rules (already byte-clean from
v0.9.2 — `evaluate_file` is a refactor of existing code).
Days 3–4: migrate the rest of the per-file rule list.
Day 5: dhat re-baseline + bench-compare + e2e regression
hunt.

## Tests

**New tests in `engine.rs`:**
- `dispatch_flip_produces_same_violations_as_rule_major` —
  build a config with 5 per-file rules sharing a scope; run
  on a fixture; assert `Report` output is identical to a
  version of the engine with `as_per_file()` short-circuited
  to `None` (forcing rule-major dispatch). Identity check
  guards the aggregation step.
- `dispatch_flip_coalesces_reads` — instrument `std::fs::read`
  calls via a wrapper helper; build a config with N rules on
  one file; assert the file is read exactly once. Direct
  guard of the optimisation's value.
- `dispatch_flip_handles_changed_mode` — combine
  `with_changed_paths` + per-file rules; assert the changed-
  set filtering applies (rules whose scope misses the diff
  are skipped) and the file index is filtered before the
  per-file loop.
- `cross_file_rules_unchanged_in_dispatch_flip` — build a
  cross-file rule (`pair`) alongside per-file rules; assert
  the cross-file rule still runs against the full index and
  produces the same violations.
- `empty_index_is_zero_cost` — assert running the engine
  on an empty `FileIndex` returns immediately without
  Rayon-startup cost. Smoke test, not a tight bound.

**Per-rule migration tests:**

For each rule that grows a `PerFileRule` impl, add one unit
test that calls `evaluate_file` directly with synthetic bytes
and asserts the violation output matches a frozen fixture.
Already covered for the line-oriented rules by their existing
unit tests (the test bodies barely change — they call
`first_offending_line` directly today).

**Bench validation per phase:**
- `single_file_rules.rs` — every per-file rule should improve
  at 1k and 10k tree sizes when more than one rule is in
  scope. Configure a multi-rule scenario benchmark to
  capture this directly.
- `cross_file_rules.rs` — should be flat.
- `rule_engine.rs` (v0.7.0 baseline) — significant
  improvement expected if the bench scenario has multiple
  per-file rules. **This is the bench most likely to need a
  threshold bump in the v0.9.3 PR**, but in the *favourable*
  direction (an unexpectedly large improvement that
  breaches the assumed bound on a non-improvement bench).
  bench-compare's `--threshold` is a regression gate, not an
  improvement gate, so improvements pass automatically.

**E2E:** the v0.8.5 e2e suite (`crates/alint-e2e/scenarios/`)
runs unchanged. v0.9.3 should produce byte-identical violation
output, captured exit codes, and stderr / stdout text.

## Open questions

1. **Should `PerFileRule::path_scope` return `&Scope` or
   `Cow<'_, Scope>`?** Today's per-file rules all own a
   `Scope` field; `&Scope` is fine. A future rule that
   computes its scope dynamically (e.g. from facts) would want
   `Cow`. Lean `&Scope` for v0.9.3; revisit when a rule
   actually needs the dynamic case.
2. **Does the engine read the file as `Vec<u8>` or `Arc<[u8]>`?**
   `Vec<u8>` works — the per-file loop owns the read and
   borrows the slice across rule dispatches inside the same
   iteration. `Arc<[u8]>` would let us cache reads across
   evaluation passes (LSP re-eval), but that's a v0.10
   concern. Lean `Vec<u8>`.
3. **Do we need `max_bytes_needed()` in v0.9.3, or is the
   hint deferable?** The hint informs a future "read only what
   the rule needs" optimisation. It costs nothing to add the
   default-`None` method now; adding it later is a trait-
   surface change. Lean add now, ignore the value in v0.9.3,
   wire it up in v0.9.x or v0.10 if dhat shows it matters.
4. **Should `fix` mode also use the file-major path?** The
   fixer-application step already runs sequentially per
   v0.4.x design (filesystem race avoidance). The
   *evaluation* step before fix could run file-major to
   coalesce reads — mild win for `alint fix --dry-run` on
   configs with many overlapping per-file rules. Lean leave
   `fix` on rule-major; the win is minor and the risk of
   subtly different semantics across `check` and `fix` is
   not. Document the choice; revisit if user feedback asks.
5. **How does this interact with `command` rules?** Each
   `command` rule invokes an external binary per matched
   file. Today they share the rule-major path, so a config
   with 3 `command` rules on the same scope spawns 3
   processes per file. Migrating them to per-file dispatch
   doesn't coalesce process spawns (each rule runs its own
   binary), but it *does* let the engine reuse the
   walked-file decision. Lean migrate; the upside is the
   same scope-filter coalescing the rest of the per-file
   rules get. The process-spawn cost is unchanged.
