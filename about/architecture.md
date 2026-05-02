---
title: Architecture
---

> Status: Living design document. Describes alint's internals for contributors
> and embedders. For the current scope by version, see [ROADMAP.md](./ROADMAP.md).

## Overview

alint is a language-agnostic linter for **repository structure, file existence, filename conventions, and file content rules**. It is a single static Rust binary that reads a declarative YAML config and enforces rules over a repository tree.

Examples of rules in scope:

- Does file `X` exist at path `Y`? Does directory `Z` contain file `W`?
- Do filenames under `components/` follow PascalCase?
- Does every `.c` file have a matching `.h` in the same directory?
- Does every `.java` file start with a required license-header comment?
- Is anything binary present under `src/`?
- Does `package.json`'s `license` field equal `"Apache-2.0"`?

Out of scope (explicitly — use the named tool instead):

- Code semantics / AST linting → ESLint, Clippy, ruff
- Static application security testing → Semgrep, CodeQL
- Infrastructure-as-code scanning → Checkov, Conftest, tfsec
- Commit-message linting → commitlint
- Secret scanning → gitleaks, trufflehog

The clarity of these non-goals is itself a feature.

## Design principles

1. **The repository tree is the input.** Every rule sees a unified file/directory index. The walk happens once per invocation.
2. **One DSL, four rule families.** *Layout* (exists/absent, directory contents), *content* (regex, header, hash, size, text/binary, structured-path queries), *naming* (case, regex, template), *cross-file* (pair, for-each, every-matching).
3. **Declarative by default, programmable at the edges.** YAML covers typical rules. A bounded expression language gates rules on facts. A plugin surface (command, later WASM) covers user-defined logic.
4. **Walk once, evaluate in parallel.** Single-pass walker; shared file index; `rayon` for rule-level parallelism.
5. **Respect ecosystem defaults.** `.gitignore` is honored by default. YAML is the config format. Case aliases (`PascalCase` / `pascalcase` / `pascal-case`) all parse.
6. **Every rule carries its own story.** Severity, message, `policy_url`, and optional `fix` are first-class fields.
7. **Modern output formats from day one.** Human, JSON, SARIF, GitHub annotations, JUnit.
8. **Single static binary.** Rust, no runtime dependency on Node, Ruby, or Python.

## Rule model

Every rule is a record:

- `id` — unique kebab-case identifier
- `kind` — the primitive rule type, namespaced (`file_exists`, `filename_case`, `pair`, ...)
- `level` — `error` | `warning` | `info` | `off`
- `paths` — the scope glob(s); accepts a string, an array (with `!negation`), or `{include, exclude}`
- `when` — optional expression gating rule application on facts
- `message` — human message; supports `{{vars.*}}` and `{{ctx.*}}` substitution
- `policy_url` — optional URL to a human-readable policy justification
- `fix` — optional fixer block
- kind-specific fields

Severity maps to exit codes: `error` with violations → 1; `warning` → 0 unless `--fail-on-warning`; `info` → 0; `off` → rule skipped. `off` is useful when overriding a rule inherited from an `extends`-ed ruleset.

The `Rule` trait is the unit of execution:

```rust
pub trait Rule: Send + Sync + std::fmt::Debug {
    fn id(&self) -> &str;
    fn level(&self) -> Level;
    fn policy_url(&self) -> Option<&str> { None }
    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>>;
}
```

Rules produce `Violation`s; the engine aggregates them into a `Report`.

## DSL

YAML, with a JSON Schema (draft 2020-12) maintained at [`schemas/v1/config.json`](../../schemas/v1/config.json) in this repository and embedded into `alint-dsl` at build time via `include_str!` (exposed as `alint_dsl::CONFIG_SCHEMA_V1`). Integration tests round-trip representative configs through a compliant validator so the schema and the engine's actual DSL stay in sync.

For editor autocomplete, reference the schema via the YAML language server pragma — either by a relative path (recommended inside this repo) or by the GitHub raw URL (for downstream users, once the repo is public):

