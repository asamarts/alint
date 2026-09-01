# alint Architecture

> Status: Living design document. Describes alint's internals for contributors
> and embedders. For the current scope by version, see [ROADMAP.md](./ROADMAP.md).

> **Diagrams:** this document embeds interactive architecture diagrams that
> render on [alint.org](https://alint.org/docs/about/architecture/); GitHub
> strips the `<likec4-view>` web component, so on GitHub they appear blank. The
> same views are static Mermaid flowcharts, generated from the same model, in
> [DIAGRAMS.md](https://github.com/asamarts/alint/blob/main/docs/design/architecture/DIAGRAMS.md).

## Overview

alint is a language-agnostic linter for **repository structure, file existence, filename conventions, and file content rules**. It is a single static Rust binary that reads a declarative YAML config and enforces rules over a repository tree.

<likec4-view view-id="index"></likec4-view>

Examples of rules in scope:

- Does file `X` exist at path `Y`? Does directory `Z` contain file `W`?
- Do filenames under `components/` follow PascalCase?
- Does every `.c` file have a matching `.h` in the same directory?
- Does every `.java` file start with a required license-header comment?
- Is anything binary present under `src/`?
- Does `package.json`'s `license` field equal `"Apache-2.0"`?

Out of scope (explicitly; use the named tool instead):

- Code semantics / AST linting → ESLint, Clippy, ruff
- Static application security testing → Semgrep, CodeQL
- Infrastructure-as-code scanning → Checkov, Conftest, tfsec
- Commit-message linting → commitlint
- Secret scanning → gitleaks, trufflehog

The clarity of these non-goals is itself a feature.

## Design principles

1. **The repository tree is the input.** Every rule sees a unified file/directory index. The walk happens once per invocation.
2. **A small set of composable rule families.** Existence, content, naming, and cross-file were the original shapes; the model has since grown to thirteen families, all built on the same rule record. The [rule reference](https://alint.org/docs/rules/) lists every kind by family.
3. **Declarative by default, programmable at the edges.** YAML covers typical rules. A bounded expression language gates rules on facts. A plugin surface (command, later WASM) covers user-defined logic.
4. **Walk once, evaluate in parallel.** Single-pass walker; shared file index; `rayon` for rule-level parallelism.
5. **Respect ecosystem defaults.** `.gitignore` is honored by default. YAML is the config format. Case aliases (`PascalCase` / `pascalcase` / `pascal-case`) all parse.
6. **Every rule carries its own story.** Severity, message, `policy_url`, and optional `fix` are first-class fields.
7. **Modern output formats from day one.** Eight formats: human, json, sarif, github, gitlab, junit, markdown, and agent.
8. **Single static binary.** Rust, no runtime dependency on Node, Ruby, or Python.

## Rule model

<likec4-view view-id="ruleTypeModel"></likec4-view>

Every rule is a record:

- `id`: unique kebab-case identifier
- `kind`: the primitive rule type, namespaced (`file_exists`, `filename_case`, `pair`, ...)
- `level`: `error` | `warning` | `info` | `off`
- `paths`: the scope glob(s); accepts a string, an array (with `!negation`), or `{include, exclude}`
- `when`: optional expression gating rule application on facts
- `message`: human message; supports `{{vars.*}}` and `{{ctx.*}}` substitution
- `policy_url`: optional URL to a human-readable policy justification
- `fix`: optional fixer block
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

### Dispatch flip + `PerFileRule` (v0.9.3)

Since v0.9.3 the engine partitions rules into two dispatch shapes for
hot-path performance. The `Rule` trait above describes the rule-major
shape, where each rule scans the file index and reads the files it needs.
That fits **cross-file rules** (`pair`, `for_each_dir`, `unique_by`,
`dir_contains`, `dir_only_contains`, `every_matching_has`,
`file_exists`, `file_absent`, ...) whose verdict spans the whole tree.

**Per-file rules**, those that read each matched file's content
individually, opt into a sibling `PerFileRule` trait, and the engine
drives a *file-major* outer loop on their behalf: each matched file is
read at most once, then dispatched to every applicable per-file rule.
The trait hands a pre-loaded byte slice to `evaluate_file` rather than
having the rule re-read inside `evaluate`:

```rust
pub trait PerFileRule: Send + Sync + std::fmt::Debug {
    fn id(&self) -> &str;
    fn level(&self) -> Level;
    fn path_scope(&self) -> &Scope;
    fn max_bytes_needed(&self) -> Option<usize> { None }
    fn evaluate_file(&self, file: &FileEntry, content: &[u8])
        -> Result<Vec<Violation>>;
}
```

Engine discrimination happens once at registry time: rules that
override `requires_full_index() = true` stay rule-major; the rest opt
into per-file dispatch. Roughly two dozen rules across the content,
text-hygiene, security-unicode, and encoding families take the per-file
path. Two metadata-driven Unix rules
(`executable_has_shebang`, `shebang_has_executable`) stay rule-major
so they can short-circuit on `metadata().permissions()` before any
read. Full design, and the rules that did or did not migrate, is in
[the v0.9 dispatch-flip pass](https://github.com/asamarts/alint/blob/main/docs/design/v0.9/dispatch_flip.md).

### Memory layout on the hot path (v0.9.2)

Path and string clones across the violation hot path were a
substantial allocator pressure source at the 100k-violation
benchmarks. v0.9.2 retyped the affected fields to share allocations
across every violation of a rule:

- `FileEntry::path: Arc<Path>`: shared across all rules touching one
  file.
- `Violation::path: Option<Arc<Path>>`: refcount bump per violation
  instead of `PathBuf::clone`.
- `Violation::message: Cow<'static, str>`: `Borrowed` for the rule's
  static `message:` field; `Owned` for templated per-match strings
  via `format!`.
- `RuleResult::rule_id: Arc<str>`, `RuleResult::policy_url:
  Option<Arc<str>>`: one allocation per rule run, shared across all
  violations of that rule.

Behavioural invariants verified via the cross-formatter snapshot
test: byte-identical output before/after the type pass. Full design:
[the v0.9 memory pass](https://github.com/asamarts/alint/blob/main/docs/design/v0.9/memory_pass.md).

## DSL

<likec4-view view-id="configModel"></likec4-view>

YAML, with a JSON Schema (draft 2020-12) maintained at [`schemas/v1/config.json`](https://github.com/asamarts/alint/blob/main/schemas/v1/config.json) in the repository and embedded into `alint-dsl` at build time via `include_str!` (exposed as `alint_dsl::CONFIG_SCHEMA_V1`). Integration tests round-trip representative configs through a compliant validator so the schema and the engine's actual DSL stay in sync.

For editor autocomplete, reference the schema via the YAML language server pragma, either by a relative path (recommended inside this repo) or by the GitHub raw URL (for downstream users):

```yaml
# yaml-language-server: $schema=./schemas/v1/config.json
# or: $schema=https://raw.githubusercontent.com/asamarts/alint/main/schemas/v1/config.json
```

```yaml
# .alint.yml
version: 1

extends:
  # Optional subresource integrity is a `#sha256-<hex>` URL fragment.
  - url: "https://raw.githubusercontent.com/example/rulesets/base.yaml#sha256-0000000000000000000000000000000000000000000000000000000000000000"
  # A local path is a bare string entry.
  - ./team-policy.alint.yml

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

### Rule families

Rules are grouped into thirteen families. Every kind shares the record shape above; what differs is the predicate. Not every kind ships in every release. The full, always-current catalogue, with a one-line summary per kind, lives at [the rule reference](https://alint.org/docs/rules/).

- **Existence** (`file_exists`, `file_absent`, `dir_exists`, `dir_absent`): a path must, or must not, be present.
- **Content** (`file_content_matches`, `file_header`, `file_hash`, `file_max_size`, `file_max_lines`, `file_is_text`, `file_shebang`, ...): assertions over a file's bytes or lines.
- **Structured query** (`json_path_equals`, `yaml_path_matches`, `toml_path_equals`, `xml_path_matches`, `json_schema_passes`, ...): query a value inside a JSON / YAML / TOML / XML / dotenv file via RFC 9535 JSONPath, or validate against a JSON Schema.
- **Naming** (`filename_case`, `filename_regex`): a basename matches a case convention or a regex.
- **Text hygiene** (`no_trailing_whitespace`, `final_newline`, `line_endings`, `line_max_width`, `indent_style`, `max_consecutive_blank_lines`): whitespace and line-shape checks, most with a fixer.
- **Security / Unicode** (`no_merge_conflict_markers`, `no_bidi_controls`, `no_zero_width_chars`): flag conflict markers and Trojan-Source bidi / zero-width characters.
- **Encoding** (`no_bom`): byte-order-mark and encoding checks.
- **Structure** (`max_directory_depth`, `max_files_per_directory`, `no_empty_files`): shape of the tree itself.
- **Portable metadata** (`no_case_conflicts`, `no_illegal_windows_names`): reject tree shapes that look fine on one OS but break checkouts on a case-insensitive or Windows filesystem.
- **Unix metadata** (`no_symlinks`, `executable_bit`, `executable_has_shebang`, `shebang_has_executable`): permission-bit and symlink checks, `#[cfg(unix)]`-gated so configs stay portable.
- **Git hygiene** (`no_submodules`, `git_no_denied_paths`, `git_commit_message`, `git_commit_signed_off`, `git_commit_subject_matches`, `git_blame_age`, ...): properties of the git tree and the commit range.
- **Cross-file** (`pair`, `for_each_dir`, `for_each_file`, `every_matching_has`, `unique_by`, `dir_contains`, `file_graph`, ...): a verdict that spans more than one file.
- **Plugin** (`command`): shell out to an external checker per matched file (see [Plugin model](#plugin-model)).

`git_tracked_only:` and `scope_filter:` are per-rule fields, not kinds; see [Closest-ancestor scoping](#closest-ancestor-scoping-scope_filter-v096) and [Git-tracked filtered index](#git-tracked-filtered-index-v0911) below.

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

- `{dir}`: parent directory of the matched file
- `{path}`: full relative path
- `{basename}`: filename including extension
- `{stem}`: filename without the final extension
- `{ext}`: final extension without the dot
- `{parent_name}`: immediate parent directory name
- `{stem_kebab}`, `{stem_snake}`, `{stem_pascal}`, ...: transformed stems

### Scope and globbing

Globs compile via `globset`:

- `*`: any run of non-separator chars
- `**`: any number of path segments (own segment only)
- `?`: one non-separator char
- `[abc]`, `[a-z]`: character classes
- `{a,b,c}`: brace alternation
- `!pattern`: negation (arrays only)

Every rule's `paths` accepts one of three shapes:

```yaml
paths: "src/**/*.rs"                                       # single glob
paths: ["src/**/*.rs", "!src/**/testdata/**"]              # array with negation
paths: {include: ["src/**"], exclude: ["**/*.test.*"]}     # explicit pair
```

`.gitignore` is honored by default. `.alintignore` provides alint-specific exclusions. `ignore:` in config adds to the exclusion set.

### Facts and conditional rules

Facts are declarative properties of the repository, evaluated once per run and cached. `when` clauses gate rules on facts.

Fact kinds are `any_file_exists`, `all_files_exist`, `count_files`, `file_content_matches`, `git_branch`, and `custom: {argv: [...]}` (shell out, stdout → value). Language/license detectors (`detect: linguist`, `detect: askalono`) are deferred — see the planned `alint-facts` crate below.

The `when` expression language is deliberately bounded:

- Operators: `==`, `!=`, `<`, `<=`, `>`, `>=`, `and`, `or`, `not`, `in`, `matches`
- Identifiers: `facts.<name>`, `vars.<name>`, `ctx.<name>`
- Literals: strings, numbers, booleans, null, lists

No user-defined functions, no recursion, no I/O. Examples:

```yaml
when: facts.has_rust
when: facts.release_branch in ["main", "release"]
when: facts.has_rust and not facts.is_workspace_member
when: facts.java_file_count > 0
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

Cross-file rules (`pair`, `for_each_dir`, `file_exists`, ...) reject `scope_filter:` at build time and direct authors to `for_each_dir + when_iter:`. Per-file rules, both `PerFileRule` and the remaining rule-major ones, honour the filter through the shared `Scope::matches` call (see the v0.9.10 structural fix below).

Used by the seven bundled ecosystem rulesets (`rust@v1`, `node@v1`, `python@v1`, `go@v1`, `java@v1`, `dotnet@v1`, `php@v1`) so their per-file content rules narrow to files inside their ecosystem's package subtree in polyglot monorepos. Full design: [the v0.9 scope-filter pass](https://github.com/asamarts/alint/blob/main/docs/design/v0.9/scope-filter.md).

**Structural fix in v0.9.10**: `Scope` owns its `Option<ScopeFilter>` and `Scope::matches(&Path, &FileIndex)` consults both predicates in one call. The signature change is compile-enforced (every per-rule path check must thread the index), so the silent-drop bug class that produced sweeps in v0.9.7 and v0.9.9 is structurally closed. No rule has a separate `scope_filter` field to forget about. Full design: [the v0.9 scope-owns-scope-filter pass](https://github.com/asamarts/alint/blob/main/docs/design/v0.9/scope-owns-scope-filter.md).

### Git-tracked filtered index (v0.9.11)

`git_tracked_only: true` on a rule narrows it to paths in `git ls-files` output, skipping locally-built but untracked artefacts (`target/`, `node_modules/`, ...). Through v0.9.10 each rule consulted `ctx.is_git_tracked(path)` inline, the same silent-drop bug class that hit `scope_filter:`.

v0.9.11 builds two filtered `FileIndex` views once per run when any rule opts in: a file-tracked subset (entries where `git_tracked.contains(path)`) and a dir-aware subset (dirs that recursively contain at least one tracked file). The engine substitutes the pre-filtered index via `pick_ctx`; the rule's `evaluate` body never sees an untracked path, and the runtime `is_git_tracked()` check disappears from per-rule code. Full design: [the v0.9 git-tracked filtered-index pass](https://github.com/asamarts/alint/blob/main/docs/design/v0.9/git-tracked-filtered-index.md).

### Composition

`extends` accepts local paths and URLs (with optional SHA-256 subresource integrity). Child configs deep-merge over parents on a per-rule-id basis. Setting `level: off` on an inherited rule disables it.

Bundled rulesets are referenced via `alint://bundled/<name>@v<major>`.

## Execution model

The pipeline from `alint check` to output:

<likec4-view view-id="checkFlow"></likec4-view>

1. **Config load.** Read `.alint.yml`; follow `extends` with caching and cycle detection; validate against JSON Schema.
2. **Facts.** Evaluate facts in parallel. Cache keyed on input hashes.
3. **Rule filter.** Evaluate `when` clauses; drop disabled rules.
4. **Walk.** One *parallel* pass over the filesystem via the `ignore` crate's `WalkBuilder::build_parallel` (v0.9.1). Each worker thread accumulates `FileEntry`s in a thread-local `Vec`; the engine merges them and runs a deterministic `sort_unstable_by` post-sort so downstream output is byte-identical to the pre-v0.9.1 sequential walker. The resulting `FileIndex` exposes lazy `OnceLock` indexes (`contains_file`, `children_of`, `descendants_of`, `file_basenames_of`) that turn common cross-file queries from linear scans into O(1) hash lookups (v0.9.5 + v0.9.8).
5. **Dispatch partition.** Rules with `requires_full_index() = true` (cross-file) stay rule-major; the rest implement `PerFileRule` and join the file-major dispatch (v0.9.3, see [Rule model](#dispatch-flip--perfilerule-v093)).
6. **Match.** Per rule, resolve matching files/dirs through `Scope::matches(&Path, &FileIndex)` (v0.9.10), so globs *and* `scope_filter:` ancestor predicates evaluate in one call. For `git_tracked_only:` rules the engine substitutes a pre-filtered `FileIndex` so out-of-scope paths never reach `evaluate` (v0.9.11).
7. **Evaluate.** Per-file rules receive a pre-loaded `&[u8]` slice via `evaluate_file` (read once per file regardless of how many per-file rules match it); cross-file rules read what they need from the index. Both fan out via `rayon`.
8. **Aggregate.** Collect `RuleResult`s into a `Report`.
9. **Fix (optional).** Apply fixers serially; re-run checks.
10. **Emit.** Format via selected output.

Invariants: the walk runs exactly once per invocation; any given file's bytes are read at most once; fact and rule evaluation are both parallelized; fixers run serially (they mutate the tree).

Step 2 in detail: facts are evaluated once (in parallel, cached), then gate which rules run via their `when:` conditions.

<likec4-view view-id="factsFlow"></likec4-view>

Steps 5 to 7 in detail: dispatch partitions rules into cross-file (rule-major) and per-file, so each matched file's bytes are read exactly once (ADR-0003).

<likec4-view view-id="dispatchFlow"></likec4-view>

## Crate layout

alint is a Cargo workspace, the standard shape for Rust tools (rustc, cargo, tokio, ruff, biome, rust-analyzer, wasmtime, ...). The reasons apply here: pre-1.0 breaking changes in the core ripple through the graph, so every such change is one PR rather than a multi-repo release; one `Cargo.lock` guarantees consistent transitive deps; one CI run (`cargo test --workspace`) validates the full graph; contributors clone once.

<likec4-view view-id="cliComponents"></likec4-view>

### Building block view

The **crate dependency graph** is generated from `cargo metadata` and committed at
[`docs/design/architecture/crate-graph.md`](https://github.com/asamarts/alint/blob/main/docs/design/architecture/crate-graph.md)
— now the generated LikeC4 `crateGraph` view (`crate-graph.gen.c4`, solid = runtime deps,
dashed = dev/build-only; the original Mermaid is subsumed by that view) plus a crate-by-tier
table, regenerated by `cargo run -p xtask -- gen-arch` and gated by `gen-arch --check` (so it
can't drift from the Cargo manifests). `alint-core` is the foundation (a runtime dependency
sink, enforced by a test); the `alint` binary and `xtask` sit at the top.

The **C4 model** (intent: system context, containers, the crates as components) now lives in
the single LikeC4 model under
[`docs/design/architecture/model/`](https://github.com/asamarts/alint/tree/main/docs/design/architecture/model)
([ADR-0005](https://github.com/asamarts/alint/blob/main/docs/adr/0005-adopt-likec4-for-architecture-diagrams.md));
its crate elements are gated against the `cargo metadata` workspace members, so the model can't
silently omit a crate. The hand-modeled Structurizr
[`workspace.dsl`](https://github.com/asamarts/alint/blob/main/docs/design/architecture/workspace.dsl)
is retained through the transition and retires once the LikeC4 model fully subsumes it.
Architecture decisions are recorded as ADRs under
[`docs/adr/`](https://github.com/asamarts/alint/tree/main/docs/adr). See
[`architecture-as-code.md`](https://github.com/asamarts/alint/blob/main/docs/design/architecture-as-code.md)
(the original Mermaid + Structurizr generator design, Phase 4 / WS3) and its LikeC4 follow-on
[`architecture-diagrams.md`](https://github.com/asamarts/alint/blob/main/docs/design/architecture-diagrams.md)
(ADR-0005).

<!-- These cross-references use absolute GitHub URLs on purpose: relative links would
     break on alint.org, whose docs bundle flattens the directory layout. -->

The directory tree below is the on-disk layout; the dependency graph above is the build-block
structure.

**Current crates:**

```
alint/
├── crates/
│   ├── alint/              binary entrypoint; `cargo install alint`
│   ├── alint-core/         engine, walker, rule trait, config AST, errors
│   ├── alint-dsl/          YAML config loader + schema validation + bundled rulesets
│   ├── alint-rules/        built-in rule implementations
│   ├── alint-output/       formatters (human, json, sarif, github, gitlab, junit, markdown, agent)
│   ├── alint-lsp/          language server behind the `lsp` subcommand (tower-lsp)
│   ├── alint-bench/        criterion micro-benches + seeded tree generator
│   ├── alint-testkit/      shared test harness (treespec materializer, scenario runner, proptest strategies)
│   └── alint-e2e/          end-to-end scenarios + coverage audits + cross-cutting invariant tests
├── xtask/                  cargo-xtask helpers (bench-release driver, docs-export, publish-benches)
├── ci/                     self-hosted runner + per-job shell scripts
├── editors/                editor integrations (VS Code, Zed, JetBrains, Neovim, Helix, Emacs, Sublime, Eclipse)
├── schemas/v1/             JSON Schema for .alint.yml + report shapes (mirrored under crates/alint-dsl/schemas/)
├── docs/
│   ├── design/             architecture, roadmap, per-cut design passes (v0.7, v0.9, ...)
│   ├── development/        contributor docs (rule-authoring.md)
│   └── benchmarks/         methodology + per-platform published numbers (micro/, macro/, investigations/, archive/)
├── install.sh              curl-pipeable platform-detecting installer
├── action.yml              official GitHub Action (composite)
├── npm/                    npm wrapper that downloads the platform-matched binary
├── Dockerfile              distroless image; rebuilt per release
├── .alint.yml              dogfood config
└── Cargo.toml              workspace manifest
```

**Planned additions (see [ROADMAP.md](./ROADMAP.md)):**

- `crates/alint-plugin/`: WASM plugin host (roadmap v0.14; the tier-1 `command` plugin already lives in `alint-rules`).
- `crates/alint-facts/`: currently subsumed by `alint-core::facts`; promotion to its own crate is deferred until the language and license detectors (`detect: linguist`, `detect: askalono`) land.

### Publishing intent (crates.io)

The public crate surface is kept narrow so the semver-stable API is small and maintainable.

| Crate | `publish` | Why |
|---|---|---|
| `alint` (binary) | public | Enables `cargo install alint`. Package name matches `[[bin]] name`. |
| `alint-core` | public | Embeddable engine for custom drivers: scripts, custom CI gates, third-party hosts. Semver-stable from 1.0. |
| `alint-dsl`, `alint-rules`, `alint-output`, and later-phase crates | `publish = false` | Internal plumbing. Promotion to public requires a concrete external consumer and a commitment to maintain the API. |

Unpublished crates still ship inside the binary and can be promoted later. The reverse, publishing a crate then realizing you don't want to maintain it as a stable API, is much harder.

## Plugin model

<likec4-view view-id="pluginModel"></likec4-view>

Two tiers, introduced across the roadmap:

- **`command` rule kind** (shipped). The rule shells out per matched file. Exit code is the verdict; stdout/stderr is the message. Environment variables expose path, rule id, level, vars, and facts. Simple, scriptable, language-agnostic.
- **`wasm` plugin kind** (roadmap v0.14). Plugins will implement a stable WIT interface, receive file bytes + metadata, and return a structured result. Distributed as `.wasm` blobs referenced by URL with SRI. Sandboxed, deterministic, no network by default (opt-in capability).

Native Rust plugins are deliberately out of scope. Dynamic library loading has ABI stability problems and would lock the plugin ecosystem to Rust. WASM is the long-term answer.

## Output formats

<likec4-view view-id="outputFormats"></likec4-view>

Selected via `--format`:

- `human` (default): colorized, per-rule grouped output with source snippets.
- `json`: stable, versioned schema.
- `sarif`: SARIF 2.1.0 for GitHub Code Scanning and Azure DevOps.
- `github`: GitHub Actions annotations (`::error file=...`).
- `gitlab`: GitLab Code Quality JSON.
- `junit`: JUnit XML for generic CI reporting.
- `markdown`: report with TOC, suitable for posting as a GitHub issue body.
- `agent`: LLM-shaped JSON sibling of `json`, with a per-violation `agent_instruction` templated from each rule's message and fix block (v0.6).

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
  # Override an inherited rule (field-merge by id: the inherited
  # `oss-readme-exists` keeps its `kind`, this narrows its `paths`).
  - id: oss-readme-exists
    paths: ["README.md", "README.adoc"]

  # Disable an inherited rule.
  - id: rust-sources-snake-case
    level: off

  # New rules.
  - id: cargo-members-are-kebab
    kind: toml_path_matches
    paths: "Cargo.toml"
    path: "$.workspace.members[*]"
    matches: "^[a-z][a-z0-9-]+$"
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
3. Unit tests alongside the impl (snapshot harness in `alint-testkit`).
4. A docs entry so the kind appears in the [rule reference](https://alint.org/docs/rules/).
5. An entry in the Full Example section if the kind is commonly used.
6. If the primitive shifts from "planned" to "shipped," an update in [ROADMAP.md](./ROADMAP.md).
