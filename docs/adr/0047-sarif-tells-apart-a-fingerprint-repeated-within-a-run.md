---
status: accepted
date: 2026-11-19
decision-makers: asamarts
---

# 0047. SARIF tells apart a fingerprint repeated within a run by occurrence, as GitLab output does

## Status

Accepted (2026-11-19). Amends [ADR-0006](0006-baseline-suppression.md), whose single
fingerprint SARIF `partialFingerprints` carried for every finding; the baseline file keeps it.

## Context

Since v0.14 the baseline file, GitLab output and SARIF `partialFingerprints` carry one canonical
fingerprint per finding (`alint_core::baseline::violation_fingerprint`), so a finding has one
identity in every format (ADR-0006, `docs/design/baseline.md` open question 7). The fingerprint
is meant to repeat: two findings of one rule on identical content share it, and the baseline
grandfathers them by count.

Two kinds of finding leave the path out of the fingerprint by design, so their repeats are
common rather than rare:

- a version 1 finding whose path the environment chose, since v0.22
  ([ADR-0030](0030-v022-owner-decisions.md) decision 1), so that a fingerprint cannot confirm a
  guessed path;
- a `constraint` finding (v0.23), whose fingerprint is its rule and its key, so that a renamed or
  re-sorted file keeps a baselined finding baselined. A `unique` rule without `determines:` gives
  one finding per repeated row, in different files, with one key.

GitLab Code Quality drops entries that share a fingerprint, so GitLab output has always given the
Nth repeat within a report (N > 0) a fingerprint derived from the shared one and N; the first keeps
the canonical one. SARIF wrote the canonical fingerprint on every result. GitHub code scanning
may merge results that share `partialFingerprints` into one alert, so all but one of those
findings could disappear from the alert list. Audit round 4 after v0.22.1 found this for
`constraint` findings.

## Decision

1. Within one SARIF run, the results that share a fingerprint are numbered in an order the
   results decide, not the order they are written in: baselined results first, then live ones,
   each by the path, line and column they show, then rule and message (a hidden path sorts as
   `<redacted>`), equal ones in written order. Number 0 keeps the canonical fingerprint, the
   identity the baseline file gives the finding.
2. Number N (N > 0) carries the hex SHA-256 of the fingerprint, `:` and N as four little-endian
   bytes. GitLab output numbers a report's findings the same way and derives the same values. It
   holds no baselined findings, so without a baseline the two formats agree finding by finding;
   under one, a live repeat's SARIF number counts the baselined repeats before it.
3. The baseline file is unchanged: it records the canonical fingerprint with a count, and matching
   stays by count.

## Consequences

- Each finding is its own alert in GitHub code scanning, as each is its own issue in GitLab.
- A run in which no fingerprint repeats is written byte for byte as before.
- An upload of a run that has repeats gives each repeat a new fingerprint, so GitHub code scanning
  may open new alerts for findings it had merged into one: a version 1 rule's findings in files the
  environment chose, or two findings on identical content. The CHANGELOG's "Upgrading from 0.22"
  says so.
- A repeat's fingerprint depends on where it sorts among the results that share its fingerprint.
  When one goes away, each that sorts after it takes the number before its own, so the alert that
  closes may be another repeat's; the number of open alerts stays right. The baseline suppresses
  the first findings of a fingerprint in path, line, column and message order, and SARIF numbers
  baselined results first, so a baselined repeat keeps its fingerprint while live repeats of its
  key come and go. GitLab output numbered repeats in written order from v0.14.0 and numbers them
  as SARIF does from v0.23.0, which can change the fingerprint it gives a version 1 finding that
  repeats one.

## Considered Options

- **Document that the findings share a fingerprint, and leave SARIF as it was.** No change for
  existing uploads, but a code-scanning user would see one alert for many findings and could not
  tell which were fixed.
- **Put the path back in the fingerprint.** Each finding would keep a stable identity of its own,
  but a renamed file would turn a baselined `constraint` finding into a new one, and a hidden-path
  fingerprint would again let its holder test a guessed path (ADR-0030). Not chosen.

## More Information

- Implementation: `number_repeated_fingerprints` in `crates/alint-output/src/sarif.rs`;
  `OccurrenceKey`, `occurrences` and `occurrence_fingerprint` in
  `crates/alint-output/src/occurrence.rs`; `write_gitlab` in `crates/alint-output/src/gitlab.rs`.
- Docs: `docs/site/reference/output-formats/index.md` ("Stable fingerprints") and
  `docs/design/baseline.md` open question 7.