```yaml
# yaml-language-server: $schema=./schemas/v1/config.json
# or: $schema=https://raw.githubusercontent.com/asamarts/alint/main/schemas/v1/config.json
```

```yaml
# .alint.yml
version: 1

extends:
  - url: https://raw.githubusercontent.com/example/rulesets/base.yaml
    sha256: "a1b2..."           # optional subresource integrity
  - path: ./team-policy.alint.yml

ignore:
  - "target/**"
respect_gitignore: true

vars:
  copyright_year: "2026"

facts:
  - id: has_rust
    any_file_exists: ["Cargo.toml"]

rules:
  - id: readme-exists
    kind: file_exists
    paths: ["README.md", "README", "README.rst"]
    root_only: true
    level: error
    message: "A README file is required at the repository root."

  - id: c-requires-h
    kind: pair
    primary: "**/*.c"
    partner: "{dir}/{stem}.h"
    level: error

  - id: components-pascalcase
    kind: filename_case
    paths: "components/**/*.{tsx,jsx}"
    case: pascal
    level: error

  - id: cargo-lock-checked-in
    when: facts.has_rust
    kind: file_exists
    paths: "Cargo.lock"
    root_only: true
    level: error
```

### Rule primitives

Not every primitive is available in every release — see [ROADMAP.md](./ROADMAP.md) for which ship in which version. The full taxonomy:

**Layout family**

| Kind | Purpose |
|---|---|
| `file_exists` | Any file matching `paths` must exist. `root_only: true` constrains to repo root. |
| `file_absent` | No file matching `paths` may exist. |
| `dir_exists` / `dir_absent` | Directory presence / absence. |
| `dir_contains` | Every directory matching `select` must contain files matching `require`. |
| `dir_only_contains` | Every directory matching `select` may contain only files matching `allow`. |

**Content family**

| Kind | Purpose |
|---|---|
| `file_content_matches` / `file_content_forbidden` | File contents must (not) match regex. Aliases: `content_matches`, `content_forbidden`. |
| `file_header` / `file_footer` | First / last N lines must match pattern. Alias for `file_header`: `header`. |
| `file_starts_with` / `file_ends_with` | Byte-level prefix / suffix check (works on binary files). |
| `file_hash` | Content SHA-256 matches expected. |
| `file_max_size` / `file_max_lines` | Upper bounds. Alias for `file_max_size`: `max_size`. |
| `file_is_text` / `file_is_binary` / `file_is_ascii` | Content is detected as text / binary / 7-bit ASCII. Alias for `file_is_text`: `is_text`. |
| `no_bom` | Flag a leading UTF-8 / UTF-16 / UTF-32 byte-order mark. Fixable via `file_strip_bom`. |
| `file_shebang` | First line is a shebang matching pattern. |
| `json_path_equals` / `json_path_matches` | JSONPath query returns expected value / matches regex. |
| `yaml_path_*` / `toml_path_*` | Same for YAML / TOML. |
| `json_schema_passes` | File validates against a JSON Schema. |

**Text-hygiene family**

| Kind | Purpose |
|---|---|
| `no_trailing_whitespace` | No line may end with space/tab. Fixable via `file_trim_trailing_whitespace`. |
| `final_newline` | File must end with `\n`. Fixable via `file_append_final_newline`. |
| `line_endings` | Every line uses the configured `target` (`lf` or `crlf`). Fixable via `file_normalize_line_endings`. |
| `line_max_width` | Cap line length in characters (optional `tab_width`). |
| `indent_style` | Every non-blank line indents with `tabs` or `spaces` (with optional `width`). Check-only. |
| `max_consecutive_blank_lines` | Cap runs of blank lines to `max`. Fixable via `file_collapse_blank_lines`. |

**Security / Unicode-sanity family**

