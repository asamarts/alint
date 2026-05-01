# Memory-footprint pass

Status: **Type pass implemented** in `7bd7bf5` (v0.9.2,
2026-04-30). Per-rule byte-slice scanning + bounded
prefix/suffix reads were originally scoped to the same
release but have moved to v0.9.3 alongside the dispatch
flip — see [`dispatch_flip.md`](./dispatch_flip.md) for
the rule conversions in their new home.

Resolved open questions (from the original design draft):

1. **`Arc<Path>` vs `Arc<PathBuf>` vs `Rc<Path>`** —
   `Arc<Path>` shipped. The construction-time `Arc::from`
   amortises over N violations referencing one file; engine
   parallelism rules out `Rc`.
2. **`ctx.root` Arc-ification** — left as `&Path`. Borrowed
   for the lifetime of `engine.run`; consumers hold `&Path`
   already.
3. **`memchr` for `\n` discovery** — deferred. Postponed to
   v0.9.3 alongside the line-scanning conversions; v0.9.2
   doesn't touch the byte-walk path.
4. **Cow on `policy_url`** — converted to `Arc<str>` in the
   same pass as `rule_id`. `RuleResult::policy_url` is now
   `Option<Arc<str>>` and shares one allocation across all
   violations of one rule.

## What v0.9.2 actually shipped (the type pass)

The Arc/Cow type changes on `FileEntry::path`,
`Violation::path`, `Violation::message`, and
`RuleResult::rule_id` / `policy_url`. Mechanical migration
of all ~70 rule call sites + 8 formatter structs.
Behavioural invariants verified: byte-identical output via
the cross-formatter snapshot test, full workspace test
suite green. Bench numbers: see
`docs/benchmarks/archive/v0.9-development-phases/v0.9.2-memory-pass/README.md`.

## What moved to v0.9.3

