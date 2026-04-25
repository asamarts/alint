---
title: Changelog
---

All notable changes to alint are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.10] — 2026-04-25

Three new content-family rule kinds rounding out the family.
Schema-compatible; every v0.4.9 config runs unchanged. JSON /
SARIF / GitHub outputs byte-equivalent.

### Added

- **`file_max_lines`** (alias `max_lines`). Mirror of
  `file_min_lines`: files in scope must have AT MOST
  `max_lines` lines. Same `wc -l` accounting. Catches the
  everything-module anti-pattern.
- **`file_footer`** (alias `footer`). Mirror of `file_header`
  anchored at the END of the file: the last `lines:` lines
  must match a regex. Use cases: license footers, signed-off-by
  trailers, generated-file sentinels. Fix op: `file_append`.
- **`file_shebang`** (alias `shebang`). First line of each
  file must match a regex. Pairs with `executable_has_shebang`
  (which checks shebang *presence*) — `file_shebang` checks
  shebang *shape*, e.g. `^#!/usr/bin/env bash$` to enforce a
  specific interpreter. Defaults to `^#!` (presence only).

  Brings the rule catalogue to ~55 kinds.

- 6 new e2e scenarios covering pass/fail paths for the three
  new kinds.

## [0.4.9] — 2026-04-25

Java bundled ruleset. Schema-compatible; every v0.4.8 config
runs unchanged. JSON / SARIF / GitHub outputs byte-equivalent.

### Added

- **`alint://bundled/java@v1`** (10 rules). Maven + Gradle
  hygiene, gated `when: facts.is_java`:
  - `java-manifest-exists` — `pom.xml`, `build.gradle`, or
    `build.gradle.kts` at the root (error).
  - `java-build-wrapper-committed` — `mvnw` / `gradlew` checked
    in for reproducible builds (info).
  - `java-no-tracked-target` / `java-no-tracked-build` —
    Maven's `target/` and Gradle's `build/` not committed.
    Both use `git_tracked_only: true` (the v0.4.8 primitive)
    so a developer's locally-built directories stay silent;
    only directories whose contents made it into git's index
    fire (error).
  - `java-no-class-files` — `*.class` files not committed
    (`git_tracked_only: true`, error).
  - `java-sources-pascal-case` — PascalCase filenames for
    `*.java`, with `package-info.java` / `module-info.java`
    excluded (warning).
  - `java-sources-final-newline` /
    `java-sources-no-trailing-whitespace` — text hygiene,
    auto-fixable (info).
  - `java-sources-no-bidi` / `java-sources-no-zero-width` —
    Trojan Source defenses (error).

  Brings the bundled catalogue to 12 rulesets. The
  `git_tracked_only` rules in this ruleset are the first
  bundled use of v0.4.8's git-aware primitive — the
  `silent_on_locally_built_target` e2e scenario proves the
  wiring end-to-end.



First git-aware primitive lands. Schema-compatible; every v0.4.7
config runs unchanged. JSON / SARIF / GitHub outputs gain no new
keys.

### Added