| Kind | Purpose |
|---|---|
| `no_merge_conflict_markers` | Flag `<<<<<<<`, `=======`, `>>>>>>>` markers at line start. |
| `no_bidi_controls` | Flag Trojan-Source bidi controls (U+202A–202E, U+2066–2069). Fixable via `file_strip_bidi`. |
| `no_zero_width_chars` | Flag body-internal U+200B/C/D and non-leading U+FEFF. Fixable via `file_strip_zero_width`. |

**Structure family**

| Kind | Purpose |
|---|---|
| `max_directory_depth` | Cap how deep the tree may go. |
| `max_files_per_directory` | Cap per-directory fanout. |
| `no_empty_files` | Flag zero-byte files. Fixable via `file_remove`. |

**Naming family**

| Kind | Purpose |
|---|---|
| `filename_case` | Basename matches a case convention (`lower`, `upper`, `pascal`, `camel`, `snake`, `kebab`, `screaming-snake`, `flat`). |
| `filename_regex` | Basename matches a regex. |
| `filename_matches_template` | Basename matches a template with captures. |
| `path_case` | Every path segment matches a case convention. |
| `path_max_depth` | Relative path has at most N segments. |

**Cross-file family**

| Kind | Purpose |
|---|---|
| `pair` | For every file matching `primary`, a file matching the `partner` template must exist. |
| `for_each_file` / `for_each_dir` | For every matching file/dir, evaluate nested `require` rules with the entry as context. |
| `every_matching_has` | Sugar for a common `for_each` shape. |
| `unique_by` | No two files matching `select` may share the value of `key`. |

**Portable-metadata family**

Portability checks that reject tree shapes which look fine on one OS but break checkouts elsewhere.

| Kind | Purpose |
|---|---|
| `no_case_conflicts` | Flag pairs of paths that differ only by case (e.g. `README.md` + `readme.md`). They can't coexist on macOS HFS+/APFS or Windows NTFS defaults. |
| `no_illegal_windows_names` | Reject reserved device names (CON, PRN, AUX, NUL, COM1–9, LPT1–9 — case-insensitive, regardless of extension), trailing dots/spaces, and the chars `<>:"|?*`. |

**Unix-metadata family**

Unix-filesystem metadata checks. All rules in this family are `#[cfg(unix)]`-gated at the engine layer: they emit no violations on Windows, so configs remain portable.

| Kind | Purpose |
|---|---|
| `no_symlinks` | Flag tracked paths that are symbolic links (portability footgun on Windows + CI). Fixable via `file_remove`. |
| `executable_bit` | Assert every file in scope has (`require: true`) or lacks (`require: false`) the `+x` bit. No fix op — chmod auto-apply deferred. |
| `executable_has_shebang` | Every `+x` file must begin with `#!`. No fix op. |
| `shebang_has_executable` | Every file starting with `#!` must have `+x` set. No fix op. |

**Git-hygiene family**

| Kind | Purpose |
|---|---|
| `no_submodules` | Flag `.gitmodules` at the repo root. Always targets `.gitmodules` (no `paths` override). Fixable via `file_remove`. |
| `git_tracked_only` | Every matching file must be git-tracked. |
| `git_no_denied_paths` | Git tree must not contain paths matching pattern. |
| `git_commit_message` | Commits in range must match / not-match patterns. |

**Plugin family**

| Kind | Purpose |
|---|---|
| `command` | Shell out to an external command with `{path}`; non-zero exit = failure. |
| `wasm` | Evaluate a WASM plugin against the file / tree. |

### Fix operations

Rules that declare a `fix:` block opt in to automatic remediation. The op is a discriminated union keyed by op name; each rule kind accepts at most one op.

**Path-only ops** (ignore `fix_size_limit`):

| Op | Shape | Rule kinds |
|---|---|---|
| `file_create` | `{content, path?, create_parents?}` | `file_exists` |
| `file_remove` | `{}` | `file_absent`, `no_empty_files`, `no_symlinks`, `no_submodules` |
| `file_rename` | `{}` (target derived from rule config) | `filename_case` |

