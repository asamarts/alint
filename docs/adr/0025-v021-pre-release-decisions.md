---
status: accepted
date: 2026-10-30
decision-makers: asamarts
---

# 0025. v0.21 pre-release decisions

## Status

Accepted (2026-10-30). Supersedes the part of [ADR-0024](0024-relation-ir-decisions-after-review.md)
decision 4 that gives every key one rendering for messages, `ignore_keys`, and baseline keys;
the rest of ADR-0024 stands.
Decision 3's gate 2 is measured as [ADR-0026](0026-measuring-the-v022-simplicity-gate.md)
defines (2026-10-30).

## Context

The pre-tag audit of v0.21 ([`pre-release-audit.md`](../design/v0.21/pre-release-audit.md))
found that `key_parity` missed dotted keys, empty peers, and gitignored literal peers, that the
release acceptance had no written go/no-go judgment of the port, and that the WIT contract had
no automated check. Fixing those raised four calls that change what ADR-0024 and the v0.21
design recorded:

- ADR-0024 decision 4 renders every key the same way, so a properties key `app.title` appears as
  `['app.title']` in findings, in baseline keys, and in what `ignore_keys` must match. Nearly
  every Java properties key contains a dot, and tools that check these formats (IntelliJ's
  resource-bundle inspection, Android lint, ResXManager, dotenv-linter) show the key as written.
  Baseline keys fix the form at the tag.
- `peers: .env` with `reference: .env.example` is the design's own example, and a fresh CI
  clone has no `.env`, so the fail-loud rule (invariant 2) fails that CI run.
- The stop rule (`general-linter.md` section 11.1) asks whether the ported kind became "simpler
  and more consistent". Consistency is shown: zero shadow differences on every scenario, the
  property generator, and 106 of 110 corpus configs, with one read and decode cache and one
  extract adapter shared with `key_parity`. Simplicity cannot be shown in v0.21: production
  `cross_file` `equals` still runs the old code, and the shadow (359 non-comment lines) is larger
  than the code it shadows and still calls into it.
- witforge, which checks and generates from the WIT contract, is private and unreleased, so the
  regeneration check runs only by hand.

## Decision

1. **Single-name keys are shown as written.** Key identity is unchanged: a key is its segment
   list, and the comparison uses the injective rendering. Findings, baseline keys, and traces
   show a dotenv, properties, or `.resx` key as its name (`app.title`, `button1.Text`); a name
   that is empty or starts with `[` keeps the bracket form, so the display stays injective. Every
   other key keeps ADR-0024's path rendering (`nav.home`, `nav['a.b']`,
   `Session['session.save_path']`). `ignore_keys` matches the shown form: in a single-name format
   a pattern is a glob over the name, where `*` stops at a `.` and `**` crosses it; in every
   other format a pattern is a key path matched segment by segment, where `*` matches within one
   segment and a `**` segment matches any number of segments (at the end of a pattern, at least
   one). A malformed path pattern is a configuration error. A rule never mixes key models, so a
   user sees one form per rule.
2. **An absent peer may be allowed, explicitly.** `allow_missing_peers: true` turns an absent
   literal peer, or a `peers` glob that selects no file, into an informational note. It defaults
   to false, so a typo in `peers` still fails, and a missing reference always fails. The option
   mirrors `cross_file`'s `allow_missing_target`. Two related refinements hold with or without
   it: a literal peer whose extension implies another key model than the reference is a
   configuration error when the configuration loads, and a rule with a literal peer runs under
   `--changed` whenever anything changed, because Git does not report edits to an ignored file.
3. **Phase 2 verdict: GO on consistency, with simplicity gated in v0.22.** v0.21 ships. Phase 3
   starts only after v0.22 meets all three gates; if any is missed, the stop rule applies and
   Phase 3 does not start:
   1. the IR path becomes the authoritative implementation of `cross_file` `equals`, and the old
      `equals` code is deleted;
   2. the code that implements `equals` afterwards is no larger than the authoritative code
      today, 190 non-comment lines measured at `f4e5fa225` (`check_equals` 93, plus the source
      extraction it uses, `source_values` 28 and `source_values_from` 69), and calls nothing in
      `cross_file::eval`;
   3. the e2e suite and the corpus replay show zero differences against v0.21, and the section
      9.6 budgets hold as ADR-0024 decision 6 measures them.
4. **The WIT contract is checked in CI before the tag.** v0.21.0 is not tagged until witforge
   0.1.0 is published and pinned, and the contract's check, generation, and regeneration
   `--check` run in default CI. A manual run does not satisfy the release acceptance.

## Consequences

- Properties, dotenv, and `.resx` findings read the way those files and their tools name keys,
  and `ignore_keys: ["app.*"]` works there. Baseline keys for those formats use the plain name
  from v0.21.0 on.
- `nav.*` now matches `nav['a.b']`, and `nav*` no longer matches `nav['']`, as the rules
  reference always said.
- The `.env` recipe works in CI with one option, without weakening the default.
- The program's continuation is decided by measurements taken when they become possible, not by
  the evidence available before the old path can be removed.
- The v0.21 release date depends on witforge 0.1.0.

## Considered Options

- **Keep one rendering for every key (ADR-0024 decision 4):** rejected; nearly every properties
  finding would be bracketed, and users would have to write `['app.*']` to ignore keys.
- **Format-native notation for every format** (INI as `[Session] key`, Android as
  `@string/app_name`): rejected; `section.key` and `string.app_name` already read naturally,
  and each extra notation needs its own escaping and documentation.
- **Bracketed findings with raw `ignore_keys` matching:** rejected; users would see one form and
  type another.
- **Treat every absent literal peer as a note, or infer it from `.gitignore`:** rejected; the
  first lets a typo pass, and the second needs gitignore evaluation for a path the walker never
  saw.
- **GO, or NO-GO, on the v0.21 evidence:** rejected; GO accepts simplicity on faith, and NO-GO
  judges it before it can be measured.
- **Accept a manual WIT check, or build the private witforge in CI:** the owner chose to wait for
  the release instead.

## More Information

- [ADR-0024](0024-relation-ir-decisions-after-review.md): the decisions this refines.
- [`v0.21/pre-release-audit.md`](../design/v0.21/pre-release-audit.md): the audit and the other
  owner decisions (benchmarks, CI, dependencies, toolchains).
- [`v0.21/r8-readiness.md`](../design/v0.21/r8-readiness.md): the release-readiness record.
