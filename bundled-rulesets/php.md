---
title: 'php@v1'
description: 'php@v1 bundled alint ruleset: Baseline conventions for PHP / Composer projects.'
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

## What it checks

6 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [php-composer-name-format](#php-composer-name-format)<br>`warning` | PHP: composer.json `name` must be a lowercase `vendor/package` identifier (Composer/Packagist reject other forms). |
| [php-composer-psr4-dirs-resolve](#php-composer-psr4-dirs-resolve)<br>`warning` | PHP: every composer.json `autoload.psr-4` directory must exist (Composer cannot build the autoloader otherwise). |
| [php-composer-autoload-files-resolve](#php-composer-autoload-files-resolve)<br>`warning` | PHP: every composer.json `autoload.files` entry must exist on disk (Composer fatals at autoload time otherwise). |
| [php-composer-autoload-dev-files-resolve](#php-composer-autoload-dev-files-resolve)<br>`info` | PHP: every composer.json `autoload-dev.files` entry should exist on disk (it is require()'d under --dev). |
| [php-composer-bin-resolve](#php-composer-bin-resolve)<br>`warning` | PHP: every composer.json `bin` entry must exist on disk (Composer symlinks it as a vendor binary). |
| [php-no-vendor-committed](#php-no-vendor-committed)<br>`warning` | PHP: Composer's `vendor/` directory must not be committed; add it to .gitignore and run `composer install` to restore it. |

All 6 rules run only when `facts.has_php` holds, so the ruleset stays quiet in repositories it doesn't apply to.

## Rules

### `php-composer-name-format`

Composer / Packagist require the package `name` to be `vendor/package`, lowercase, with `-`/`_`/`.` separators. `if_present`: an application (non-published) composer.json may legitimately omit `name`, so a missing name is silent; only a malformed name fires. Vendored manifests are excluded.

- **kind**: [`json_path_matches`](/docs/rules/structured-query/json_path_matches/)
- **level**: `warning`
- **when**: `facts.has_php`
- **policy**: <https://getcomposer.org/doc/04-schema.md#name>

> PHP: composer.json `name` must be a lowercase `vendor/package` identifier (Composer/Packagist reject other forms).

```yaml
- id: php-composer-name-format
  when: facts.has_php
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
```

### `php-composer-psr4-dirs-resolve`

Every PSR-4 autoload namespace root must exist as a directory — Composer fails to build its autoloader otherwise. (Array- valued PSR-4 entries, the rare multi-dir form, are skipped.)

- **kind**: [`registry_paths_resolve`](/docs/rules/cross-file/registry_paths_resolve/)
- **level**: `warning`
- **when**: `facts.has_php`
- **policy**: <https://getcomposer.org/doc/04-schema.md#psr-4>

> PHP: every composer.json `autoload.psr-4` directory must exist (Composer cannot build the autoloader otherwise).

```yaml
- id: php-composer-psr4-dirs-resolve
  when: facts.has_php
  kind: registry_paths_resolve
  source: composer.json
  extract: { json: "$.autoload['psr-4'].*" }
  expect: dir
  level: warning
  message: >-
    PHP: every composer.json `autoload.psr-4` directory must exist
    (Composer cannot build the autoloader otherwise).
  policy_url: "https://getcomposer.org/doc/04-schema.md#psr-4"
```

### `php-composer-autoload-files-resolve`

`autoload.files` are eagerly require()'d on every request; a missing one is a hard fatal.

- **kind**: [`registry_paths_resolve`](/docs/rules/cross-file/registry_paths_resolve/)
- **level**: `warning`
- **when**: `facts.has_php`
- **policy**: <https://getcomposer.org/doc/04-schema.md#files>

> PHP: every composer.json `autoload.files` entry must exist on disk (Composer fatals at autoload time otherwise).

```yaml
- id: php-composer-autoload-files-resolve
  when: facts.has_php
  kind: registry_paths_resolve
  source: composer.json
  extract: { json: "$.autoload.files[*]" }
  expect: file
  level: warning
  message: >-
    PHP: every composer.json `autoload.files` entry must exist on
    disk (Composer fatals at autoload time otherwise).
  policy_url: "https://getcomposer.org/doc/04-schema.md#files"
```

### `php-composer-autoload-dev-files-resolve`

The dev counterpart (test bootstraps / fixtures). Info-level because it only loads under `--dev`.

- **kind**: [`registry_paths_resolve`](/docs/rules/cross-file/registry_paths_resolve/)
- **level**: `info`
- **when**: `facts.has_php`
- **policy**: <https://getcomposer.org/doc/04-schema.md#files>

> PHP: every composer.json `autoload-dev.files` entry should exist on disk (it is require()'d under --dev).

```yaml
- id: php-composer-autoload-dev-files-resolve
  when: facts.has_php
  kind: registry_paths_resolve
  source: composer.json
  extract: { json: "$['autoload-dev'].files[*]" }
  expect: file
  level: info
  message: >-
    PHP: every composer.json `autoload-dev.files` entry should
    exist on disk (it is require()'d under --dev).
  policy_url: "https://getcomposer.org/doc/04-schema.md#files"
```

### `php-composer-bin-resolve`

Declared console binaries (`composer global require` symlinks them into the bin-dir); a missing target breaks the installed command.

- **kind**: [`registry_paths_resolve`](/docs/rules/cross-file/registry_paths_resolve/)
- **level**: `warning`
- **when**: `facts.has_php`
- **policy**: <https://getcomposer.org/doc/articles/vendor-binaries.md>

> PHP: every composer.json `bin` entry must exist on disk (Composer symlinks it as a vendor binary).

```yaml
- id: php-composer-bin-resolve
  when: facts.has_php
  kind: registry_paths_resolve
  source: composer.json
  extract: { json: "$.bin[*]" }
  expect: file
  level: warning
  message: >-
    PHP: every composer.json `bin` entry must exist on disk
    (Composer symlinks it as a vendor binary).
  policy_url: "https://getcomposer.org/doc/articles/vendor-binaries.md"
```

### `php-no-vendor-committed`

`vendor/` is Composer's install directory — generated, not source. The standard composer `.gitignore` excludes it. Disable on the rare repo that vendors its dependencies for deployment.

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `warning`
- **when**: `facts.has_php`
- **policy**: <https://getcomposer.org/doc/01-basic-usage.md#installing-dependencies>

> PHP: Composer's `vendor/` directory must not be committed; add it to .gitignore and run `composer install` to restore it.

```yaml
- id: php-no-vendor-committed
  when: facts.has_php
  kind: dir_absent
  paths: ["vendor", "**/vendor"]
  level: warning
  message: >-
    PHP: Composer's `vendor/` directory must not be committed;
    add it to .gitignore and run `composer install` to restore it.
  policy_url: "https://getcomposer.org/doc/01-basic-usage.md#installing-dependencies"
```

## Facts

The `when:` clauses above read these facts. Each is resolved once per run; [`alint facts`](/docs/cli/facts/) prints what they resolved to in your repository.

```yaml
facts:
  - id: has_php
    any_file_exists:
      - composer.json
      - "**/composer.json"
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/php@v1
rules:
  - id: php-composer-name-format
    level: off
  - id: php-composer-psr4-dirs-resolve
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/php@v1
    except: [php-composer-name-format]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/php.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/php.yml) in the alint repo.
