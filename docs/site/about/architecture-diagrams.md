---
title: Architecture diagrams
description: Interactive system, container, and flow diagrams generated from alint's LikeC4 model and kept in lockstep with the code by CI gates.
---

These diagrams come from a single [LikeC4](https://likec4.dev) model in the alint
repository. They stay in lockstep with the code: the crate elements and their
dependency edges are gated against `cargo metadata`, the rule catalogue against
`docs/rules.md`, the config keys against the JSON schema, and the whole model is
checked by `likec4 validate` in CI. The same model is exported to Mermaid for the
GitHub repository.

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

<likec4-view view-id="ruleTypeModel"></likec4-view>

## Facts and conditional rules

<likec4-view view-id="factsFlow"></likec4-view>

## Fix flow

<likec4-view view-id="fixFlow"></likec4-view>

## Walk, gitignore, and filtering

<likec4-view view-id="walkerFlow"></likec4-view>

## Template expansion

<likec4-view view-id="templateFlow"></likec4-view>

## Monorepo nesting and scoping

<likec4-view view-id="monorepoNesting"></likec4-view>

## Output formats

<likec4-view view-id="outputFormats"></likec4-view>

## LSP

<likec4-view view-id="lspFlow"></likec4-view>

## Editor integrations

<likec4-view view-id="editorArch"></likec4-view>

## CI: GitHub Action and SARIF

<likec4-view view-id="ciActionFlow"></likec4-view>

## Release and distribution

<likec4-view view-id="distributionFlow"></likec4-view>

## Contributing a rule kind

<likec4-view view-id="addRuleKindFlow"></likec4-view>

## Docs as code and drift gates

<likec4-view view-id="docsAsCodeFlow"></likec4-view>

## Deterministic performance gating

<likec4-view view-id="perfGatingFlow"></likec4-view>

## Plugin model

<likec4-view view-id="pluginModel"></likec4-view>

## Security: path confinement and spawning-kind gating

<likec4-view view-id="pathConfinement"></likec4-view>

## Security: Trojan-source and unicode hygiene

<likec4-view view-id="trojanSourceFlow"></likec4-view>