- **`git_tracked_only: bool`** option on `RuleSpec`, currently
  honoured by `file_exists`, `file_absent`, `dir_exists`, and
  `dir_absent`. When `true`, the rule's `paths`-matched entries
  are intersected with `git ls-files`'s output so only files /
  directories actually in git's index participate. Closes the
  approximation gap documented on the
  [walker-and-gitignore concept page](https://alint.org/docs/concepts/walker-and-gitignore/):
  a `dir_absent` rule on `**/target` with `git_tracked_only: true`
  fires only when `target/` was actually committed, never on a
  developer's locally-built `target/` (gitignored or not). Outside
  a git repo, or when `git` isn't on PATH, the tracked-set is
  empty and rules with the flag set become silent no-ops — the
  right default for "don't let X be committed" semantics.

  ```yaml
  - id: target-not-tracked
    kind: dir_absent
    paths: "**/target"
    git_tracked_only: true
    level: error
  ```

  Other rule kinds currently ignore the field; we'll extend
  coverage as concrete use cases come up. The roadmap'd
  `git_no_denied_paths` and `git_commit_message` primitives are
  still pending.

### Changed

- `alint-core::Context` gains a `git_tracked: Option<&HashSet<PathBuf>>`
  field, plus `is_git_tracked` / `dir_has_tracked_files` helpers.
  External embedders constructing a `Context` by hand need to add
  `git_tracked: None`. The engine collects the set at most once
  per `run` / `fix`, only when at least one rule's
  `wants_git_tracked()` is true — zero cost when no rule opts in.
- `alint-testkit`'s `Given` block accepts an optional `git: { init,
  add, commit }` block so e2e scenarios can stand up a real git
  repo in their tempdir before alint runs.



Distribution breadth. Schema-compatible; every v0.4.6 config
runs unchanged. JSON/SARIF/GitHub outputs byte-equivalent. No
Rust code changes — this release ships new install paths only.

### Added

#### Docker image

- **`ghcr.io/asamarts/alint`** — distroless multi-arch
  (`linux/amd64`, `linux/arm64`) image based on
  `gcr.io/distroless/static-debian12:nonroot`. Built by the
  release workflow from the same statically-linked musl
  binaries shipped in the GitHub Release tarballs, so the
  in-image binary matches the tarballed one byte-for-byte.
  Tags published per release: the exact git tag (`:v0.4.7`),
  the bare semver (`:0.4.7`), the `<major>.<minor>` channel
  (`:0.4`), and `:latest`.

  ```bash
  docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:latest
  ```

  Runs as the distroless `nonroot` user (UID 65532). For
  `alint fix` workflows that need to write with host
  ownership, pass `-u $(id -u):$(id -g)`.

#### Homebrew tap

- **`asamarts/alint`** — dedicated Homebrew tap at
  [asamarts/homebrew-alint](https://github.com/asamarts/homebrew-alint)
  shipping a `Formula/alint.rb` that resolves the right
  pre-built tarball for each platform (macOS arm64 + x86_64,
  Linuxbrew arm64 + x86_64) and verifies its SHA-256.

  ```bash
  brew tap asamarts/alint
  brew install alint
  ```

  The formula is regenerated on every tagged release by a new
  `homebrew` job in `.github/workflows/release.yml` driving
  `ci/scripts/update-homebrew-formula.sh`. The script takes
  SHAs directly from the release's `SHA256SUMS` artifact —
  no re-download, no re-build — and pushes via a per-repo
  ed25519 deploy key scoped to the tap.

### Infrastructure

- New release-workflow jobs: `docker` (builds + pushes the
  multi-arch image to ghcr.io) and `homebrew` (regenerates
  the formula and pushes to the tap). Both run after the
  existing `build` / `release` jobs; failures there skip the
  distribution jobs cleanly.
- New script `ci/scripts/update-homebrew-formula.sh` emits a
  complete `Formula/alint.rb` given `VERSION` + a
  `SHA256SUMS` path. Handles both `sha256sum`-style and
  Windows-mode (`*file`) sum lines, errors clearly on
  missing platforms, validates required env up front.
- New test harness `ci/scripts/test-update-homebrew-formula.sh`
  (14 assertions — happy path, asterisk-prefix handling,
  missing-platform + missing-env error paths, version /
  license / test-block shape). Wired into `ci/scripts/test.sh`
  so it runs on every CI pass.



Ecosystem coverage + debugging ergonomics. Schema-compatible;
every v0.4.5 config runs unchanged. JSON output unchanged for
existing commands; SARIF and GitHub outputs byte-equivalent.

### Added

#### Two new bundled rulesets

- **`alint://bundled/python@v1`** (7 rules). Canonical
  Python-project hygiene:
  - `python-manifest-exists` — pyproject.toml / setup.py /
    setup.cfg at the root (error).
  - `python-has-lockfile` — uv.lock / poetry.lock / Pipfile.lock
    / pdm.lock (warning).
  - `python-pyproject-declares-name` — PEP 621 `$.project.name`
    via `toml_path_matches` (warning).
  - `python-pyproject-declares-requires-python` — a
    `requires-python` floor via `toml_path_matches` on
    `$.project['requires-python']` (info).
  - `python-module-snake-case` — PEP 8 snake_case filenames
    for top-level and `src/**/*.py` (info).
  - `python-sources-final-newline` + `python-sources-no-trailing-whitespace`
    (info, auto-fixable).
  - `python-sources-no-bidi` — Trojan Source defense (error).

  Every rule gated with `when: facts.is_python`, so the ruleset
  silently no-ops on non-Python repos.

- **`alint://bundled/go@v1`** (7 rules). Go-module hygiene:
  - `go-mod-exists` — go.mod at the root (error).
  - `go-sum-exists` — go.sum at the root (warning).
  - `go-mod-declares-module-path` — `module <path>` directive
    (error, `file_content_matches`).
  - `go-mod-declares-go-version` — `go <major>.<minor>` directive
    (warning, `file_content_matches`).
  - `go-sources-no-bidi` / `go-sources-no-zero-width` —
    Trojan Source defenses (error).
  - `go-sources-final-newline` (info, auto-fixable).

  Every rule gated with `when: facts.is_go`.

  Brings the bundled catalog to eleven rulesets.

#### `alint facts` subcommand

- New top-level subcommand that evaluates every `facts:` entry
  in the effective config and prints the resolved value.
  Debugging aid for `when:` clauses — quickly answers "did my
  `facts.is_python` actually match?" without running the full
  check pass. Supports `--format human` (columnar) and
  `--format json` (`{facts: [{id, kind, value}, ...]}`).

### Changed

- `alint-core::FactKind::name()` added — returns the YAML
  discriminator string (`any_file_exists`, `count_files`, etc.).
  Used by the `facts` subcommand's renderers; available to
  external embedders.



Supply-chain hardening ruleset + composition ergonomics.
Schema-compatible; every v0.4.4 config runs unchanged. JSON
output gains no new keys; SARIF and GitHub outputs are
byte-equivalent.

### Added

#### New bundled ruleset

- **`alint://bundled/ci/github-actions@v1`** (3 rules). GitHub
  Actions hardening guided by the two OpenSSF Scorecard checks
  with the strongest supply-chain signal:
  - `gha-workflow-contents-read` — every workflow declares
    `permissions.contents: read` at the workflow level
    (`yaml_path_equals`, warning).
  - `gha-pin-actions-to-sha` — every `uses:` across every job
    is pinned to a 40-char commit SHA, not a mutable tag
    (`yaml_path_matches` with `if_present: true`, warning).
  - `gha-workflow-has-name` — every workflow declares a
    `name:` so the Actions UI shows something friendlier than
    the filename (`yaml_path_matches`, info).

  Scoped to `.github/workflows/*.y{,a}ml`, so it no-ops in
  repos that don't use GitHub Actions. Brings the bundled
  catalogue to nine rulesets.

#### Structured-query `if_present`

