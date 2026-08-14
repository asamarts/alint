---
title: Configuration
description: Every top-level field of .alint.yml, with examples and the JSON Schema reference.
sidebar:
  order: 1
---

`.alint.yml` is the only file alint reads. It declares the rules, the rule sources, the facts they're gated on, and a handful of run-time knobs.

The entities of a config and how they relate:

<likec4-view view-id="configModel"></likec4-view>

Point your YAML language server at the JSON Schema for editor autocomplete:

```yaml
# yaml-language-server: $schema=https://alint.org/_alint/configuration/schema.json
version: 1
extends:
  - alint://bundled/oss-baseline@v1
```

The schema is also published in the alint repo at [`schemas/v1/config.json`](https://github.com/asamarts/alint/blob/main/schemas/v1/config.json).

## Top-level fields

### `version`

Schema version. Always `1` for the current schema. Required.

```yaml
version: 1
```

A future schema bump (`version: 2`, …) would be an explicit migration; v1 is stable.

### `extends`

Configs to inherit from. Resolved left-to-right; later entries override earlier ones; the current file's own definitions override everything it extends.

Each entry is either a bare string or a mapping with `url:` and optional `only:` / `except:` filters:

```yaml
extends:
  # Local file (relative to the current .alint.yml):
  - ./shared/team-defaults.yml

  # HTTPS URL with required SHA-256 SRI:
  - https://example.com/rules.yml#sha256-abc123…

  # Bundled ruleset, resolved offline from the binary:
  - alint://bundled/oss-baseline@v1

  # Mapping form, same source kinds, but with filters:
  - url: alint://bundled/ci/github-actions@v1
    only: [gha-pin-actions-to-sha]

  - url: alint://bundled/oss-baseline@v1
    except: [oss-code-of-conduct-exists]
```

`only:` and `except:` are mutually exclusive on a single entry. Listing an unknown rule id is a load-time error.

Bundled and HTTPS configs cannot themselves declare `extends:`, because relative-path resolution in a fetched body has no principled base. Nest extends locally instead.

### `ignore`

Extra glob patterns to exclude from the walk, on top of `.gitignore`. Same gitignore-style syntax. Use this for repo-specific exclusions you don't want in `.gitignore` itself (because they're an alint concern, not a git concern):

```yaml
ignore:
  - "vendor/**"
  - "**/*.snapshot.json"
  - "fixtures/golden/**"
```

`ignore:` patterns apply regardless of `respect_gitignore`. See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for what gets filtered by default and how absence-style rules interpret git state.

### `respect_gitignore`

Whether to honor `.gitignore` files (and `.git/info/exclude`, the global gitignore, and `.ignore` files) during the walk. Default `true`.

```yaml
respect_gitignore: true   # default; honor .gitignore
# respect_gitignore: false  # lint everything on disk regardless
```

Setting it to `false` is rarely useful during normal development because absence-style rules (`dir_absent`, `file_absent`) start firing on every locally-built artefact (`target/`, `node_modules/`, `__pycache__/`, and so on). It's appropriate for one-off audits or for directories that aren't git repos at all. The CLI's `--no-gitignore` flag overrides this for one invocation.

