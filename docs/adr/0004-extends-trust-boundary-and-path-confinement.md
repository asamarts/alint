---
status: accepted
date: 2026-06-10
decision-makers: asamarts
---

# 4. Extends trust boundary and path confinement

## Status

Accepted. Backfilled on 2026-06-10; the decision and its hardening cycle predate this record
(see ADR-0001). The path-confinement portion was strengthened in the v0.12 security cycle.

## Context

`extends:` lets a config pull rulesets from local paths, HTTPS URLs, and bundled sources. A
fetched or shared ruleset is, in effect, third-party code. Without a boundary, an `extends`'d
ruleset could run arbitrary commands or read files outside the repository, turning "reuse a
governance ruleset" into a remote code execution and exfiltration risk.

## Decision

We will treat only the user's own top-level config as trusted for privileged operations, and
confine all file access to the repository root by default.

- Process-spawning rule kinds (`SPAWNING_RULE_KINDS`: `command`, `generated_file_fresh`,
  `command_idempotent`) are honored only in the top-level config and rejected in any
  `extends`'d ruleset (`reject_command_rules_in`).
- Custom facts that spawn a process, and the `allow_out_of_root:` escape hatch, are likewise
  top-level-only (`reject_allow_out_of_root_in`).
- File access is confined to the repo root by default; the walker prunes symlinks that escape
  the root, and git-backed rules sanitize range arguments (the `since:` argument-injection fix).

Whenever a new rule kind can spawn a process or read outside the root, it must be added to the
appropriate gate. This is a release-blocking invariant, not a convention.

## Consequences

Positive: detection-only rulesets can be shared and extended safely; the blast radius of a
malicious or careless ruleset is bounded.

Negative and accepted: legitimate uses of spawning kinds or out-of-root reads must be declared
in the user's own config, which is slightly less ergonomic for shared rulesets, but the safety
property is worth it. Every new spawning-capable kind adds a gate-maintenance obligation, which
the constitution and a regression test enforce.

## More Information

- `docs/design/ARCHITECTURE.md` (trust model).
- `docs/design/constitution.md` (the spawning-kind and path-confinement invariants).
- The v0.12 CHANGELOG security entries (allow_out_of_root, walker symlink pruning, git `since:` fix).
