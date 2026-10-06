# alint

[![crates.io](https://img.shields.io/github/v/tag/asamarts/alint?sort=semver&label=crates.io)](https://crates.io/crates/alint)
[![CI](https://github.com/asamarts/alint/actions/workflows/ci.yml/badge.svg)](https://github.com/asamarts/alint/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue)](#license)

**A linter for the files in your repository.**

Code linters check what's inside a source file. alint checks the repository around it: files that should or shouldn't exist, naming conventions, file contents, values buried in `package.json`, `Cargo.toml` or a workflow YAML, and the relationships between files. The rules live in one `.alint.yml` and run on every pull request, in pre-commit and in your editor.

Every repository has a shape it is supposed to keep. Every package carries a README. Workflows pin their actions to a commit SHA. Nobody commits `target/` or `node_modules/`. Source files that need a license header have one. These rules are real, and a repo drifts when they slip, but they usually live in a reviewer's head or a line of `CONTRIBUTING.md`, where nothing fails when a pull request ignores them.

alint writes those rules down and enforces them. It takes over from [Repolinter](https://github.com/todogroup/repolinter) (archived in 2026), runs alongside ESLint, Clippy, or Ruff, and ships as a single static Rust binary with nothing to install at runtime.

<!-- Absolute URL on purpose: this README is also the crates.io and npm landing
     page, and neither resolves relative image paths. The GIF is generated from
     demo/alint.tape (via demo/render.sh), never hand-recorded, so it cannot
     drift from real output.

     width="880" is REQUIRED and is half the GIF's native 1760px. Most people
     read GitHub on a HiDPI display, where a 1x asset gets upscaled 2x by the
     browser and small antialiased terminal glyphs turn to mush. At 2x native,
     HiDPI maps this 1:1 to device pixels (sharp) and 1x displays downscale it
     exactly 2:1 (clean). Drop the attribute and the 1760px source blows out the
     column; set it to 100% and the letters go blurry again. -->
<img src="https://raw.githubusercontent.com/asamarts/alint/main/demo/alint.gif" width="880" alt="alint finding structural violations in a repo, fixing the mechanical ones, and leaving the structural ones for a human">

*A tracked `target/`, an unpinned GitHub Action, a scratch planning doc at the repo root, and whitespace drift. alint finds all of it, fixes what is mechanical, and leaves what needs a decision. No per-language linter sees any of it.*

## Quickstart

```sh
# Install (Linux and macOS; Windows via npm, cargo, or the release tarball):
curl -sSL https://alint.org/install.sh | bash

# Start from a few bundled rulesets:
cat > .alint.yml <<'YAML'
version: 1
extends:
  - alint://bundled/oss-baseline@v1          # README, LICENSE, hygiene, merge markers
  - alint://bundled/rust@v1                   # auto-skips when there is no Rust
  - alint://bundled/ci/github-actions@v1      # pin actions, least-privilege workflows
YAML

alint check            # report what is wrong
alint fix --dry-run    # preview the mechanical repairs
alint fix              # apply them
```

`alint init` writes that file for you by detecting the ecosystems in your repo, so you rarely start from a blank page. The [quickstart on alint.org](https://alint.org/docs/getting-started/quickstart/) walks a first config end to end.

## How it works

A config is a list of rules, but you rarely write them all by hand. `extends:` composes **rulesets**, and alint ships 22 of them compiled into the binary, so there is no download and no network round trip. Most are gated on what the repo actually contains, so listing the Rust ruleset in a repo with no Rust does nothing. You layer your own rules on top, and override or switch off anything you inherit.

A rule is three things: a **kind** (the check to run, such as `file_exists`, `filename_case`, or `pair`), the **paths** it applies to, and a **severity**. There are 105 rule kinds across 13 families. A rule can also carry a `when:` condition (run only on the default branch, or only when some file exists) and a `fix:`.

```yaml
# .alint.yml
version: 1
extends:
  - alint://bundled/oss-baseline@v1
rules:
  - id: no-focused-tests                 # a rule of your own, on top of the ruleset
    kind: file_content_forbidden
    paths: ["**/*.test.ts"]
    pattern: '\.only\('
    message: "Remove .only() so CI runs the whole suite, not one test"
    level: error
```

Under the hood, alint walks the repository once, in parallel, honoring `.gitignore`, and reads each file's bytes at most once. That is why a 100,000-file workspace bundle checks in about a second and a half and a million files in under twenty seconds ([benchmarks, measured per release](https://alint.org/benchmarks/)). The engine, the rules, the output formatters, and a language server for editors are all one binary, with no plugin system and nothing from Node, the JVM, or Python in the path. The [concepts guide](https://alint.org/docs/concepts/) and [ARCHITECTURE.md](docs/design/ARCHITECTURE.md) go deeper.

## What it can check

The 105 kinds group into a handful of themes. The [rule reference](https://alint.org/docs/rules/) has every one with an example; a tour of the highlights:

- **Files and structure.** Require or forbid specific files and directories; cap file size, line count, directory depth, and files per directory; reject committed build output and OS junk.
- **Naming.** Filename case and regex conventions, with case-collision and reserved-name safety for cross-platform checkouts.
- **Content.** Required headers and footers, forbidden patterns such as merge markers or debug residue, and the everyday hygiene: trailing whitespace, final newlines, line endings, line width.
- **Config values.** Read into JSON, YAML, TOML, XML, dotenv, INI, `.properties`, and HCL with RFC 9535 JSONPath, then assert a value, match a pattern, forbid a key, or validate the whole file against a JSON Schema. Require `package.json` to declare a `license`, or every workflow to set `permissions: contents: read`.
- **Cross-file relationships.** The primitives most tools lack: `pair` (every `.proto` has its generated binding), `every_matching_has` (every `packages/*` has a README and a manifest), `unique_by` (no two crates share a name), `file_graph` (no import cycles or orphaned modules), and `generated_file_fresh` (a checked-in generated file still matches its source).
- **Security and encoding.** Trojan-Source bidirectional controls, zero-width characters, byte-order marks, and text-encoding sanity.
- **Git hygiene.** Commit-message shape, sign-off and GPG signing, denied paths, and blame age.
- **Metadata.** Executable bits and shebangs, symlink and submodule policy, portable-filename checks.

When no kind fits, the `command` kind runs a tool you already trust and turns its exit code into a violation, so a one-off check still lives in the same config as everything else.

## Fixing, not just finding

Plenty of violations are mechanical: a missing final newline, a stray byte-order mark, an unsorted `CODEOWNERS`, a license header sitting above the shebang instead of below it. `alint fix` repairs those in place, with 26 ops covering content hygiene, structured-config edits, file and git operations, and list ordering. The ones that need a human decision it shows you or suggests, rather than guessing.

Each of the 26 auto-fix ops carries a safety tier, so a bare `alint fix` never surprises you:

- **Safe** fixes apply on a plain `alint fix`: whitespace and newline hygiene, line endings, header placement, sorting a marked block, reindentation.
- **Unsafe** fixes change meaning or remove content, so they wait for `alint fix --unsafe-fixes`: deleting a file, a regex `replace`, dropping a committed artifact from git's index. (As of v0.17, removing a file is Unsafe. A plain `alint fix` now *suggests* the deletion rather than doing it.)
- **Suggestions** are reported but never written, for violations with no mechanical repair.

Preview with `alint fix --dry-run`, or `alint fix --diff` for the exact edits. alint batches a file's edits in memory, writes each file once, and re-runs until the tree stops changing; a size limit (1 MiB by default) skips oversize files rather than rewriting them.

Because `extends:` can pull a ruleset from a URL, a fix that injects or remotely aims content (`replace`, `file_create`, `file_prepend`, and similar operations) from a remote or nested ruleset is demoted to a suggestion unless you list that source under `trusted_extends:`. The cap is applied to the effective fixer after field composition and template expansion, while a fix that shells out is refused from anywhere but your own top-level config. Every op is listed in the [rule reference](https://alint.org/docs/rules/).

## Adopt it without a flag day

You do not have to fix everything the first time alint runs. `alint baseline` records today's violations behind a content fingerprint, and `alint check --baseline` then passes on those and fails only on genuinely new ones. Turn it on as a blocking gate on day one and burn down the backlog on your own schedule, the way you would introduce a type checker to an untyped codebase.

## Where it runs

- **Pull requests.** The [`asamarts/alint` GitHub Action](https://alint.org/docs/integrations/github-actions/) annotates changed lines inline or uploads SARIF to Code Scanning, and `alint check --changed` lints only the files a PR touched.
- **Commits.** A [pre-commit](https://alint.org/docs/integrations/pre-commit/) hook (a prebuilt wheel, no toolchain) checks every commit; a manual `alint-fix` hook repairs on request.
- **Your editor.** `alint lsp` is a language server (diagnostics, hover-to-explain, apply-fix code actions), with marketplace extensions for VS Code and JetBrains, a source extension for Zed, and ready configs for Neovim, Sublime Text, Emacs, and Helix.
- **Coding agents.** The `agent` output format carries a per-violation instruction and fix command, and `alint export-agents-md` keeps the directives in `AGENTS.md` or `CLAUDE.md` in step with the rules alint enforces, so the agent and CI agree on the contract.
- **Any CI.** [8 output formats](https://alint.org/docs/reference/output-formats/) cover most pipelines: `human`, `json`, `sarif`, `github`, `markdown`, `junit`, `gitlab`, and `agent`. Exit codes are stable: `0` clean, `1` violations, `2` config error, `3` internal.

## Bundled rulesets

alint ships 22 bundled ecosystem rulesets inside the binary, pinned to the version you run and reachable as `alint://bundled/<name>@v1`. They group into:

- **Ecosystem baselines**, each gated so it is a no-op off-ecosystem: `rust`, `node`, `python`, `go`, `java`, `dotnet`, `php`, plus the always-on `oss-baseline` (the README / LICENSE / hygiene starting point, and a clean migration target for Repolinter).
- **Monorepo overlays**: a `monorepo` base plus `cargo-workspace`, `pnpm-workspace`, and `yarn-workspace` overlays that scope per-member checks to real package directories.
- **CI and tooling**: `ci/github-actions` (OpenSSF-guided workflow hardening), `tooling/editorconfig`, `docs/adr`, and the `hygiene/*` artifact and lockfile sets.
- **Compliance and governance**: `compliance/reuse`, `compliance/apache-2`, and `apache/governance` (the Apache TLP release discipline that arrow, spark, and airflow each re-implement by hand).
- **Agent-aware**: `agent-hygiene` (scratch docs, duplicate-versioned files, and debug residue that cluster in agent-authored commits) and `agent-context` (keeps `AGENTS.md` and friends honest).

Rulesets ship non-blocking by default: `info` or `warning` for recommendations, `error` only for unambiguous bugs. Redeclare a rule id in your own config to change its severity or scope, or set `level: off`. The [bundled-ruleset reference](https://alint.org/docs/bundled-rulesets/) lists every rule in each.

## On real repositories

alint ships [working configs for 30 open-source projects](https://alint.org/examples/), from single-language libraries to polyglot monorepos, each with a writeup of what alint catches that the project's own tooling misses. Writing them surfaced a few patterns that recur:

- Projects that lean on a convention without enforcing it. tokio has no validation scripts, yet its pipeline quietly assumes conventions alint can hold to.
- Projects drowning in hand-rolled verify scripts. Kubernetes maintains dozens of `hack/verify-*.sh`; the structural ones collapse into a handful of declarative rules.
- Projects with excellent code linters but no structural layer. ruff ships 900+ Python rules and still cannot check that its own internal crates keep `publish = false`.
- Polyglot trees no single per-language linter sees across: apache/arrow spans six languages, vercel/next.js is TypeScript and Rust, NixOS/nixpkgs is tens of thousands of files.

Start from the [example](https://alint.org/examples/) closest to your repo, or see [how alint compares](https://alint.org/compare/) with other tools and how to [migrate from Repolinter](https://alint.org/migrating-from/repolinter/).

## What alint is not

alint checks the shape and contents of a repository, not the semantics of the code inside it. It is deliberately not:

- a code or AST linter (use [ESLint](https://eslint.org/), [Clippy](https://doc.rust-lang.org/clippy/), [Ruff](https://docs.astral.sh/ruff/))
- a SAST scanner (use [Semgrep](https://semgrep.dev/), [CodeQL](https://codeql.github.com/))
- an IaC scanner (use [Checkov](https://www.checkov.io/), [Conftest](https://www.conftest.dev/))
- a commit-message linter (use [commitlint](https://commitlint.js.org/))
- a secret scanner (use [gitleaks](https://github.com/gitleaks/gitleaks), [TruffleHog](https://github.com/trufflesecurity/trufflehog))

It runs underneath those, and keeps its rules on the filesystem so they can keep theirs on the code.

## Install

```bash
# install.sh (Linux, macOS; x86_64 and aarch64)
curl -sSL https://alint.org/install.sh | bash

# Homebrew (macOS, Linuxbrew)
brew install asamarts/alint/alint

# crates.io
cargo install alint

# npm (also puts alint on PATH; for Windows or a project-local dev dependency)
npm install -g @asamarts/alint

# PyPI (uvx / pipx / pip; the wheel embeds the binary, so no Python in the hot path)
uvx alint check
uv tool install alint
```

`install.sh` detects your platform, downloads the matching tarball, verifies its SHA-256, and installs to `~/.local/bin`. The npm and PyPI packages ship the same prebuilt binary; the PyPI wheel even installs cleanly where the npm shim cannot, including under `--ignore-scripts`, on offline mirrors, and on Windows. A distroless multi-arch Docker image is published to ghcr.io on every release:

```bash
docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:v0.17.0 check
```

Build from source with `cargo build --release -p alint`. Every channel and platform is covered in the [installation guide](https://alint.org/docs/getting-started/installation/).

alint is telemetry-free and makes no network access at runtime, except for the `extends:` URLs you write yourself, which must be SRI-pinned. The threat model is in [SECURITY.md](SECURITY.md).

## Documentation

[alint.org](https://alint.org) is the narrative home. The pages worth bookmarking:

- [Concepts](https://alint.org/docs/concepts/): the rule model, scopes, and `when:` conditions
- [Rule reference](https://alint.org/docs/rules/) and [bundled rulesets](https://alint.org/docs/bundled-rulesets/)
- [CLI](https://alint.org/docs/cli/) (all 12 subcommands) and the [configuration schema](https://alint.org/docs/configuration/)
- [Cookbook](https://alint.org/docs/cookbook/): monorepos, CI hardening, package shape, custom `command` rules
- [Architecture](https://alint.org/docs/about/architecture/) and [roadmap](https://alint.org/docs/about/roadmap/) through v1.0

In the repo: [CHANGELOG.md](CHANGELOG.md) for the per-version history, [docs/benchmarks/](docs/benchmarks/) for methodology and per-release results, and [SECURITY.md](SECURITY.md) for the threat model.

## Development

```bash
git clone https://github.com/asamarts/alint
cd alint
cargo test --workspace     # ~2,300 Rust tests, plus ~500 declarative end-to-end scenarios
cargo run -- check         # dogfood: alint lints its own repo
cargo bench -p alint-bench # criterion micro-benches
```

alint is a single Cargo workspace of nine crates, of which only `alint` and `alint-core` are published. End-to-end tests are declarative YAML under `crates/alint-e2e/scenarios/`, so adding one is adding a file, and CLI snapshots run through `trycmd`. CI is per-job bash scripts under `ci/scripts/` that behave the same locally and on GitHub. Contributions are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Dual-licensed under either [Apache License 2.0](LICENSE-APACHE) or the [MIT License](LICENSE-MIT), at your option. Unless you state otherwise, any contribution you submit for inclusion in alint is dual-licensed the same way, with no additional terms.
