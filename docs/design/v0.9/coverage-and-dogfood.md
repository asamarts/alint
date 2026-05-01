# v0.9.5–v0.9.9 — Coverage expansion + self-dogfooding

Status: Design draft, written 2026-05-01 after the
cross-file dispatch fast-path work landed (commits
`1cc6c5c` + `26075f3`). Reopens v0.9 for five additional
sub-phases that follow the same engine-quality theme: harden
the test/coverage floor that lets future engine work land
without surprises like the +28-37% v0.9.x 1M S3 regression
that motivated the path-index fix.

## Why reopen v0.9 (and not start v0.10)

The path-index work that closed the 1M S3 cliff is tightly
themed with the engine optimizations v0.9.1-.4 shipped — it's
the same family of "engine-internal change with no
user-visible API impact." Bundling .5–.9 under v0.9 keeps the
LSP work (v0.10) focused on its own scope. v0.9 closes when
the test/coverage floor is something we can hand to v0.10
without back-pressure from "but how do we verify a per-file-
edit re-evaluation doesn't regress this rule?".

## Phase summary

| Phase | What ships | Status |
|---|---|---|
| **v0.9.5** | Cross-file dispatch fast paths (already merged on `main`: `1cc6c5c` + `26075f3` + `a7b2354`) | Code in; release pending |
| **v0.9.6** | Coverage audits — pass/fail symmetry, bundled-ruleset coverage, git-mode symmetry | Not started |
| **v0.9.7** | Coverage scenarios filling the gaps v0.9.6 audits surface | Not started |
| **v0.9.8** | Bench-scale extension: S6 (per-file content fan-out), S7 (cross-file relational), S8 (git-tracked overlay) + `generate_git_monorepo` helper | Not started |
| **v0.9.9** | alint self-dogfooding: `.alint.yml` at the repo root that lints alint with itself, gated in CI | Not started |

## v0.9.5 — Cross-file dispatch fast paths *(release pending)*

### Problem

For each `for_each_dir` rule with a `crates/*` select and a
nested `file_exists`-shaped check, the engine instantiated a
fresh nested rule per matched directory and each ran a full
linear scan of `ctx.index.files()`. Dispatch shape was
O(D × N): D matched dirs × N total entries. Fine at 10k,
painful at 100k, terminal at 1M (the published v0.5.6 1M S3
hyperfine number was 528 s, dominated by these scans; v0.9.4
landed at 731 s with a +184 s drift on top).

### Fix (already in place)

Two commits:

- `1cc6c5c perf(core): lazy path-index on FileIndex +
  scaling-profile instrumentation` — `FileIndex` gains a lazy
  `OnceLock<HashSet<Arc<Path>>>` keyed on every file entry;
  new `contains_file(&Path) -> bool` is the canonical O(1)
  "does this path exist?" query. `find_file` keeps its
  signature but does the O(1) check first. Engine adds
  `tracing::info!` per-phase + per-cross-file-rule wall-time
  emission gated by `ALINT_LOG=alint_core=info`.
- `26075f3 perf(rules): O(1) literal-path fast paths in
  file_exists, structured_path, iter.has_file` — three rules
  detect literal-path patterns at build time and short-
  circuit through `contains_file`. Glob/exclude/git-tracked
  paths keep the existing O(N) scan.

### Numbers

`xtask gen-monorepo --size 1m`, S3 = `oss-baseline + rust +
monorepo + cargo-workspace`, hyperfine `--warmup 1 --runs 3`:

| Cell | v0.9.4 baseline | After fix | Speedup |
|---|---:|---:|---:|
| `1m S3 full` | 731.856 s | 11.194 s ± 0.154 | **65.4×** |
| `1m S3 changed` | 724.362 s | 6.728 s ± 0.059 | **107.7×** |

vs the published v0.5.6 1M S3 baseline (528 s changed): also
~80× faster than alint has ever been at this scale.

### Release work

- `chore(release): bump workspace to 0.9.5`
- CHANGELOG `[0.9.5]` entry
- Tag, push, watch the 5-channel release pipeline (GitHub
  Releases / crates.io / npm / Homebrew / Docker) green
- Update `project_alint-releases.md` memory

