---
title: Architecture diagrams
description: Interactive system, container, and flow diagrams generated from alint's LikeC4 model and kept in lockstep with the code by CI gates.
---

These diagrams come from a single [LikeC4](https://likec4.dev) model in the alint
repository. They stay in lockstep with the code: the crate elements and their
dependency edges are gated against `cargo metadata`, the rule catalogue against
`docs/rules.md`, the config keys against the JSON schema, and the whole model is
checked by `likec4 validate` in CI. The same model is also exported to
[static Mermaid diagrams](https://github.com/asamarts/alint/blob/main/docs/design/architecture/DIAGRAMS.md)
for viewing in the GitHub repository.

Drag to pan, scroll to zoom, and click an element to step into it.

## System context

Who and what alint interacts with: developers and CI, the linted repository,
package registries, editors (via the LSP), GitHub Code Scanning, and alint.org.

<likec4-view view-id="index"></likec4-view>

## Containers

The `alint` system as containers: the CLI, the LSP server, and the dev or build
tooling.

<likec4-view view-id="containers"></likec4-view>

## CLI components

The crates inside the CLI and their runtime relationships.

<likec4-view view-id="cliComponents"></likec4-view>

## Crate dependency graph

All workspace crates and their dependency edges, grouped by container: runtime
dependencies solid, dev/build-only dashed. Generated from `cargo metadata`.

<likec4-view view-id="crateGraph"></likec4-view>

## Execution pipeline

`alint check`: config load, facts, rule filtering, the single parallel walk,
dispatch, evaluation, aggregation, optional fix, and output.

<likec4-view view-id="checkFlow"></likec4-view>

## Config load and the extends trust boundary

How `extends:` resolves (local path, https with SRI, bundled) and why
process-spawning rule kinds and out-of-root access are rejected outside the
top-level config (ADR-0004).

<likec4-view view-id="configLoad"></likec4-view>

## Dispatch and read-coalescing

Rule-major versus per-file dispatch, and how each matched file is read at most
once (ADR-0003).

<likec4-view view-id="dispatchFlow"></likec4-view>

## Rule catalogue

The built-in rule families and the canonical kinds within them. Click a family to
step in.

<likec4-view view-id="catalogueOverview"></likec4-view>

## Config DSL domain model

The `.alint.yml` entities and how they relate.

<likec4-view view-id="configModel"></likec4-view>

## Rule execution type model

The engine's core types: how a `Rule` (or `PerFileRule`), a `Fixer`, and the
`Violation` / `RuleResult` / `Report` values relate.

<likec4-view view-id="ruleTypeModel"></likec4-view>

## Facts and conditional rules

Facts are evaluated once (in parallel, cached), then used to filter which rules
run via their `when:` conditions.

<likec4-view view-id="factsFlow"></likec4-view>

## Fix flow

`alint fix`: how auto-fixable violations are applied (including content-reading
fix operations) and re-checked.

<likec4-view view-id="fixFlow"></likec4-view>

## Walk, gitignore, and filtering

The single parallel directory walk: gitignore handling, include/exclude
filtering, and the deterministic, sorted `FileIndex`.

<likec4-view view-id="walkerFlow"></likec4-view>

## Template expansion

How `{{vars.X}}` template variables in rule options are expanded before evaluation.

<likec4-view view-id="templateFlow"></likec4-view>

## Monorepo nesting and scoping

How nested `.alint.yml` files layer and scope rules across a monorepo.

<likec4-view view-id="monorepoNesting"></likec4-view>

## Output formats

How a `Report` is rendered into each output format (human, JSON, SARIF, and the
rest).

<likec4-view view-id="outputFormats"></likec4-view>

## LSP

The language server: how an editor's open/change/save events drive a per-file
check and publish diagnostics.

<likec4-view view-id="lspFlow"></likec4-view>

## Editor integrations

How the editor extensions (VS Code, JetBrains) connect to the alint LSP server.

<likec4-view view-id="editorArch"></likec4-view>

## CI: GitHub Action and SARIF

The GitHub Action: how a CI run produces SARIF for GitHub Code Scanning.

<likec4-view view-id="ciActionFlow"></likec4-view>

## pre-commit: the commit gate

How the pre-commit hook runs `alint check` on commit and blocks on errors, plus
the manual `alint-fix` hook.

<likec4-view view-id="preCommitFlow"></likec4-view>

## Release and distribution

How a tagged release fans out to crates.io, npm, Homebrew, Docker, and the
install script.

<likec4-view view-id="distributionFlow"></likec4-view>

## Contributing a rule kind

The touch-points for a new rule kind: the registry, the options struct and JSON
schema, and the generated docs.

<likec4-view view-id="addRuleKindFlow"></likec4-view>

## Docs as code and drift gates

How the generated contracts (facts, schema, rules, this model) flow to alint.org
and are gated against drift.

<likec4-view view-id="docsAsCodeFlow"></likec4-view>

## Deterministic performance gating

The load-immune, Valgrind-based performance check that guards the engine against
regressions independently of machine load.

<likec4-view view-id="perfGatingFlow"></likec4-view>

## Plugin model

How an external rule plugin is declared, gated (spawning kinds rejected outside
the top-level config), and invoked.

<likec4-view view-id="pluginModel"></likec4-view>

## Security: path confinement and spawning-kind gating

Two guards: keeping file access within the repository root, and rejecting
process-spawning rule kinds pulled in via `extends:` (ADR-0004).

<likec4-view view-id="pathConfinement"></likec4-view>

## Security: Trojan-source and unicode hygiene

How bidirectional-override and invisible-character (Trojan-source) attacks are
detected in scanned files.

<likec4-view view-id="trojanSourceFlow"></likec4-view>
