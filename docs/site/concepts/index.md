---
title: Concepts
description: The rule model, scopes, when-expressions, composition, auto-fix, output formats.
sidebar:
  order: 1
---

The Concepts section is the conceptual foundation that the rest of the docs assume. Skim it once; come back when something downstream confuses you.

## What alint is

alint is a static Rust binary that lints the *shape* of a repository. Where ESLint lints code and Semgrep lints semantics, alint lints the things in between — required files (READMEs, LICENSEs, SECURITY.md), filename conventions, content patterns, the values inside `package.json` / `Cargo.toml` / GitHub workflows, and cross-file relationships like "every package has a README" or "every header has a matching source file."

## The rule model

Every rule has:

- **`id`** — a stable, kebab-case identifier. Required. Used to override or disable the rule from a child config.
- **`kind`** — which built-in rule implementation to invoke (e.g. `file_exists`, `json_path_equals`, `for_each_dir`). Required.
- **`level`** — `error`, `warning`, `info`, or `off`. Required.
- **`paths`** — a glob, list of globs, or `{include, exclude}` pair selecting which files the rule applies to. Required for most kinds.
- **`when`** — an expression gating the rule on facts or vars. Optional.
- **`fix`** — a fix-op declaration that turns this rule into an auto-fixable one. Optional.
- **`message`** / **`policy_url`** — display fields shown when the rule fires. Optional.

Plus kind-specific options. For example, `file_min_lines` takes `min_lines: <int>`; `json_path_matches` takes `path` (a JSONPath) and `matches` (a regex).

## Composition

alint configs compose via `extends:`. A config can inherit from local files, HTTPS URLs (with SRI hashes), or `alint://bundled/<name>@<rev>`. Children override inherited rules **field-by-field** by id — you only declare the fields that change. `only:` / `except:` filters narrow the inherited rule set further. `nested_configs: true` opts in to discovering `.alint.yml` files in subdirectories and auto-scoping their rules to the subtree they live in.

See the [Cookbook](/docs/cookbook/) for composition patterns in practice.

## Facts and `when:` expressions

Facts evaluate properties of the repo *once per run* and surface them as named values:

```yaml
facts:
  - id: is_rust
    any_file_exists: [Cargo.toml]
```

Rules reference facts in `when:` to gate themselves conditionally:

```yaml
- id: rust-snake-case
  when: facts.is_rust
  kind: filename_case
  paths: "src/**/*.rs"
  case: snake
  level: error
```

The `when:` grammar supports boolean logic (`and` / `or` / `not`), comparison (`==` `!=` `<` `<=` `>` `>=`), `in` (list / substring), `matches` (regex), literal types, and `facts.X` / `vars.X` identifiers. It's deliberately bounded — no arbitrary code, no dynamic evaluation.

## Auto-fix

Rules that opt in declare a `fix:` block. Twelve ops cover content edits (trim whitespace, append newline, normalize line endings, strip BOM / bidi / zero-width, collapse blank lines) and path-level changes (create / remove / rename / prepend / append).

```yaml
- id: trim-trailing-whitespace
  kind: no_trailing_whitespace
  paths: "**/*.md"
  level: info
  fix:
    file_trim_trailing_whitespace: {}
```

Preview with `alint fix --dry-run`; apply with `alint fix`. Content-editing ops honour `fix_size_limit` (default 1 MiB) and skip oversize files rather than rewriting them.

## Output formats

Four formats: `human` (default; colorized; grouped by file), `json` (stable schema), `sarif` (SARIF 2.1.0 for GitHub Code Scanning), `github` (`::error::` / `::warning::` workflow commands for inline PR annotations).

```bash
alint check --format json --compact
```

The `--compact` flag flips human output to one line per violation, suitable for piping to editors / grep / `wc -l`.