- **`if_present: true`** option on every structured-query rule
  kind (`{json,yaml,toml}_path_{equals,matches}`). When
  enabled, a JSONPath query that returns zero matches is
  silently OK — only actual matches that fail the op produce
  violations. Preserves the default "missing = violation"
  semantics (`if_present: false`, the existing behaviour).
  Required for conditional predicates like "every `uses:` is
  SHA-pinned" where a workflow with only `run:` steps
  shouldn't be flagged.

#### Selective bundled adoption

- **`only:` / `except:` on `extends:` entries.** An entry can
  now be a mapping that filters the inherited rule set by id
  before merging:

  ```yaml
  extends:
    - url: alint://bundled/oss-baseline@v1
      except: [oss-code-of-conduct-exists]      # drop one rule

    - url: alint://bundled/ci/github-actions@v1
      only: [gha-pin-actions-to-sha]            # keep one rule
  ```

  Filters resolve against the fully-resolved rule set of the
  entry (i.e. anything it transitively extends). `only:` and
  `except:` are mutually exclusive on a single entry. Listing
  an unknown id is a load-time error so typos don't silently
  drop anything.

  Closes the "all-or-nothing" limitation on bundled-ruleset
  adoption that forced users to extend + then restate overrides
  with `level: off` for every rule they wanted to skip.

### Changed

- `Config.extends` type changed from `Vec<String>` to
  `Vec<ExtendsEntry>`. `ExtendsEntry` is an untagged enum that
  accepts either a bare string (classic form) or a mapping
  `{url, only?, except?}`. YAML ergonomics unchanged for the
  string form; existing configs continue to parse as before.



Rule-catalogue expansion + README rewrite. Schema-compatible;
every v0.4.3 config runs unchanged. JSON output gains no new
keys; SARIF and GitHub outputs are byte-equivalent.

### Added

#### Content-family additions

- **`file_min_size`** — files in scope must be at least
  `min_bytes` bytes. Complements `file_max_size`. Picks up the
  "zero-byte LICENSE" case that passes `file_exists` but carries
  no information.
- **`file_min_lines`** — files in scope must have at least
  `min_lines` lines (`wc -l` semantics: every `\n` terminates a
  line, plus one more when the file has trailing unterminated
  content). Catches the classic "README is a title plus
  `TODO`" stub. Both kinds register short aliases (`min_size`,
  `min_lines`) alongside the prefixed names.

#### Structured-query family (six new rule kinds)

JSONPath (RFC 9535) queries over JSON / YAML / TOML documents,
powered by `serde_json_path`. YAML and TOML files are
deserialized through serde into a `serde_json::Value` so the
same path-expression engine applies to all three. Missing
JSONPath matches are treated as violations (conservative — scope
narrowly or relax the path for optional keys); when a query
returns multiple matches, every match must satisfy the rule.
Unparseable files surface a single per-file violation rather
than being silently skipped.

- **`json_path_equals`** / **`json_path_matches`** — `equals`
  compares by value (string / number / bool / null); `matches`
  runs a regex against the string form of the matched value.
  Canonical use: enforce a `package.json` license, require a
  semver `version`, lock a `private: true` flag.
- **`yaml_path_equals`** / **`yaml_path_matches`** — same
  engine over YAML. Canonical use: lock GitHub Actions
  workflows to `permissions.contents: read`, require every
  `uses:` across every job to be pinned to a 40-char commit
  SHA.
- **`toml_path_equals`** / **`toml_path_matches`** — same
  engine over TOML. Canonical use: require `edition = "2024"`
  across every `Cargo.toml` in a workspace, enforce
  `$.project.version` semver in `pyproject.toml`.

#### `oss-baseline` ruleset extensions

- `oss-license-non-empty` — `file_min_size` at 200 bytes on the
  LICENSE, catching zero-byte placeholders.
- `oss-readme-non-stub` — `file_min_lines` at 3 on the README,
  gentle enough to pass for early-stage repos.

### Changed

- **README rewrite.** Replaced the single monolithic `.alint.yml`
  example with a 12-pattern cookbook covering the real-world use
  cases v0.4 now spans: bundled-ruleset adoption, composition
  overrides, structured queries against `package.json` / GitHub
  workflows / `Cargo.toml`, monorepo per-package rules via
  `for_each_dir`, nested-config subtree scoping, auto-fix
  hygiene, fact-gated conditionals, cross-file `pair` / `unique_by`,
  and the security-family bans. Bumped the family count from
  ten to eleven and the rule-kind count from ~42 to ~50.



Composition ergonomics + monorepo support + four new bundled
rulesets. Schema-compatible; every v0.4.2 config runs unchanged.
JSON output gains no new keys; SARIF and GitHub outputs are
byte-equivalent.

### Added

#### Composition

- **Field-level rule override.** Children in the `extends:`
  chain can specify only the fields that change. A common
  override shrinks from four lines to two:

  ```yaml
  # before — had to restate kind + paths to tweak level
  rules:
    - id: no-bak
      kind: file_absent
      paths: "**/*.bak"
      level: warning
  ```

  ```yaml
  # after — id + changed fields are enough; rest inherits
  rules:
    - id: no-bak
      level: warning
  ```

  The loader keeps rules as raw `serde_yaml_ng::Mapping`s
  through the `extends:` chain and field-merges by id. After
  all extends resolve, each merged mapping is deserialized
  once into a `RuleSpec` — a rule that never receives a
  `kind` anywhere in its chain surfaces as a clean error
  referencing the offending id. Facts still replace wholesale
  by id (their kind is a discriminated union).

