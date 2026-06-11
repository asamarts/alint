/*
 * alint — C4 model (Structurizr DSL, Apache-2.0).
 *
 * Hand-modeled architectural INTENT for C4 levels 1-3 (System Context,
 * Container, Component). The ten Rust crates are components, not
 * containers (C4 stops at level 3 here, per C4's own guidance).
 *
 * This is the source of truth for the C4 views. The authoritative,
 * code-extracted crate DEPENDENCY graph lives in `crate-graph.md`
 * (generated from `cargo metadata`); the component relationships below
 * are the stable, architecturally-meaningful subset, not the full edge
 * set. `xtask gen-arch --check` fails if the crate components here drift
 * from the `cargo metadata` member set.
 *
 * Render offline with the consolidated `structurizr` tool (the archived
 * standalone `structurizr/cli` was folded into `structurizr.war` on
 * 2026-02-04; pin the consolidated artifact). Not required in CI —
 * alint.org and GitHub render the Mermaid graph in `crate-graph.md`.
 */
workspace "alint" "Language-agnostic repository linter" {

    model {
        developer = person "Developer" "Authors `.alint.yml`, runs alint locally and fixes findings."
        ci = person "CI / GitHub Actions" "Runs alint as a gate in pipelines and via the official Action."

        repo = softwareSystem "Linted repository" "The target repo: files, structure, git history." {
            tags "External"
        }
        registries = softwareSystem "Package registries" "crates.io, npm, ghcr.io, Homebrew — distribution channels." {
            tags "External"
        }
        site = softwareSystem "alint.org" "Docs + marketing site; consumes the docs bundle (schema.json, facts.json, crate-graph)." {
            tags "External"
        }

        alint = softwareSystem "alint" "Repository linter: structure, existence, content, cross-file, git, and structured-query rules." {

            cli = container "CLI" "The `alint` binary (`cargo install alint`): argument parsing, subcommand dispatch, the engine, and report formatting." "Rust" {
                comp_alint = component "alint" "Binary entrypoint: CLI parsing, subcommand dispatch, process exit codes." "Rust crate"
                comp_core = component "alint-core" "Engine, file walker, Rule trait, config AST, facts, errors — the foundation." "Rust crate"
                comp_dsl = component "alint-dsl" "YAML config loader, schema validation, bundled rulesets, extends/SRI." "Rust crate"
                comp_rules = component "alint-rules" "Built-in rule implementations + fixers across all dispatch classes." "Rust crate"
                comp_output = component "alint-output" "Report formatters: human, json, sarif, github, gitlab, junit, markdown, agent." "Rust crate"
            }

            lsp = container "LSP server" "The `alint lsp` mode: a Language Server over stdio for editor integrations." "Rust / tower-lsp" {
                comp_lsp = component "alint-lsp" "Language Server Protocol server (diagnostics, validation) over stdio." "Rust crate"
            }

            tooling = container "Dev + build tooling" "Workspace-internal crates: benchmarks, test harness, end-to-end scenarios, and cargo-xtask automation." "Rust" {
                comp_bench = component "alint-bench" "Criterion micro-benchmarks and the seeded deterministic tree generator." "Rust crate"
                comp_testkit = component "alint-testkit" "Tree-spec materializer, scenario runner, and proptest strategies for tests." "Rust crate"
                comp_e2e = component "alint-e2e" "End-to-end scenarios plus the coverage-audit and invariant tests." "Rust crate"
                comp_xtask = component "xtask" "Build automation: bench-release, docs-export, gen-schema, gen-facts, gen-arch." "Rust crate"
            }
        }

        # Context relationships.
        developer -> cli "Runs `alint check` / `alint fix`"
        developer -> lsp "Gets inline diagnostics in the editor"
        ci -> cli "Runs as a pipeline gate / GitHub Action"
        ci -> registries "Publishes release artifacts to"
        cli -> repo "Reads files + git history; reports violations"
        lsp -> repo "Validates the open workspace"
        comp_xtask -> site "Produces the docs bundle consumed by"

        # Stable, architecturally-meaningful component relationships.
        # (The full, authoritative edge set is the generated crate-graph.md.)
        comp_alint -> comp_core "Drives the engine"
        comp_alint -> comp_dsl "Loads config"
        comp_alint -> comp_rules "Registers built-in rules"
        comp_alint -> comp_output "Renders reports"
        comp_alint -> comp_lsp "Starts the server (`alint lsp`)"
        comp_dsl -> comp_core "Builds the config AST"
        comp_dsl -> comp_rules "Resolves rule kinds"
        comp_rules -> comp_core "Implements the Rule trait"
        comp_output -> comp_core "Formats the engine's Report"
        comp_lsp -> comp_core "Runs the engine"
        comp_lsp -> comp_dsl "Validates config"
    }

    views {
        systemContext alint "SystemContext" {
            include *
            autolayout lr
        }
        container alint "Containers" {
            include *
            autolayout lr
        }
        component cli "CliComponents" {
            include *
            autolayout lr
        }

        styles {
            element "External" {
                background "#999999"
                color "#ffffff"
            }
        }

        theme default
    }
}
