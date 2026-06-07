---
title: 'php@v1'
description: 'Baseline conventions for PHP / Composer projects. alint bundled ruleset php@v1.'
---

Baseline conventions for PHP / Composer projects. Adopt it with:

```yaml
extends:
  - alint://bundled/php@v1
```

Gated with `when: facts.has_php` (true if any composer.json
exists anywhere in the tree) so the whole ruleset is a silent
no-op in non-PHP repositories — useful in polyglot monorepos
where a PHP package sits alongside other ecosystems. Override
`has_php` with your own `facts:` block if your project uses a
non-standard layout.

The heart of the ruleset is the set of **"Composer-fatals"
invariants**: composer.json declares autoload roots and console
binaries, and Composer aborts at install/autoload time if any of
those paths is missing. Those are pure cross-file path-existence
checks (`registry_paths_resolve`) that alint expresses natively,
without running Composer — the same checks laravel and phpstan
hand-roll. Around them sit a composer.json metadata check
(`name`, structured-query) and a build-hygiene guard (no
committed `vendor/`).

Levels are deliberately non-blocking (no `error`) given the
broad adopter surface (every composer/* package, every
Symfony/Laravel app); upgrade severity in your own config when
you are ready to enforce. The structured-query rule is
`if_present: true` — it flags a *misconfiguration*, never forces
a property to exist (an application composer.json with no
published `name` is fine). The path-resolve rules are naturally
silent when their field is absent (zero extracted entries = no
violations).

Scope note: the path-resolve rules read the ROOT composer.json.
In a monorepo of sub-packages each with its own composer.json,
add per-package rules (or a `for_each_dir`) in your own config;
this baseline covers the dominant single-root-package case.

## Rules

### `php-composer-name-format`

- **kind**: [`json_path_matches`](/docs/rules/structured-query/json_path_matches/)
- **level**: `warning`
- **when**: `facts.has_php`
- **policy**: <https://getcomposer.org/doc/04-schema.md#name>

> PHP: composer.json `name` must be a lowercase `vendor/package` identifier (Composer/Packagist reject other forms).

### `php-composer-psr4-dirs-resolve`

- **kind**: [`registry_paths_resolve`](/docs/rules/cross-file/registry_paths_resolve/)
- **level**: `warning`
- **when**: `facts.has_php`
- **policy**: <https://getcomposer.org/doc/04-schema.md#psr-4>

> PHP: every composer.json `autoload.psr-4` directory must exist (Composer cannot build the autoloader otherwise).

### `php-composer-autoload-files-resolve`

- **kind**: [`registry_paths_resolve`](/docs/rules/cross-file/registry_paths_resolve/)
- **level**: `warning`
- **when**: `facts.has_php`
- **policy**: <https://getcomposer.org/doc/04-schema.md#files>

> PHP: every composer.json `autoload.files` entry must exist on disk (Composer fatals at autoload time otherwise).

### `php-composer-autoload-dev-files-resolve`

- **kind**: [`registry_paths_resolve`](/docs/rules/cross-file/registry_paths_resolve/)
- **level**: `info`
- **when**: `facts.has_php`
- **policy**: <https://getcomposer.org/doc/04-schema.md#files>

> PHP: every composer.json `autoload-dev.files` entry should exist on disk (it is require()'d under --dev).

### `php-composer-bin-resolve`

- **kind**: [`registry_paths_resolve`](/docs/rules/cross-file/registry_paths_resolve/)
- **level**: `warning`
- **when**: `facts.has_php`
- **policy**: <https://getcomposer.org/doc/articles/vendor-binaries.md>

> PHP: every composer.json `bin` entry must exist on disk (Composer symlinks it as a vendor binary).

### `php-no-vendor-committed`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `warning`
- **when**: `facts.has_php`
- **policy**: <https://getcomposer.org/doc/01-basic-usage.md#installing-dependencies>

> PHP: Composer's `vendor/` directory must not be committed — add it to .gitignore and run `composer install` to restore it.

## Source

The full ruleset definition is committed at [`crates/alint-dsl/rulesets/v1/php.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/php.yml) in the alint repo (the snapshot below is generated verbatim from that file).

```yaml
# alint://bundled/php@v1
#
# Baseline conventions for PHP / Composer projects. Adopt it with:
#
#     extends:
#       - alint://bundled/php@v1
#
# Gated with `when: facts.has_php` (true if any composer.json
# exists anywhere in the tree) so the whole ruleset is a silent
# no-op in non-PHP repositories — useful in polyglot monorepos
# where a PHP package sits alongside other ecosystems. Override
# `has_php` with your own `facts:` block if your project uses a
# non-standard layout.
#
# The heart of the ruleset is the set of **"Composer-fatals"
# invariants**: composer.json declares autoload roots and console
# binaries, and Composer aborts at install/autoload time if any of
# those paths is missing. Those are pure cross-file path-existence
# checks (`registry_paths_resolve`) that alint expresses natively,
# without running Composer — the same checks laravel and phpstan
# hand-roll. Around them sit a composer.json metadata check
# (`name`, structured-query) and a build-hygiene guard (no
# committed `vendor/`).
#
# Levels are deliberately non-blocking (no `error`) given the
# broad adopter surface (every composer/* package, every
# Symfony/Laravel app); upgrade severity in your own config when
# you are ready to enforce. The structured-query rule is
# `if_present: true` — it flags a *misconfiguration*, never forces
# a property to exist (an application composer.json with no
# published `name` is fine). The path-resolve rules are naturally
# silent when their field is absent (zero extracted entries = no
# violations).
#
# Scope note: the path-resolve rules read the ROOT composer.json.
# In a monorepo of sub-packages each with its own composer.json,
# add per-package rules (or a `for_each_dir`) in your own config;
# this baseline covers the dominant single-root-package case.

version: 1

facts:
  - id: has_php
    any_file_exists:
      - composer.json
      - "**/composer.json"

rules:
  # --- composer.json metadata -------------------------------------
  - id: php-composer-name-format
    when: facts.has_php
    # Composer / Packagist require the package `name` to be
    # `vendor/package`, lowercase, with `-`/`_`/`.` separators.
    # `if_present`: an application (non-published) composer.json
    # may legitimately omit `name`, so a missing name is silent;
    # only a malformed name fires. Vendored manifests are excluded.
    kind: json_path_matches
    paths:
      include: ["composer.json", "**/composer.json"]
      exclude: ["vendor/**", "**/vendor/**"]
    path: "$.name"
    matches: '^[a-z0-9]([_.-]?[a-z0-9]+)*/[a-z0-9]([_.-]?[a-z0-9]+)*$'
    if_present: true
    level: warning
    message: >-
      PHP: composer.json `name` must be a lowercase `vendor/package`
      identifier (Composer/Packagist reject other forms).
    policy_url: "https://getcomposer.org/doc/04-schema.md#name"

  # --- "Composer fatals" invariants (cross-file path resolve) -----
  - id: php-composer-psr4-dirs-resolve
    when: facts.has_php
    # Every PSR-4 autoload namespace root must exist as a directory
    # — Composer fails to build its autoloader otherwise. (Array-
    # valued PSR-4 entries, the rare multi-dir form, are skipped.)
    kind: registry_paths_resolve
    source: composer.json
    extract: { json: "$.autoload['psr-4'].*" }
    expect: dir
    level: warning
    message: >-
      PHP: every composer.json `autoload.psr-4` directory must exist
      (Composer cannot build the autoloader otherwise).
    policy_url: "https://getcomposer.org/doc/04-schema.md#psr-4"

  - id: php-composer-autoload-files-resolve
    when: facts.has_php
    # `autoload.files` are eagerly require()'d on every request;
    # a missing one is a hard fatal.
    kind: registry_paths_resolve
    source: composer.json
    extract: { json: "$.autoload.files[*]" }
    expect: file
    level: warning
    message: >-
      PHP: every composer.json `autoload.files` entry must exist on
      disk (Composer fatals at autoload time otherwise).
    policy_url: "https://getcomposer.org/doc/04-schema.md#files"

  - id: php-composer-autoload-dev-files-resolve
    when: facts.has_php
    # The dev counterpart (test bootstraps / fixtures). Info-level
    # because it only loads under `--dev`.
    kind: registry_paths_resolve
    source: composer.json
    extract: { json: "$['autoload-dev'].files[*]" }
    expect: file
    level: info
    message: >-
      PHP: every composer.json `autoload-dev.files` entry should
      exist on disk (it is require()'d under --dev).
    policy_url: "https://getcomposer.org/doc/04-schema.md#files"

  - id: php-composer-bin-resolve
    when: facts.has_php
    # Declared console binaries (`composer global require`
    # symlinks them into the bin-dir); a missing target breaks the
    # installed command.
    kind: registry_paths_resolve
    source: composer.json
    extract: { json: "$.bin[*]" }
    expect: file
    level: warning
    message: >-
      PHP: every composer.json `bin` entry must exist on disk
      (Composer symlinks it as a vendor binary).
    policy_url: "https://getcomposer.org/doc/articles/vendor-binaries.md"

  # --- build-output hygiene (PHP-specific, ecosystem-gated) -------
  - id: php-no-vendor-committed
    when: facts.has_php
    # `vendor/` is Composer's install directory — generated, not
    # source. The standard composer `.gitignore` excludes it.
    # Disable on the rare repo that vendors its dependencies for
    # deployment.
    kind: dir_absent
    paths: ["vendor", "**/vendor"]
    level: warning
    message: >-
      PHP: Composer's `vendor/` directory must not be committed —
      add it to .gitignore and run `composer install` to restore it.
    policy_url: "https://getcomposer.org/doc/01-basic-usage.md#installing-dependencies"
```
