---
title: 'docs/adr@v1'
description: Bundled alint ruleset at alint://bundled/docs/adr@v1.
---

Architecture Decision Records following the MADR ("Markdown
Architectural Decision Records") convention: files named
NNNN-title.md under docs/adr/, each with at least Status,
Context, and Decision sections.

If your repo uses a different ADR home (docs/decisions/,
.adr/, adrs/, …) override the paths of these rules rather
than disabling the ruleset. The field-level override
introduced in v0.4.2 makes that a three-line change.

Deferred: gap-free ADR numbering (e.g. no 0001, 0002, 0004 —
missing 0003). Needs the `numeric_sequence` primitive on the
v0.5+ roadmap.

## Adopt with

```yaml
extends:
  - alint://bundled/docs/adr@v1
```

## Rules

### `adr-filename-pattern`

- **kind**: [`filename_regex`](/docs/rules/naming/filename_regex/)
- **level**: `warning`
- **policy**: <https://adr.github.io/madr/>

> ADR filename should match `NNNN-kebab-case-title.md` (four-digit number + hyphen + lowercase kebab title).

### `adr-has-status-section`

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `info`

> MADR ADRs include a ## Status section.

### `adr-has-context-section`

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `info`

> MADR ADRs include a ## Context section.

### `adr-has-decision-section`

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `info`

> MADR ADRs include a ## Decision section.

## Source

The full ruleset definition is committed at [`crates/alint-dsl/rulesets/v1/docs/adr.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/docs/adr.yml) in the alint repo (the snapshot below is generated verbatim from that file).

```yaml
# alint://bundled/docs/adr@v1
#
# Architecture Decision Records following the MADR ("Markdown
# Architectural Decision Records") convention: files named
# NNNN-title.md under docs/adr/, each with at least Status,
# Context, and Decision sections.
#
# If your repo uses a different ADR home (docs/decisions/,
# .adr/, adrs/, …) override the paths of these rules rather
# than disabling the ruleset. The field-level override
# introduced in v0.4.2 makes that a three-line change.
#
# Deferred: gap-free ADR numbering (e.g. no 0001, 0002, 0004 —
# missing 0003). Needs the `numeric_sequence` primitive on the
# v0.5+ roadmap.

version: 1

rules:
  - id: adr-filename-pattern
    # NNNN-kebab-case-title.md. Matches both MADR and Nygard
    # conventions; keeps the list sortable by number.
    kind: filename_regex
    paths: "docs/adr/*.md"
    stem: true
    pattern: '^\d{4}-[a-z0-9][a-z0-9-]*$'
    level: warning
    message: >-
      ADR filename should match `NNNN-kebab-case-title.md`
      (four-digit number + hyphen + lowercase kebab title).
    policy_url: "https://adr.github.io/madr/"

  - id: adr-has-status-section
    kind: file_content_matches
    paths: "docs/adr/*.md"
    pattern: '(?m)^##\s+Status\b'
    level: info
    message: "MADR ADRs include a ## Status section."

  - id: adr-has-context-section
    kind: file_content_matches
    paths: "docs/adr/*.md"
    pattern: '(?m)^##\s+Context\b'
    level: info
    message: "MADR ADRs include a ## Context section."

  - id: adr-has-decision-section
    kind: file_content_matches
    paths: "docs/adr/*.md"
    pattern: '(?m)^##\s+Decision\b'
    level: info
    message: "MADR ADRs include a ## Decision section."

  # Skipped for this release: gap-free ADR numbering. Requires
  # a `numeric_sequence` primitive that extracts capture groups
  # across matching files — tracked as a v0.5+ follow-up.
```