- **Nested `.alint.yml` discovery for monorepos.** Opt in with
  `nested_configs: true` on the root config. The loader walks
  the tree (respecting `.gitignore` + `ignore:`), picks up
  every nested `.alint.yml` / `.alint.yaml`, and prefixes each
  nested rule's path-like fields (`paths`, `select`, `primary`)
  with the nested config's relative directory. A rule declared
  in `packages/frontend/.alint.yml` with `paths: "**/*.ts"`
  evaluates as if it read `paths: "packages/frontend/**/*.ts"`
  at the root.

  MVP guardrails: nested configs can only declare `version:`
  and `rules:`; every nested rule must have at least one
  scope field; absolute paths and `..`-prefixed globs are
  rejected; rule-id collisions across configs error with a
  clear message (per-subtree overrides are a follow-up).

#### Bundled rulesets

Four new rulesets pulled from a research pass across
Turborepo/Nx/Bazel/Cargo/pnpm docs, OpenSSF Scorecard, and
Repolinter's archived corpus. Buildable on the existing
primitive set — no new rule kinds required.

- **`alint://bundled/hygiene/no-tracked-artifacts@v1`** — 11
  rules. `dir_absent` on `node_modules`, `target`, `dist`,
  `build`, `out`, `.next`, `.nuxt`, `.svelte-kit`, `.turbo`,
  `coverage`, `__pycache__`, `.venv`, `.mypy_cache`,
  `.pytest_cache`, `.ruff_cache`, `.bundle`, `vendor/bundle`,
  `.go-build`. `file_absent` on `.DS_Store`, `._*`, `Thumbs.db`,
  `desktop.ini`, `*~`, `*.swp`, `*.swo`, `*.bak`, `*.orig`,
  `.env`, `.env.local`, `.env.*.local`, `.env.development` /
  `production` / `staging` (`.env.example` is exempt). 10 MiB
  size gate. Several rules auto-fixable via `file_remove`.

- **`alint://bundled/hygiene/lockfiles@v1`** — 7 rules, one per
  package manager (npm / pnpm / yarn / bun / Cargo / Poetry /
  uv). Each uses an `include/exclude` path pair so the root
  lockfile is exempted while nested copies are flagged as a
  workspace-misconfiguration smell.

- **`alint://bundled/tooling/editorconfig@v1`** — 3 info-level
  rules: root `.editorconfig` + `.gitattributes` exist, and
  `.gitattributes` contains a `text=` normalization directive.

- **`alint://bundled/docs/adr@v1`** — 4 rules. Files under
  `docs/adr/` match `NNNN-kebab-case-title.md`; each ADR has
  `## Status`, `## Context`, `## Decision` sections. Gap-free
  ADR numbering deferred to a future `numeric_sequence`
  primitive.

Bundled catalog now: 8 rulesets (4 ecosystem + 4 namespaced).
Slash-namespaced names (`hygiene/*`, `tooling/*`, `docs/*`)
route through the existing `alint://bundled/<name>@<rev>` URI
scheme — the `@` separator parses cleanly around slashes in
the name.

#### Config

- **`nested_configs: true`** field on the root `Config` to
  opt in to nested-config discovery.

### Changed

- **`extends:` schema description** refreshed to cover SRI
  syntax, `alint://bundled/` URLs, merge semantics, and the
  `level: off` disable idiom. Old description claimed HTTPS
  was "reserved for a future version" (shipped in v0.2.1).

### Tests

Workspace: 422 → 437 tests (+15). Includes 6 new unit tests
on nested-discovery, 3 e2e on field-level override, 2 e2e on
nested discovery, 4 e2e on Phase A bundled rulesets.

## [0.4.2] — 2026-04-22

Pretty-output overhaul of the `human` formatter. Schema-compatible;
every v0.4.1 config still runs, every JSON/SARIF/GitHub output is
byte-equivalent. Only the human-mode rendering and three new global
CLI flags (`--color`, `--ascii`, `--compact`) are new.

### Added

- **Grouped-by-file layout** — violations now render under a
  dim section header per file (`─── src/foo.rs ─────…`), with
  a leading "Repository-level" bucket for path-less findings.
  Each violation shows `<sigil>  <level>  <rule-id>  [fixable]`
  on one line and the message (optionally prefixed with
  `line:col`) on the next. Policy URLs render as `docs: <url>`
  immediately under the relevant violation.
- **Per-severity summary** — `Summary (N violations): ✗ 2 errors
  ⚠ 1 warning  ℹ 5 info` + `X passing · Y failing · Z
  auto-fixable` + a call-to-action line `→ run `alint fix` to
  resolve N fixable violation(s).` when anything's auto-fixable.
  All-passed gets a concise green `✓ All N rule(s) passed.`.
- **`--color <auto|always|never>` global flag.** Defaults to
  `auto` — honors `NO_COLOR`, `CLICOLOR_FORCE`, and TTY status
  via `anstream::AutoStream`. JSON / SARIF / GitHub formats are
  unaffected.
- **`--ascii` global flag** forces ASCII glyphs (e.g. `x`/`!`/`i`
  instead of `✗`/`⚠`/`ℹ`, `---` instead of `───`). Auto-enabled
  when `TERM=dumb`.
- **`--compact` global flag** switches to one-line-per-violation
  output suitable for editors, `grep`, `wc -l`. Format:
  `path:line:col: level: rule-id: message  [fixable]`, with
  `<repo>` as the pseudo-path for path-less findings.
- **OSC 8 hyperlinks on policy URLs** when the terminal supports
  them (detected via `supports-hyperlinks`). Modern terminals
  (iTerm2, Kitty, WezTerm, Alacritty, VSCode, GNOME Terminal,
  Windows Terminal) make the `docs:` URL clickable without any
  visible change. Older terminals see the plain URL they always
  saw.
