---
status: accepted
date: 2026-10-25
decision-makers: asamarts
---

# 0024. Relation IR decisions after review

## Status

Accepted (2026-10-25). Supersedes [ADR-0023](0023-relation-ir-key-parity-and-plugin-contract-0x.md).
Decision 6 relaxes how ADR-0019 DEC-13's budget is measured, and the decision-maker confirmed it;
the other decisions correct or complete ADR-0023 without changing what was chosen.
Decision 4's single rendering of every key is partly superseded by
[ADR-0025](0025-v021-pre-release-decisions.md) (2026-10-30).

## Context

ADR-0023 recorded the eight v0.21 decisions on 2026-10-24. Three independent reviews of the
design and of the first work items (R0, the v0.18.2 reference gate; R5, corpus replay) then found
that parts of that record cannot work as written or leave out what an implementation must decide:
WIT has no recursive types; S4 cannot run on v0.18.2; `key_parity` would flag every Rails locale
file; a published crate cannot depend on an unpublished one; the relation module cannot be
`pub(crate)` while `alint-rules` builds on it; byte offsets are not known for every row; and the
replayed corpus contains rules that run commands. The ADR template keeps accepted records
immutable, so the corrections are recorded here instead of being written into ADR-0023.

## Decision

1. **Value types (V21-1, Q4).** Rows carry `null`, boolean, a number that keeps its source
   lexeme, string, bytes, a normalized repository path, lists, and records; `semver` and SPDX
   wait for several consumers. Across the plugin contract a value is a flat node list (node 0 is
   the root, children come after their parent, each non-root node has one parent), because WIT
   has no recursive types; a node list from outside the engine is validated, never repaired. The
   contract form is a std-only type in `alint_core::plugin_api`, and `alint_core::relation`
   converts to and from it. A new value variant is additive inside alint but a new contract
   minor version, since a new variant case breaks a WIT contract.
2. **Paths (V21-2, Q13).** Paths in relations are root-relative, `/`-separated, with `.`, `..`,
   and empty segments resolved, compared byte for byte on every operating system; case folding
   is an explicit opt-in. A path parsed from file content splits on `/` only. Relations order
   paths segment by segment, the walker's index order.
3. **`key_parity` formats (V21-3).** JSON, YAML, dotenv, INI, Java properties, TOML (a new
   `toml_edit` row adapter), Android string resources (keyed by element type and `name`,
   `translatable="false"` skipped), and .NET `.resx` (string `<data>` entries only). The dialects
   are `key_parity`'s own format names, not `Format` variants, and its `KindMeta` lists only the
   general formats it reads. Files are compared only within one key model: the tree formats
   (JSON, YAML, TOML) together, the flat formats (dotenv, properties) together, and INI and each
   XML dialect on their own.
4. **Key model (V21-4).** A key is a leaf path identified by its segment list; arrays are values.
   One rendering serves messages, `ignore_keys`, and baseline keys: segments joined by `.`,
   `[n]` for indices, and bracket notation with `\` and `'` escaped for a key that is empty or
   contains `.`, `[`, `]`, or a quote. `strip_root` compares below a single top-level key (Rails'
   `en:`). YAML aliases and merge keys are an analysis failure. `plurals: cldr` (opt-in) counts a
   CLDR plural group as one key.
5. **Plugin contract (V21-5, Q10).** The contract types and the `Adapter` and `Analyzer` contract
   traits live in a doc-hidden `alint_core::plugin_api` module that may import only `std`,
   `core`, `alloc`, and itself; a test enforces that. The engine keeps one internal trait set,
   `alint_core::relation::Adapter` and its kin, which the built-in formats implement; contract
   adapters reach the engine through a bridge to that trait set (R6). Not a separate crate in
   v0.21: one that the published `alint-core` depends on would have to be published too. The
   WIT draft has `adapter` and `analyzer` worlds (`fixer` follows with P2), and the traits are not
   deferrable to v0.22.
6. **Performance references (V21-6, DEC-13), as measured.** v0.18.2 is the reference for the
   deterministic scenarios S1-S3 (1k and 10k files): at most +3% instructions per cell, and no
   read/stat-family syscall growth with the tree, allowing 32 extra calls per family at 10k files
   for fixed per-run probes. The v0.18.2 binary is rebuilt in the gate's job and measured by the
   current harness, so no committed number can drift. S4 is not compared, because it extends
   bundles that arrived in v0.19; it stays on the release-time bench gates. The gate runs on
   relation-layer pull requests and on every Rust push to main (the comparison is absolute, so
   whatever crossed the budget is caught where it landed). The release check measures S1-S3 x
   {full, changed} wall-clock (+3%; 30 runs at 100k, bench-scale's three runs at 1M, since 30
   would take about 16 hours for S3 alone) and 1M peak RSS (+10%). `docs/benchmarks/ANCHOR`
   (v0.20.0, +10%) still serves every release's bench gates.
7. **Corpus replay (V21-7).** On the bench host's runner, weekly and before releases with
   relation-layer changes. Nothing from a corpus repository runs: spawning, keyring, and
   catalog-unknown rule kinds and spawning fix operations are removed, configs with custom facts
   are refused, and alint gets a fresh `HOME` and an absolute-only `PATH`. Comparisons cover the
   JSON report with proposed edits and the baseline fingerprints. A committed expectations file
   lists the configs that do not replay; any other skip or fetch failure fails the run.
8. **Shadow scope (V21-8).** The `shadow-ir` feature is defined on `alint-rules` and enabled only
   from `alint-e2e`; corpus replay builds its own binary with the feature. No benchmark or release
   build enables it, and CI checks the release and bench build feature trees.
9. **Resources and row order.** A resource is a confined repository path with an origin: an
   index entry, or a literal path read from disk (as `cross_file` reads its source and target
   files, even ignored ones), with a resource-level missing state. Rows sort by path (segment
   order), structural pointer, label, and emission order, with the byte offset only as a
   tie-breaker where known. The `equals` port emits findings in the existing implementation's
   order, so the shadow comparison stays byte-identical.
10. **Work-item order.** R2 is proven with a test-only relation consumer; the relation scenario
    and the cross-relation read count move to R3, which has `key_parity`. R6 splits into R6a
    (contract traits, the bridge, registry injection; after R2) and R6b (`key_parity` through the
    traits; after R3).

The work items, invariants, and acceptance tests are normative in
[`docs/design/v0.21/`](../design/v0.21/README.md).

## Consequences

- Each correction above is now a recorded decision instead of an edit to an accepted ADR.
- DEC-13's intent holds, but its measurement differs from `general-linter.md` section 9.6 in
  three places (S1-S3 only, a fixed syscall allowance, three runs at 1M); section 9.6 points here.
- `key_parity` refuses comparisons whose key models cannot match, rather than reporting every
  key as both missing and extra.
- The R1 types are public but doc-hidden, so `alint-rules` can build on them without a semver
  promise.

## Considered Options

- **Edit ADR-0023 in place:** rejected; the template keeps accepted ADRs immutable.
- **Keep DEC-13's literal measurement (S1-S4, no syscall allowance, 30 runs at 1M):** rejected
  because S4 cannot load on v0.18.2, two identical runs differ by a syscall or two, and 30 runs at
  1M would take about 16 hours for S3 alone.

## More Information

- [ADR-0023](0023-relation-ir-key-parity-and-plugin-contract-0x.md): the original record.
- [`v0.21/README.md`](../design/v0.21/README.md): the normative design.
- [`docs/benchmarks/REFERENCE-v0.18.2.md`](../benchmarks/REFERENCE-v0.18.2.md): the R0 measurement.
