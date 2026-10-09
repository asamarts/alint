# `generated_file_fresh` — a committed file must match its generator's output

Status: **Implemented** — lands with the rule in v0.10 (this
commit; rule kind #4 of the case-study coverage push). Was a
design draft (2026-05-18). v0.10 demand #4 (8 sources,
ROADMAP-canonical). Open questions resolved on implementation:
stdout-only, in-place/temp-sandbox deferred (Q1); blocking
`.output()`, opt-in `timeout:` deferred (Q2); one `file` per
rule (Q3); sibling of `command_idempotent` #6, docs cross-link
(Q4); first-differing-line hint, unified-diff deferred (Q5).

Demand evidence:
[`docs/development/launch-evidence.md`](../../development/launch-evidence.md)
("run a generator, diff output against on-disk file", 6 sources:
uv, cpython, pytorch, bazel, TF, spark — with the explicit
"alint's deliberate non-goal is running codegen — propose as
opt-in primitive" note) and the per-repo tracker in
[`examples/README.md`](../../../examples/README.md#primitive-demand-tracker)
(`generated_file_fresh` row: airflow, spark, kubernetes,
nixpkgs, protobuf, cpython, pytorch, tensorflow). Canonical
scope: [`../ROADMAP.md`](../ROADMAP.md#v010-case-study-coverage-push)
(#4).

## The non-goal tension (read this first)

**alint's deliberate non-goal is running codegen as a build
step.** A linter that silently regenerates files is a build
tool, and that is out of scope.

`generated_file_fresh` does *not* cross that line. It is a
*freshness check*: it runs an **explicitly-declared,
maintainer-trusted** generator the user wrote into their own
`.alint.yml`, **captures its stdout**, and compares that to the
committed file. It **never writes the working tree** and never
"updates" anything. It is exactly the same trust tier as the
existing `command` rule (which already shells out to
user-declared CLIs); this rule is the structured, non-mutating
specialisation for the "the committed artefact must match what
the generator would produce" shape. Opt-in: no `command:` is a
config error, the rule does nothing on its own.

## Problem

A file is the output of a generator and carries a
"// Code generated … DO NOT EDIT" banner, but nothing enforces
that the committed bytes still match the generator. It drifts:
someone hand-edits the generated file, or changes the source
without re-running the generator, and the staleness is invisible
until a downstream consumer breaks. The shape recurs:

- **protobuf / buf generated stubs** (protobuf, bazel, TF,
  spark): `*.pb.go` / `*_pb2.py` must match `protoc` / `buf
  generate` output.
- **`pip-compile` / `uv` lock outputs** (uv): `requirements.txt`
  must match `uv pip compile pyproject.toml`.
- **cpython generated tables** (cpython): opcode / AST headers
  produced by `Tools/` scripts.
- **FFI / binding headers** (pytorch): `cbindgen` / `bindgen`
  output committed into the tree.
- **k8s / nixpkgs generated manifests and `*.generated.*`**.

`command` can shell out per file but only checks an exit code;
there is no "run *this* generator once and assert its output
equals *this* committed file".

Precise, not heuristic: byte comparison of captured stdout vs the
committed file (under an optional `normalize`), no guessing.

## Surface area

New rule kind `generated_file_fresh` in `alint-rules`.
`version: 1` unchanged.

```yaml
- id: bindings-fresh
  kind: generated_file_fresh
  file: crates/ffi/include/core.h     # the committed generated file
  command: ["cbindgen", "--config", "cbindgen.toml", "crates/core"]
  workdir: "."                        # cwd for the generator (default: lint root)
  normalize: final-newline            # none (default) | trim | final-newline
  level: error
  message: "{{ctx.file}} is stale — re-run the generator and commit it"
```

`command` is an argv array (no shell), spawned with stdin closed,
stdout/stderr captured, cwd `workdir` (relative to the lint
root). The generator **must emit to stdout** (most do, or have a
`-`/`--output -` mode) — that is what keeps this non-mutating.
Env threaded in mirrors the `command` rule: `ALINT_ROOT`,
`ALINT_RULE_ID`, `ALINT_LEVEL`, `ALINT_VAR_<NAME>`.

## Semantics

Single-shot (not per-file): the rule evaluates once per run
(`requires_full_index() == true`, `path_scope() == None` — same
dispatch class as `pair`; it does not walk the index, it runs
the generator and reads one declared file).

1. Spawn `command` in `workdir`; capture stdout + stderr.
2. **Spawn failure** (program not found, …) ⇒ one violation
   naming the program.
3. **Non-zero exit** ⇒ one violation with the truncated stderr
   (the generator itself is broken; staleness is moot).
4. **Committed `file` missing** ⇒ one violation (the generator
   would create it; it isn't there).
5. Compare `normalize(stdout)` to `normalize(<committed file>)`.
   Mismatch ⇒ one violation: "`<file>` is stale — its committed
   contents differ from `command`'s output", with the
   first-differing line number as a hint.
6. Equal ⇒ silent.

At most one violation (it is a single check), anchored on `file`.
`normalize`: `none` (exact bytes), `trim` (trim leading/trailing
whitespace of the whole output — absorbs final-newline and
indentation-only diffs), `final-newline` (only normalise a
single trailing `\n` — the most common generator/editor diff).

## False-positive surface

- **Final-newline churn.** Generators and editors disagree on
  the trailing newline constantly → `normalize: final-newline`
  is the expected default for most configs; documented in the
  rule's docs example.
- **Non-deterministic generators.** A generator that embeds a
  timestamp / absolute path / RNG will always "mismatch". This
  is the generator's bug, not the rule's; documented as a
  precondition (the generator must be deterministic — which the
  demand sources' generators are). No attempt to scrub.
- **Generator not installed on the runner.** Spawn-failure is a
  *violation*, not a silent pass — but it is a distinct,
  clearly-worded one ("`protoc` not found") so CI triage is
  immediate; users gate the rule behind `when:` /
  `extends:`-profile if the toolchain isn't guaranteed.
- **Mutating generators.** A generator with no stdout mode that
  only writes in place is out of v0.10 scope (stdout-only keeps
  the rule non-mutating). Documented; temp-sandbox mode is
  Open question 1.
- **Encoding.** With `normalize: none`, raw-byte compare (binary
  generated files work); otherwise UTF-8 lossy + normalise.

## Implementation notes

- Module: `crates/alint-rules/src/generated_file_fresh.rs`.
  Spawn mirrors `command::run_one` (argv, `current_dir`,
  `Stdio::null()` stdin, piped stdout/stderr, the `ALINT_*`
  env). Spawn + capture goes through the shared
  `crate::spawn::run_capturing` helper (added v0.10 post-audit
  P2): one spawn, stdout/stderr drained **concurrently** on
  reader threads (so the *full* generator stdout is captured for
  the diff — never truncated) while a poll loop enforces an
  opt-in `timeout:` (default 120 s — generous for a single-shot
  whole-repo generator, bounded so a deadlocked child can't hang
  CI forever; raise via `timeout:`). On timeout the child is
  killed and one violation is emitted. This is **not** a literal
  copy of `command::run_one`: that drains *after* exit with a
  16 KB cap (right for command's error snippet, but it would
  truncate this rule's full-stdout diff and risk a pipe-fill
  deadlock). `command.rs` is left untouched (shipped rule, out
  of this remediation's scope).
- Single-shot rule: `impl Rule { rule_common_impl!();
  requires_full_index()->true; path_scope()->None;
  evaluate(ctx) }`. `evaluate` runs the generator and reads
  `ctx.root.join(file)` — it does not iterate `ctx.index`.
- No `FileIndex`, no shared `crate::extract`. Reads the one
  declared file directly (`ctx.root` join), same as the other
  cross-file rules read their manifests.
- No `include_str!` data; nothing leaves the crate.

## Tests

- Fresh (committed file == generator stdout) ⇒ silent; stale ⇒
  one violation with the first-differing line.
- Committed file missing ⇒ violation; generator non-zero exit ⇒
  violation (stderr surfaced); program-not-found ⇒ violation.
- `normalize` matrix: `none` (trailing-newline diff fails),
  `final-newline` (same diff passes), `trim`.
- Tests use a trivially portable generator (`printf` / a small
  `sh -c 'cat fixture'`) + a tempdir so they run in the CI
  sandbox without a real toolchain.
- Lockstep with the codebase invariants (same checklist
  #1/#2/#3 followed): `coverage_audit_pass_fail` (pass + fail
  e2e scenarios using a portable command), schema `$def` +
  dispatch `$ref` in both mirrored `config.json`,
  `all_kinds.yaml` entry, regenerated default-options snapshot,
  rule-count **73 → 74** across README ×2 / `docs/site/about` /
  `coverage_audit_readme_claims`, `docs/rules.md` section,
  CHANGELOG `[Unreleased]` Added (the fourth v0.10 item).
- **Bench-compare threshold:** the rule spawns one process; it
  is excluded from the hot per-file path. Full-run S-class wall
  must not regress vs the pre-phase baseline (`xtask
  bench-gate`, per `RELEASING.md`) — a single spawn at run end
  is negligible at scale.

## Open questions

Resolve inline when implementation lands.

1. **In-place / temp-sandbox generators.** Generators with no
   stdout mode (write-in-place only). v0.10 is stdout-only
   (keeps non-mutating). A future `regenerate_in: <tempdir>`
   mode (copy inputs to a temp tree, run, diff) covers the rest
   — deferred until a demand source needs it.
2. **Timeout.** *Resolved (v0.10 post-audit P2).* Opt-in
   `timeout:` (seconds, default 120) via the shared
   `crate::spawn::run_capturing` helper (concurrent pipe-drain +
   poll/kill); a hung generator now yields one timeout violation
   instead of hanging the run. See Implementation notes.
3. **Multi-file generators.** A generator producing several
   files (one `protoc` invocation → many stubs). v0.10: one
   `file` per rule (declare N rules, or one rule per output).
   A `files:`/manifest mode is a v0.11 consideration.
4. **Relationship to `command_idempotent` (#6).** Sibling:
   `command_idempotent` runs a *formatter* in `--check` mode and
   trusts its exit code; `generated_file_fresh` runs a
   *generator* and diffs its output. Docs cross-link; not the
   same kind.
5. **Diff richness.** v0.10 reports the first-differing line
   number. A unified-diff snippet in the message is a follow-up
   if users want more than "stale at line N".
