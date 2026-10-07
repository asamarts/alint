# alint Architecture

> Status: Living design document. Describes alint's internals for contributors
> and embedders. For the current scope by version, see
> [ROADMAP.md](https://github.com/asamarts/alint/blob/main/docs/design/ROADMAP.md).

> **Diagrams:** this document embeds interactive architecture diagrams that
> render on [alint.org](https://alint.org/docs/about/architecture/); GitHub
> strips the `<likec4-view>` web component, so on GitHub they appear blank. The
> same views are static Mermaid flowcharts, generated from the same model, in
> [DIAGRAMS.md](https://github.com/asamarts/alint/blob/main/docs/design/architecture/DIAGRAMS.md).

## Overview

alint is a language-agnostic, general linter for **finite, inspectable project state**.
The current implementation inspects a repository's working-tree files and directories,
structured documents, filesystem metadata, and selected local Git state and history. It is a
single static Rust binary that reads a declarative YAML config and emits findings or bounded
remediations.

For the precise scope of the "general linter" claim, the limits of the current model, and
the proposed selector/extractor/relation/constraint direction, see
[`general-linter.md`](https://github.com/asamarts/alint/blob/main/docs/design/general-linter.md).
The companion
[`rule-coverage-gaps.md`](https://github.com/asamarts/alint/blob/main/docs/design/rule-coverage-gaps.md)
inventories missing detection classes.

<likec4-view view-id="index"></likec4-view>

Examples of rules in scope:

- Does file `X` exist at path `Y`? Does directory `Z` contain file `W`?
- Do filenames under `components/` follow PascalCase?
- Does every `.c` file have a matching `.h` in the same directory?
- Does every `.java` file start with a required license-header comment?
- Is anything binary present under `src/`?
- Does `package.json`'s `license` field equal `"Apache-2.0"`?
- Do dependency declarations agree across manifests?
- Do commits in a range satisfy subject, sign-off, author, and signature policy?

Out of scope (explicitly; use the named tool instead):

- Code semantics / AST linting → ESLint, Clippy, ruff
- Static application security testing → Semgrep, CodeQL
- Infrastructure-as-code scanning → Checkov, Conftest, tfsec
- Secret scanning → gitleaks, trufflehog

alint does include bounded commit and history policy. It does not try to replace a complete
semantic code analyzer, security scanner, build system, or package resolver. The exact boundary
and the proposal for generalizing it are documented in `general-linter.md`.

The clarity of these non-goals is itself a feature.

## Design principles

1. **A repository snapshot is the primary input.** Every filesystem rule sees a unified file/directory index; Git-aware rules may also request explicit local Git inputs. The engine builds its main index once for `check` and once per `fix` fixpoint pass (effectful delegated rules may perform their own explicitly documented probes; see the [v0.17 fixpoint design](https://github.com/asamarts/alint/blob/main/docs/design/v0.17/fixpoint.md)).
2. **A small set of composable rule families.** Existence, content, naming, and cross-file were the original shapes; the model has since grown to thirteen families, all built on the same rule record. The [rule reference](https://alint.org/docs/rules/) lists every kind by family.
3. **Declarative by default, programmable at the edges.** YAML covers typical rules. A bounded expression language gates rules on facts. The `command` rule is the current escape hatch for user-defined logic; a capability-limited WASM host remains a possible future extension.
4. **Walk once per pass; share and parallelize safe work.** The walker and check evaluator use `rayon`; the shared index and file-major dispatch coalesce hot-path reads. Fix application remains serial and ordered.
5. **Respect ecosystem defaults.** `.gitignore` is honored by default. YAML is the config format. Case aliases (`PascalCase` / `pascalcase` / `pascal-case`) all parse.
6. **Every rule carries its own story.** Severity, message, `policy_url`, and optional `fix` are first-class fields.
7. **Modern output formats from day one.** Eight formats: human, json, sarif, github, gitlab, junit, markdown, and agent.
8. **Single static binary.** Rust, no runtime dependency on Node, Ruby, or Python.

## Rule model

<likec4-view view-id="ruleTypeModel"></likec4-view>

Every rule is a record:

- `id`: stable identifier, unique after composition (the schema currently accepts a lowercase
  ASCII letter followed by lowercase letters, digits, `_`, or `-`)
- `kind`: the primitive rule type, namespaced (`file_exists`, `filename_case`, `pair`, ...)
- `level`: `error` | `warning` | `info` | `off`
- `paths`: the scope glob(s); accepts a string, an array (with `!negation`), or `{include, exclude}`
- `when`: optional expression gating rule application on facts
- `message`: optional human-message override. `{{vars.*}}` substitution occurs while expanding a
  template instance; supported `{{ctx.*}}` fields are kind-specific and are rendered only by the
  kinds that document them. Other messages are literal.
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
    fn fixer(&self) -> Option<&dyn Fixer> { None }
    fn as_per_file(&self) -> Option<&dyn PerFileRule> { None }
    // Additional hooks describe full-index, Git, scope, nesting, and trust needs.
}
```

Rules produce `Violation`s; the engine aggregates them into a `Report`.

### Dispatch flip + `PerFileRule` (v0.9.3)

Since v0.9.3 the engine partitions rules into two execution loops, covering three semantic
classes. The rule-major loop contains both **full-index rules** (cross-file, existence, and other
whole-repository predicates) and **ordinary rule-major rules** that have not or cannot opt into
file-major dispatch. `requires_full_index()` independently controls which index a rule sees under
`--changed`; it does not choose the dispatch loop.

**Per-file rules**, those that can evaluate each matched file independently
(usually from its content), opt into a sibling `PerFileRule` trait, and the engine
drives a *file-major* outer loop on their behalf: each matched file is
read at most once, then dispatched to every applicable per-file rule.
The trait hands a pre-loaded byte slice to `evaluate_file` rather than
having the rule re-read inside `evaluate`:

```rust
pub trait PerFileRule: Send + Sync + std::fmt::Debug {
    fn path_scope(&self) -> &Scope;
    fn evaluate_file(&self, ctx: &Context<'_>, path: &Path, bytes: &[u8])
        -> Result<Vec<Violation>>;
    fn max_bytes_needed(&self) -> Option<usize> { None }
}
```

The engine discriminates at evaluation time through `Rule::as_per_file()`: `Some` joins the
file-major loop and `None` stays rule-major. The two traits are views of the same rule object;
identity, severity, fixes, and other common behavior stay on `Rule`. The opted-in set spans
content, text-hygiene, security/Unicode, and encoding rules. Metadata rules and full-index rules
remain rule-major. Full design, and the rules that did or did not migrate, is in
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

YAML, with a JSON Schema (draft 2020-12) maintained at [`schemas/v1/config.json`](https://github.com/asamarts/alint/blob/main/schemas/v1/config.json) in the repository and embedded into `alint-dsl` at build time via `include_str!` (exposed as `alint_dsl::CONFIG_SCHEMA_V1`). The schema drives editor completion and documentation; the runtime performs typed Serde decoding plus explicit builder validation rather than invoking a JSON Schema validator. Integration tests validate representative configs against both paths and gate the root and embedded schema copies byte-for-byte.

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
- **Structured query** (`json_path_equals`, `yaml_path_matches`, `toml_path_equals`, `xml_path_matches`, `json_schema_passes`, ...): query a value inside a JSON / YAML / TOML / XML / dotenv / properties / INI / HCL file via RFC 9535 JSONPath, or validate against a JSON Schema.
- **Naming** (`filename_case`, `filename_regex`): a basename matches a case convention or a regex.
- **Text hygiene** (`no_trailing_whitespace`, `final_newline`, `line_endings`, `line_max_width`, `indent_style`, `max_consecutive_blank_lines`): whitespace and line-shape checks, most with a fixer.
- **Security / Unicode** (`no_merge_conflict_markers`, `no_bidi_controls`, `no_zero_width_chars`): flag conflict markers and Trojan-Source bidi / zero-width characters.
- **Encoding** (`no_bom`): byte-order-mark checks. General text/binary and ASCII checks live in
  the content family.
- **Structure** (`max_directory_depth`, `max_files_per_directory`, `no_empty_files`): shape of the tree itself.
- **Portable metadata** (`no_case_conflicts`, `no_illegal_windows_names`): reject tree shapes that look fine on one OS but break checkouts on a case-insensitive or Windows filesystem.
- **Unix metadata** (`no_symlinks`, `executable_bit`, `executable_has_shebang`, `shebang_has_executable`): permission-bit and symlink checks. Platform-specific operations are isolated so the binary and configs remain portable.
- **Git hygiene** (`no_submodules`, `git_no_denied_paths`, `git_commit_message`, `git_commit_signed_off`, `git_commit_subject_matches`, `git_blame_age`, ...): properties of the git tree and the commit range.
- **Cross-file** (`pair`, `for_each_dir`, `for_each_file`, `every_matching_has`, `unique_by`, `dir_contains`, `file_graph`, ...): a verdict that spans more than one file.
- **Plugin** (`command`): shell out to an external checker per matched file (see [Plugin model](#plugin-model)).

`scope_filter:` is a common field accepted by compatible file-scoped kinds. `git_tracked_only:`
is instead a kind-specific option of `file_exists`, `file_absent`, `dir_exists`, and
`dir_absent`. See [Rule scoping](#rule-scoping-scope_filter-v096) and
[Git-tracked filtered index](#git-tracked-filtered-index-v0911) below.

### Fix operations

Rules that declare a `fix:` block opt in to automatic remediation. The op is a discriminated union keyed by op name; each rule kind accepts at most one op.

**Path-only ops** (ignore `fix_size_limit`):

| Op | Shape | Rule kinds |
|---|---|---|
| `file_create` | `{content \| content_from, path?, create_parents?, applicability?}` | `file_exists` |
| `file_remove` | `{applicability?}` | `file_absent`, `no_empty_files`, `no_symlinks`, `no_submodules` |
| `file_rename` | `{applicability?}` (target derived from rule config) | `filename_case` |
| `dir_create` | `{applicability?}` | `dir_exists` |
| `relocate` | `{applicability?}` (moves the file to the repo root) | `file_absent` |
| `chmod` | `{applicability?}` (direction from the rule) | `executable_bit`, `shebang_has_executable` |
| `git_untrack` | `{applicability?}` (`git rm --cached`; *spawning*) | `file_absent` |
| `command` | `{run, timeout?, applicability?}` (runs argv; *spawning*) | `command` |

**Content-editing ops** (skipped on files over `fix_size_limit`; default 1 MiB, `null` disables):

| Op | Shape | Rule kinds |
|---|---|---|
| `file_prepend` | `{content \| content_from, applicability?}` | `file_header` |
| `file_append` | `{content \| content_from, applicability?}` | `file_content_matches`, `file_footer` |
| `file_trim_trailing_whitespace` | `{applicability?}` | `no_trailing_whitespace` |
| `file_append_final_newline` | `{applicability?}` | `final_newline` |
| `file_normalize_line_endings` | `{applicability?}` (target read from parent rule) | `line_endings` |
| `file_strip_bidi` | `{}` | `no_bidi_controls` |
| `file_strip_zero_width` | `{}` | `no_zero_width_chars` |
| `file_strip_bom` | `{applicability?}` | `no_bom` |
| `file_collapse_blank_lines` | `{applicability?}` (max read from parent rule) | `max_consecutive_blank_lines` |
| `replace` | `{replacement, pattern?, applicability?}` (structured matches require their own search `pattern`) | `file_content_forbidden`, `{json,yaml,toml,xml,dotenv,properties,ini,hcl}_path_matches` |
| `set_value` | `{applicability?}` (value from the rule's `equals:`) | `{json,yaml,toml,xml,dotenv,properties,ini,hcl}_path_equals` |
| `remove_value` | `{applicability?}` (target from parent rule) | `{json,yaml,toml,xml,dotenv,properties,ini,hcl}_path_absent` |
| `sync_from` | `{applicability?}` (source + relation from parent rule) | `cross_file` (`relation: identical` / `equals`) |
| `create_and_register` | `{content?, content_from?, applicability?}` | `cross_file` (`relation: registered`) |
| `sort` | `{applicability?}` (markers / comparator / `unique` / `select` from parent rule) | `ordered_block` |
| `indent_style` | `{applicability?}` (style / width from parent rule) | `indent_style` (`style: spaces` + `width`) |
| `insert_line` | `{applicability?}` (require lines / comparator from parent rule) | `ordered_block` (markerless, with `require:`) |
| `insert_header` | `{content \| content_from, applicability?}` (inserts after a leading BOM / shebang / `<?xml?>`) | `file_header` |

Over-limit content-editing ops report `Skipped` with a stderr warning instead of applying. Reads
are streaming where possible; otherwise the file is loaded in full. `set_value` and
`remove_value` are located ops over selected structured nodes; `sort` is a whole-file rewrite,
and `create_and_register` combines file creation, when needed, with a structured list append.

`replace`, `set_value`, and `remove_value` are *located* ops: they emit byte-range edits that the
located regime batches, orders, overlap-skips, verifies, and splices in a single pass. `replace`
and `remove_value` are `Unsafe` by default; scalar `set_value` is `Safe` by default, while
ambiguous or unsupported shapes are withheld or demoted.

Every built fixer resolves to an applicability tier (`Safe`, `Unsafe`, `Suggestion`, or
`Never`), although fixed-behavior marker ops do not all expose a YAML override. Most
normalizations default to `Safe`. Destructive, arbitrary, or direction-sensitive operations
default to `Unsafe`, including `file_remove`, `git_untrack`, `command`, `relocate`, `sync_from`,
`create_and_register`, `remove_value`, `replace`, `indent_style`, `insert_line`, and `sort` when
`unique: true`. A bare `alint fix` reports unsafe work as suggested; `--unsafe-fixes` applies it.
Per-op details are generated in [Rules](https://alint.org/docs/rules/). An inherited ruleset may
demote but not promote a fix, and spawning fixes are top-level-only. Fixers run serially. Content
edits compose in config order in memory and flush atomically once per touched file; path-only ops
apply directly (or are staged for `--diff`). `--dry-run` writes nothing, and `--diff` renders the
staged result.

### Path template tokens

Used in path-shaped fields such as `partner`, nested `require` paths, `command` argv,
cross-file mapping templates, and `unique_by.key`:

- `{dir}`: parent directory of the matched file
- `{path}`: full relative path
- `{basename}`: filename including extension
- `{stem}`: filename without the final extension
- `{ext}`: final extension without the dot
- `{parent_name}`: immediate parent directory name

Unknown tokens are preserved literally. Case conversion is a behavior of the
`filename_case`/`file_rename` pair, not an additional family of path-template tokens.

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

`.gitignore` (plus `.ignore` and git exclude files) is honored by default. `ignore:` in config adds to the exclusion set.

### Facts and conditional rules

Facts are declarative scalar properties of the repository. A normal check evaluates them once
per engine run; the LSP's repeated single-file path caches them on the lifetime of its
`FileIndex`. `when` clauses gate rules on facts.

Fact kinds are `any_file_exists`, `all_files_exist`, `count_files`, `file_content_matches`, `git_branch`, and `custom: {argv: [...]}` (shell out, stdout → value). Language/license detectors (`detect: linguist`, `detect: askalono`) are deferred — see the planned `alint-facts` crate below.

The `when` expression language is deliberately bounded:

- Operators: `==`, `!=`, `<`, `<=`, `>`, `>=`, `and`, `or`, `not`, `in`, `matches`
- Identifiers: `facts.<name>`, `vars.<name>`, `iter.<name>`, `env.<name>` (`ctx.<name>` is available in messages, not in `when:`)
- Literals: strings, numbers, booleans, null, lists

There are no user-defined functions or recursion. The grammar includes the bounded built-in
`iter.has_file(...)`; I/O happens only while named fact providers are evaluated, never from the
expression itself. Examples:

```yaml
when: facts.has_rust
when: facts.release_branch in ["main", "release"]
when: facts.has_rust and not facts.is_workspace_member
when: facts.java_file_count > 0
```

### Rule scoping (`scope_filter:`, v0.9.6+)

`scope_filter:` is a second file-scope gate orthogonal to `when:` and `paths:`. Its predicates
AND-compose:

- `has_ancestor` admits a file when a named manifest occurs in its ancestor chain;
- `changed_since` admits paths in a `<ref>...HEAD` Git diff;
- `include_manifest_paths` and `exclude_manifest_paths` use path sets extracted from a
  manifest, with optional target derivation.

The closest-ancestor lookup walks from the file's directory toward the root and stops at the
nearest directory containing any configured manifest name.

```yaml
- id: rust-sources-no-bidi
  when: facts.has_rust              # tree-level gate
  kind: no_bidi_controls
  paths: "**/*.rs"                  # path glob
  scope_filter:                     # ancestor walk
    has_ancestor: Cargo.toml
  level: error
```

Conceptually the gates are: tree-level `when:`, the kind's target selector/`paths:`, compatible
`scope_filter:` predicates, any kind-specific Git-tracked view, then the rule body. The exact
iteration order is an optimization; all declared gates must admit the target.

Cross-file rules (`pair`, `for_each_dir`, `file_exists`, ...) reject `scope_filter:` at build time and direct authors to `for_each_dir + when_iter:`. Per-file rules, both `PerFileRule` and the remaining rule-major ones, honour the filter through the shared `Scope::matches` call (see the v0.9.10 structural fix below).

Used by the seven bundled ecosystem rulesets (`rust@v1`, `node@v1`, `python@v1`, `go@v1`, `java@v1`, `dotnet@v1`, `php@v1`) so their per-file content rules narrow to files inside their ecosystem's package subtree in polyglot monorepos. Full design: [the v0.9 scope-filter pass](https://github.com/asamarts/alint/blob/main/docs/design/v0.9/scope-filter.md).

**Structural fix in v0.9.10**: `Scope` owns its `Option<ScopeFilter>` and `Scope::matches(&Path, &FileIndex)` consults both predicates in one call. The signature change is compile-enforced (every per-rule path check must thread the index), so the silent-drop bug class that produced sweeps in v0.9.7 and v0.9.9 is structurally closed. No rule has a separate `scope_filter` field to forget about. Full design: [the v0.9 scope-owns-scope-filter pass](https://github.com/asamarts/alint/blob/main/docs/design/v0.9/scope-owns-scope-filter.md).

### Git-tracked filtered index (v0.9.11)

`git_tracked_only: true` on one of the four existence kinds narrows its view to paths in
`git ls-files` output, skipping locally built but untracked artefacts (`target/`,
`node_modules/`, ...). `git_no_denied_paths` is separately and inherently tracked-path based.
Through v0.9.10 each opted-in existence rule consulted `ctx.is_git_tracked(path)` inline, the
same silent-drop bug class that hit `scope_filter:`.

v0.9.11 builds two filtered `FileIndex` views once per run when any rule opts in: a file-tracked subset (entries where `git_tracked.contains(path)`) and a dir-aware subset (dirs that recursively contain at least one tracked file). The engine substitutes the pre-filtered index via `pick_ctx`; the rule's `evaluate` body never sees an untracked path, and the runtime `is_git_tracked()` check disappears from per-rule code. Full design: [the v0.9 git-tracked filtered-index pass](https://github.com/asamarts/alint/blob/main/docs/design/v0.9/git-tracked-filtered-index.md).

### Composition

`extends` accepts local paths, HTTPS URLs (with optional SHA-256 subresource integrity), and
bundled URIs. Entries may filter inherited rule IDs with `only`/`except`. Sources resolve
left-to-right with cycle detection and caching; rule mappings merge one field level deep by ID,
and the child wins. Setting `level: off` disables an inherited rule.

Bundled rulesets are referenced via `alint://bundled/<name>@v<major>`.

Top-level templates provide string substitution before typed rule decoding. Optional nested
configs scope rules to subtrees. Baselines suppress known finding identities after evaluation.
Trust is monotone: an inherited source cannot grant itself process execution, outside-root
reads, top-level exception/baseline authority, or a more permissive fix tier. These are distinct
composition mechanisms and intentionally do not form a general macro language.

## Execution model

The pipeline from `alint check` to output:

<likec4-view view-id="checkFlow"></likec4-view>

1. **Config load and build.** Read `.alint.yml`; resolve composition with caching, cycle and
   trust checks; decode the merged YAML into typed specs; build every enabled top-level rule and
   structurally validate its nested rules; reject invalid kinds/options and cross-field
   combinations. JSON Schema is the editor/test contract, not a runtime validation pass.
2. **Walk.** Build one full `FileIndex` with the `ignore` crate's parallel walker. Workers collect
   entries locally; a deterministic post-sort removes filesystem/thread scheduling from output.
   Lazy `OnceLock` indexes accelerate membership, child, descendant, and basename queries.
3. **Inputs and derived views.** Evaluate facts sequentially against the full index. Collect Git
   tracked/blame data only if requested, resolve changed/manifest-derived scope maps once, and
   build changed or Git-tracked index views as needed. The LSP's single-file evaluator caches
   facts on its long-lived index; a normal CLI run does not use a persistent content-hash cache.
4. **Gates.** Skip `off` entries during loading/building; evaluate `when:` once per live rule;
   apply changed-mode, `expect_matches`, scope, and kind-specific target gates. Gate errors become
   visible findings rather than silent passes.
5. **Dispatch.** `Rule::as_per_file() == Some(...)` joins the file-major loop. All other rules
   use the rule-major loop. Independently, `requires_full_index()` selects the full rather than
   changed-only index for rules whose verdict needs whole-repository context.
6. **Evaluate.** During `check`, both loops fan out through `rayon`. The file-major loop reads one
   matched file once and shares its byte slice among applicable `PerFileRule`s (respecting each
   rule's prefix-read cap). Rule-major evaluators own any reads they require and may therefore
   reread a file used by another rule.
7. **Aggregate and post-process.** Reassemble results in config order, partition notes from
   violations, attach fixability/proposed edits where the selected formatter needs them, and
   apply an optional baseline before deciding the exit status.
8. **Fix (optional).** Fix evaluation and application are serial to preserve configuration order.
   Applicability gates writes; whole-file edits compose in memory, located edits are ordered and
   overlap-checked, and path operations apply directly or stage for `--diff`. A real fix re-walks
   after a pass that changed bytes and repeats to a bounded fixpoint; dry-run is a non-mutating
   single-pass preview. Outcomes are `Applied`, `Skipped`, `Suggested`, or `Unfixable`/error.
9. **Emit.** Render one of the selected output formats and derive the command exit code.

The engine builds its main `FileIndex` with one repository walk per pass. An explicitly effectful
kind such as `generated_file_fresh` may run a tool and perform its own before/after inspection;
that work is outside the shared-walk guarantee. The stronger read-once guarantee applies to the
file-major partition only, not to every rule or fact provider. Check evaluation is parallel; fix
evaluation/application is ordered and serial. Content writes are atomic per flushed file, but the
whole multi-file fix pass is not a transaction. See the
[v0.17 fixpoint design](https://github.com/asamarts/alint/blob/main/docs/design/v0.17/fixpoint.md).

Step 3 in detail: facts are evaluated once per normal engine run, then gate rules through their
`when:` conditions. The LSP reuses a cached result while its index is valid.

<likec4-view view-id="factsFlow"></likec4-view>

Steps 5 and 6 in detail: dispatch partitions rules into rule-major and opted-in file-major work;
only the latter coalesces reads across matching rules (ADR-0003).

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
│   ├── alint-dsl/          YAML composition/typed loading + embedded editor schema + bundled rulesets
│   ├── alint-rules/        built-in rule implementations
│   ├── alint-output/       formatters (human, json, sarif, github, gitlab, junit, markdown, agent)
│   ├── alint-lsp/          language server behind the `lsp` subcommand (tower-lsp)
│   ├── alint-bench/        criterion micro-benches + seeded tree generator
│   ├── alint-testkit/      shared test harness (treespec materializer, scenario runner, proptest strategies)
│   └── alint-e2e/          end-to-end scenarios + coverage audits + cross-cutting invariant tests
├── xtask/                  cargo-xtask helpers (bench-release driver, docs-export, publish-benches)
├── ci/                     self-hosted runner + per-job shell scripts
├── editors/                editor integrations (VS Code, Zed, JetBrains, Neovim, Helix, Emacs, Sublime, Eclipse)
├── schemas/v1/             JSON Schemas for .alint.yml and report shapes (only config.json is mirrored in alint-dsl)
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

**Possible future extraction/extension points (see
[ROADMAP.md](https://github.com/asamarts/alint/blob/main/docs/design/ROADMAP.md)):**

- `crates/alint-plugin/`: a WASM plugin host remains unscheduled backlog; the tier-1 `command`
  plugin already lives in `alint-rules`.
- `crates/alint-facts/`: currently subsumed by `alint-core::facts`; promotion to its own crate is deferred until the language and license detectors (`detect: linguist`, `detect: askalono`) land.

### Publishing intent (crates.io)

The public crate surface is kept narrow so the semver-stable API is small and maintainable.

| Crate | `publish` | Why |
|---|---|---|
| `alint` (binary) | public | Enables `cargo install alint`. Package name matches `[[bin]] name`. |
| `alint-core` | public | Embeddable engine for custom drivers: scripts, custom CI gates, third-party hosts. Semver-stable from 1.0. |
| `alint-dsl`, `alint-rules`, `alint-output`, `alint-lsp` | published, but documented as internal | Cargo requires the binary's workspace dependencies to be resolvable from crates.io. Publication does not make these semver-stable public APIs. |
| `alint-bench`, `alint-testkit`, `alint-e2e`, `xtask` | `publish = false` | Repository-only benchmarking, testing, and maintenance tooling. |

`publish` is a packaging property, not an API-stability promise. Only `alint-core` is presented
as an embeddable library; implementation crates carry an `Internal` package description even
though the release pipeline publishes them as dependencies of `alint`.

## Plugin model

<likec4-view view-id="pluginModel"></likec4-view>

Two tiers, introduced across the roadmap:

- **`command` rule kind** (shipped). The rule spawns a configured argv per matched file; it does
  not implicitly invoke a shell. Exit code is the verdict; bounded stdout/stderr form the default
  failure message. Environment variables expose path, root, rule id, level, vars, and facts.
  Simple, scriptable, language-agnostic.
- **Future `wasm` plugin kind** (unscheduled backlog). The current direction is a stable WIT
  interface, a filesystem sandbox, and a signed registry. Exact inputs, distribution, authority,
  determinism, and capability grants remain design decisions; `general-linter.md` proposes the
  stricter typed adapter/analyzer boundary.

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
- `agent`: agent-oriented JSON sibling of `json`, with a per-violation instruction composed from
  severity, message, location, actual Safe-fix availability, and policy URL, plus a machine-usable
  fix command/edit when available (v0.6).

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
  - id: cargo-member-paths-are-canonical
    kind: toml_path_matches
    paths: "Cargo.toml"
    path: "$.workspace.members[*]"
    matches: "^crates/[a-z][a-z0-9-]*$"
    level: error

  - id: crates-have-readme
    kind: for_each_dir
    select: "crates/*"
    require:
      - kind: file_exists
        paths: "{path}/README.md"
    level: error

  - id: handlers-have-integration-tests
    kind: pair
    primary: "src/handlers/*.rs"
    partner: "tests/{stem}_test.rs"
    level: warning

  - id: bench-gated
    when: facts.has_benches
    kind: file_exists
    paths: "benches/Cargo.toml"
    level: error
```

## Contributing new rule kinds

A new kind follows the repository's spec-driven workflow, not only a trait implementation:

1. Establish demand and semantics in the appropriate design document: target domain, empty-set
   behavior, changed-mode/full-index needs, errors, finding identity, trust, and fixes.
2. Define a `deny_unknown_fields` options type and generated schema fragment, then implement the
   `Rule` (and `PerFileRule` only when it truly accepts one preloaded file at a time).
3. Register the builder, option schema, canonical kind, aliases, and family/category metadata in
   their single sources of truth.
4. Add unit/property tests plus at least one firing and one silent end-to-end scenario. Cover
   changed mode, scoping, limits, trust, output identity, and fix convergence when applicable.
5. Add examples and generated rule documentation; run all catalogue/schema/docs/coverage gates.
6. Update roadmap/design status only when the implementation and evidence ship together.

The authoritative checklist is
[`docs/development/rule-authoring.md`](https://github.com/asamarts/alint/blob/main/docs/development/rule-authoring.md),
with non-negotiable invariants in
[`constitution.md`](https://github.com/asamarts/alint/blob/main/docs/design/constitution.md) and
the [spec-driven development guide](https://github.com/asamarts/alint/blob/main/docs/design/spec-driven-development.md).