- **`RuleResult.is_fixable`** exposed on the JSON output as
  `fixable: bool`, letting tooling decide whether to prompt
  users toward `alint fix` without cross-referencing rule
  metadata.

### Changed

- **Terminal-width-aware section separators.** Auto-detected via
  `terminal_size` on TTY, clamped to `[40, 120]` cols so wide
  terminals don't produce unreadably long rules and narrow ones
  still get some visual fill. Falls back to 80 cols off-TTY.
- **Tightened vertical density** — no blank lines between
  violations within a bucket. Visual separation still reads
  clearly because the colored sigil anchors at column 2 while
  continuation lines indent to column 14. A typical 8-violation
  run drops from ~36 to ~27 lines of output.

### Internal

- New `alint-output::style` module centralizing role-based
  `anstyle::Style` constants, `GlyphSet` (Unicode / ASCII), and
  `HumanOptions` (plumbing for glyphs / hyperlinks / width /
  compact). Swapping the palette is a one-file edit.
- `Format::write_with_options` / `Format::write_fix_with_options`
  added; existing `write` / `write_fix` remain as default-opts
  shims so external embedders compile unchanged.

### Dependencies

- `anstyle` + `anstream` (ANSI styling with built-in `NO_COLOR` /
  TTY handling).
- `supports-hyperlinks` (OSC 8 detection).
- `terminal_size` (column-width detection).

## [0.4.1] — 2026-04-21

Packaging fix. v0.4.0 is functionally identical but failed to
publish beyond `alint-core` on crates.io — the bundled-rulesets
`include_str!` paths crossed the crate boundary, so
`cargo publish` for `alint-dsl` couldn't find
`rulesets/v1/*.yml` when packaging the tarball.

### Fixed

- **Move `rulesets/` → `crates/alint-dsl/rulesets/`**. The
  rulesets now live inside the crate that embeds them, so
  `cargo publish` picks them up automatically. Compile-time
  `include_str!` paths in `bundled.rs` change from
  `"../../../rulesets/…"` to `"../rulesets/…"`. No user-visible
  behaviour change — the `alint://bundled/<name>@<rev>` URI
  scheme and all four rulesets work identically.

### Known leftover

- `alint-core@0.4.0` is live on crates.io (it published
  successfully before the packaging error stopped the chain).
  It's functionally identical to `alint-core@0.4.1` and nothing
  transitively depends on it — safe to ignore or yank later.

## [0.4.0] — 2026-04-21

Headline: **bundled rulesets**. The single biggest adoption
lever identified during pre-launch review — reduces onboarding
from "write a ruleset" to "add one `extends:` line." Also lands
pre-commit framework integration so any pre-commit user can
adopt alint with 4 lines of YAML.

### Added

#### Bundled rulesets

- **`alint://bundled/<name>@<rev>` URI scheme** for offline
  resolution of built-in rulesets. Rulesets live under
  `rulesets/<rev>/<name>.yml` and are embedded in the binary via
  `include_str!` at compile time. Cycle-safe, leaf-only — a
  bundled ruleset cannot itself declare `extends:` and cannot
  introduce `custom:` facts, inheriting the same safety guards
  as HTTPS extends.
- **`alint://bundled/oss-baseline@v1`** — 9 rules. Community
  docs (README, LICENSE, SECURITY.md, CODE_OF_CONDUCT.md,
  .gitignore) + merge-marker + bidi-control bans +
  trailing-whitespace / final-newline hygiene (auto-fixable).
- **`alint://bundled/rust@v1`** — 10 rules. Cargo.toml /
  Cargo.lock / rust-toolchain existence, no tracked `target/`,
  snake_case source filenames, Trojan-Source defenses. Every
  rule gated `when: facts.is_rust` so extending it from a
  polyglot repo is a safe no-op outside Rust trees.
- **`alint://bundled/node@v1`** — 8 rules. package.json +
  lockfile (npm / pnpm / yarn / bun), no tracked `node_modules/`
  or common build outputs, Node version pinned via `.nvmrc` /
  `.node-version` / `.tool-versions`, JS/TS source hygiene.
  Gated `when: facts.is_node`.
- **`alint://bundled/monorepo@v1`** — 4 rules. Every directory
  under `{packages,crates,apps,services}/*` has a README +
  ecosystem manifest; unique basenames. Pair with rust@v1 /
  node@v1 for per-package ecosystem checks.

#### Distribution

- **`.pre-commit-hooks.yaml`** at the repo root exposes two
  hooks for [pre-commit](https://pre-commit.com/) users:
  - `alint` — runs `alint check`; non-mutating.
  - `alint-fix` — runs `alint fix`; `stages: [manual]` by
    default so it only runs when explicitly invoked via
    `pre-commit run alint-fix`.

  Both use `language: rust`, so pre-commit builds alint on
  first run — zero install step.

### Changed

- **README quickstart** gains a "Bundled rulesets (one-line
  baseline)" section and a "pre-commit" subsection under "Use
  in CI".
- **`docs/rules.md`** gains a Bundled rulesets section with a
  per-ruleset table (rule id / kind / default level / fix op)
  for every shipped ruleset, plus the override pattern.
- **ROADMAP**: the original v0.4 scope (structured-query
  primitives, git-aware primitives, Homebrew / Docker / npm,
  markdown/junit/gitlab outputs, `command` plugin, nested
  config discovery) rolls forward to v0.5. Bundled rulesets
  moved from v0.5 → v0.4 because they're the largest
  single-step adoption lever.

### Compatibility

