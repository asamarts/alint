---
status: proposed
date: 2026-06-21
decision-makers: alint maintainers
---

# 6. Baseline suppression via a committed fingerprint file

## Status

Proposed. (One of: Proposed | Accepted | Rejected | Deprecated | Superseded by ADR-NNNN.)

## Context

alint cannot be adopted as a blocking merge gate on a large existing codebase.
Turning on a ruleset surfaces hundreds of pre-existing violations, and the only
suppression levers (`level: off`, `when:` gates, `ignore:` globs) also suppress
future new violations of the same rule. An external adoption evaluation named
this the single biggest blocker to flipping alint on as a gate (design doc
`docs/design/baseline.md`, demand evidence section).

The existing `--changed` / `--base` flags are a partial workaround, but they are
a file-SCOPE filter: cross-file and existence rules consult the full tree by
definition, so legacy state still fails the build. A real grandfathering
mechanism has to suppress already-known violations everywhere, including from the
full-tree rules that `--changed` cannot scope.

Decision drivers: adoptability on legacy code; not masking genuinely new
violations; determinism and low diff churn (the constitution's anti-drift floor);
no new trust or code-execution surface (ADR-0004).

## Decision

We will add a baseline (grandfathering) mode built on a committed, machine-
generated fingerprint file, separate from `.alint.yml`:

- `alint baseline` writes `.alint-baseline.json` (default path): a
  `schema_version`-gated, deterministically sorted list of the current
  violations, each reduced to a fingerprint with an occurrence count.
- The baseline is applied only on **explicit opt-in**: a `baseline: <path>` key
  in `.alint.yml` (persistent) or a `--baseline <file>` flag (override). alint
  does NOT silently auto-detect a baseline file, so suppression is never active
  just because a file exists.
- `alint check --baseline <file>` suppresses every current violation whose
  fingerprint is present (up to its recorded count) and reports only new
  violations; suppressed findings do not count toward the exit code.
- The fingerprint is `rule_id + path + content`, hashed with SHA-256, and
  **excludes the line and column number**. For a line-anchored violation the
  content is the exact bytes of the offending line; for a file-level violation it
  is the path alone; for a repository-level (no-path / cross-file) violation it is
  a stable **structural key the rule supplies** (e.g. the sorted set of involved
  paths) via a new `Violation.baseline_key` field — not the message, so a
  rule-kind reword cannot re-baseline it. This re-triggers when the offending
  code is edited, survives unrelated line motion, and keeps duplicate occurrences
  honest via per-fingerprint counts.
- A stale entry (the fixed/edited case) warns and re-tightens but does not fail
  the build by default; `--strict-baseline` opts into failing on stale.
- Suppression is a deterministic, order-preserving post-evaluation transform on
  the `Report`, between `Engine::run` and the formatters (the same layer as the
  notes partition and the `--only` filter). It introduces no rule kind and no
  new dependency (`sha2` is already in the workspace).
- The baseline is local-only: it cannot be introduced through `extends:`, so a
  published ruleset can never ship a suppression, and its path is confined within
  the repository root like every other read path.

`--baseline` complements `--changed`; the two compose (scope filter plus
suppression filter) and are documented as orthogonal.

## Consequences

Easier: a team can flip alint on as a blocking gate on a legacy repo in one step
(`alint baseline`, commit, gate on new violations), which is the primary
adoption path the tool lacked. Fingerprints survive benign refactors, so the
baseline does not churn on every edit.

Harder / costs: a new committed artifact that must be reviewed in PRs and can, if
abused, mask a real finding (mitigated by stale-entry warnings, `--show-baselined`,
and PR review). Every no-path-emitting cross-file / repo-level rule kind must
supply a structural `baseline_key` before this ships (the chief rule-side cost,
chosen over message-keying to make repo-level fingerprints reword-proof). File
renames re-trigger the moved file's baselined violations (path is part of the
identity). These are accepted for v1 and tracked in the design doc.

## Considered Options

- **Config-embedded suppressions** (a `suppress:` block in `.alint.yml`): rejected
  because it conflates hand-edited policy with a machine-regenerated debt
  snapshot, and would tempt per-line suppression entries that drift.
- **Count-only baseline keyed on `(rule, file)`** (RuboCop / ESLint model):
  simpler, but cannot distinguish *which* occurrence, so editing a baselined line
  would not re-trigger and a new occurrence could be silently absorbed up to the
  count. We adopt the count model *per content-fingerprint* instead to keep
  precision and duplicate tolerance.
- **Message-keyed repo-level fingerprints** (the simpler no-path option): rejected
  in favour of a rule-supplied structural key, because hashing the message text
  re-baselines a cross-file violation whenever its message is reworded. The
  structural key costs a per-kind hook but makes repo-level entries reword-proof.
- **Silent auto-detect of `.alint-baseline.json`**: rejected as a default —
  suppression must be an explicit, reviewable opt-in (config key or flag), never
  triggered by a file's mere presence.
- **Line-number-keyed fingerprints**: rejected because every insert above an
  entry would churn the baseline.
- **Diff-only `--new-from-rev`** (golangci-lint model): this is essentially the
  existing `--changed`, and cannot grandfather full-tree (cross-file / existence)
  rules. Baseline is a strict superset of that capability.

## More Information

Design doc: `docs/design/baseline.md` (the full surface area, fingerprint scheme,
false-positive analysis, test plan, and open questions). Related: ADR-0004
(extends trust boundary and path confinement, which the baseline file stays
inside) and ADR-0003 (rule-engine dispatch and determinism, which the
suppression transform upholds). Implementation PR(s): to follow.