**Content-editing ops** (skipped on files over `fix_size_limit`; default 1 MiB, `null` disables):

| Op | Shape | Rule kinds |
|---|---|---|
| `file_prepend` | `{content}` | `file_header` |
| `file_append` | `{content}` | `file_content_matches` |
| `file_trim_trailing_whitespace` | `{}` | `no_trailing_whitespace` |
| `file_append_final_newline` | `{}` | `final_newline` |
| `file_normalize_line_endings` | `{}` (target read from parent rule) | `line_endings` |
| `file_strip_bidi` | `{}` | `no_bidi_controls` |
| `file_strip_zero_width` | `{}` | `no_zero_width_chars` |
| `file_strip_bom` | `{}` | `no_bom` |
| `file_collapse_blank_lines` | `{}` (max read from parent rule) | `max_consecutive_blank_lines` |

Over-limit content-editing ops report `Skipped` with a stderr warning instead of applying. Reads are streaming where possible; otherwise the file is loaded in full. Fixers run serially after parallel evaluation so the tree is mutated from a single thread.

### Path template tokens

Used in `partner`, nested `require`, messages, and rename fixers:

- `{dir}` — parent directory of the matched file
- `{path}` — full relative path
- `{basename}` — filename including extension
- `{stem}` — filename without the final extension
- `{ext}` — final extension without the dot
- `{parent_name}` — immediate parent directory name
- `{stem_kebab}`, `{stem_snake}`, `{stem_pascal}`, ... — transformed stems

### Scope and globbing

Globs compile via `globset`:

- `*` — any run of non-separator chars
- `**` — any number of path segments (own segment only)
- `?` — one non-separator char
- `[abc]`, `[a-z]` — character classes
- `{a,b,c}` — brace alternation
- `!pattern` — negation (arrays only)

Every rule's `paths` accepts one of three shapes:

```yaml
paths: "src/**/*.rs"                                       # single glob
paths: ["src/**/*.rs", "!src/**/testdata/**"]              # array with negation
paths: {include: ["src/**"], exclude: ["**/*.test.*"]}     # explicit pair
```

`.gitignore` is honored by default. `.alintignore` provides alint-specific exclusions. `ignore:` in config adds to the exclusion set.

### Facts and conditional rules

Facts are declarative properties of the repository, evaluated once per run and cached. `when` clauses gate rules on facts.

Fact kinds include `any_file_exists`, `all_files_exist`, `file_content_matches`, `detect: linguist` (primary languages), `detect: askalono` (SPDX license), `count_files`, `count_contributors`, `git_branch`, and `custom: {command: [...]}` (shell out, JSON stdout → value).

The `when` expression language is deliberately bounded:

- Operators: `==`, `!=`, `<`, `<=`, `>`, `>=`, `and`, `or`, `not`, `in`, `matches`
- Identifiers: `facts.<name>`, `vars.<name>`, `ctx.<name>`
- Literals: strings, numbers, booleans, null, lists

No user-defined functions, no recursion, no I/O. Examples:

```yaml
when: facts.has_rust
when: facts.primary_language in ["Rust", "Go"]
when: facts.has_rust and not facts.is_workspace_member
when: count_files("**/*.java") > 0
```

### Closest-ancestor scoping (`scope_filter:`, v0.9.6+)