## v0.9.6 — Coverage audits

Three new tests under `crates/alint-e2e/tests/`. Land first
because their failures **define the punch list for v0.9.7**.

### `coverage_audit_pass_fail.rs`

Every canonical rule-kind has at least one scenario where its
rule emits a violation AND at least one where it stays
silent. Detected by:

1. Walking every `crates/alint-e2e/scenarios/check/**/*.yml`.
2. For each scenario, parsing `expect.violations:` to
   determine which kinds emit violations.
3. For each kind, asserting both buckets are non-empty.

Aliases (`max_size` ↔ `file_max_size`, …) handled the same
way `coverage_audit.rs` already does.

### `coverage_audit_bundled_rulesets.rs`

Every `crates/alint-dsl/rulesets/v1/**/*.yml` has at least
one well-formed (silent) and one ill-formed (flagging)
scenario referencing it via `extends:`.

### `coverage_audit_git_modes.rs`

Rules that return `wants_git_tracked()` or `wants_git_blame()`
true (or take a `git_tracked_only:` parameter) have both an
in-repo (`given.git: { init: true, … }`) and a non-git
scenario. A small declarative allowlist for kinds where
git-mode doesn't apply (`filename_*`, etc.).

### Bench-scale soft warning

Extend the existing `coverage_audit.rs` to emit a (non-
failing) listing of rule-kinds present in the registry but
absent from any `xtask/src/bench/scenarios/*.yml` or
embedded bundled-ruleset extends. Bench coverage isn't
required for correctness — but the warning surfaces gaps
when expanding S* scenarios.

### Bench-compare gating

No perf change in v0.9.6. Existing `bench-compare` against
v0.7.0 stays green by definition (no engine code touched).

## v0.9.7 — Coverage scenarios

Author scenarios surfaced by v0.9.6 audits. ~30-50 new YAMLs
under existing family directories. Naming convention
codified:

- `<family>/<kind>_pass.yml` — well-formed input, rule silent
- `<family>/<kind>_fires.yml` — ill-formed input, rule emits
  expected violations
- `<family>/<kind>_no_op_outside_git.yml` — git-aware rule
  outside a git repo
- `<family>/<kind>_in_repo.yml` — git-mode behaviour

Existing scenarios already follow most of this; this phase
codifies it and fills the gaps. Spread across multiple PRs,
one family at a time. Each PR makes the audits greener.

Phase ships when `cargo test -p alint-e2e --test
coverage_audit_*` is green for all four audits across the
full registry.

## v0.9.8 — Bench-scale extension

S1-S5 unchanged. Three new bench scenarios + a
`generate_git_monorepo` helper in `alint-bench/src/tree.rs`.

| New | Shape | Rule kinds exercised |
|---|---|---|
| **S6 — Per-file content fan-out** | 13 content rules over `**/*.rs`. Stresses the per-file dispatch path width. | `final_newline`, `no_trailing_whitespace`, `no_bidi_controls`, `no_zero_width_chars`, `no_bom`, `line_endings`, `indent_style`, `line_max_width`, `max_consecutive_blank_lines`, `file_max_lines`, `file_min_lines`, `file_is_text`, `file_is_ascii` |
| **S7 — Cross-file relational** | `for_each_file`, `every_matching_has`, `dir_only_contains`, `dir_contains`, `pair`, `unique_by` over the synthetic monorepo. | The remaining cross-file kinds the path-index fix doesn't touch. |
| **S8 — Git-tracked overlay** | S3 reshape: synthetic monorepo gets `.git/` initialised + every file `git add`'d. `git_tracked_only: true` overlaid; `git_no_denied_paths` and `git_blame_age` added. | Exercises `Engine::collect_git_tracked_if_needed` + `BlameCache` at scale. |

S6/S7 trees reuse `generate_monorepo`. S8 needs the new
`generate_git_monorepo` helper that runs `git init && git add
-A && git commit` after generation — same shape as
e2e's git-fixture helper but at 1k/10k/100k/1m scale.

100k cells get `bench-compare --threshold 10` gates against
the v0.9.5 baseline (frozen at the path-index fix landing).