The full implications (including how absence-style rules interpret "tracked" vs "ignored" and where this approximation diverges from git's actual index) live in [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/).

### `vars`

Free-form string variables referenced from rule messages as `{{vars.<name>}}` and from `when:` clauses as `vars.<name>`.

```yaml
vars:
  copyright_year: "2026"
  org_name: "Acme"

rules:
  - id: copyright-header
    kind: file_header
    paths: "src/**/*.rs"
    pattern: '^// Copyright \\(c\\) {{vars.copyright_year}} {{vars.org_name}}'
    level: error
```

### `facts`

Properties of the repo evaluated once per run. Used in `when:` clauses to gate rules conditionally.

```yaml
facts:
  - id: has_rust
    any_file_exists: [Cargo.toml]
  - id: n_rs_files
    count_files: "**/*.rs"
  - id: has_src_dir
    all_files_exist: ["src/.keep"]

rules:
  - id: rust-snake-case
    when: facts.has_rust and facts.n_rs_files > 5
    kind: filename_case
    paths: "src/**/*.rs"
    case: snake
    level: error
```

Available fact kinds: `any_file_exists`, `all_files_exist`, `count_files`, `file_content_matches`, `git_branch`, `custom`.

`custom` (which spawns a subprocess) is a security boundary: it's only allowed in your own top-level config. Any `extends:` ancestor that declares one is rejected at load time, so a malicious or compromised ruleset can't execute arbitrary code merely by being fetched.

### `rules`

The rules themselves. Each has at least an `id`, `kind`, and `level`. Most have a `paths` glob; some kinds add their own option fields (e.g. `min_lines:`, `path:` + `equals:` for structured queries). See the [Rules](/docs/rules/) section for every kind.

```yaml
rules:
  - id: readme-exists
    kind: file_exists
    paths: ["README.md", "README", "README.rst"]
    root_only: true
    level: error
    fix:
      file_create:
        content: "# Project\n"

  - id: no-bidi-controls
    kind: no_bidi_controls
    paths: "**/*"
    level: error
    policy_url: "https://trojansource.codes/"
```

Common per-rule fields:

- **`id`** *(required)*: kebab-case identifier. Stable; used to override or disable the rule from a child config.
- **`kind`** *(required)*: which built-in implementation to invoke. Required somewhere in the `extends:` chain.
- **`level`** *(required)*: `error`, `warning`, `info`, or `off`. `off` disables the rule entirely.
- **`paths`**: glob, list of globs, or `{include, exclude}` pair. Required for most kinds.
- **`when`**: bounded expression gating the rule on facts / vars.
- **`scope_filter`**: extra per-file scoping — by ancestor manifest presence, git diff, or membership in a manifest-declared path set (see below). Cross-file rules reject this field at build time.
- **`fix`**: fix-op declaration (e.g. `file_trim_trailing_whitespace: {}`).
- **`message`**: override the rule's display message.
- **`policy_url`**: link surfaced when the rule fires.

#### `scope_filter` *(per-file rules, v0.9.6+)*

Narrows a per-file rule to files that have a specified manifest somewhere in their ancestor directory chain. The engine walks `Path::parent()` upward from the file (the file's own directory counts as an ancestor) and consults the file index at each step; first-match-wins on the upward walk gates the rule per-file. Combine with the rule's existing `paths:`. Both must match for the rule to fire.

```yaml
rules:
  - id: rust-sources-no-bidi
    when: facts.has_rust
    kind: no_bidi_controls
    paths: "**/*.rs"
    scope_filter:
      has_ancestor: Cargo.toml      # single string OR a list
    level: error
```

`has_ancestor:` accepts a literal filename or a list of filenames; path separators and glob metacharacters are rejected at build time. The bundled ecosystem rulesets (`rust@v1`, `node@v1`, `python@v1`, `go@v1`, `java@v1`) use this to scope per-file content rules to their ecosystem's package subtrees in polyglot monorepos.

`changed_since: <git-ref>` (v0.11+) narrows a per-file rule to files in the `<ref>...HEAD` diff — the same merge-base diff as `alint check --changed`. Use it to grandfather pre-existing files in a PR (e.g. require an SPDX header only on files the PR touched):

```yaml
rules:
  - id: spdx-on-new-files
    kind: file_header
    paths: "src/**/*.rs"
    pattern: "^// SPDX-License-Identifier:"
    scope_filter:
      changed_since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
    level: error
```

It accepts the `{{env.X}}` interpolation, resolves the diff once per run, matches nothing outside a git repo (silent), and hard-errors on an unresolvable ref with a shallow-clone hint. The `scope_filter:` predicates AND-compose when more than one is set; at least one of `has_ancestor:`, `changed_since:`, `include_manifest_paths:`, or `exclude_manifest_paths:` must be present.

`include_manifest_paths:` / `exclude_manifest_paths:` (v0.15+) scope a per-file rule by membership in a path set a **manifest** declares, so the manifest that owns the truth and the rule that depends on it stay in one place instead of a hand-maintained `paths.exclude` that drifts. `exclude_manifest_paths:` drops files in the set; `include_manifest_paths:` keeps only files in it.

Exempt the `package.json` `bin` entrypoints from a `no-console` rule — even though `bin` names the *build output* (`dist/cli.js`), `derive_target:` maps it back to source:

```yaml
rules:
  - id: no-stray-console
    kind: file_content_forbidden
    paths: "src/**/*.ts"
    pattern: 'console\.(log|debug|info)\('
    scope_filter:
      exclude_manifest_paths:
        source: package.json              # the manifest (always repo-root-confined)
        extract: { json: "$.bin.*" }      # the shared extract one-of
        derive_target:                    # optional: map declared output -> source
          from: '^dist/(.*)\.js$'
          to:   'src/$1.ts'
    level: error
```

Or scope a rule to only the source directories a workspace manifest declares:

```yaml
rules:
  - id: rust-hygiene
    kind: no_trailing_whitespace
    paths: "**/*.rs"
    scope_filter:
      include_manifest_paths:
        source: Cargo.toml
        extract: { toml: "$.workspace.members[*]" }
    level: warning
```

Each predicate takes a **`source:`** (the manifest file, always repo-root-confined; its declared paths resolve relative to its own directory), an **`extract:`** (the shared `{ json | toml | yaml: <JSONPath> }` / `{ lines }` / `{ regex }` extractor `registry_paths_resolve` and `file_graph` also use; non-literal entries are dropped), an optional **`derive_target: { from, to }`** regex mapping applied to each extracted path (a path that does not match `from` is dropped), and, for `include_manifest_paths:` only, **`expect_nonempty:`** (default `true`) to warn when the set is empty — an empty include set would otherwise silently no-op the whole rule.

Membership is **directory-aware**: a declared file matches itself; a declared directory (a workspace member) matches every file under it, respecting component boundaries (`crates/a` does not match `crates/ab`). The set is extracted once per run and cached, like `changed_since:`. A manifest that is absent or unreadable contributes nothing — the rule runs full-scope for `exclude`, matches nothing for `include`. A manifest **value** gates *which* files a rule sees, never *what* it decides about a file: content rules never read the manifest, extraction is pure-parse (no spawn, safe inside an `extends:`'d ruleset), and `alint explain <rule>` prints the resolved set.

Cross-file rules (`pair`, `for_each_dir`, `file_exists`, etc.) reject `scope_filter:` at build time with a pointer to the `for_each_dir + when_iter:` pattern. Rule-major rules like `filename_case` silently ignore the field; gate them via the rule's `paths:` glob instead.

### `fix_size_limit`

Maximum file size, in bytes, that content-editing fixes will read and rewrite. Files over this limit are reported as `Skipped` in the fix report and a one-line warning is printed to stderr.

```yaml
fix_size_limit: 1048576   # 1 MiB; the default
# fix_size_limit: null     # disable the cap entirely (not recommended)
```

Path-only fixes (`file_create`, `file_remove`, `file_rename`) ignore the cap, since they don't read content.

### `nested_configs`

Opt in to discovery of `.alint.yml` / `.alint.yaml` files in subdirectories. Default `false`.

```yaml
# repo-root .alint.yml
version: 1
nested_configs: true
extends:
  - alint://bundled/oss-baseline@v1
```

When `true`, the loader walks the tree from the root config's directory (respecting `.gitignore` and `ignore:`) and picks up every nested config. Each nested rule's path-like scope fields (`paths`, `select`, `primary`) are auto-prefixed with the nested config's relative directory, so the rule scopes to that subtree.

```yaml
# packages/frontend/.alint.yml
version: 1
rules:
  - id: components-pascal
    kind: filename_case
    paths: "components/**/*.{tsx,jsx}"
    # ↑ evaluates as if it read paths: "packages/frontend/components/**/*.{tsx,jsx}"
    case: pascal
    level: error
```

Guardrails: nested configs may only declare `version:` and `rules:`; every nested rule must have at least one scope field; absolute paths and `..`-prefixed globs are rejected; rule-id collisions across configs error with a clear message.

Only the user's top-level config may set `nested_configs: true`. Nested configs themselves cannot spawn further nested discovery (one level of opt-in, intentionally).

### `allow_out_of_root`

By default alint confines every config-declared path to the repository root: a rule can never read or resolve a path outside the tree it was pointed at. `allow_out_of_root:` is a deliberate, top-level-only opt-in to relax that for *reads* — when a trusted config needs to reference an external file (a shared JSON schema, a manifest in a sibling checkout).

```yaml
allow_out_of_root: true            # every rule may read out-of-root paths
```

Or scope it to specific rule kinds and/or ids — a rule is permitted if its `kind` is in `kinds` **or** its `id` is in `rules`:

```yaml
allow_out_of_root:
  kinds: [json_schema_passes, pair_hash]   # any rule of these kinds
  rules: [external-shared-schema]          # specific rule ids
```

Absent or `false` keeps the secure default (full confinement).

**Security.** Like the spawning-rule trust gate, `allow_out_of_root:` is honored **only** from your own top-level config — any `extends:`'d ruleset that declares it is a load-time error, so adopting a published ruleset can never grant it out-of-tree reads. It currently applies to the read kinds `json_schema_passes` (`schema_path:`), `pair_hash` (`target:`), and `registry_paths_resolve` (`source:`); a permitted read emits an informational note so the escape is never silent. Resolve/index existence checks stay confined regardless.

### `baseline`

Path to a committed baseline file that grandfathers pre-existing violations, so `alint check` reports and gates on only *new* findings. Persisting it here means CI need not pass `--baseline` on every run.

```yaml
baseline: .alint-baseline.json
```

A `--baseline <path>` flag overrides this key. There is **no silent auto-detect**: a baseline suppresses findings only when it is explicitly opted in (via this key or the flag), never because a baseline file merely exists on disk. Write and refresh the file with `alint baseline`. See [Baseline mode](/docs/concepts/baseline/) for the full workflow, the fingerprinting semantics, and which output formats are baseline-aware.

## See also

- [JSON Schema](https://alint.org/_alint/configuration/schema.json): authoritative source for option types.
- [Rules](/docs/rules/): every rule kind, organised by family, with per-rule options.
- [Concepts](/docs/concepts/): the rule model and `when:` expression language explained in depth.
