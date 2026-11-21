---
status: accepted
date: 2026-11-15
decision-makers: asamarts
---

# 0040. The v0.23 security advisory, and the YAML allowlist approved on ayaml's fixes

## Status

Accepted (2026-11-15). Settles the security-advisory decision that phase 10 of the
[v0.23 plan](../design/v0.23/README.md) names for item 13, and records phase 9's approval under
[ADR-0035](0035-keep-the-early-ayaml-cutover.md) decision 2, with a condition.
[ADR-0043](0043-trust-and-disclosure-decisions-from-the-post-v0221-audit.md) (2026-11-16) widens
decision 1 to the holes the post-v0.22.1 audit found, and corrects its count: #527 fixed eight
holes, not the three named here. It also widens decision 2's condition to an ayaml release with
the fixes of every gap alint filed. On 2026-11-20, `ayaml` 0.2.2 (#605) met decision 2's
condition by fixing every ayaml defect and removing all four defect entries from the allowlist;
on 2026-11-21, the owner approved the four non-defect entries added after this ADR in #530
(`empty-flow-implicit-key`, `negative-zero-in-an-unsigned-field`, `rule-that-is-null`, and
location class `verbatim-tag-that-names-nothing`), completing phase 9 sign-off for the 34
deviations and 8 location classes shipped in v0.23.0.

## Context

- **Four fixes in `alint test`.** #515, #519 and #522 closed four ways for a policy-test package
  the user does not trust to act without its capability. It could run a command, through a
  `command` rule nested in `require:`, a `given.env` that reached alint's own Git, or a fixture
  shaped like a bare repository. Or it could read the host's environment, through `when:` and
  `${VAR}`. Each affects v0.20.0 to v0.22.1, and the CHANGELOG on `main` describes them.
- **Three more, found by the review of #522 and its fix.** None is public yet.
  - A chain of symlinks in `given.tree` passes the sandbox check, so `alint test` writes files
    outside its sandbox before any capability check. A planted `git` then runs.
  - A `given.fixture` symlink is checked where it resolves in the fixture, but resolves from the
    copy, so `expect_tree` can read a host file.
  - A fixer can write a `.git` file or directory, and Git then runs a command from a
    configuration the fixer wrote. In `alint test` this needs no capability. In `alint fix`
    since v0.2.1, a ruleset that `extends:` brings in from the local tree gets past the trust
    gate that stops it from spawning.
- **What SECURITY.md says.** Only the latest released minor is supported, and fixes ship in a new
  release. v0.23.0 is phases 6 to 10 away.
- **The harness's verdict.** The differential harness (#482, #521, #525) documents 33 deviations
  and 7 location classes between ayaml 0.1.2 and `yaml_serde` 0.10.7, the parser v0.22 used.
  None of the 905 YAML files of the corpus differs, and 13.4 million generated and mutated inputs
  left no unexplained difference. Three entries are ayaml defects, each in a malformed document
  only: asamarts/ayaml#132 (`block-scalar-under-an-anchor`), asamarts/ayaml#133
  (`ayaml-error-without-a-place`) and asamarts/ayaml#137, which `byte-order-mark-starting-a-line`
  documents as hiding a missed error.

## Decision

1. **No v0.22.x patch release.** A GitHub security advisory is drafted now and published with
   v0.23.0, which it names as the first patched version.
   - It covers all seven holes. The owner weighed a private fix and a v0.22.2 for the three found
     later, and chose to fix them on `main` in public pull requests like the first four.
   - Until v0.23.0, the CHANGELOG's advice stands: run `alint test` only on policy-test packages
     whose configurations and `given:` blocks you trust, and `alint fix` only with rulesets you
     trust, including those `extends:` reads from the local tree.
2. **The owner approves the allowlist, on one condition.** v0.23.0 is tagged only on an ayaml
   release that fixes asamarts/ayaml#132, #133 and #137.
   - That bump removes `block-scalar-under-an-anchor` and `ayaml-error-without-a-place`, and
     `byte-order-mark-starting-a-line` no longer lists an error it hides.
   - The bump follows the ayaml-bump procedure of
     [`yaml-parser.md`](../design/yaml-parser.md): the harness's long runs, the depth guard's
     parity runs and the allowlist's examples, on the new release.
   - A difference the bump adds comes back to the owner before the tag.

## Consequences

- The v0.23.0 tag waits on an ayaml release. ayaml's maintainers schedule the three fixes.
- Users of v0.20 to v0.22 learn of the fixes from the advisory only when v0.23.0 ships. In
  between, only the CHANGELOG on `main` describes them, and the pull request that fixes the
  three later holes discloses them before any release is patched.
- Phase 9 is met once the condition is: the cutover is qualified, and #510 stays.

## Considered Options

- **The advisory:** a v0.22.2 with the four fixes and an advisory now; an advisory now that names
  0.23.0 as patched; no advisory. The owner chose to publish it with v0.23.0.
- **The later holes:** fix them privately in the advisory's temporary fork and release
  v0.22.2 with every fix; fix them in public and release v0.22.2 soon after; keep them private
  until v0.23.0. The owner chose public fixes, with the advisory at v0.23.0.
- **The allowlist:** approve it as is; approve it but keep v0.22's reading of the seven forms
  v0.23 rejects, with workarounds in alint; withhold approval, which under ADR-0035 holds the
  release or reverts #510. The owner chose to approve it once ayaml fixes its three defects.

## More Information

- [ADR-0035](0035-keep-the-early-ayaml-cutover.md): the qualification this approval completes.
- [ADR-0041](0041-a-yaml-parser-facade-with-test-only-reference-parsers.md): the parser facade
  whose compare mode qualifies the ayaml bump across alint's test suites.
- The fixes: #515, #519, #522 and #527. The advisory: GHSA-78jj-xfpm-g6px, a private draft. The
  harness: #482, #521, #525. The depth guard: #483, #524.
