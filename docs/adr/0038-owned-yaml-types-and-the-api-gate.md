---
status: accepted
date: 2026-11-14
decision-makers: asamarts
---

# 0038. Owned YAML types, the public-API gate, and a sealed `Error`

## Status

Accepted (2026-11-14). Refines [ADR-0032](0032-v023-owner-decisions.md) decision 3 for phase 4 of
the [v0.23 plan](../design/v0.23/README.md) (#484). The rest of ADR-0032 stands.

## Context

- **The parser in the public API.** On `main` after #513, these public items of `alint-core`,
  `alint-dsl` and `alint-testkit` name ayaml types:
  - `RuleSpec::extra`, `NestedRuleSpec::extra` and `RuleEntry::extra()`;
  - `alint_core::Error::Yaml`, and its `From` implementation;
  - `alint_core::yaml::from_str` and `yaml_depth::map_compat_error`;
  - `template::render_mapping`, `render_value` and `render_nested_options`;
  - the hidden `mask_configuration_values` and `mask_configuration_field`;
  - six `alint-dsl` functions that take `&[ayaml::Mapping]`: `reject_command_rules_in`,
    `reject_fix_promotion_in`, `reject_fix_promotion_templates_in`,
    `reject_spawning_fix_op_templates_in`, `reject_spawning_fix_ops_in` and
    `reject_spawning_templates_in`;
  - `alint_testkit::Error::Yaml`, `PolicyTestParseError::Yaml`, `TreeSpec::from_yaml` and
    `TreeSpec::to_yaml`.

  While they do, a change of parser is a breaking change for library users (ADR-0032
  decision 3), and saphyr leaves in v0.24
  ([ADR-0036](0036-ayaml-issues-and-the-saphyr-exit.md)).
- **What uses them.** Rules read their options through `RuleSpec::deserialize_options` and do
  not touch the mapping. `alint inspect` and `explain` iterate an entry's options and serialize
  them, the policy runner walks nested rule mappings, and `alint-dsl` masks configuration values
  and decodes documents it holds as parser values.
- **A gate on stable Rust.** The plan (section 3.1) asks for a public-API check or a scan of the
  generated documentation. rustdoc's JSON output and `--document-hidden-items` need a nightly
  compiler. With the dependencies documented, rustdoc's HTML links each external type to its
  crate's pages, so a link to `ayaml/` marks a public item that names it.
- **Hidden items.** A `#[doc(hidden)]` public item is still public. A source search for
  `ayaml::` in hidden items misses a type brought in with `use`, as the six `alint-dsl`
  functions show: they say `&[Mapping]`.
- **`alint_core::Error`.** Its variants are public and it is not `#[non_exhaustive]`, so a new
  variant breaks a library's exhaustive `match`. Changing `Error::Yaml`'s type is a breaking
  change anyway. `Walk` wraps `ignore::Error` and `Glob` wraps `globset::Error`; neither is a
  parser.
- **The v1.0 wording.** The ROADMAP's v1.0 section says "the remaining public structs are sealed
  in v0.23". ADR-0032 decision 3 seals five: `Violation`, `Context`, `RuleSpec`,
  `NestedRuleSpec` and `WalkOptions`. A source count finds about 90 other public structs in
  `alint-core` whose fields are all public, and more in `alint-testkit` and `alint-output`; many
  are plain output data.

## Decision

1. **alint's own YAML types.** `alint_core::yaml` holds `Value`, `Mapping`, `Sequence`, `Error`
   and `Location`, with `from_str`, `from_value`, `to_value` and `to_string`.
   - Each type wraps the parser's in a private field. Borrowed views (`Value::as_mapping`, a
     mapping's entries) are casts behind private functions, made with `ref-cast`, which is
     already in the dependency graph and builds under `forbid(unsafe_code)`.
   - `Value` and `Mapping` implement `Serialize` and `Deserialize` as the parser's do, so
     `#[serde(flatten)]` and typed decoding behave as before. `{:?}` prints as the parser's
     types do, which keeps snapshot output unchanged.
   - The accessors look through a tag, as the parser's do. `Mapping` looks keys up by string,
     and `remove` keeps the order of the remaining keys.
   - `Error` prints the parser's message and gives `message()` and `location()`. It implements
     `serde::de::Error` and `serde::ser::Error`.
2. **The changed signatures.**
   - `RuleSpec::extra`, `NestedRuleSpec::extra` and `RuleEntry::extra()` hold an
     `alint_core::yaml::Mapping`.
   - `alint_core::Error::Yaml`, `alint_testkit::Error::Yaml` and
     `PolicyTestParseError::Yaml` hold an `alint_core::yaml::Error`, as do the results of
     `alint_core::yaml::from_str`, `yaml_depth::map_compat_error`, `TreeSpec::from_yaml` and
     `TreeSpec::to_yaml`.
   - `mask_configuration_values` and `mask_configuration_field` take an
     `alint_core::yaml::Value`.
   - Crate-private now: the three `template::render_*` functions and the six `alint-dsl`
     functions, which only their own crates called.
   - Workspace crates that hold parser values internally (`alint-dsl`'s loader, the policy-test
     reader) keep them and cross into alint's types through Serde, which carries every scalar,
     collection and tag unchanged.
3. **The gate.** `ci/scripts/public-api-parser-gate.sh` documents the three crates with their
   dependencies, from a copy of the sources with `#[doc(hidden)]` removed. It fails if any page
   of the three crates links to a YAML parser crate (ayaml, saphyr, saphyr-parser, `yaml_serde`
   and others). Removing `#[doc(hidden)]` first is the check on hidden items: rustdoc resolves
   imported names, which a source search cannot. The Docs job runs it from `docs.sh`, and
   `test-public-api-parser-gate.sh` tests its scan.
4. **`alint_core::Error` is `#[non_exhaustive]`** from this release, the one that changes
   `Error::Yaml`. `Walk` and `Glob` keep their types; the v1.0 audit below reviews them.
5. **The v1.0 wording.** The ROADMAP says that v0.23 seals the five structs of ADR-0032 decision
   3 and `alint_core::Error`, not every public struct. A v1.0 item audits the other public
   structs of the published crates. For each, it decides whether to seal it, keep it open as
   plain data, or make it private, and a gate lists the structs that stay open.
6. **Order.** Phase 4 lands as three pull requests: the types, the gate and the sealed `Error`
   (4.1); constructors and setters for the five structs, with the workspace converted to them
   (4.2); then `#[non_exhaustive]` on the five (4.3).

## Consequences

- A later parser change, such as v0.24's removal of saphyr, does not change alint's library API.
- Library users migrate once. The CHANGELOG lists each changed signature and its replacement.
  Code that matched on `ayaml::Value` variants uses the accessors (`as_str`, `as_mapping`,
  `as_sequence` and the rest) instead.
- The Docs job documents the dependencies of the three crates as well, which adds build time;
  the gate keeps its target directory under `target/public-api-gate`.
- A match on `alint_core::Error` needs a wildcard arm.
- The v1.0 milestone gains an audit of the open public structs.