A second per-file gate orthogonal to `when:` and `paths:`. Per-file rules can declare `scope_filter: { has_ancestor: <list> }` to narrow themselves to files that have a specified manifest somewhere in their ancestor directory chain. The engine walks `Path::parent()` upward (the file's own directory counts as an ancestor) and consults the v0.9.5 path-index at each step; first-match-wins gates the rule per-file.

```yaml
- id: rust-sources-no-bidi
  when: facts.has_rust              # tree-level gate
  kind: no_bidi_controls
  paths: "**/*.rs"                  # path glob
  scope_filter:                     # ancestor walk
    has_ancestor: Cargo.toml
  level: error
```

The composition order is: 1. Tree-level `when:` (skip rule entirely if false), 2. Per-file `paths:` glob, 3. Per-file `scope_filter:` ancestor walk, 4. Per-file `git_tracked_only:` consult, 5. Rule-specific evaluate body.

Cross-file rules (`pair`, `for_each_dir`, `file_exists`, …) reject `scope_filter:` at build time and direct authors to `for_each_dir + when_iter:`. Rule-major rules like `filename_case` silently ignore the field — gate via the rule's `paths:` glob instead.

Used by the five bundled ecosystem rulesets (`rust@v1`, `node@v1`, `python@v1`, `go@v1`, `java@v1`) so their per-file content rules narrow to files inside their ecosystem's package subtree in polyglot monorepos. Full design: [`v0.9/scope-filter.md`](./v0.9/scope-filter.md).

### Composition

`extends` accepts local paths and URLs (with optional SHA-256 subresource integrity). Child configs deep-merge over parents on a per-rule-id basis. Setting `level: off` on an inherited rule disables it.

Bundled rulesets are referenced via `alint://bundled/<name>@v<major>`.

## Execution model

1. **Config load.** Read `.alint.yml`; follow `extends` with caching and cycle detection; validate against JSON Schema.
2. **Facts.** Evaluate facts in parallel. Cache keyed on input hashes.
3. **Rule filter.** Evaluate `when` clauses; drop disabled rules.
4. **Walk.** One pass over the filesystem via the `ignore` crate. Build a `FileIndex` of path → metadata (size, is-text heuristic, extension, parent).
5. **Match.** For each rule, resolve matching files/dirs from the index via compiled `GlobSet`.
6. **Read cache.** Files requested by multiple content rules are read once; bytes cached for the run.
7. **Evaluate.** Rule evaluation fans out via `rayon`.
8. **Aggregate.** Collect `RuleResult`s into a `Report`.
9. **Fix (optional).** Apply fixers serially; re-run checks.
10. **Emit.** Format via selected output.

Invariants: the walk runs exactly once per invocation; any given file's bytes are read at most once; fact and rule evaluation are both parallelized; fixers run serially (they mutate the tree).

## Crate layout

alint is a Cargo workspace — the standard shape for Rust tools (rustc, cargo, tokio, ruff, biome, rust-analyzer, wasmtime, ...). The reasons apply here: pre-1.0 breaking changes in the core ripple through the graph, so every such change is one PR rather than a multi-repo release; one `Cargo.lock` guarantees consistent transitive deps; one CI run (`cargo test --workspace`) validates the full graph; contributors clone once.

**Current crates:**

```
alint/
├── crates/
│   ├── alint/              binary entrypoint; `cargo install alint`
│   ├── alint-core/         engine, walker, rule trait, config AST, errors
│   ├── alint-dsl/          YAML config loader + schema validation
│   ├── alint-rules/        built-in rule implementations
│   ├── alint-output/       formatters (human, json, ...)
│   └── alint-bench/        criterion micro-benches + seeded tree generator
├── xtask/                  cargo-xtask helpers (bench-release driver)
├── ci/                     self-hosted runner + per-job shell scripts
├── schemas/v1/             JSON Schema for .alint.yml
├── docs/
│   ├── design/             architecture and roadmap
│   └── benchmarks/         methodology and per-platform published numbers
├── install.sh              curl-pipeable platform-detecting installer
├── .alint.yml              dogfood config
└── Cargo.toml              workspace manifest
```

**Planned additions (see [ROADMAP.md](./ROADMAP.md)):**

- `crates/alint-facts/` — language and license detectors
- `crates/alint-plugin/` — command runner and WASM plugin host
- `crates/alint-lsp/` — language-server implementation
- `crates/alint-test/` — snapshot test harness for rule behaviors
- `editors/` — VS Code, Zed, Helix extensions
- `rulesets/` — bundled rulesets (embedded at build time; also published as raw YAML)
- `xtask/` — benchmark harness and release tooling

### Publishing intent (crates.io)

The public crate surface is kept narrow so the semver-stable API is small and maintainable.

| Crate | `publish` | Why |
|---|---|---|
| `alint` (binary) | public | Enables `cargo install alint`. Package name matches `[[bin]] name`. |
| `alint-core` | public | Embeddable engine for custom drivers — scripts, custom CI gates, third-party hosts. Semver-stable from 1.0. |
| `alint-dsl`, `alint-rules`, `alint-output`, and later-phase crates | `publish = false` | Internal plumbing. Promotion to public requires a concrete external consumer and a commitment to maintain the API. |

Unpublished crates still ship inside the binary and can be promoted later. The reverse — publishing a crate then realizing you don't want to maintain it as a stable API — is much harder.

## Plugin model

Two tiers, introduced across the roadmap:

- **`command` rule kind.** Rule shells out per matched file. Exit code is the verdict; stdout/stderr is the message. Environment variables expose path, rule id, level, vars, and facts. Simple, scriptable, language-agnostic.
- **`wasm` plugin kind.** Plugins implement a stable WIT interface, receive file bytes + metadata, and return a structured result. Distributed as `.wasm` blobs referenced by URL with SRI. Sandboxed, deterministic, no network by default (opt-in capability).

Native Rust plugins are deliberately out of scope. Dynamic library loading has ABI stability problems and would lock the plugin ecosystem to Rust. WASM is the long-term answer.

## Output formats

Selected via `--format`:

- `human` (default) — colorized, per-rule grouped output with source snippets.
- `json` — stable, versioned schema.
- `sarif` — SARIF 2.1.0 for GitHub Code Scanning and Azure DevOps.
- `github` — GitHub Actions annotations (`::error file=...`).
- `gitlab` — GitLab Code Quality JSON.
- `junit` — JUnit XML for generic CI reporting.
- `markdown` — report with TOC, suitable for posting as a GitHub issue body.
- `summary` — one-line status per rule.

## Full example

A Rust project dogfood config, showing composition, facts, and multiple rule families:

```yaml
# yaml-language-server: $schema=./schemas/v1/config.json
version: 1

extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/rust@v1

vars:
  copyright_year: "2026"
  org: "Acme Corp"

ignore:
  - "target/**"

facts:
  - id: has_benches
    any_file_exists: ["benches/**/*.rs"]

rules:
  # Override an inherited rule.
  readme-exists:
    paths: ["README.md", "README.adoc"]

  # Disable an inherited rule.
  no-todo-comments:
    level: off

  # New rules.
  - id: cargo-members-are-kebab
    kind: toml_path_matches
    paths: "Cargo.toml"
    query: "$.workspace.members[*]"
    pattern: "^[a-z][a-z0-9-]+$"
    level: error

  - id: crates-have-readme
    kind: for_each_dir
    select: "crates/*"
    require:
      - kind: file_exists
        paths: "{dir}/README.md"
    level: error

  - id: integration-test-pair
    kind: pair
    primary: "crates/*/src/*.rs"
    partner: "crates/*/tests/{stem}_test.rs"
    level: warning

  - id: bench-gated
    when: facts.has_benches
    kind: file_exists
    paths: "benches/Cargo.toml"
    level: error
```

## Contributing new rule kinds

A new rule kind typically needs:

1. A `Rule` impl in `crates/alint-rules/src/<kind>.rs`.
2. Registration in `register_builtin` in `crates/alint-rules/src/lib.rs`.
3. Unit tests alongside the impl (snapshot harness arrives with `alint-test`).
4. A row in the primitives tables above.
5. An entry in the Full Example section if the kind is commonly used.
6. If the primitive shifts from "planned" to "shipped," an update in [ROADMAP.md](./ROADMAP.md).
