# `alint suggest` subcommand

Status: Implemented in v0.7.4. See `crates/alint/src/suggest/`
+ `crates/alint/src/progress.rs`.

Resolved open questions (from the original design draft, listed
at the bottom of this doc):

1. **Confidence calibration** — kept as designed. Bundled
   ecosystem hits ship at HIGH; antipattern + stale-TODO ship
   at MEDIUM. Default floor is MEDIUM so LOW-confidence noise
   doesn't surface unless asked for.
2. **Existing-config bias** — the bundled-ruleset suggester
   skips proposals already declared in the user's `.alint.yml`
   `extends:` list. `--include-bundled` overrides for
   prospecting / what-if mode. Rule-shaped proposals (the
   `git_blame_age` template from the stale-TODO suggester)
   always pass through — there's no clean way to detect
   "user already has a similar rule" without a much heavier
   semantic check.
3. **Deterministic ordering** — sorted by `(confidence
   desc, rule_id asc)`. Two runs over the same repo state
   produce identical output.
4. **`--against <future-config>` mode** — deferred per the
   draft.
5. **CI usage** — exits 0 unconditionally on scan success.
   The command is exploration, not a gate. `alint check`
   remains the CI entry point.

Two suggester families from the original draft were
**deferred to a later cut**:

- **Convention suggester** (sample-based filename-case at
  ≥ 80%) — high noise risk; needs careful tuning before
  shipping.
- **Cross-file suggester** (`.c` + `.h` `pair` proposals) —
  niche use case; few projects need it.

Both can land as additive v0.7.x point releases without
breaking the existing suggester interface.

## Implementation notes (post-design)

- Detection logic is shared between `alint init` and
  `alint suggest` by importing `crate::init::Detection`
  directly (both modules live in the binary crate). No
  extraction to `alint-core` was needed; lifting CLI logic
  into the engine crate would have grown alint-core's
  surface for no real reuse.
- Progress reporting is a separate module
  (`crate::progress`) using `indicatif`. It's exposed via a
  `&Progress` handle that suggesters thread without
  branching on visibility — `Progress::null()` (test-only)
  and the silent-mode handle (production) are
  observationally identical.

## Problem

A team adopting alint for the first time on an existing
codebase faces a cold-start problem: which rules to enable?
The bundled rulesets cover ~80% but every repo has its own
shape. Today the workflow is "read the rule reference,
guess what fits, run, tune." That's mechanical and
agent-shaped — alint should do it for you.

`alint init` (v0.5.4) already detects ecosystem markers
(Cargo / pnpm / poetry / etc.) and writes an `extends:`
list. `alint suggest` goes further: scan the actual repo
state for evidence that a rule would help, and propose it.

The output is a YAML snippet the user can paste into their
`.alint.yml` after reviewing. **No automatic apply.**
v0.7 is suggestion-only; auto-apply waits until the LSP
lands and the fix-application UX is mature.

## Schema

This is a CLI subcommand, not a config rule. Invocation:

```
alint suggest [options] [path]

Options:
  --format <human|yaml|json>   Output format. Default human.
  --include-bundled            Suggest bundled rulesets even if
                               the repo already extends some.
  --confidence <low|medium|high>
                               Lower bound on signal strength
                               for proposals. Default `medium`.
  --explain                    Print the evidence for each proposal.
```

Default output (human):

```
$ alint suggest

Found evidence for 6 rule(s) you may want to enable:

  ✓ alint://bundled/oss-baseline@v1
    └─ README.md exists at root, LICENSE exists, but no SECURITY.md.

  ✓ alint://bundled/rust@v1
    └─ Cargo.toml at root; 4 .rs files in src/.

  ✓ agent-no-console-log
    └─ 3 console.log calls in src/ (likely production source).

  ✓ filename_case (snake_case for src/**/*.rs)
    └─ 12/13 files in crates/alint-rules/src/ are snake_case.
       1 outlier: WorkspaceResolver.rs (PascalCase).

  ⚠ commented_out_code (src/**/*.{rs,ts})
    └─ Found 4 candidate blocks; FP rate uncertain — review
       before enabling.

  ⚠ git_blame_age (TODO/FIXME, max_age_days: 180)
    └─ 3 TODO markers >180 days old.

Run with --format yaml to print as a config snippet.
Run with --explain to see file-level evidence per proposal.
```

YAML output (paste-able):

```yaml
# Suggested by `alint suggest` 2026-04-28T15:23:01Z.
# Review each rule before adopting.

extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/rust@v1

rules:
  - id: agent-no-console-log
    kind: file_content_forbidden
    paths: "src/**/*.{ts,tsx,js,jsx}"
    pattern: '\\bconsole\\.(log|debug|trace)\\('
    level: warning
  ...
```

JSON output: a structured dump of `{ proposals: [{ rule,
evidence, confidence }, … ] }` suitable for agent
consumption (parallel to `--format=agent` for `check`).

## Semantics

Each suggester is a small heuristic that:
1. Inspects the file index, file contents, and git state.
2. If it finds a triggering signal, returns a Proposal
   (rule definition + evidence).
3. Reports a confidence (`low`, `medium`, `high`) based
   on the strength of the signal.

Suggesters cluster:

- **Bundled-ruleset suggesters** — detect ecosystem markers
  (Cargo.toml → `rust@v1`, package.json → `node@v1`, etc.).
  Largely overlaps with `alint init`'s detection — share
  code via a `detect::ecosystem` module.
- **Antipattern suggesters** — scan for known leftovers and
  propose the matching agent-hygiene rules. Examples:
  finding a `console.log` in `src/` proposes
  `agent-no-console-log`; finding `*.bak` proposes
  `agent-no-versioned-duplicates` or
  `hygiene-no-editor-backups`.