`xtask` enum (`Scenario::S1 .. S5`) gets three more variants
(`S6`, `S7`, `S8`). Default `--scenarios` stays `S1,S2,S3`
(the publication trio); S4–S8 are opt-in.

## v0.9.9 — alint self-dogfooding *(infra in place; coverage rule kinds deferred)*

> Status update — 2026-05-01 implementation pass discovered
> that the declarative-coverage idea below requires a rule
> kind alint doesn't yet have: aggregate "does any file in
> this scope contain pattern X?" semantics. Today's
> `file_content_matches` is per-file (a non-matching file =
> one violation per file). The right primitive is
> `any_file_content_matches` or facts-style "fires once if
> the predicate holds nowhere across the scope". That's a
> v0.10+ rule-kind addition.
>
> What v0.9.9 actually ships:
> - The two-layer enforcement framing (file-presence + Rust
>   audits) is documented in
>   `docs/development/RULE-AUTHORING.md` (new) so future
>   contributors land scenarios alongside their rules.
> - The existing `.alint.yml` already lints alint on every
>   push via `.github/workflows/action-selftest.yml`. Layer 1
>   ("alint runs on alint") is real — just not for the
>   coverage rules below, which need a future rule kind.
> - Rust audits from v0.9.6 + scenarios from v0.9.7 are the
>   actual enforcement. The dogfood `.alint.yml` rules below
>   are aspirational.

The most novel piece. alint can express most of its own
authoring invariants through the rule catalogue itself — and
where it can't, the Rust audits from v0.9.6 cover the rest.

### Two-layer enforcement

| Layer | Tool | Catches |
|---|---|---|
| **1 — File presence** | `alint check .` (the `.alint.yml` below) | Missing source files, missing scenario YAMLs, missing bundled-ruleset coverage. Fast feedback during local development; runs in pre-commit. |
| **2 — Semantic** | `cargo test -p alint-e2e` (`coverage_audit_*.rs`) | Pass/fail symmetry, alias-aware kind coverage, registry consistency. |

Both gates land in CI. Adding a rule without a scenario
fails layer 1 at lint time; adding only a passing scenario
fails layer 2 at test time.

### What alint can express

```yaml
# .alint.yml at the alint repo root
extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/rust@v1
  - alint://bundled/monorepo@v1
  - alint://bundled/monorepo/cargo-workspace@v1

rules:
  # Every rule-source file has ≥1 e2e scenario referencing it.
  - id: rule-source-has-e2e-scenario
    kind: for_each_file
    select: "crates/alint-rules/src/*.rs"
    when_iter: '!(iter.basename in ["lib.rs","test_support.rs","fixers.rs","io.rs","case.rs"])'
    require:
      - kind: file_content_matches
        paths: "crates/alint-e2e/scenarios/check/**/*.yml"
        pattern: "kind:\\s*{stem}\\b"
    level: error
    message: "Rule `{stem}` has no e2e scenario; add one under crates/alint-e2e/scenarios/check/<family>/{stem}_{pass,fires}.yml"

  # Every bundled ruleset has ≥1 well-formed + 1 flagging scenario.
  - id: bundled-ruleset-has-pass-scenario
    kind: for_each_file
    select: "crates/alint-dsl/rulesets/v1/**/*.yml"
    require:
      - kind: file_exists
        paths: "crates/alint-e2e/scenarios/check/bundled*/{stem}_*pass*.yml"
    level: error

  - id: bundled-ruleset-has-flagging-scenario
    kind: for_each_file
    select: "crates/alint-dsl/rulesets/v1/**/*.yml"
    require:
      - kind: file_exists
        paths: "crates/alint-e2e/scenarios/check/bundled*/{stem}_*{flag,fire,gap}*.yml"
    level: error

  # Soft: every rule-source file has ≥1 bench-scale reference.
  - id: rule-source-in-some-bench-scenario
    kind: for_each_file
    select: "crates/alint-rules/src/*.rs"
    when_iter: '!(iter.basename in ["lib.rs","test_support.rs","fixers.rs","io.rs","case.rs"])'
    require:
      - kind: file_content_matches
        paths: "xtask/src/bench/scenarios/*.yml"
        pattern: "kind:\\s*{stem}\\b"
    level: info  # soft — bench coverage isn't required for correctness
    message: "Rule `{stem}` has no bench-scale scenario; next regression of its dispatch shape won't be gated"
```

