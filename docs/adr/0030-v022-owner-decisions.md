---
status: accepted
date: 2026-11-07
decision-makers: asamarts
---

# 0030. v0.22 owner decisions: hidden-path fingerprints, sealed structs, and order of work

## Status

Accepted (2026-11-07). Settles the two calls v0.21 left for v0.22 (the fingerprint of a finding
whose path alint hides, and public structs that break downstream code when they gain a field) and
the order of v0.22's work. The relation-layer gates of
[ADR-0025](0025-v021-pre-release-decisions.md) decision 3, as
[ADR-0026](0026-measuring-the-v022-simplicity-gate.md) measures them, are unchanged.

## Context

- **Hidden-path fingerprints.** A finding in a file chosen by a field read from the environment
  (`paths: "{{env.X}}"`, an env-read `scope_filter:`, selector, `require:` or `when_iter:`) shows
  its path as `<redacted>`. Its fingerprint in the baseline file, SARIF `partialFingerprints` and
  GitLab output is still `sha256(rule, path, "sensitive")`, unsalted, so whoever holds one of those
  files can confirm a guessed path. Baseline matching already counts duplicates: several findings
  with one fingerprint are grandfathered up to the recorded `count`. Changing the hash makes every
  existing baseline entry for such a finding resurface as new. v0.21.2 documented the residual
  (`configuration/variable-interpolation.md`) and left the change to v0.22.
- **Public structs.** v0.21.1 added public fields to `FixContext`, `Config` and
  `CompiledNestedSpec`. Code outside alint that builds one with a struct literal stopped
  compiling, which is why patch releases now run `ci/scripts/check-patch-compat.sh`. No public
  struct in `alint-core` is `#[non_exhaustive]`; `Violation`, `Context`, `RuleSpec` and
  `WalkOptions` have the same exposure.
- **Order of work.** Gate 3 requires zero differences against v0.21 on the e2e suite and the
  corpus replay. The fingerprint change alters baseline fingerprints, which the replay compares,
  and one corpus configuration (facebook-react's `changed_since: "{{env.GITHUB_BASE_REF ...}}"`)
  chooses files from the environment. Done together with the port, the gate could not tell a
  port regression from an intended change.

## Decision

1. **A hidden-path finding's fingerprint leaves the path out.** It becomes
   `sha256(rule, "hidden-path")`, plus the existing constant discriminator, so the baseline file,
   SARIF and GitLab output no longer say anything about the path. Findings of one rule with
   hidden paths share the fingerprint and are matched by count.
   - **Compatibility for one minor release.** In v0.22, `check --baseline`, `fix --baseline` and
     the language server also accept the v0.21 fingerprint for such a finding, so existing
     baselines keep matching. `alint baseline` writes only the new form. v0.23 removes the old
     form, and its CHANGELOG says to re-run `alint baseline` first.
   - SARIF `partialFingerprints` and GitLab `fingerprint` carry the new form from v0.22.
2. **`FixContext`, `Config` and `CompiledNestedSpec` become `#[non_exhaustive]`, with
   constructors.** Each gets a constructor for its required parts and setters for the optional
   ones; fields stay readable. Code inside the workspace uses the constructors. A later field is
   then additive. Sealing the other public `alint-core` structs (`Violation`, `Context`,
   `RuleSpec`, `WalkOptions`) is a pre-v1.0 roadmap item, done before the API freeze rather than in
   v0.22.
3. **The port comes first.** The relation-layer port lands and passes gate 3 against v0.21 before
   decisions 1 and 2 and the dry-run follow-ups change behavior or API. Each of those then has its
   own e2e expectations, and the release's replay difference list contains only the
   fingerprint change.
4. **Also in v0.22:** `fix --dry-run` predicts a create through a dangling symlink whose target the
   create's own directory creation makes (the one mismatch v0.21.2's OS differential test found),
   and the landing checks that v0.21.2 added to each predicted create share their lookups within a
   run. The npm package's move to per-platform binaries follows v0.22.0 as its own release.

## Consequences

- The residual documented in v0.21.2 closes for every new baseline, SARIF and GitLab report.
  Existing baseline files still hold v0.21 fingerprints until regenerated; v0.22's CHANGELOG
  says so.
- Within one rule, fixing one hidden-path finding while another appears elsewhere keeps the count
  level, so `check --baseline` stays green. That is the price of keeping the path out: alint cannot
  tell the two apart without hashing the path.
- GitHub code scanning groups results by fingerprint, so a rule's hidden-path results may
  collapse into one alert. Their locations are redacted already, so the alert could not point at
  a file anyway.
- Code that builds the three structs with literals moves to the constructors once, in v0.22.0.
  New fields stop requiring a minor release or a patch-compatibility shim.
- The relation-layer gates are measured on a tree whose only change is the port.

## Considered Options

- **Fingerprint:** a path-free hash without the compatibility window (simpler, but every existing
  baseline entry for such a finding resurfaces once); a keyed hash with a user-supplied secret
  (keeps per-path identity, but every CI job that uses a baseline needs the secret, and a new
  configuration surface); keeping the fingerprint and the documented residual.
- **Structs:** sealing every public `alint-core` struct now (the most future-proof, but `Violation`
  alone is built in hundreds of places); constructors without `#[non_exhaustive]` (does not
  remove the hazard); no change, relying on `check-patch-compat.sh`.
- **Order:** the port and the behavior changes in one stream (rejected: gate 3 would mix
  regressions with intended changes).

## More Information

- [`v0.22/README.md`](../design/v0.22/README.md): the v0.22 plan these decisions schedule.
- [ADR-0006](0006-baseline-suppression.md): baseline matching by fingerprint and count.
- [ADR-0025](0025-v021-pre-release-decisions.md) and
  [ADR-0026](0026-measuring-the-v022-simplicity-gate.md): the relation-layer gates.
