---
title: 'versioning@v1'
description: 'versioning@v1 bundled alint ruleset: One repository release version, declared in root VERSION and synchronized into common package, language, chart, API...'
---

One repository release version, declared in root VERSION and synchronized into
common package, language, chart, API, and changelog surfaces. Presence of a
valid root VERSION activates the policy; repositories without it are silent.
Target fields are optional. Unsafe sync_from fixes update existing values only.
Git tags remain the separate C6 local-Git gap.

## Adopt with

```yaml
extends:
  - alint://bundled/versioning@v1
```

## What it checks

11 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [versioning-version-file-one-line](#versioning-version-file-one-line)<br>`error` | `VERSION` must contain exactly one portable literal version token. |
| [versioning-package-json](#versioning-package-json)<br>`error` | package.json version differs from VERSION. |
| [versioning-cargo-package](#versioning-cargo-package)<br>`error` | Cargo package version differs from VERSION. |
| [versioning-cargo-workspace](#versioning-cargo-workspace)<br>`error` | Cargo workspace version differs from VERSION. |
| [versioning-pyproject-project](#versioning-pyproject-project)<br>`error` | pyproject.toml project.version differs from VERSION. |
| [versioning-pyproject-poetry](#versioning-pyproject-poetry)<br>`error` | Poetry package version differs from VERSION. |
| [versioning-python-module](#versioning-python-module)<br>`error` | Python \_\_version\_\_ differs from VERSION. |
| [versioning-helm-chart](#versioning-helm-chart)<br>`error` | Helm Chart appVersion differs from VERSION. |
| [versioning-openapi-json](#versioning-openapi-json)<br>`error` | OpenAPI info.version differs from VERSION. |
| [versioning-openapi-yaml](#versioning-openapi-yaml)<br>`error` | OpenAPI info.version differs from VERSION. |
| [versioning-changelog](#versioning-changelog)<br>`error` | The top released CHANGELOG entry differs from VERSION. |

One rule runs only when `facts.versioning_has_version_file` holds.

9 rules run only when `facts.versioning_has_valid_version` holds.

One rule runs only when `facts.versioning_has_valid_version and facts.versioning_has_changelog_file` holds.

## Rules

### `versioning-version-file-one-line`

Report a malformed source once. The ten relation rules run only after this exact-token gate passes, so bad source text does not fan out into ten errors.

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `error`
- **when**: `facts.versioning_has_version_file`

> `VERSION` must contain exactly one portable literal version token.

```yaml
- id: versioning-version-file-one-line
  when: facts.versioning_has_version_file
  kind: file_content_matches
  paths: VERSION
  pattern: '\A[ \t]*(?:[0-9][0-9A-Za-z._+!~-]{0,255}|[vV][0-9][0-9A-Za-z._+!~-]{0,254})[ \t]*(?:\r?\n)?\z'
  level: error
  message: "`VERSION` must contain exactly one portable literal version token."
```

### `versioning-package-json`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `error`
- **when**: `facts.versioning_has_valid_version`
- **fix**: `sync_from` (applied by `alint fix`)

> package.json version differs from VERSION.

```yaml
- id: versioning-package-json
  when: facts.versioning_has_valid_version
  kind: cross_file
  source:
    file: VERSION
    extract:
      regex: '\A[ \t]*((?:[0-9][0-9A-Za-z._+!~-]{0,255}|[vV][0-9][0-9A-Za-z._+!~-]{0,254}))[ \t]*(?:\r?\n)?\z'
  targets:
    files: "**/package.json"
    extract: { json: "$.version" }
  relation: equals
  allow_missing_target: true
  level: error
  message: "package.json version differs from VERSION."
  fix: { sync_from: {} }
```

### `versioning-cargo-package`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `error`
- **when**: `facts.versioning_has_valid_version`
- **fix**: `sync_from` (applied by `alint fix`)

> Cargo package version differs from VERSION.

```yaml
- id: versioning-cargo-package
  when: facts.versioning_has_valid_version
  kind: cross_file
  source:
    file: VERSION
    extract:
      regex: '\A[ \t]*((?:[0-9][0-9A-Za-z._+!~-]{0,255}|[vV][0-9][0-9A-Za-z._+!~-]{0,254}))[ \t]*(?:\r?\n)?\z'
  targets:
    files: "**/Cargo.toml"
    extract: { toml: "$.package.version" }
  relation: equals
  allow_missing_target: true
  level: error
  message: "Cargo package version differs from VERSION."
  fix: { sync_from: {} }
```

### `versioning-cargo-workspace`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `error`
- **when**: `facts.versioning_has_valid_version`
- **fix**: `sync_from` (applied by `alint fix`)

> Cargo workspace version differs from VERSION.

```yaml
- id: versioning-cargo-workspace
  when: facts.versioning_has_valid_version
  kind: cross_file
  source:
    file: VERSION
    extract:
      regex: '\A[ \t]*((?:[0-9][0-9A-Za-z._+!~-]{0,255}|[vV][0-9][0-9A-Za-z._+!~-]{0,254}))[ \t]*(?:\r?\n)?\z'
  targets:
    files: "**/Cargo.toml"
    extract: { toml: "$.workspace.package.version" }
  relation: equals
  allow_missing_target: true
  level: error
  message: "Cargo workspace version differs from VERSION."
  fix: { sync_from: {} }
```

### `versioning-pyproject-project`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `error`
- **when**: `facts.versioning_has_valid_version`
- **fix**: `sync_from` (applied by `alint fix`)

> pyproject.toml project.version differs from VERSION.

```yaml
- id: versioning-pyproject-project
  when: facts.versioning_has_valid_version
  kind: cross_file
  source:
    file: VERSION
    extract:
      regex: '\A[ \t]*((?:[0-9][0-9A-Za-z._+!~-]{0,255}|[vV][0-9][0-9A-Za-z._+!~-]{0,254}))[ \t]*(?:\r?\n)?\z'
  targets:
    files: "**/pyproject.toml"
    extract: { toml: "$.project.version" }
  relation: equals
  allow_missing_target: true
  level: error
  message: "pyproject.toml project.version differs from VERSION."
  fix: { sync_from: {} }
```

### `versioning-pyproject-poetry`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `error`
- **when**: `facts.versioning_has_valid_version`
- **fix**: `sync_from` (applied by `alint fix`)

> Poetry package version differs from VERSION.

```yaml
- id: versioning-pyproject-poetry
  when: facts.versioning_has_valid_version
  kind: cross_file
  source:
    file: VERSION
    extract:
      regex: '\A[ \t]*((?:[0-9][0-9A-Za-z._+!~-]{0,255}|[vV][0-9][0-9A-Za-z._+!~-]{0,254}))[ \t]*(?:\r?\n)?\z'
  targets:
    files: "**/pyproject.toml"
    extract: { toml: "$.tool.poetry.version" }
  relation: equals
  allow_missing_target: true
  level: error
  message: "Poetry package version differs from VERSION."
  fix: { sync_from: {} }
```

### `versioning-python-module`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `error`
- **when**: `facts.versioning_has_valid_version`
- **fix**: `sync_from` (applied by `alint fix`)

> Python \_\_version\_\_ differs from VERSION.

```yaml
- id: versioning-python-module
  when: facts.versioning_has_valid_version
  kind: cross_file
  source:
    file: VERSION
    extract:
      regex: '\A[ \t]*((?:[0-9][0-9A-Za-z._+!~-]{0,255}|[vV][0-9][0-9A-Za-z._+!~-]{0,254}))[ \t]*(?:\r?\n)?\z'
  targets:
    files: "**/{__init__,version,_version}.py"
    extract:
      regex: >-
        (?m)^__version__[ \t]*(?::[ \t]*[^=\r\n]+)?=[ \t]*["']([^"'\r\n]{1,256})["']
  relation: equals
  allow_missing_target: true
  level: error
  message: "Python __version__ differs from VERSION."
  fix: { sync_from: {} }
```

### `versioning-helm-chart`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `error`
- **when**: `facts.versioning_has_valid_version`
- **fix**: `sync_from` (applied by `alint fix`)

> Helm Chart appVersion differs from VERSION.

```yaml
- id: versioning-helm-chart
  when: facts.versioning_has_valid_version
  kind: cross_file
  source:
    file: VERSION
    extract:
      regex: '\A[ \t]*((?:[0-9][0-9A-Za-z._+!~-]{0,255}|[vV][0-9][0-9A-Za-z._+!~-]{0,254}))[ \t]*(?:\r?\n)?\z'
  targets:
    files: "**/Chart.yaml"
    extract: { yaml: "$.appVersion" }
  relation: equals
  allow_missing_target: true
  level: error
  message: "Helm Chart appVersion differs from VERSION."
  fix: { sync_from: {} }
```

### `versioning-openapi-json`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `error`
- **when**: `facts.versioning_has_valid_version`
- **fix**: `sync_from` (applied by `alint fix`)

> OpenAPI info.version differs from VERSION.

```yaml
- id: versioning-openapi-json
  when: facts.versioning_has_valid_version
  kind: cross_file
  source:
    file: VERSION
    extract:
      regex: '\A[ \t]*((?:[0-9][0-9A-Za-z._+!~-]{0,255}|[vV][0-9][0-9A-Za-z._+!~-]{0,254}))[ \t]*(?:\r?\n)?\z'
  targets:
    files: "**/{openapi,swagger}.json"
    extract: { json: "$.info.version" }
  relation: equals
  allow_missing_target: true
  level: error
  message: "OpenAPI info.version differs from VERSION."
  fix: { sync_from: {} }
```

### `versioning-openapi-yaml`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `error`
- **when**: `facts.versioning_has_valid_version`
- **fix**: `sync_from` (applied by `alint fix`)

> OpenAPI info.version differs from VERSION.

```yaml
- id: versioning-openapi-yaml
  when: facts.versioning_has_valid_version
  kind: cross_file
  source:
    file: VERSION
    extract:
      regex: '\A[ \t]*((?:[0-9][0-9A-Za-z._+!~-]{0,255}|[vV][0-9][0-9A-Za-z._+!~-]{0,254}))[ \t]*(?:\r?\n)?\z'
  targets:
    files: "**/{openapi,swagger}.{yml,yaml}"
    extract: { yaml: "$.info.version" }
  relation: equals
  allow_missing_target: true
  level: error
  message: "OpenAPI info.version differs from VERSION."
  fix: { sync_from: {} }
```

### `versioning-changelog`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `error`
- **when**: `facts.versioning_has_valid_version and facts.versioning_has_changelog_file`
- **fix**: `sync_from` (applied by `alint fix`)

> The top released CHANGELOG entry differs from VERSION.

```yaml
- id: versioning-changelog
  when: facts.versioning_has_valid_version and facts.versioning_has_changelog_file
  kind: cross_file
  source:
    file: VERSION
    extract:
      regex: '\A[ \t]*((?:[0-9][0-9A-Za-z._+!~-]{0,255}|[vV][0-9][0-9A-Za-z._+!~-]{0,254}))[ \t]*(?:\r?\n)?\z'
  targets:
    - file: CHANGELOG.md
      extract:
        regex: >-
          (?m)\A[^`~<]*?^##[ \t]+(?:Version[ \t]+)?(?:\[[ \t]*)?([vV]?[0-9][-0-9A-Za-z._+!~]{0,255})(?:[ \t]*\])?(?:[ \t]+|$)
  relation: equals
  allow_missing_target: false
  level: error
  message: "The top released CHANGELOG entry differs from VERSION."
  fix: { sync_from: {} }
```

## Facts

The `when:` clauses above read these facts. Each is resolved once per run; [`alint facts`](/docs/cli/facts/) prints what they resolved to in your repository.

```yaml
facts:
  - id: versioning_has_version_file
    any_file_exists: VERSION
  - id: versioning_has_valid_version
    file_content_matches:
      paths: VERSION
      pattern: '\A[ \t]*(?:[0-9][0-9A-Za-z._+!~-]{0,255}|[vV][0-9][0-9A-Za-z._+!~-]{0,254})[ \t]*(?:\r?\n)?\z'
  - id: versioning_has_changelog_file
    any_file_exists: CHANGELOG.md
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/versioning@v1
rules:
  - id: versioning-version-file-one-line
    level: off
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/versioning@v1
    except: [versioning-version-file-one-line]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/versioning.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/versioning.yml) in the alint repo.