- **Convention suggesters** — sample filenames and propose
  a `filename_case` rule when ≥80% of files in a directory
  follow one case. Note outliers in evidence so the user
  can decide whether to enforce or fix the outlier first.
- **Cross-file suggesters** — find paired patterns
  (`.c` files with `.h` siblings) and propose `pair`.
- **TODO/age suggesters** — count TODO markers, count
  those over 180 days; propose `git_blame_age` when
  there are ≥3 stale ones.

A suggester can be skipped by:
- The repo already extends a bundled ruleset that includes
  the proposed rule (avoid duplicate suggestion). Override
  with `--include-bundled`.
- Confidence below the user's `--confidence` floor.

## False-positive surface

`alint suggest` proposes; it doesn't enforce. The FP cost
is "user reviews a proposal that doesn't fit and ignores
it" — a few seconds of attention, not a CI failure.

Even so, suggestion noise erodes trust. Defaults:
- Confidence floor: `medium`. Low-confidence suggesters
  (e.g. "you have one .py file, here's `python@v1`") only
  surface when the user opts in via `--confidence low`.
- Limit to ≤10 proposals per run. Sort by confidence
  descending. The output should be reviewable in one
  sitting.

## Implementation notes

**Crate location:** new module
`crates/alint/src/suggest/` containing:
- `mod.rs` — subcommand entry point + dispatch.
- `proposal.rs` — `Proposal` type, confidence enum,
  evidence types.
- `detect.rs` — shared ecosystem-detection helpers
  (lifted from `alint::init` to avoid duplication).
- One `suggesters/<topic>.rs` per family
  (`bundled.rs`, `antipattern.rs`, `convention.rs`,
  `cross_file.rs`, `todo_age.rs`).

**Sharing with `alint init`:** the ecosystem-detection
logic is currently in `alint::init`. Lift it into
`crates/alint-core/src/detect.rs` (or a new
`alint-detect` crate if it grows large) so both
subcommands consume the same detector. Keep
`publish = false` initially — it's internal.

**Output formats:**
- `human` — colourised table à la the existing rule list.
- `yaml` — paste-ready config snippet, written to stdout.
- `json` — `{ schema_version: 1, format: "suggest",
  generated_at, proposals: [{rule_id, kind, paths, level,
  evidence, confidence}] }`. Stable behind
  `schema_version`.

**No fix support.** This is a generation command, not a
mutation. `alint init` already has a clear "write a
file" UX; users who want apply-on-disk can pipe `alint
suggest --format yaml` into their config.

**Sketch:**

```rust
// crates/alint/src/suggest/mod.rs
pub fn run(opts: SuggestOptions) -> Result<i32> {
    let scan = scan_repo(&opts.path)?;
    let proposals = suggesters::all()
        .into_iter()
        .filter(|s| s.applies(&scan))
        .flat_map(|s| s.propose(&scan))
        .filter(|p| p.confidence >= opts.confidence)
        .filter(|p| !p.is_already_covered(&scan))
        .take(MAX_PROPOSALS)
        .collect::<Vec<_>>();

    match opts.format {
        Format::Human => render_human(&proposals, opts.explain, ...),
        Format::Yaml  => render_yaml(&proposals, ...),
        Format::Json  => render_json(&proposals, ...),
    }
}
```

**Test surface:**

- Per-suggester unit tests with synthetic file indexes.
- Integration: run against the existing testkit fixtures
  for each bundled ruleset and check the proposals match
  what's expected.
- Regression: run against alint's own repo and assert
  the proposal set is stable run-to-run.
- Confidence floor honoured.
- Already-covered detection (don't propose `rust@v1` when
  the user already extends it).

**Complexity estimate:** ~5 days. The framework is
straightforward but each suggester needs tuning, and
sharing detection logic with `alint init` requires
extraction work.

## Tests

- Unit per suggester.
- Format renderer tests (human / yaml / json byte
  comparisons against canonical fixtures).
- Already-covered logic doesn't double-propose.
- Confidence threshold honoured.
- Empty repo → empty proposal list (graceful).
- Repo with all rules already enabled → empty proposal
  list with informational message.

## Open questions

1. **Confidence calibration** — how to decide a
   suggester's default confidence? Plan: hand-pick per
   suggester based on the FP risk if the proposed rule
   were enabled. Bundled rulesets gated by ecosystem
   markers are `high` (false positives are low —
   ecosystem detection is ~99% precise). Heuristic
   antipattern suggesters are `medium`. Statistical
   convention inference (`filename_case` at 80%) is
   `medium` only when ≥80% threshold is hit; below that,
   `low`. Document the calibration.

2. **What if the user has a very different config style
   from what we propose?** — e.g. the project uses
   `kind: command` plugins for everything. `suggest`
   shouldn't propose redundant alternatives. Plan:
   detect the existing config's footprint and bias
   suggestions away from areas already covered.

3. **Deterministic ordering** — proposals should be
   stable across runs so users can diff `suggest`
   output. Plan: sort by `(confidence desc,
   suggester_name, rule_id)`.

4. **Should `suggest` accept a base config?** — i.e.
   `alint suggest --against my-future-config.yml` to
   ask "what's missing if I plan to extend with these
   rulesets?" Defer — the implicit base config (what's
   currently in `.alint.yml`) covers the common case.

5. **CI usage** — should `suggest` exit non-zero when
   it finds proposals? Lean **no** — it's an
   exploration tool, not a gate. CI gating happens via
   `check`. Document that `suggest` always exits 0
   unless the scan itself fails.