### What alint can't (cleanly) express — keep the Rust audit

- **Pass/fail symmetry** based on `expect.violations:`
  semantics is parsing scenario-DSL semantics, not file
  presence. Stays in `coverage_audit_pass_fail.rs`.
- **Alias resolution** — alint sees `kind: max_size` and
  `kind: file_max_size` as different strings. Rust knows
  they're the same registered builder.
- **Registry walk** — Rust enumerates the canonical set;
  alint sees only files. Rust audit complements the file-
  presence checks.

### CI integration

- New `alint-self-check` GitHub Actions job (or addition to
  the existing `ci.yml`): `cargo run --release --quiet -p
  alint -- check .` runs against the repo at PR + push time.
- Existing `cargo test -p alint-e2e` already gates the Rust
  audits.

### Marketing side-benefit

"alint lints itself" is a credible quality signal. The
`.alint.yml` doubles as a worked example of the agent-tier
rule-authoring workflow we've been converging toward
(shipped behind `RULE-AUTHORING.md`, drafted in v0.9.6).

## Out of scope for v0.9.5–.9

- **LSP server** — still v0.10. The path-index hot-path data
  structure is itself useful for v0.10's per-file-edit
  re-evaluation (a single-file change only needs to re-test
  the rules whose `path_scope` matches that path; existing
  contains-checks become O(1)).
- **HashMap<Arc<Path>, &FileEntry>** — `find_file` still has
  an O(1) check + O(N) fallback to fetch the entry. Only
  tests use it now; a future point release could upgrade
  the OnceLock value to a `HashMap<Arc<Path>, usize>` for
  full O(1). Not urgent.
- **Bench-scale `--keep-tree`** — `xtask gen-monorepo`
  already covers the persistent-tree case. bench-scale's
  tempdir lifecycle stays as is.

## Phase ordering rationale

| Phase | Why this slot |
|---|---|
| .5 | Done — release first to ship the perf win independently of process work. |
| .6 | Lands **before** scenarios because audit failures define the work. Tag-eligible even with red audits if we mark them `#[ignore]` with tracking issues, OR we don't tag .6 until .7 catches up. Recommend the latter. |
| .7 | Drains the punch list. Could split into .7a / .7b across families if it gets long. |
| .8 | Independent of .7 — depends only on .5's path-index landing. Could even ship before .7 if scheduling matters. |
| .9 | Last because the `.alint.yml` rules need the scenario population from .7 to actually pass. Until .7 is done, dogfooding fires too many violations to be a clean gate. |

## Open questions

1. **`when_iter:` glob exclusion DSL.** The `.alint.yml`
   sample uses
   `'!(iter.basename in ["lib.rs",…])'` — that syntax
   needs to be reachable from the parser. Today `iter.has_file`
   is the only `iter.*` method; need to extend the
   when-expression evaluator with `iter.basename` (string)
   and the `in [...]` operator. Both are minor parser
   additions.
2. **`select` glob alternation.** The bundled-ruleset audit's
   `paths: "crates/alint-e2e/scenarios/check/bundled*/{stem}_*pass*.yml"`
   relies on globset's brace alternation in path components;
   needs verification that this works through `Scope`'s
   `literal_separator(true)` setting.
3. **Pre-commit vs CI-only.** Adding `alint check .` as a
   pre-commit hook gives fast feedback but slows commits.
   Recommend CI-only at first; revisit when `alint check`
   itself is fast enough on the repo (it's < 100 ms today
   without any rules; the dogfood rules add measurable but
   small cost).

## Cross-references

- v0.9 phases .1-.4: `docs/design/v0.9/{parallel_walker,
  memory_pass, dispatch_flip}.md`
- v0.9.5 commits: `git log 9050745..HEAD`
- v0.9.5 bench data: `docs/benchmarks/v0.9/scale-after-pathindex/`
- v0.9.5 investigation traces:
  `docs/perf/cross-file-rules/`
- Existing coverage audit:
  `crates/alint-e2e/tests/coverage_audit.rs`