Per-rule byte-slice scanning conversions (the 6 line-oriented
rules) and bounded prefix/suffix reads (the 4 first-/last-
bytes-only rules) were originally scoped here. They moved
to v0.9.3 because v0.9.3's per-file dispatch flip hands each
rule a pre-loaded `&[u8]` slice from the engine's single
`fs::read` — doing the byte-slice conversions now in v0.9.2
and re-touching the same evaluate bodies in v0.9.3 was
strictly duplicate work. The 4 bounded-read rules are slightly
different (the engine wouldn't read for them either way) but
were bundled with v0.9.3 to keep the per-rule conversion
diff in one commit. See
[`dispatch_flip.md`](./dispatch_flip.md) for the v0.9.3
home.

## Problem

Two memory-shape problems show up at the v0.8.4 bench
scenarios — distinct enough to merit separate fixes inside the
same v0.9.2 cut.

**Cloned-string churn on the violation hot path.** Every
violation today owns its `path: Option<PathBuf>` via
`.with_path(&entry.path)`, which `PathBuf::clone`s the bytes out
of the `FileIndex`. Same for `RuleResult::rule_id: String`,
which is `entry.rule.id().to_string()` once per rule run. At
100k violations the path clones dominate the engine's
heap allocations. `Violation::message: String` is genuinely
needed for per-match templated messages
(`format!("line {n}: ...")`) but the *non-templated* messages —
the rule's optional `message:` field — clone the same string
into every violation produced by that rule.

**Whole-file reads for line-oriented rules.** Each of the line-
scanners (`no_trailing_whitespace`, `final_newline`,
`line_endings`, `line_max_width`, `max_consecutive_blank_lines`,
`indent_style`) calls `std::fs::read(full)`, then
`std::str::from_utf8(&bytes)`, then `text.split('\n')`. Two of
those passes are the same walk over the same buffer — UTF-8
validation, then `\n` discovery. For rules that care about
specific byte patterns (`b' '`, `b'\t'`, `b'\r'`), the UTF-8
validation pass is dead work. And the rule walks the *full*
file even after finding its first offender.

The dhat profile (run against `single_file_rules.rs` and
`cross_file_rules.rs` at the published 10k / 100k tree sizes)
is the gating activity for picking which conversions actually
matter — the list above is a hypothesis, not a budget.

## Surface area

Two layers change:

**`alint-core` types** (`crates/alint-core/src/`):

- `walker.rs::FileEntry::path: PathBuf` → `Arc<Path>`.
- `rule.rs::Violation::path: Option<PathBuf>` → `Option<Arc<Path>>`.
- `rule.rs::Violation::with_path(impl Into<PathBuf>)` →
  `with_path(impl Into<Arc<Path>>)` (and a convenience
  `with_path_pathbuf(PathBuf)` shim if too many call sites
  fight the migration).
- `rule.rs::RuleResult::rule_id: String` → `Arc<str>`.
- `rule.rs::Violation::message: String` → `Cow<'static, str>`.

The public methods on each type continue to return `&str` /
`&Path`, so downstream consumers (formatters, the JSON Schema,
the SARIF builder) never see the change. `Display` impls and
`serde::Serialize` impls remain byte-identical.

**Per-rule scanners** (`crates/alint-rules/src/`):

- The line-oriented rules listed above migrate from
  `std::fs::read + str::from_utf8 + split('\n')` to
  `std::fs::read + bytes.split(|&b| b == b'\n')`. UTF-8
  validation is skipped where the rule's predicate operates on
  bytes; kept (per-line) where the rule needs character counts
  (`line_max_width`).
- `executable_has_shebang`, `shebang_has_executable`,
  `file_starts_with`, `file_ends_with` — all of these read the
  whole file but only consult the first or last few KB. They
  switch to bounded reads via a new
  `read_prefix(path, n_bytes)` / `read_suffix(path, n_bytes)`
  helper in `alint-rules/src/io.rs`. Helps independently of
  v0.9.3 because the prefix/suffix is all the rule ever needed.

The `BufReader::lines()` shape the v0.9 ROADMAP block calls
out resolves to **byte-slice scanning over a single
`std::fs::read` result**, not a streaming `BufReader`. Reasons:
`BufReader::lines()` heap-allocs per line (`Result<String>`),
which can regress on tiny files; and v0.9.3 hands rules a
pre-loaded `&[u8]` slice from the engine's single read — per-
rule streaming actively conflicts with that contract. Bounded
prefix/suffix reads keep the streaming benefit where it
actually matters.

## Semantics

For each per-rule conversion, the rule produces **byte-
identical** violations to the v0.8.2 implementation: same
message text, same line/column, same path. The point of the
pass is to lower allocation count and peak heap, not change
the verdict.

For each `Arc`-ification:

- `FileEntry::path: Arc<Path>` is built once per walker entry
  (one `Arc::from(PathBuf)` per file). Every rule that does
  `.with_path(&entry.path)` now does an `Arc::clone` (atomic
  refcount bump) instead of a `PathBuf::clone` (allocation +
  byte copy). At 100k violations: ~100k `Arc::clone`s vs
  ~100k allocations. dhat-measurable.
- `RuleResult::rule_id: Arc<str>` is built once per rule
  registration. Every violation of that rule shares the same
  Arc; the rule_id field on every violation is a refcount bump.
- `Violation::message: Cow<'static, str>` keeps templated
  messages on `Cow::Owned(String)` (no change in cost) and
  lets fixed messages live as `Cow::Borrowed("static")` for
  the few rules that don't templatise. Net win is small but
  the type change is a stepping stone for a future
  `'static` rule-message audit.

## Behavioural invariants

- **Byte-identical output.** Every formatter (`json`,
  `human`, `sarif`, `agent`, `markdown`, `junit`, `gitlab`,
  `github`) produces identical bytes for identical input
  trees compared to v0.8.2. The `cross_formatter.rs`
  invariants test in `alint-output/tests/` is the guard.
- **No new allocations on the rule-evaluation hot path.**
  The whole point: dhat output for v0.9.2 should show fewer
  allocations than v0.8.2, not more.
- **No regression on the v0.7.0 baseline benches.**
  `glob_compile`, `glob_match`, `regex_content`, and
  `rule_engine` should be flat or faster. If `rule_engine`
  regresses past 10% it means the Arc atomic-bump cost on
  hot paths is dominating the alloc savings; back out the
  specific Arc change that caused it.

## False-positive surface

(Bugs the new shape can introduce.)

- **`Arc<Path>` cloned across thread boundaries adds atomic
  contention.** The engine's `par_iter` over rules already
  multi-threads; v0.9.3's file-major dispatch will add another
  layer. Each `Arc::clone` is an `AtomicUsize::fetch_add`. At
  100k violations on 8 cores that's ~12k atomic ops per core,
  not a bottleneck on modern x86. Document the choice; revisit
  if the dhat → criterion delta is unfavourable.
- **`Cow<'static, str>` for messages may invite incorrect
  `'static` claims.** A rule that does
  `Cow::Borrowed(self.message_field.as_str())` would compile
  errors out (correctly), but a rule that does
  `Cow::Owned(self.message_field.clone())` papers over the
  win. Mitigation: the migration ships rule-by-rule with a
  reviewer comment at each non-static borrow site explaining
  why `Owned` is the right call.
- **A latent bug becomes visible.** Switching from
  `std::str::from_utf8 + split` to `bytes.split` in a rule
  that *did* depend on UTF-8 validation (e.g. an
  identifier-counting heuristic that assumed valid UTF-8)
  could change behaviour on a non-UTF-8 file. Mitigation: the
  v0.8.2 unit test suite already covers UTF-8 and non-UTF-8
  inputs for every line-oriented rule (the existing skip-on-
  non-UTF-8 path is tested). Per-rule conversion PRs run those
  tests before landing.
- **`bytes.split(|&b| b == b'\n')` allocates a Vec for each
  call?** No — `slice::split` returns a lazy `Split` iterator;
  no allocation. Belt-and-braces: a unit test asserts
  `Vec::with_capacity(0)` semantics by counting allocations
  via `dhat-rs` in a `#[cfg(test)]` block (or by visual
  inspection of the generated assembly).

## Implementation notes

**Crate locations:**

- `crates/alint-core/src/walker.rs` — `FileEntry::path` Arc'd.
- `crates/alint-core/src/rule.rs` — `Violation::path`,
  `Violation::message`, `RuleResult::rule_id`.
- `crates/alint-core/src/report.rs` — anywhere `RuleResult` is
  built or aggregated.
- `crates/alint-rules/src/io.rs` — **new file**.
  `read_prefix(path, n)`, `read_suffix(path, n)`,
  `lines_byte_slice(bytes)` (a thin re-export of
  `slice::split`). Single source of truth for the
  conversions.
- Per-rule files (`no_trailing_whitespace.rs`,
  `final_newline.rs`, `line_endings.rs`, `line_max_width.rs`,
  `max_consecutive_blank_lines.rs`, `indent_style.rs`,
  `executable_has_shebang.rs`, `shebang_has_executable.rs`,
  `file_starts_with.rs`, `file_ends_with.rs`) — each a small
  diff inside its `evaluate` body. Around 8–12 lines per file,
  no API change visible to the rule's caller.

**Sketch — Arc'd FileEntry::path:**

```rust
// walker.rs
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: Arc<Path>,
    pub is_dir: bool,
    pub size: u64,
}

// rule.rs
#[derive(Debug, Clone)]
pub struct Violation {
    pub path: Option<Arc<Path>>,
    pub message: Cow<'static, str>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl Violation {
    pub fn new(message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            path: None,
            message: message.into(),
            line: None,
            column: None,
        }
    }

    pub fn with_path(mut self, path: impl Into<Arc<Path>>) -> Self {
        self.path = Some(path.into());
        self
    }
}
```

`From<&PathBuf>` and `From<&Path>` for `Arc<Path>` already
exist via `Arc::from`. Rules that today do `.with_path(&entry.path)`
keep that exact call site — `&Arc<Path>: Into<Arc<Path>>` works
through `Arc::clone`.

**Sketch — line-oriented rule conversion:**

```rust
// before
let Ok(bytes) = std::fs::read(&full) else { continue; };
let Ok(text) = std::str::from_utf8(&bytes) else { continue; };
for (idx, line) in text.split('\n').enumerate() {
    let trimmed = line.strip_suffix('\r').unwrap_or(line);
    if trimmed.ends_with(' ') || trimmed.ends_with('\t') {
        return Some((idx + 1, trimmed));
    }
}

// after
let Ok(bytes) = std::fs::read(&full) else { continue; };
for (idx, line) in bytes.split(|&b| b == b'\n').enumerate() {
    let trimmed = line.strip_suffix(b"\r").unwrap_or(line);
    if matches!(trimmed.last(), Some(b' ' | b'\t')) {
        return Some(idx + 1);
    }
}
```

Rules that need character counts (`line_max_width`,
`indent_style`'s mixed-tab-and-space detector) keep the
`std::str::from_utf8` step but apply it per-line, after the
byte-level `\n` split, so the validation cost is paid only on
lines that actually need it.

**dhat workflow:**

```sh
# under alint-bench/
cargo bench --features dhat-bench --bench single_file_rules \
    -- --profile-time 10  # sample
# or build a small dhat-instrumented binary that runs the same
# scenario and writes dhat.json; load in dhat-rs viewer.
```

Adding `dhat-bench` as a Cargo feature on `alint-bench`
(initially gated, off by default) lets the v0.9.2 conversions
be checked one at a time. The feature flag stays in the
codebase — useful for future mem audits — but isn't in the
default build set.

**Per-rule conversion list (hypothesis, dhat-confirmed):**

| Rule | Conversion | Expected win |
|---|---|---|
| `no_trailing_whitespace` | `bytes.split + last()` | Skip UTF-8 pass; same alloc |
| `final_newline` | last byte check; no split needed | Skip UTF-8 pass + split |
| `line_endings` | byte scan for `\r` / `\n` runs | Skip UTF-8 pass |
| `max_consecutive_blank_lines` | `bytes.split + Bytes::is_empty` | Skip UTF-8 pass |
| `indent_style` | byte prefix per line; UTF-8 only on tab-after-space | Cheaper FP path |
| `line_max_width` | byte-split + per-line UTF-8 + `chars().count()` | Skip whole-file UTF-8 pass |
| `executable_has_shebang` | `read_prefix(path, 2)` for `#!` | Read 2 bytes, not 100KB |
| `shebang_has_executable` | `read_prefix(path, 2)` | Same |
| `file_starts_with` | `read_prefix(path, pattern.len())` | Read N bytes, not whole file |
| `file_ends_with` | `read_suffix(path, pattern.len())` | Read N bytes from EOF |

Whole-file rules (`file_hash`, `no_bom`, `no_bidi_controls`,
`no_zero_width_chars`, `file_is_ascii`, `file_content_matches`,
`file_content_forbidden`, `file_header`, `file_footer`,
`file_max_size`, `file_min_size`, `file_max_lines`,
`file_min_lines`, `file_shebang` if regex multiline,
`json_schema_passes`, `json_path_*` / `yaml_path_*` /
`toml_path_*`, `markdown_paths_resolve`,
`commented_out_code`, `no_merge_conflict_markers`) keep
their `std::fs::read` for v0.9.2 — bounded reads don't apply.
v0.9.3's per-file dispatch will eliminate redundant reads
across these.

**Complexity estimate:** ~3 days. Day 1: Arc / Cow type
changes + cross-formatter test verification. Day 2: line-
oriented rule conversions + per-rule unit tests. Day 3:
prefix/suffix-bounded rules + dhat baseline + bench-compare.

May further-split per-rule into individual commits if the dhat
output flags one conversion as standalone-worth-reviewing
(e.g. if `file_starts_with`'s win is dramatic enough that
dropping it into a separate commit makes the PR easier to
read).

## Tests

**Existing tests at v0.8.2:**
- Per-rule unit tests for all line-oriented rules already
  cover the byte-pattern + UTF-8 + non-UTF-8 input matrix.
- `cross_formatter.rs` invariants test on the entire output
  layer — Arc / Cow type changes are caught here if they
  break serialisation.

**New tests for v0.9.2:**
- `walker.rs::file_entry_path_arc_clone_is_cheap` — assert
  `Arc::strong_count` after cloning N entries from one
  `FileIndex`. Catches an accidental switch back to `PathBuf`.
- Per-converted-rule: assert byte-identical violation output
  against a frozen fixture. The v0.8.5 e2e suite already does
  this at the alint-binary level; a unit-level fixture inside
  each rule's `tests` mod gives faster feedback.
- `dhat-rs` smoke test gated on the `dhat-bench` feature:
  run a fixture through `engine.run`; assert
  `dhat::Profiler::stats().total_blocks` is below a baseline
  number. Tunable; not gating CI; documents intent.

**Bench validation per phase:**
- `single_file_rules.rs` — every line-oriented rule should
  improve at 1k and 10k tree sizes. Whole-file rules should
  be flat (no change to their read path).
- `output_formats.rs` — should be flat. Output formatters
  don't change; the type-level Arc / Cow are transparent at
  the formatter boundary.
- `rule_engine.rs` (v0.7.0 baseline) — should be flat or
  faster. If it regresses past 5% the Arc atomic-bump cost
  on the engine hot path is dominating; investigate before
  merge.

## Open questions

1. **`Arc<Path>` vs `Arc<PathBuf>` vs `Rc<Path>`.** `Arc<Path>`
   needs `Arc::from(PathBuf)` which clones the bytes once on
   construction; subsequent clones are cheap. `Rc<Path>` is
   single-threaded and incompatible with the engine's Rayon
   parallelism. Lean `Arc<Path>` — the construction-time clone
   is amortised across N violations referencing it.
2. **Convert `ctx.root: &Path` → `ctx.root: Arc<Path>` too?**
   `Context::root` is borrowed for the lifetime of `engine.run`,
   so it doesn't need the Arc — every consumer holds a `&Path`
   already. Lean leave it.
3. **Does `bytes.split(|&b| b == b'\n')` need a `memchr`-based
   replacement?** `slice::split` is byte-by-byte scalar code.
   `memchr::Memchr` is SIMD-vectorised and ~10x faster on
   large buffers. Add `memchr` as an `alint-rules` dep behind
   an `iter::IterChunks` helper if the dhat output flags
   `slice::split` as hot. Otherwise skip — adding deps that
   the bench-compare gate doesn't justify is wasted dependency
   surface.
4. **Should we go further on `Cow` for `RuleResult::policy_url`
   and `Violation::message`?** `policy_url: Option<String>` is
   set once per rule from config; today it's cloned once per
   violation. Same pattern as `rule_id` → `Arc<str>` would
   apply. Worth doing if the dhat output flags the clone.
5. **Should `Violation::path` carry a `line_count_hint` for
   downstream callers?** Out-of-scope — that's a feature, not
   a memory pass. Mention only to defer.