- Schema version remains `1`. Every v0.3 config runs unchanged.
- No changes to the `Rule`, `Fixer`, or `Engine` APIs.
  `alint-dsl` gains a new public `bundled` module; the existing
  `load` / `load_with` entry points are unchanged.

## [0.3.2] — 2026-04-21

Patch release fixing a broken `action.yml` that affects every
`asamarts/alint@v0.2.1` / `v0.3.0` / `v0.3.1` consumer. No code
changes to the CLI; no schema changes.

### Fixed

- **`action.yml`** — the `outputs.sarif-file.description` value
  was an unquoted YAML scalar containing `` `format: sarif` ``.
  GitHub's Actions YAML parser recently tightened and now
  rejects the embedded `:` as an ambiguous nested mapping,
  causing `uses: asamarts/alint@<any-released-tag>` to fail at
  job-setup time with `Mapping values are not allowed in this
  context.`. The description is now double-quoted.
- **docs build** — rustdoc's `redundant_explicit_links` became
  warn-by-default in a recent stable; combined with the
  workspace's `RUSTDOCFLAGS="-D warnings"` it was breaking the
  `cargo doc` CI job. Fixed a redundant link target in
  `alint-testkit::runner`. Affects contributors / anyone
  building from source with the current stable; no effect on
  the release binary.

### Changed

- **Action self-test workflow** now pins `uses:` refs to
  `@main` so it exercises the current action code instead of
  an immutable (and, as of this week, unparsable) `@v0.2.1`.
  The explicit-version test still passes a stable release tag
  as the `version:` input — bumped from `v0.2.1` to `v0.3.1`
  so the downloaded CLI understands the dogfood config's v0.3
  rule kinds.

### Upgrade note

Consumers pinning `asamarts/alint@v0.2.1` / `v0.3.0` / `v0.3.1`
should bump to `@v0.3.2`. There are no API / CLI / config
changes — configs that worked under `@v0.3.1` continue to work
verbatim.

## [0.3.1] — 2026-04-21

Documentation-only patch release following v0.3.0. No code
changes; no schema changes; v0.3.0 configs run unchanged.

### Added

- **`docs/rules.md`** — per-rule user reference organised by the
  ten families (Existence, Content, Naming, Text hygiene,
  Security / Unicode, Encoding, Structure, Portable metadata,
  Unix metadata, Git hygiene, Cross-file). Each entry has a
  purpose, a small YAML example, and a pointer to its fix op if
  one exists.

### Changed

- **`docs/design/ARCHITECTURE.md`** — rule-catalogue section
  expanded with new family tables (Text hygiene, Security /
  Unicode sanity, Structure, Portable metadata, Unix metadata,
  Git hygiene) and a new Fix operations subsection listing
  every op with its rule-kind cross-reference.
- **`README.md`** — status line bumped from "v0.2 / 18 rules /
  4 families" to "v0.3 / ~42 rules / 10 families / 12 fix ops";
  GitHub Action references updated from `asamarts/alint@v0.2.1`
  to `@v0.3.0`.
- **`.alint.yml`** (dogfood) — expanded from 17 to 32 rules to
  exercise the v0.3 catalogue against alint's own tree. All
  rules pass.

## [0.3.0] — 2026-04-21

Rule-catalogue expansion. Adds ~25 new rule kinds across seven
phase commits plus one new fix op, covering categories other
repo-linters don't reach: Windows-name reserved words, bidi /
zero-width Unicode scanners, Unix-metadata checks, and
byte-level prefix/suffix. Also introduces the `fix_size_limit`
config knob and short-name aliases for the rules that don't
have a `dir_*` sibling.

### Added

#### Rule kinds (text hygiene — Phase 1)

- **`no_trailing_whitespace`** — flag trailing space/tab on any
  line. Fixable via `file_trim_trailing_whitespace` (preserves
  LF vs CRLF endings).
- **`final_newline`** — file must end with `\n`. Fixable via
  `file_append_final_newline`.
- **`line_endings`** — `target: lf | crlf`; every line must use
  the configured ending. Fixable via
  `file_normalize_line_endings`.
- **`line_max_width`** — cap line length in characters (not
  bytes); optional `tab_width` for tab expansion.

#### Rule kinds (security / Unicode — Phase 2)

- **`no_merge_conflict_markers`** — flag `<<<<<<< `, `=======`,
  `>>>>>>> ` markers at the start of a line.
- **`no_bidi_controls`** — flag Trojan-Source bidi overrides
  (U+202A–202E, U+2066–2069). Fixable via `file_strip_bidi`.
- **`no_zero_width_chars`** — flag body-internal zero-width
  characters (U+200B/C/D plus non-leading U+FEFF). Leading BOM
  is `no_bom`'s concern. Fixable via `file_strip_zero_width`.

#### Rule kinds (encoding + content fingerprint — Phase 3)

- **`file_is_ascii`** — every byte must be < 0x80.
- **`no_bom`** — flag UTF-8 / UTF-16 LE/BE / UTF-32 LE/BE
  byte-order marks. Fixable via `file_strip_bom`.
- **`file_hash`** — assert a SHA-256 digest for specific files
  (rules-as-tripwire for generated artefacts).

#### Rule kinds (structure — Phase 4)

- **`max_directory_depth`** — cap how deep the tree may go.
- **`max_files_per_directory`** — cap per-directory fanout.
- **`no_empty_files`** — flag zero-byte files. Fixable via
  `file_remove`.

#### Rule kinds (portable metadata — Phase 5)

- **`no_case_conflicts`** — flag paths that collide under a
  case-insensitive filesystem (macOS HFS+/APFS, Windows NTFS
  defaults).
