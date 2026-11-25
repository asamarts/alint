# Design doc: `facts.json` — the machine-readable surface-area contract

Status: Implemented (`xtask gen-facts` + committed `facts.json`, Phase 3);
format v4 added the canonical `KindMeta` inventory in v0.19 W1 (2026-10-16), format v5 added
language compatibility discovery in v0.20 P1/P4, and format v6 adds trace-stage inventory in P5.
Decisions: ADR-0001 (spec-driven development), extended by ADR-0019 Phase 0 and ADR-0022.
Demand evidence: the v0.9.22 audit found a "60 rule kinds" README claim that was
ten behind reality for months; the 2026-05-22 alint.org bump silently failed CF
Pages because the consumer and producer disagreed on a hand-counted number.

## 1. Problem

alint asserts its own surface area as prose in many places — the README sentence
("102 rule kinds across 13 families, plus 11 aliases; 24 bundled ecosystem rulesets, 26 auto-fix
ops, 8 output formats", "13 subcommands"), `docs/site/about/index.md`, and the
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
  "format_version": 7,
  "alint_version": "0.19.0",
  "counts": {
    "rule_kinds": 102,
    "rule_aliases": 11,
    "families": 13,
    "bundled_rulesets": 24,
    "auto_fix_ops": 26,
    "output_formats": 8,
    "subcommands": 12
  },
  "rule_kinds": ["commented_out_code", "cross_file", "..."],
  "families": ["Existence", "Content", "..."],
  "categories": [{ "slug": "existence", "title": "Existence", "order": 0 }, { "slug": "content", "title": "Content", "order": 1 }],
  "rule_categories": { "commented_out_code": ["git-hygiene", "content"], "cross_file": ["cross-file"] },
  "rule_aliases": { "content_matches": "file_content_matches", "header": "file_header" },
  "kind_meta": {
    "command": {
      "dispatch": { "kind": "rule_major_scoped" },
      "paths": "required",
      "capabilities": [
        { "capability": { "kind": "spawn_user_command" }, "condition": { "kind": "always" } },
        { "capability": { "kind": "env_export" }, "condition": { "kind": "always" } }
      ],
      "fix_ops": ["command"],
      "trace": {
        "allowed_stages": ["selection", "assertion"],
        "required_stages": ["selection", "assertion"],
        "emits_preview_rows": false,
        "nested_evaluation": false
      },
      "...": "other KindMeta fields"
    }
  },
  "compatibility": {
    "config_versions": [1],
    "experimental_config_versions": [2],
    "kind_config_versions": { "constraint": 2 },
    "experimental_rule_kinds": ["constraint"],
    "policy_test_versions": [1]
  },
  "bundled_rulesets": ["apache/governance", "go", "rust", "..."],
  "output_formats": ["agent", "github", "gitlab", "human", "json", "junit", "markdown", "sarif"],
  "subcommands": ["check", "explain", "export-agents-md", "..."],
  "fact_predicates": ["all_files_exist", "any_file_exists", "count_files", "custom", "file_content_matches", "git_branch"]
}
```

Every list is sorted for deterministic output. The accepted-name `rule_kinds`
list retains aliases for backward compatibility, with the enforced invariant
`counts.rule_kinds + counts.rule_aliases == rule_kinds.len()`. The other
list-backed counts equal their list lengths; `auto_fix_ops` is count-only.
`kind_meta` is keyed by canonical kind only; consumers resolve an alias through
`rule_aliases` and then read the canonical record.

`facts.json` ships alongside the volatile build-time `manifest.json`. Format v4 added the
`kind_meta` inventory. Format v5 added `compatibility`, derived from the config and policy-test
runtime constants. Format v6 adds each canonical kind's allowed/required trace stages, preview
capability, and nested-evaluation capability to that same inventory. Format v7 adds
`compatibility.experimental_config_versions`, the versions a build loads but does not yet call
stable (`[2]` in v0.23); `config_versions` stays the stable list. It also adds
`compatibility.kind_config_versions`, the lowest config version of each kind that needs more than
version 1 (`{"constraint": 2}`), and `compatibility.experimental_rule_kinds`, the kinds only an
experimental config version can use; both were added before v0.23 shipped, so the format stays
v7. A kind in neither runs in every config version. The alint.org facts loader
ignores unknown keys; its supported-version checks apply to the separate bundle manifest.

## 3. Semantics

`gen-facts` derives every field from the same canonical source the README audit
uses, so `facts.json` can never disagree with the README:

| Field | Source of truth |
|---|---|
| `alint_version` | `env!("CARGO_PKG_VERSION")` (workspace version) |
| `rule_kinds` (count) | canonical entries in the generated category bridge |
| `rule_aliases` (count and map) | `ALIAS_TO_CANONICAL` in the generated category bridge |
| `kind_meta` | canonical `KindMeta` records in the built-in `RuleRegistry`; aliases resolve through `rule_aliases` |
| `compatibility.config_versions` | `Config::{MIN_SUPPORTED_VERSION,CURRENT_VERSION}` |
| `compatibility.experimental_config_versions` | `alint_dsl::EXPERIMENTAL_CONFIG_VERSIONS` |
| `compatibility.kind_config_versions` | `RuleRegistry::min_config_version` of each built-in kind, kept when above 1 |
| `compatibility.experimental_rule_kinds` | the kinds whose minimum config version is in `EXPERIMENTAL_CONFIG_VERSIONS` |
| `compatibility.policy_test_versions` | `alint_testkit::POLICY_TEST_VERSION` |
| `rule_kinds` (list) | accepted `kind:` values in `crates/alint-dsl/tests/fixtures/all_kinds.yaml` and `all_kinds_v2.yaml`, aliases included |
| `families` | non-meta `## ` headings in `docs/rules.md` |
| `bundled_rulesets` | `.yml` files (recursive) under `crates/alint-dsl/rulesets/<rev>/` |
| `output_formats` | variants of `enum Format` in `crates/alint-output/src/lib.rs`, lowercased |
| `subcommands` | `CLI_REFERENCE_SUBCMDS` (itself pinned to `enum Command` by a test) |
| `auto_fix_ops` (count) | `FixSpec::ALL_OP_NAMES` in `alint-core` |
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
tracked file at the repo root. Format v4 clones the canonical registry metadata
into the generated contract; aliases remain a separate alias-to-canonical map.
The original implementation left `manifest.json` untouched; manifest v4 later
added the same canonical-kind/alias split after the site accepted that version.

## 6. Tests

In `xtask` (`facts.rs` `#[cfg(test)]`):

- `gen_facts_check_passes_on_committed_tree` — `run(true)` succeeds on the
  committed file (idempotency / freshness; mirrors the schema test).
- `counts_equal_list_lengths_and_lists_are_sorted` — canonical kinds plus aliases
  equal the accepted-name list length; other list-backed counts equal their list
  lengths; lists are sorted and de-duplicated.
- `counts_match_readme_claims` — recompute each count the way
  `coverage_audit_readme_claims` does and assert equality, binding `facts.json`
  to the same truth as the README.
- `kind_meta_covers_canonical_kinds_with_structured_capabilities` — the map keys
  equal the canonical registry, aliases are not duplicated, and conditional
  capabilities, typed selectors, and fix-op lists retain their JSON structure.

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
