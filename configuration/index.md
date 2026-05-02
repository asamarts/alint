---
title: Configuration
description: Every top-level field of .alint.yml, with examples and the JSON Schema reference.
sidebar:
  order: 1
---

`.alint.yml` is the only file alint reads. It declares the rules, the rule sources, the facts they're gated on, and a handful of run-time knobs.

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

  # Mapping form — same source kinds, but with filters:
  - url: alint://bundled/ci/github-actions@v1
    only: [gha-pin-actions-to-sha]

  - url: alint://bundled/oss-baseline@v1
    except: [oss-code-of-conduct-exists]
```

`only:` and `except:` are mutually exclusive on a single entry. Listing an unknown rule id is a load-time error.

Bundled and HTTPS configs cannot themselves declare `extends:` — relative-path resolution in a fetched body has no principled base. Nest extends locally instead.

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
respect_gitignore: true   # default — honor .gitignore
# respect_gitignore: false  # lint everything on disk regardless
```

Setting it to `false` is rarely useful during normal development because absence-style rules (`dir_absent`, `file_absent`) start firing on every locally-built artefact (`target/`, `node_modules/`, `__pycache__/`, …). It's appropriate for one-off audits or for directories that aren't git repos at all. The CLI's `--no-gitignore` flag overrides this for one invocation.

The full implications — including how absence-style rules interpret "tracked" vs "ignored" and where this approximation diverges from git's actual index — live in [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/).

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

- **`id`** *(required)* — kebab-case identifier. Stable; used to override or disable the rule from a child config.
- **`kind`** *(required)* — which built-in implementation to invoke. Required somewhere in the `extends:` chain.
- **`level`** *(required)* — `error`, `warning`, `info`, or `off`. `off` disables the rule entirely.
- **`paths`** — glob, list of globs, or `{include, exclude}` pair. Required for most kinds.
- **`when`** — bounded expression gating the rule on facts / vars.
- **`scope_filter`** — closest-ancestor manifest scoping for per-file rules (see below). Cross-file rules reject this field at build time.
- **`fix`** — fix-op declaration (e.g. `file_trim_trailing_whitespace: {}`).
- **`message`** — override the rule's display message.
- **`policy_url`** — link surfaced when the rule fires.

#### `scope_filter` *(per-file rules, v0.9.6+)*

Narrows a per-file rule to files that have a specified manifest somewhere in their ancestor directory chain. The engine walks `Path::parent()` upward from the file (the file's own directory counts as an ancestor) and consults the file index at each step; first-match-wins on the upward walk gates the rule per-file. Combine with the rule's existing `paths:` — both must match for the rule to fire.

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

Cross-file rules (`pair`, `for_each_dir`, `file_exists`, etc.) reject `scope_filter:` at build time with a pointer to the `for_each_dir + when_iter:` pattern. Rule-major rules like `filename_case` silently ignore the field — gate them via the rule's `paths:` glob instead.

### `fix_size_limit`

Maximum file size, in bytes, that content-editing fixes will read and rewrite. Files over this limit are reported as `Skipped` in the fix report and a one-line warning is printed to stderr.

```yaml
fix_size_limit: 1048576   # 1 MiB; the default
# fix_size_limit: null     # disable the cap entirely (not recommended)
```

Path-only fixes (`file_create`, `file_remove`, `file_rename`) ignore the cap — they don't read content.

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

Only the user's top-level config may set `nested_configs: true` — nested configs themselves cannot spawn further nested discovery (one level of opt-in, intentionally).

## See also

- [JSON Schema](https://alint.org/_alint/configuration/schema.json) — authoritative source for option types.
- [Rules](/docs/rules/) — every rule kind, organised by family, with per-rule options.
- [Concepts](/docs/concepts/) — the rule model and `when:` expression language explained in depth.
