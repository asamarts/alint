# alint

[![crates.io](https://img.shields.io/github/v/tag/asamarts/alint?sort=semver&label=crates.io)](https://crates.io/crates/alint)
[![CI](https://github.com/asamarts/alint/actions/workflows/ci.yml/badge.svg)](https://github.com/asamarts/alint/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue)](#license)

**Enforce the rules your repository assumes but never checks.**

Every repository has a shape it is supposed to keep. Every package carries a README. Workflows pin their actions to a commit SHA. Nobody commits `target/` or `node_modules/`. The lockfile lives at the root and nowhere else. Source files that need a license header have one. These rules are real, and a repo quietly rots when they slip, but they usually live in a reviewer's head or a paragraph of `CONTRIBUTING.md`, where nothing actually fails when a pull request ignores them.

alint writes those rules down in one `.alint.yml` and enforces them: on every pull request, in pre-commit, and in your editor. It reads the whole tree rather than one file at a time, so it sees things a per-language linter cannot: files that should or should not exist, naming conventions, values buried inside `package.json` or `Cargo.toml` or a workflow YAML, and relationships between files. It runs alongside ESLint, Clippy, or Ruff, picks up the ground that [Repolinter](https://github.com/todogroup/repolinter) left when it was archived in 2026, and ships as a single static Rust binary with no runtime to install.

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

`alint init` will write that file for you by detecting the ecosystems in your repo. Everything after it is optional: add rules, override severities, or switch a bundled rule off. The [60-second quickstart on alint.org](https://alint.org/docs/getting-started/quickstart/) walks through a first config end to end.

## How it works

A config is a list of rules, but you rarely write them all by hand. `extends:` composes **rulesets**, and alint ships 22 of them compiled into the binary, so there is no download and no network round trip. Each `extends:` line pulls in a set of rules; most are gated on what the repo actually contains, so listing the Rust ruleset in a repo with no Rust is a silent no-op. From there you add, override, or disable individual rules by id.

A rule is three things: a **kind** (the check to run, such as `file_exists`, `filename_case`, or `pair`), the **paths** it applies to, and a **severity**. alint has 105 rule kinds across 13 families. A rule can also carry a `when:` condition (run only on the default branch, only when some file exists) and a `fix:` (how to repair a violation when one is found).

```yaml
# .alint.yml
version: 1
extends:
  - alint://bundled/oss-baseline@v1
rules:
  - id: workflows-pin-actions          # your own rule, on top of the ruleset
    kind: file_content_forbidden
    paths: [".github/workflows/*.yml"]
    pattern: 'uses:.*@v\d+$'            # a tag, not a commit SHA
    message: "Pin third-party actions to a full commit SHA"
    level: error
```

Under the hood, alint walks the repository once, in parallel, honoring `.gitignore`. It reads each file's bytes at most once and fans the rules out across cores, which is why a 100,000-file workspace bundle checks in about a second and a half and a million files in under twenty seconds ([benchmarks, per release](docs/benchmarks/HISTORY.md)). The engine, the rules, the eight output formats, and a language server for editors are all one static binary. There is no plugin system to install and nothing from Node, the JVM, or Python in the path. For the full picture, see [ARCHITECTURE.md](docs/design/ARCHITECTURE.md).

## What it can check

The 105 kinds group into a handful of themes. The [rule catalogue](docs/rules.md) has every kind with a YAML example; the highlights:

- **Files and structure.** Require or forbid specific files and directories, cap file size, line count, directory depth, and files per directory, and forbid committed build output or OS junk.
- **Naming.** Filename case and regex conventions, plus case-collision and Windows-reserved-name safety for cross-platform checkouts.
- **Content and hygiene.** Required headers and footers, forbidden patterns (merge markers, debug residue), trailing whitespace, final newlines, line endings, and line width.
- **Config values.** Read into JSON, YAML, TOML, XML, dotenv, INI, `.properties`, and HCL with RFC 9535 JSONPath: assert a value, match a pattern, require a key's absence, or validate a whole file against a JSON Schema. For example, require `package.json` to declare a `license`, or a workflow to set `permissions: contents: read`.
- **Cross-file relationships.** The primitives few other tools offer: `pair` (every `.proto` has its generated binding), `every_matching_has` (every `packages/*` has a README and a manifest), `unique_by` (no two crates share a name), `file_graph` (no import cycles, no orphaned modules), and `generated_file_fresh` (a checked-in generated file still matches its source).
- **Security and Unicode.** Trojan-Source bidirectional controls, zero-width characters, byte-order marks, and text-encoding sanity.
- **Git hygiene.** Commit-message shape, sign-off and GPG signing, denied paths, and blame age.
- **Metadata.** Executable bits and shebangs, symlink and submodule policy, portable-filename checks.

When no kind fits, the `command` kind runs a tool you already trust and turns its exit code into a violation, so a one-off check still lives in the same config as everything else.

## Fixing, not just finding

Plenty of violations are mechanical: a missing final newline, a stray byte-order mark, an unsorted `CODEOWNERS`, a license header sitting above the shebang instead of below it. `alint fix` repairs those in place. The ones that need a human decision it will show you or suggest, rather than guess.

Every fix carries a safety tier, so a bare `alint fix` never surprises you:

- **Safe** fixes apply on a plain `alint fix`: whitespace and newline hygiene, line endings, header placement, sorting a marked block, reflowing indentation width, and so on.
- **Unsafe** fixes change meaning or remove content, so they wait for `alint fix --unsafe-fixes`: deleting a file, a regex `replace`, dropping a committed artifact from git's index. (As of v0.17 this includes removing a file: a plain `alint fix` now *suggests* the deletion instead of doing it.)
- **Suggestions** are reported but never written, for violations with no mechanical repair.

Preview everything with `alint fix --dry-run`, or `alint fix --diff` for the exact edits. alint composes all of a file's edits in memory and writes each file once, atomically, then re-runs until the tree stops changing. A size limit (1 MiB by default) skips oversize files instead of rewriting them.

Because `extends:` can reach a URL, fixes that inject content (`replace`, `file_create`, `file_prepend`, and friends) coming from a remote or nested ruleset are quietly demoted to suggestions unless you opt that source into `trusted_extends:`, and fixes that shell out are refused from anywhere but your own top-level config. There are 26 fix ops in all, cross-referenced from the [rule catalogue](docs/rules.md).

## Adopt it without a flag day

You do not have to fix everything the first time alint runs. `alint baseline` records today's violations behind a content fingerprint, and `alint check --baseline` then passes on those and fails only on genuinely new ones. Drop it into CI as a blocking gate on day one and clean up the backlog on your own schedule, the same way you would introduce a type checker to an untyped codebase.

## Where it runs

- **Pull requests.** The [`asamarts/alint`](https://github.com/asamarts/alint) GitHub Action annotates changed lines inline or uploads SARIF to Code Scanning. `alint check --changed` lints only the files a PR touched.
- **Commits.** A [pre-commit](https://pre-commit.com/) hook (a prebuilt wheel, no toolchain) checks every commit; a manual `alint-fix` hook repairs on request.
- **Your editor.** `alint lsp` is a language server (diagnostics, hover-to-explain, apply-fix code actions) with packaged extensions for VS Code, JetBrains, and Zed, and ready configs for Neovim, Sublime Text, Emacs, and Helix.
- **Coding agents.** The `agent` output format carries a per-violation instruction and fix command, and `alint export-agents-md` keeps the directives in `AGENTS.md` or `CLAUDE.md` in sync with the rules alint actually enforces, so the agent and CI agree on the contract.
- **Any CI.** Eight output formats cover most pipelines: `human`, `json`, `sarif`, `github`, `markdown`, `junit`, `gitlab`, and `agent`. Exit codes are stable (`0` clean, `1` violations, `2` config error, `3` internal).

## Bundled rulesets

Twenty-two rulesets ship inside the binary, pinned to the version of alint you run and reachable as `alint://bundled/<name>@v1`. They fall into a few groups:

- **Ecosystem baselines**, each gated on a fact so it is a no-op off-ecosystem: `rust`, `node`, `python`, `go`, `java`, `dotnet`, `php`, plus the always-on `oss-baseline` (the README / LICENSE / hygiene starting point, and a clean migration target for Repolinter).
- **Monorepo overlays**: a `monorepo` base plus `monorepo/cargo-workspace`, `monorepo/pnpm-workspace`, and `monorepo/yarn-workspace`, which scope per-member checks to real package directories.
- **CI and tooling**: `ci/github-actions` (OpenSSF-guided workflow hardening), `tooling/editorconfig`, `docs/adr`, `hygiene/no-tracked-artifacts`, and `hygiene/lockfiles`.
- **Compliance and governance**: `compliance/reuse`, `compliance/apache-2`, and `apache/governance` (the Apache TLP release discipline that arrow, spark, and airflow each re-implement by hand).
- **Agent-aware**: `agent-hygiene` (the scratch docs, duplicate-versioned files, and debug residue that show up disproportionately in agent-authored commits) and `agent-context` (keeps `AGENTS.md` and friends honest).

Every ruleset ships non-blocking by default (`info` or `warning` for recommendations, `error` only for unambiguous bugs). Redeclare a rule id in your own config to change its severity or scope, or set `level: off` to drop it. Full per-ruleset rule lists are in the [catalogue](docs/rules.md#bundled-rulesets).

## On real repositories

alint ships [working configs for 30 open-source repos](examples/README.md), from single-language libraries to polyglot monorepos and 39k-file trees, each with a short writeup of what alint catches that the repo's own tooling misses. Writing them was how we found where alint earns its keep:

- Projects with **verify-script sprawl.** Kubernetes hand-maintains roughly 50 `hack/verify-*.sh` scripts; a dozen declarative rules cover the structural ones. apache/airflow runs over 100 pre-commit hooks, and about 40% map cleanly onto alint.
- Projects that **rely on a convention without checking it.** tokio has no validation scripts at all, yet alint catches 15 conventions its pipeline silently assumes. uv's 67-crate workspace discipline is enforced nowhere in CI today.
- Projects with **mature linters but no structural layer.** astral-sh/ruff ships 900+ Python lint rules, and none of them check ruff's own `publish = false` discipline on its internal crates. dotnet/runtime carries thousands of XML manifests whose structural invariants no existing tool checks.
- **Polyglot trees** no single per-language linter can see across: apache/arrow spans six languages, vercel/next.js is TypeScript and Rust, NixOS/nixpkgs is 39k files.

Start from whichever example is closest to your repo's shape to see what a real config looks like.

## What alint is not

alint checks the shape and contents of a repository, not the semantics of the code inside it. It is deliberately not:

- a code or AST linter (use [ESLint](https://eslint.org/), [Clippy](https://doc.rust-lang.org/clippy/), [Ruff](https://docs.astral.sh/ruff/))
- a SAST scanner (use [Semgrep](https://semgrep.dev/), [CodeQL](https://codeql.github.com/))
- an IaC scanner (use [Checkov](https://www.checkov.io/), [Conftest](https://www.conftest.dev/))
- a commit-message linter (use [commitlint](https://commitlint.js.org/))
- a secret scanner (use [gitleaks](https://github.com/gitleaks/gitleaks), [TruffleHog](https://github.com/trufflesecurity/trufflehog))

It runs underneath these, and its rules stay focused on the filesystem so the tools above can stay focused on the code.

## Install

```bash
# install.sh (Linux, macOS; x86_64 and aarch64)
curl -sSL https://alint.org/install.sh | bash

# Homebrew (macOS, Linuxbrew)
brew install asamarts/alint/alint

# crates.io
cargo install alint

# npm (also puts alint on PATH; use for Windows or a project-local dev dep)
npm install -g @asamarts/alint

# PyPI (uvx / pipx / pip; the wheel embeds the binary, so no Python in the hot path)
uvx alint check
uv tool install alint
```

`install.sh` detects your platform, downloads the matching tarball, verifies its SHA-256, and installs to `~/.local/bin`. The npm and PyPI packages ship the same prebuilt binary (no source build); the PyPI wheel installs cleanly where the npm shim cannot, including `--ignore-scripts`, offline mirrors, and Windows. A distroless multi-arch Docker image is published to ghcr.io on every release:

```bash
docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:v0.17.0 check
```

To build from source: `git clone https://github.com/asamarts/alint && cd alint && cargo build --release -p alint`. Full platform and channel detail is in the [installation guide](https://alint.org/docs/getting-started/installation/).

alint is telemetry-free and makes no network access at runtime, except for the `extends:` URLs you write yourself, which must be SRI-pinned. The threat model is in [SECURITY.md](SECURITY.md).

## Docs

- [alint.org](https://alint.org) is the narrative documentation: quickstart, concepts, cookbook, and the full rule and ruleset reference.
- [docs/rules.md](docs/rules.md) is the per-kind reference, one entry per rule kind with an example and its fix ops.
- [ARCHITECTURE.md](docs/design/ARCHITECTURE.md) covers the rule model, the DSL, the execution model, and the crate layout.
- [CHANGELOG.md](CHANGELOG.md) has the per-version history; [ROADMAP.md](docs/design/ROADMAP.md) has the plan through v1.0.
- [docs/benchmarks/](docs/benchmarks/) holds the methodology and per-release, per-platform results.

## Development

```bash
git clone https://github.com/asamarts/alint
cd alint
cargo test --workspace     # ~2,300 Rust tests, plus ~500 declarative end-to-end scenarios
cargo run -- check         # dogfood: alint lints its own repo
cargo bench -p alint-bench # criterion micro-benches
```

alint is a single Cargo workspace of nine crates; only `alint` and `alint-core` are published, the rest are internal. End-to-end tests live in `crates/alint-e2e/scenarios/` as declarative YAML, so adding a scenario is adding a file, and CLI snapshots live under `crates/alint/tests/cli/` via `trycmd`. CI runs as per-job bash scripts under `ci/scripts/` that behave the same locally and in GitHub Actions. Contributions are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Dual-licensed under either [Apache License 2.0](LICENSE-APACHE) or the [MIT License](LICENSE-MIT), at your option. Unless you state otherwise, any contribution you submit for inclusion in alint is dual-licensed the same way, with no additional terms.
