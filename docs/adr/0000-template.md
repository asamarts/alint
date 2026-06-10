---
status: proposed
date: YYYY-MM-DD
decision-makers: who-decided
---

# NNNN. Short title of the decision

<!--
HOW TO USE THIS TEMPLATE

- Copy this file to docs/adr/NNNN-kebab-case-title.md, using the next free
  four-digit number (the directory is sorted by number).
- Keep the three headings `## Status`, `## Context`, and `## Decision`. alint's
  own bundled `docs/adr@v1` ruleset is wired into .alint.yml and checks for them,
  so the project dogfoods its own ADR linter on its own ADRs.
- ADRs are immutable records, not living documents. To change a decision, add a
  NEW ADR and set this one's status to `Superseded by ADR-NNNN`. Do not rewrite
  the substance of an accepted ADR.
- Format: MADR 4.0.0 (https://adr.github.io/madr/). Sections after Decision are
  optional; delete the ones you do not need.
- This file (0000-template.md) is itself a valid ADR shape so it both passes the
  ruleset and stays a working example. Never assign 0000 to a real decision.
-->

## Status

Proposed. (One of: Proposed | Accepted | Rejected | Deprecated | Superseded by ADR-NNNN.)

## Context

What is the problem or force that prompts this decision? State the constraints and
the decision drivers in plain language. Link to the design doc, issue, or PR.

## Decision

The decision in active voice: "We will ...". Be specific enough that a reader can
tell whether a later change contradicts it.

## Consequences

What becomes easier and what becomes harder as a result. Include the negative
consequences honestly; an ADR with only upsides is usually underexamined.

## Considered Options

- Option A: ...
- Option B: ...

(Optional. Delete if there were no real alternatives.)

## More Information

Links to the design doc, related ADRs, the PRs or commits that implement this, and
any follow-up work. (Optional.)
