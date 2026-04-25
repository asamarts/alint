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

Extra glob patterns to exclude from the walk, on top of `.gitignore`.

```yaml
ignore:
  - "vendor/**"
  - "**/*.snapshot.json"
```

### `respect_gitignore`

Whether to honor `.gitignore` files during the walk. Default `true`. Set to `false` to lint every file regardless of git's ignore rules — useful for CI runs that want to enforce policy on otherwise-ignored content.

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
  - id: is_rust
    any_file_exists: [Cargo.toml]
  - id: n_rs_files
    count_files: "**/*.rs"
  - id: has_src_dir
    all_files_exist: ["src/.keep"]

rules:
  - id: rust-snake-case
    when: facts.is_rust and facts.n_rs_files > 5
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
- **`fix`** — fix-op declaration (e.g. `file_trim_trailing_whitespace: {}`).
- **`message`** — override the rule's display message.
- **`policy_url`** — link surfaced when the rule fires.

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