- **`no_illegal_windows_names`** — reject CON/PRN/AUX/NUL,
  COM1-9, LPT1-9 (case-insensitive, regardless of extension),
  trailing dots/spaces, and the reserved chars `<>:"|?*`.

#### Rule kinds (Unix metadata + git — Phase 6)

- **`no_symlinks`** — flag tracked paths that are symbolic
  links. Fixable via `file_remove`.
- **`executable_bit`** — `require: true|false`; enforce or
  forbid the `+x` bit. Unix-only; no-op on Windows.
- **`executable_has_shebang`** — `+x` files must begin with
  `#!`. Unix-only.
- **`shebang_has_executable`** — files starting with `#!` must
  have `+x` set. Unix-only.
- **`no_submodules`** — flag `.gitmodules` at the repo root.
  Always targets `.gitmodules` (no `paths` override). Fixable
  via `file_remove`.

#### Rule kinds (hygiene + fingerprint — Phase 7)

- **`indent_style`** — `style: tabs|spaces`, optional `width`
  for spaces; every non-blank line must indent with the
  configured style.
- **`max_consecutive_blank_lines`** — `max: N`; cap runs of
  blank lines. Fixable via new op `file_collapse_blank_lines`.
- **`file_starts_with`** — byte-level prefix check. Works on
  binary files, unlike `file_header` which is UTF-8 text.
- **`file_ends_with`** — byte-level suffix check.

#### Fix ops

- **`file_trim_trailing_whitespace`** — strip trailing space/tab
  on every line (preserves line endings).
- **`file_append_final_newline`** — add `\n` when missing.
- **`file_normalize_line_endings`** — rewrite to the parent
  rule's `lf` / `crlf` target.
- **`file_strip_bidi`** — remove U+202A–202E, U+2066–2069.
- **`file_strip_zero_width`** — remove U+200B/C/D and
  body-internal U+FEFF.
- **`file_strip_bom`** — strip a leading UTF-8/16/32 BOM.
- **`file_collapse_blank_lines`** — collapse blank-line runs to
  the parent rule's `max`.

#### Config + ergonomics

- **`fix_size_limit`** (top-level config field) — maximum bytes
  a content-editing fix will touch. Default 1 MiB; explicit
  `null` disables the cap; path-only fixes (`file_create`,
  `file_remove`, `file_rename`) ignore it. Over-limit files
  report `Skipped` with a stderr warning.
- **Short-name rule aliases** — rules without a `dir_*` sibling
  also resolve under their unprefixed name:
  `content_matches`, `content_forbidden`, `header`, `max_size`,
  `is_text`. `file_exists` / `file_absent` keep the prefix
  because they mirror `dir_exists` / `dir_absent`.

### Changed

- JSON Schema (`schemas/v1/config.json`) gains every new rule
  kind, fix op, and the `fix_size_limit` field. Root and
  in-crate copies stay byte-identical via the drift-guard test.

### Compatibility

- Schema version remains `1`. Every v0.2 config runs unchanged
  under v0.3. The `Rule` and `Fixer` traits gained no new
  required methods; out-of-tree implementations compile
  unmodified. `Engine::with_fix_size_limit` is additive.

## [0.2.1] — 2026-04-20

Patch release. Finishes the v0.2 roadmap item that didn't make it
into v0.2.0: `extends:` composition.

### Added

- **`extends:` for local files.** A config can inherit rules,
  facts, vars, and ignore globs from another YAML file on disk
  (relative or absolute path). Resolution is recursive with cycle
  detection; merge is id-based with child-overrides-parent
  semantics for rules + facts, dict-merge for vars, and
  concatenation for ignore globs.
- **`extends:` for HTTPS URLs with SHA-256 SRI.** Remote entries
  take the form `https://.../foo.yml#sha256-<64 hex chars>`. The
  SRI is non-negotiable: URLs without it are rejected. Responses
  are verified against the declared hash before use and cached
  atomically on disk at `<user-cache-dir>/alint/rulesets/<sri>.yml`.
  `ureq` is the underlying HTTPS client (rustls for TLS — no
  OS-native crypto linking).
- **`alint_dsl::load_with(path, &LoadOptions)`** for embedders and
  tests that need to pin the cache path or override the fetcher.
- **Action self-test workflow** (`.github/workflows/action-selftest.yml`)
  that dogfoods `asamarts/alint@<tag>` across `ubuntu-latest` on
  four configurations (default, `format: sarif` + JSON-parse
  assertion, `format: json`, explicit `version:` input). Catches
  regressions in the release-tarball → install.sh → binary
  distribution chain that in-process tests don't exercise.

### Security

- HTTPS `extends:` requires SRI on every entry; no
  trust-on-first-use. `http://` schemes are rejected outright.
  Cache entries are re-verified against SRI on read, so a
  tampered on-disk cache fails loudly rather than serving bad
  content. Body size is capped at 16 MiB to bound memory against
  a hostile server.

### Known limitations

- Remote configs cannot themselves contain `extends:` (nested
  remote extends deferred — a relative path inside a fetched
  config has no principled base for resolution).
- The config-wide `respect_gitignore` field cannot distinguish
  "unset" from the `true` default during merge; the child's
  value wins unconditionally.

### Added dependencies

- `ureq` (rustls TLS), `sha2`, `directories`. Release-binary size
  impact: ~+1.5–2 MiB, mostly from rustls' embedded root certs.

## [0.2.0] — 2026-04-19

Second release. The theme is composition and remediation: cross-file rules,
conditional gating, auto-fix, and two new output formats. Every v0.1 config
continues to validate and run under v0.2 without changes.

