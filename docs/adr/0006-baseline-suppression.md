---
status: accepted
date: 2026-06-21
decision-makers: alint maintainers
---

# 6. Baseline suppression via a committed fingerprint file

## Status

Accepted (2026-06-21). The design doc (`docs/design/baseline.md`) stays Draft
until the implementation lands; this records that the decision itself is settled.

**Amended (2026-06-22, design v3 — implementation-time audit).** Running the
fingerprint against the whole firing corpus showed the discriminator should
**default to `(rule_id, path)`** for a path-bearing violation (the message is *not*
hashed), rather than demanding a `baseline_key` on every path-only rule. This
fixes threshold churn for free and narrows the explicit-key requirement to the
~15 kinds whose identity genuinely isn't `(rule_id, path)`. The Decision and
Consequences below are updated in place; see the design doc's v3 changelog.

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
- The fingerprint is `SHA-256(rule_id ‖ path ‖ discriminator)` with a
  length-prefixed join (so component tuples can't collide), **excluding the line
  and column number and the level**. The discriminator is, in order: a
  rule-supplied `Violation.baseline_key` if set; else, for a line-anchored
  violation, the offending line's content with its trailing `\r?\n` stripped (so
  line motion and CRLF↔LF conversion don't churn); else, for a path-bearing
  violation, **empty — so its identity is `(rule_id, path)` and the volatile
  message is not hashed** (v3); else (no path, no line) a last-resort anti-panic
  message. A rule sets `baseline_key` **only** when its identity isn't
  `(rule_id, path)`: structured-query (the JSONPath/value — many findings per
  file), cross-file/no-path (the sorted involved-path set), first-offender line
  rules (the path), and line-collision (the per-finding target) — enforced by a
  coverage-audit collision-invariant. Threshold rules (`file_max_lines`, …) need
  **no** key: the empty default keeps them stable as the magnitude grows. This
  re-triggers on edit, survives benign motion, keeps duplicates honest via
  per-fingerprint counts, and is reword-proof. (v1 keyed only no-path rules; v2
  over-corrected to "key every non-line-content rule"; the v3 audit showed the
  `(rule_id, path)` default is both safe and far smaller.)
- This is *intended* as **one** fingerprint definition for the whole tool:
  SARIF gains it as `partialFingerprints` (shipped). Migrating the existing
  `gitlab.rs` cross-run-dedup fingerprint onto it is **deferred** — it needs
  report-fingerprint plumbing (see `docs/design/baseline.md` §5/§7). Until
  then `gitlab.rs` keeps its own `rule_id|path|message|occurrence` fingerprint
  (the `occurrence` discriminator added in the post-v0.13 audit guarantees the
  per-report uniqueness the Code Climate spec requires).
- A stale entry (the fixed/edited case) warns and re-tightens but does not fail
  the build by default; `--strict-baseline` opts into failing on stale.
- Suppression **marks rather than removes**: a deterministic, order-preserving
  post-evaluation transform (between `Engine::run` and the formatters, the layer
  of the notes partition / `--only` filter) partitions suppressed violations into
  a parallel `RuleResult.suppressed`. Most formatters omit them; **SARIF emits
  them with `result.suppressions` / `baselineState: unchanged`** so GitHub Code
  Scanning dismisses (not closes-then-reopens) the alert. No rule kind, no new
  dependency (`sha2` is already in the workspace).
- The baseline is a **trusted top-level input**, like `-c`: honoured only from
  the user's own top-level config or the CLI, and **never from `extends:` or a
  nested subtree config** (a non-default `baseline:` reaching the loader from an
  inherited/nested config is rejected, mirroring `allow_out_of_root`). So a
  published or subtree ruleset can never ship a suppression. (It is not subjected
  to read-path root-confinement, because the config path it parallels isn't
  either; the trust boundary is `extends:`/nesting, not the filesystem root.)

`--baseline` complements `--changed`; the two compose (scope filter plus
suppression filter) and are documented as orthogonal.

## Consequences

Easier: a team can flip alint on as a blocking gate on a legacy repo in one step
(`alint baseline`, commit, gate on new violations), which is the primary
adoption path the tool lacked. Fingerprints survive benign refactors, so the
baseline does not churn on every edit.

Harder / costs: a new committed artifact, reviewed in PRs (made reviewable by an
advisory, non-matching `message` per entry) and capable, if abused, of masking a
real finding (mitigated by stale warnings, `--show-baselined`, and review). The
chief rule-side cost is giving a `baseline_key` to the ~15 kinds whose identity
isn't `(rule_id, path)` — structured-query, cross-file/no-path, first-offender,
and line-collision — gated by the coverage-audit collision-invariant.
Marking-not-removing means each formatter handles suppression explicitly (SARIF in
particular). Two footguns are designed out: `alint baseline` rejects `--changed`
(a changed-scope baseline would be silently partial) and refuses to grandfather
*new* debt on regeneration without `--accept-new`. Accepted limitations for v1
(design doc §7): file renames re-fire baselined violations; threshold rules don't
ratchet (a baselined over-limit file may grow further); level escalation is not
re-surfaced on grandfathered code; per-subtree baselines under `nested_configs`
are not supported.

## Considered Options

- **Config-embedded suppressions** (a `suppress:` block in `.alint.yml`): rejected
  because it conflates hand-edited policy with a machine-regenerated debt
  snapshot, and would tempt per-line suppression entries that drift.
- **Count-only baseline keyed on `(rule, file)`** (RuboCop / ESLint model):
  simpler, but cannot distinguish *which* occurrence, so editing a baselined line
  would not re-trigger and a new occurrence could be silently absorbed up to the
  count. We adopt the count model *per content-fingerprint* instead to keep
  precision and duplicate tolerance.
- **Message-keyed fingerprints** (the simplest discriminator): rejected as the
  primary key because a reworded rule message re-baselines its violations. Kept
  only as a never-load-bearing last-resort fallback; the rule-supplied
  `baseline_key` is the real discriminator.
- **Shape-inferred discriminator only** (line-content for line rules, path for
  file-level, no per-rule key) — the v1 design: rejected *as the sole mechanism*
  because some rules break the shape assumption (`structured_path` emits many
  path-only violations per file distinguished only by the query), silently
  suppressing a different new finding. v3's resolution is a **hybrid**: shape
  inference is the *default* (`(rule_id, path)` for path-bearing violations — which
  correctly handles `file_max_lines` and the bulk of path-only rules), with a
  per-violation `baseline_key` overriding it for the ~15 kinds that genuinely
  emit multiple findings per path or have none. Pure shape-inference was too
  little; v2's "key everything non-line-content" was too much.
- **Remove (not mark) suppressed violations** before formatting — the v1 design:
  rejected because for SARIF it makes GitHub Code Scanning close the alert and
  reopen it when the finding resurfaces (flapping). Marking with
  `result.suppressions`/`baselineState` keeps the alert dismissed-not-fixed.
- **Truncated fingerprints** (for a smaller file): rejected — a fingerprint
  collision is a *silent mis-suppression*; the full 64-hex SHA-256 is kept and
  human-readability comes from the advisory `message` field instead.
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
false-positive analysis, per-format suppression behaviour, test plan, and open
questions; the v2 revision followed an adversarial review). Related: ADR-0004
(the `extends:` trust boundary this reuses to keep the baseline local-only) and
ADR-0003 (rule-engine dispatch and determinism, which the suppression transform
upholds). Implementation PR(s): to follow.
