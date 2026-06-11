# Design doc: `facts.json` — the machine-readable surface-area contract

Status: Implemented (`xtask gen-facts` + committed `facts.json`, Phase 3).
Decisions: ADR-0001 (spec-driven development). Phase 3 / WS1e of
`spec-driven-development.md`.
Demand evidence: the v0.9.22 audit found a "60 rule kinds" README claim that was
ten behind reality for months; the 2026-05-22 alint.org bump silently failed CF
Pages because the consumer and producer disagreed on a hand-counted number.

## 1. Problem

alint asserts its own surface area as prose in many places — the README sentence
("89 rule kinds across 13 families, 22 bundled ecosystem rulesets, 12 auto-fix
ops, 8 output formats", "10 subcommands"), `docs/site/about/index.md`, and the
alint.org marketing site (a separate, private repo). Every one of these is a
hand-maintained number that drifts the instant a rule kind, fixer, formatter, or
subcommand lands without someone editing the copy.

Two mechanisms already fight this, but partially:

- `coverage_audit_readme_claims` pins the README/about numbers to deterministic
  sources (the `all_kinds.yaml` fixture, `docs/rules.md` headings, the rulesets
  directory, the `Format`/`Command` enums, the `*Fixer` structs). Good, but it
  only guards *this repo's* prose.
- `docs-export` already emits a build-time `manifest.json` into the docs bundle
  with `alint_version` + five counts, which alint.org consumes via a drift gate.
  Good, but it (a) omits the **families** count the README claims, (b) carries
  only counts — no catalogue *lists* a site could render — and (c) is volatile
  (`git_sha`, `generated_at`), so it can't be committed and content-diff gated
  in-repo.

What stays unsolved: there is no single, committed, gated, list-bearing contract
that the README, the docs, and alint.org can all render from instead of
restating numbers. This doc specifies that file.

## 2. Surface area

A new committed artifact `facts.json` at the repo root, regenerated and gated by
a new `xtask gen-facts [--check]` (mirroring `xtask gen-schema`). It carries no
volatile fields, so it is committed and content-diff gated like the schema.

```json
{
  "format_version": 1,
  "alint_version": "0.12.0",
  "counts": {
    "rule_kinds": 89,
    "families": 13,
    "bundled_rulesets": 22,
    "auto_fix_ops": 12,
    "output_formats": 8,
    "subcommands": 10
  },
  "rule_kinds": ["commented_out_code", "cross_file", "..."],
  "families": ["Existence", "Content", "..."],
  "bundled_rulesets": ["apache/governance", "go", "rust", "..."],
  "output_formats": ["agent", "github", "gitlab", "human", "json", "junit", "markdown", "sarif"],
  "subcommands": ["check", "explain", "export-agents-md", "..."],
  "fact_predicates": ["all_files_exist", "any_file_exists", "count_files", "custom", "file_content_matches", "git_branch"]
}
```

Every list is sorted for deterministic output. The five list-backed counts
satisfy `counts.X == X.len()` as an enforced invariant; `auto_fix_ops` is
count-only (the `*Fixer` struct names are an internal detail, not a public
catalogue).

The live `manifest.json` shape is **left untouched** — alint.org's sync depends
on it, and the 2026-05-22 incident showed that changing the producer ahead of
the consumer breaks CF Pages. `facts.json` ships *alongside* `manifest.json`;
adopting it on the site (WS5) is a separate, paced change in that repo.

## 3. Semantics

`gen-facts` derives every field from the same canonical source the README audit
uses, so `facts.json` can never disagree with the README:

| Field | Source of truth |
|---|---|
| `alint_version` | `env!("CARGO_PKG_VERSION")` (workspace version) |
| `rule_kinds` | distinct `kind:` values in `crates/alint-dsl/tests/fixtures/all_kinds.yaml` |
| `families` | non-meta `## ` headings in `docs/rules.md` |
| `bundled_rulesets` | `.yml` files (recursive) under `crates/alint-dsl/rulesets/v1/` |
| `output_formats` | variants of `enum Format` in `crates/alint-output/src/lib.rs`, lowercased |
| `subcommands` | `CLI_REFERENCE_SUBCMDS` (itself pinned to `enum Command` by a test) |
| `auto_fix_ops` (count) | `pub struct *Fixer` declarations under `crates/alint-rules/src/fixers/` |
| `fact_predicates` | the `FactSpec::name()` arms in `crates/alint-core/src/facts.rs` |

`gen-facts` (no flag) rewrites `facts.json`. `gen-facts --check` regenerates
in-memory and content-diffs against the committed file, failing with a
"run `cargo run -p xtask -- gen-facts`" hint if they differ — identical
ergonomics to `gen-schema --check`, and wired into `ci/scripts/docs.sh` and the
preflight the same way. `docs-export` copies the committed `facts.json` into the
bundle next to `manifest.json` and `schema.json`, so alint.org gets it at a
stable URL.

Dispatch class: not an engine rule — a build-time generator, like `gen-schema`
and `docs-export`.

## 4. False-positive surface

N/A for an engine rule sense. The generator's failure modes:

- **Counting drift between `gen-facts` and the README audit.** Mitigated by a
  test that asserts `facts.json`'s counts equal the audit's independently
  computed counts — if the two counting implementations ever diverge, the test
  fails. (They read the same files with the same rules, so they should always
  agree.)
- **Stale committed file.** Mitigated by `gen-facts --check` in CI + preflight
  (content diff, not mtime).
- **Non-determinism.** All lists are sorted; the struct serializes in a fixed
  field order; output ends in a trailing newline. `--check` would catch any
  accidental nondeterminism as spurious drift.

## 5. Implementation notes

New module `xtask/src/facts.rs` with `run(check: bool)` mirroring
`gen_schema::run`. A `Facts` struct derives `serde::Serialize` with fields in
the documented order; `serde_json::to_string_pretty` + trailing newline is the
rendered form. The canonical computations are reused where they already exist:
`CLI_REFERENCE_SUBCMDS` and the `count_enum_variants` helper in `docs_export`
are promoted to `pub(crate)`; the `all_kinds.yaml` / `docs/rules.md` / rulesets
/ fixers walks are small and live in `facts.rs`.

No new dependencies (`serde`, `serde_json` already in `xtask`). `facts.json` is a
new tracked file at the repo root. `manifest.json` and `write_manifest` are not
touched. Constitution invariants: none affected (no engine, DSL, or trust-gate
change).

## 6. Tests

In `xtask` (`facts.rs` `#[cfg(test)]`):

- `gen_facts_check_passes_on_committed_tree` — `run(true)` succeeds on the
  committed file (idempotency / freshness; mirrors the schema test).
- `facts_json_counts_match_list_lengths` — the five list-backed counts equal
  their list lengths; lists are sorted and de-duplicated.
- `facts_counts_agree_with_readme_audit_sources` — recompute each count the way
  `coverage_audit_readme_claims` does and assert equality, binding `facts.json`
  to the same truth as the README.

No e2e scenario (not an engine rule). `ci/scripts/docs.sh` gains
`gen-facts --check`; preflight inherits it.

## 7. Open questions

- **Supported-languages field?** WS1e lists "supported languages". Resolved:
  omitted from v1 — the only drift-free signal is the ecosystem entries already
  in `bundled_rulesets` (`rust`, `go`, `java`, `dotnet`, `php`, `python`,
  `node`); a separate hand-curated language list would itself be a drift source.
  alint.org can derive the language set from `bundled_rulesets`.
- **Fold `manifest.json` into `facts.json`?** Deferred. The volatile fields
  (`git_sha`, `generated_at`) keep `manifest.json` build-time-only; unifying
  would mean `write_manifest` reads `facts.json` and overlays the volatile
  fields. Safe to do later once alint.org consumes `facts.json` (WS5); doing it
  now risks the live contract for no immediate gain.
- **WS5 (site rendering + cross-repo content test).** Lives in the private
  alint.org repo: render counts/lists from the synced `facts.json`, add a
  build-time content test that fails if rendered claims disagree with the
  shipped contract, and a version-pin parity check. Tracked as the follow-up to
  this repo-side work.
