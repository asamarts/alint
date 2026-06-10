# alint constitution

Status: living document. Last reviewed 2026-06-10.

These are alint's load-bearing invariants: the properties that must hold on every change,
stated once so they stop being re-derived each cycle. Each invariant names its enforcing
mechanism where one exists. Changing an invariant requires an ADR (see `../adr/`), not just
a code edit.

This document is prose scaffolding (it is allowed to lag reality and is corrected on review);
the enforcing tests and gates are the actual source of truth. Where they disagree, the test
wins and this document is wrong.

## Correctness and behavior

1. Determinism. Output (report, violations, fixes) is byte-identical across runs on the same
   input. No reliance on HashMap iteration order or filesystem readdir order. Enforced by the
   post-walk sort and per-rule output sorting; see ADR-0003.
2. Cross-file dispatch. A rule with `requires_full_index() == true` must have
   `path_scope() == None`. Enforced by `v010_cross_file_kinds_require_full_index_and_no_path_scope`
   and by build-time rejection of `scope_filter:` on cross-file rules. See ADR-0003.
3. Bounded analysis reads. Whole-file analysis reads (not fixers) go through
   `crate::io::read_capped` (256 MiB `MAX_ANALYZE_BYTES`); over-cap files produce a clear
   violation, never a silent skip or OOM.
4. Bounded fixes. Content-editing fixers skip files over `fix_size_limit` (default 1 MiB) with
   a Skipped status; path-only operations bypass the cap.

## Security

5. Extends trust boundary. Every process-spawning rule kind is a member of `SPAWNING_RULE_KINDS`
   and is honored only in the user's own top-level config. Custom process-spawning facts and the
   `allow_out_of_root:` escape hatch are likewise top-level-only. See ADR-0004.
6. Path confinement. File access is confined to the repository root by default; the walker prunes
   escaping symlinks; git-backed rules sanitize range arguments. See ADR-0004.

## Coverage and consistency (the anti-drift floor)

7. Every registered rule kind has a schema dispatch entry. Enforced by `coverage_audit_schema_drift`.
8. Every registered rule kind has at least one firing scenario and one silent scenario. Enforced by
   `coverage_audit_pass_fail`.
9. Numeric claims in the README and the about page are derived from code and fixtures, not hand-typed.
   Enforced by `coverage_audit_readme_claims`.
10. User-facing install-snippet version pins match `[workspace.package].version`. Enforced by
    `check-version-pins.sh` (surfaced in dogfood as `install-snippets-match-workspace-version`).
11. Generated artifacts are committed and gated: regenerate, then fail on any diff. New generated
    artifacts use `generated_file_fresh` in `.alint.yml` where practical. (Introduced by ADR-0001;
    being rolled out across the schema, reference docs, and `facts.json`.)

## Process

12. Design-doc-first. A behavior-bearing rule kind ships with a design doc under
    `docs/design/vX.Y/` following `docs/design/TEMPLATE.md`, drafted before implementation.
13. Architectural decisions are recorded as ADRs under `docs/adr/` (MADR 4.0.0), linted by alint's
    own `docs/adr@v1` ruleset. ADRs are immutable; supersede rather than rewrite.
14. Top-level user-facing docs contain no em dashes or smart quotes. Enforced by the dogfood rules
    `top-level-docs-no-em-dash` and `top-level-docs-no-smart-quotes`.
