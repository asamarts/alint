---
status: accepted
date: 2026-06-10
decision-makers: asamarts
---

# 2. The configuration language is YAML with extends-based composition

## Status

Accepted. Backfilled on 2026-06-10; the decision itself predates this record and is
documented here retrospectively as part of adopting ADRs (see ADR-0001).

## Context

alint needs a declarative configuration format that non-Rust users (CI and platform
engineers) can author and review, that supports composition and reuse across many repos,
and that editors can validate. The candidates were YAML, TOML, JSON, and a bespoke DSL.

## Decision

We will use YAML for `.alint.yml` and for the bundled rulesets, parsed via `serde_yaml_ng`.

- A `version: 1` envelope gates the schema generation.
- Composition is expressed with `extends:` (local paths, HTTPS URLs, and
  `alint://bundled/...` rulesets), resolved at the YAML value layer so a child can override
  individual fields of an inherited rule by `id` (last write wins). `only:`/`except:` filter
  inherited rules.
- A JSON Schema at `schemas/v1/config.json` is the authoritative shape and gives editors
  autocomplete and validation; it is embedded into `alint-dsl` via `include_str!`.
- Rules are deserialized from merged mappings into typed `RuleSpec`s only after the extends
  chain resolves, so unknown fields are caught at load time.

## Consequences

Positive: familiar to the target audience; reuse via `extends` and bundled rulesets;
editor support through the schema.

Negative and accepted: YAML has real footguns (the `key: value` frontmatter trap,
single-quote escaping, type coercion of unquoted scalars). These are mitigated by the
schema, the `coverage_audit_site_docs_frontmatter` test, and the CONFIG-AUTHORING pitfalls
catalogue, but they remain a source of user error.

The hand-written schema is itself a drift risk; ADR-0001 and `docs/design/spec-driven-development.md`
record the decision to generate it from the Rust types instead, which supersedes only the
"hand-written" mechanism, not the choice of YAML.

## More Information

- `docs/design/ARCHITECTURE.md` (DSL and composition model).
- ADR-0004 (the trust boundary that constrains what an `extends`'d ruleset may do).