### Added

#### Rule kinds

- **Cross-file primitives (7 kinds)** — `pair`, `for_each_dir`, `for_each_file`,
  `dir_contains`, `dir_only_contains`, `unique_by`, `every_matching_has`. These
  support path-template substitution (`{dir}`, `{stem}`, `{ext}`, `{basename}`,
  `{path}`, `{parent_name}`) and nested rule instantiation for per-iteration
  semantics.

#### Facts + conditional rules

- **Facts system** — repository properties evaluated once per run. Three kinds
  shipped: `any_file_exists`, `all_files_exist`, `count_files`. Referenced
  from `when:` clauses and rule messages.
- **`when:` expression language** — bounded recursive-descent parser with
  boolean logic (`and`, `or`, `not`), comparison operators (`==`, `!=`, `<`,
  `<=`, `>`, `>=`), `in` (list or substring), `matches` (regex), literal
  types (bool/int/string/list/null), and `facts.*` / `vars.*` identifiers.
  Parsed at rule-build time; gates both top-level rules and nested rules
  inside `for_each_*`.

#### `fix` subcommand

- **`alint fix [path]`** — applies mechanical corrections for violations whose
  rule declares a fix strategy. Five ops:
  - `file_create` (paired with `file_exists`) — writes declared content.
  - `file_remove` (paired with `file_absent`) — deletes the violating file.
  - `file_prepend` (paired with `file_header`) — injects content at the top,
    preserving UTF-8 BOM.
  - `file_append` (paired with `file_content_matches`) — appends content.
  - `file_rename` (paired with `filename_case`) — converts the stem to the
    rule's target case, preserving extension and parent directory.
- **`--dry-run`** previews the outcome without touching disk.
- **Safety** — `file_create` refuses to overwrite existing files;
  `file_rename` refuses to overwrite a collision; all ops skip cleanly with
  a diagnostic when preconditions aren't met.

#### Output formats

- **`sarif`** — SARIF 2.1.0 JSON, targeting GitHub Code Scanning's upload
  action. Each violation becomes a `result` with a `physicalLocation`
  anchored on the violating path.
- **`github`** — GitHub Actions workflow-command annotations
  (`::error title=...::`). Renders inline on PR file-changed view.

#### Distribution

- **Official GitHub Action** at `asamarts/alint@v0.2.0` — composite action
  wrapping `install.sh`. Inputs: `version`, `path`, `config` (multi-line),
  `format` (default `github`), `fail-on-warning`, `args`, `working-directory`.
  Output: `sarif-file` for feeding the Code Scanning uploader.

#### Testing infrastructure

- **`alint-testkit` + `alint-e2e`** (internal crates) — scenario-driven
  end-to-end tests. 51 YAML scenarios auto-generated into `#[test]`s via
  `dir-test`, 20 CLI snapshot tests via `trycmd`, and 4 property-based
  invariants via `proptest`. See the [ARCHITECTURE / testing
  sections](docs/design/ARCHITECTURE.md) for the rationale.

### Changed

- **Internal workspace crates published** — `alint-dsl`, `alint-rules`, and
  `alint-output` are now published on crates.io. They carry
  `description: "Internal: ... Not a stable public API."`; only `alint` and
  `alint-core` are semver-stable. This change is required so that
  `cargo install alint` resolves its transitive path-or-crates-io
  dependencies.
- **JSON Schema** (`schemas/v1/config.json`) gains the `fix:` block, every
  new rule kind, and the `facts:` + `when:` fields. Root + in-crate copies
  are kept byte-identical by a drift-guard test.

### Fixed

- **`install.sh`** no longer aborts on SIGPIPE when resolving the latest
  release tag. Previously, `curl | awk '{...; exit}'` under
  `set -o pipefail` caused curl to error out with exit 23 after awk's early
  exit; the fetch is now decoupled from the parse.

### Compatibility

- Config schema version remains `1`. Existing v0.1 configs run unchanged.
- The public API of `alint-core` has not been broken. The `Rule` trait
  gained a `fixer()` method with a `None` default, so out-of-tree rule
  implementations compile without modification.

## [0.1.0] — 2026-04-19

Initial release. MVP.

### Added

- 11 rule primitives — `file_exists`, `file_absent`, `dir_exists`,
  `dir_absent`, `file_content_matches`, `file_content_forbidden`,
  `file_header`, `filename_case`, `filename_regex`, `file_max_size`,
  `file_is_text`.
- Walker that honors `.gitignore`; globset-based scopes with Git-style
  `**` semantics (separator-aware).
- CLI subcommands: `check`, `list`, `explain`.
- Output formats: `human`, `json`.
- JSON Schema at `schemas/v1/config.json` for editor autocomplete.
- Benchmarks (criterion micros + hyperfine macros).
- Static binaries on GitHub Releases for Linux
  (x86_64/aarch64 musl), macOS (x86_64/aarch64), and Windows (x86_64).
- Install script (`install.sh`) with platform detection + SHA-256
  verification.
- Dogfood `.alint.yml` exercising the tool against its own repo.

[Unreleased]: https://github.com/asamarts/alint/compare/v0.4.3...HEAD
[0.4.3]: https://github.com/asamarts/alint/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/asamarts/alint/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/asamarts/alint/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/asamarts/alint/compare/v0.3.2...v0.4.0
[0.3.2]: https://github.com/asamarts/alint/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/asamarts/alint/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/asamarts/alint/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/asamarts/alint/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/asamarts/alint/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/asamarts/alint/releases/tag/v0.1.0
