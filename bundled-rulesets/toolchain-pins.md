---
title: 'toolchain-pins@v1'
description: 'toolchain-pins@v1 bundled alint ruleset: Exact toolchain-version consistency for Node.js, Rust, Python, Ruby, JVM, and .NET repositories.'
---

Exact toolchain-version consistency for Node.js, Rust, Python, Ruby, JVM, and
.NET repositories. Each ecosystem has one explicit root authority; no source
is guessed from whichever optional pin happens to exist. Adopt it with:

```yaml
extends:
  - alint://bundled/toolchain-pins@v1
```

Exact literal pins in local declarations, Docker FROM instructions, and the
matching GitHub Actions setup step are compared. Ranges, moving selectors,
matrices, and computed values are outside this bundle; range semantics are C4.
Unsafe sync_from fixes copy the authority into drifted exact-pin fields.

## What it checks

24 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [toolchain-node-anchor-exact](#toolchain-node-anchor-exact)<br>`warning` | .nvmrc must contain one exact Node.js version, not a range or moving alias. |
| [toolchain-node-declarations-coherent](#toolchain-node-declarations-coherent)<br>`warning` | Node.js declaration differs from .nvmrc. |
| [toolchain-node-dockerfiles-coherent](#toolchain-node-dockerfiles-coherent)<br>`warning` | Node.js Docker base release differs from .nvmrc. |
| [toolchain-node-github-actions-coherent](#toolchain-node-github-actions-coherent)<br>`warning` | actions/setup-node version differs from .nvmrc. |
| [toolchain-rust-anchor-exact](#toolchain-rust-anchor-exact)<br>`warning` | rust-toolchain.toml must declare an exact release or dated nightly channel. |
| [toolchain-rust-declarations-coherent](#toolchain-rust-declarations-coherent)<br>`warning` | Rust toolchain declaration differs from rust-toolchain.toml. |
| [toolchain-rust-dockerfiles-coherent](#toolchain-rust-dockerfiles-coherent)<br>`warning` | Rust Docker base release differs from rust-toolchain.toml. |
| [toolchain-rust-github-actions-coherent](#toolchain-rust-github-actions-coherent)<br>`warning` | GitHub Actions Rust toolchain differs from rust-toolchain.toml. |
| [toolchain-python-anchor-exact](#toolchain-python-anchor-exact)<br>`warning` | .python-version must contain one exact Python version. |
| [toolchain-python-declarations-coherent](#toolchain-python-declarations-coherent)<br>`warning` | Python toolchain declaration differs from .python-version. |
| [toolchain-python-dockerfiles-coherent](#toolchain-python-dockerfiles-coherent)<br>`warning` | Python Docker base release differs from .python-version. |
| [toolchain-python-github-actions-coherent](#toolchain-python-github-actions-coherent)<br>`warning` | actions/setup-python version differs from .python-version. |
| [toolchain-ruby-anchor-exact](#toolchain-ruby-anchor-exact)<br>`warning` | .ruby-version must contain one exact Ruby version. |
| [toolchain-ruby-declarations-coherent](#toolchain-ruby-declarations-coherent)<br>`warning` | Ruby toolchain declaration differs from .ruby-version. |
| [toolchain-ruby-dockerfiles-coherent](#toolchain-ruby-dockerfiles-coherent)<br>`warning` | Ruby Docker base release differs from .ruby-version. |
| [toolchain-ruby-github-actions-coherent](#toolchain-ruby-github-actions-coherent)<br>`warning` | ruby/setup-ruby version differs from .ruby-version. |
| [toolchain-jvm-anchor-exact](#toolchain-jvm-anchor-exact)<br>`warning` | .java-version must contain one exact three-component JVM release. |
| [toolchain-jvm-declarations-coherent](#toolchain-jvm-declarations-coherent)<br>`warning` | JVM release declaration differs from .java-version. |
| [toolchain-jvm-dockerfiles-coherent](#toolchain-jvm-dockerfiles-coherent)<br>`warning` | JVM Docker base release differs from .java-version. |
| [toolchain-jvm-github-actions-coherent](#toolchain-jvm-github-actions-coherent)<br>`warning` | actions/setup-java release differs from .java-version. |
| [toolchain-dotnet-anchor-exact](#toolchain-dotnet-anchor-exact)<br>`warning` | global.json sdk.version must be an exact .NET SDK version. |
| [toolchain-dotnet-declarations-coherent](#toolchain-dotnet-declarations-coherent)<br>`warning` | .NET SDK declaration differs from global.json sdk.version. |
| [toolchain-dotnet-dockerfiles-coherent](#toolchain-dotnet-dockerfiles-coherent)<br>`warning` | .NET SDK Docker image differs from global.json sdk.version. |
| [toolchain-dotnet-github-actions-coherent](#toolchain-dotnet-github-actions-coherent)<br>`warning` | actions/setup-dotnet SDK version differs from global.json sdk.version. |

One rule runs only when `facts.toolchain_node_anchor_present` holds.

2 rules run only when `facts.toolchain_node_anchor_exact` holds.

One rule runs only when `facts.toolchain_node_anchor_exact and facts.toolchain_node_anchor_stable` holds.

One rule runs only when `facts.toolchain_rust_anchor_present` holds.

2 rules run only when `facts.toolchain_rust_anchor_exact` holds.

One rule runs only when `facts.toolchain_rust_anchor_exact and facts.toolchain_rust_anchor_stable` holds.

One rule runs only when `facts.toolchain_python_anchor_present` holds.

2 rules run only when `facts.toolchain_python_anchor_exact` holds.

One rule runs only when `facts.toolchain_python_anchor_exact and facts.toolchain_python_anchor_stable` holds.

One rule runs only when `facts.toolchain_ruby_anchor_present` holds.

2 rules run only when `facts.toolchain_ruby_anchor_exact` holds.

One rule runs only when `facts.toolchain_ruby_anchor_exact and facts.toolchain_ruby_anchor_stable` holds.

One rule runs only when `facts.toolchain_jvm_anchor_present` holds.

3 rules run only when `facts.toolchain_jvm_anchor_exact` holds.

One rule runs only when `facts.toolchain_dotnet_anchor_present` holds.

2 rules run only when `facts.toolchain_dotnet_anchor_exact` holds.

One rule runs only when `facts.toolchain_dotnet_anchor_exact and facts.toolchain_dotnet_anchor_stable` holds.

## Rules

### `toolchain-node-anchor-exact`

Node.js ----------------------------------------------------------

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `warning`
- **when**: `facts.toolchain_node_anchor_present`

> .nvmrc must contain one exact Node.js version, not a range or moving alias.

```yaml
- id: toolchain-node-anchor-exact
  when: facts.toolchain_node_anchor_present
  kind: file_content_matches
  paths: .nvmrc
  pattern: '\A[ \t]*v?[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?[ \t]*(?:\r?\n)?\z'
  level: warning
  message: ".nvmrc must contain one exact Node.js version, not a range or moving alias."
```

### `toolchain-node-declarations-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_node_anchor_exact`
- **fix**: `sync_from` (applied by `alint fix`)

> Node.js declaration differs from .nvmrc.

```yaml
- id: toolchain-node-declarations-coherent
  when: facts.toolchain_node_anchor_exact
  kind: cross_file
  source:
    file: .nvmrc
    extract:
      regex: '\A[ \t]*v?([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?)[ \t]*(?:\r?\n)?\z'
  targets:
    - file: .node-version
      extract:
        regex: '\A[ \t]*v?([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?)[ \t]*(?:\r?\n)?\z'
    - file: .tool-versions
      extract:
        regex: '(?m)^[ \t]*(?:node|nodejs)[ \t]+v?([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?)[ \t]*(?:#.*)?$'
    - file: package.json
      extract: { json: "$['engines','volta'].node" }
      matches: '^[0-9]+[.][0-9]+[.][0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:[+][0-9A-Za-z]+[0-9A-Za-z.-]*)?$'
  relation: equals
  allow_missing_target: true
  level: warning
  message: "Node.js declaration differs from .nvmrc."
  fix: { sync_from: {} }
```

### `toolchain-node-dockerfiles-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_node_anchor_exact and facts.toolchain_node_anchor_stable`
- **fix**: `sync_from` (applied by `alint fix`)

> Node.js Docker base release differs from .nvmrc.

```yaml
- id: toolchain-node-dockerfiles-coherent
  when: facts.toolchain_node_anchor_exact and facts.toolchain_node_anchor_stable
  kind: cross_file
  source: { file: .nvmrc, extract: { regex: '\A[ \t]*v?([0-9]+\.[0-9]+\.[0-9]+)[ \t]*(?:\r?\n)?\z' } }
  targets:
    files: "**/Dockerfile*"
    extract:
      regex: '(?mi)^[ \t]*FROM(?:[ \t]+--platform=\S+)?[ \t]+(?:[A-Za-z0-9._-]+(?::[0-9]+)?/)*(?:library/)?node:v?([0-9]+\.[0-9]+\.[0-9]+)(?:-[A-Za-z0-9._-]+)*(?:@sha256:[a-f0-9]{64})?(?:[ \t]+AS[ \t]+\S+)?[ \t]*$'
  relation: equals
  allow_missing_target: true
  level: warning
  message: "Node.js Docker base release differs from .nvmrc."
  fix: { sync_from: {} }
```

### `toolchain-node-github-actions-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_node_anchor_exact`
- **fix**: `sync_from` (applied by `alint fix`)

> actions/setup-node version differs from .nvmrc.

```yaml
- id: toolchain-node-github-actions-coherent
  when: facts.toolchain_node_anchor_exact
  kind: cross_file
  source: { file: .nvmrc, extract: { regex: '\A[ \t]*v?([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?)[ \t]*(?:\r?\n)?\z' } }
  targets:
    files: ".github/workflows/*.{yml,yaml}"
    extract:
      yaml: "$.jobs.*.steps[?match(@.uses, '^actions/setup-node@.+') && match(@.with['node-version'], '^[0-9]+[.][0-9]+[.][0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:[+][0-9A-Za-z]+[0-9A-Za-z.-]*)?$')].with['node-version']"
  relation: equals
  allow_missing_target: true
  level: warning
  message: "actions/setup-node version differs from .nvmrc."
  fix: { sync_from: {} }
```

### `toolchain-rust-anchor-exact`

Rust -------------------------------------------------------------

- **kind**: [`toml_path_matches`](/docs/rules/structured-query/toml_path_matches/)
- **level**: `warning`
- **when**: `facts.toolchain_rust_anchor_present`

> rust-toolchain.toml must declare an exact release or dated nightly channel.

```yaml
- id: toolchain-rust-anchor-exact
  when: facts.toolchain_rust_anchor_present
  kind: toml_path_matches
  paths: rust-toolchain.toml
  path: "$.toolchain.channel"
  matches: '^(?:[0-9]+[.][0-9]+[.][0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:[+][0-9A-Za-z]+[0-9A-Za-z.-]*)?|nightly-[0-9]{4}-[0-9]{2}-[0-9]{2})$'
  level: warning
  message: "rust-toolchain.toml must declare an exact release or dated nightly channel."
```

### `toolchain-rust-declarations-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_rust_anchor_exact`
- **fix**: `sync_from` (applied by `alint fix`)

> Rust toolchain declaration differs from rust-toolchain.toml.

```yaml
- id: toolchain-rust-declarations-coherent
  when: facts.toolchain_rust_anchor_exact
  kind: cross_file
  source:
    file: rust-toolchain.toml
    extract: { toml: "$.toolchain.channel" }
  targets:
    - file: rust-toolchain
      extract:
        regex: '(?m)(?:\A[ \t]*|^[ \t]*channel[ \t]*=[ \t]*["''])((?:[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?|nightly-[0-9]{4}-[0-9]{2}-[0-9]{2}))(?:["''][ \t]*(?:#.*)?$|[ \t]*(?:\r?\n)?\z)'
    - file: .tool-versions
      extract:
        regex: '(?m)^[ \t]*rust[ \t]+((?:[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?|nightly-[0-9]{4}-[0-9]{2}-[0-9]{2}))[ \t]*(?:#.*)?$'
  relation: equals
  allow_missing_target: true
  level: warning
  message: "Rust toolchain declaration differs from rust-toolchain.toml."
  fix: { sync_from: {} }
```

### `toolchain-rust-dockerfiles-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_rust_anchor_exact and facts.toolchain_rust_anchor_stable`
- **fix**: `sync_from` (applied by `alint fix`)

> Rust Docker base release differs from rust-toolchain.toml.

```yaml
- id: toolchain-rust-dockerfiles-coherent
  when: facts.toolchain_rust_anchor_exact and facts.toolchain_rust_anchor_stable
  kind: cross_file
  source:
    file: rust-toolchain.toml
    extract: { toml: "$.toolchain.channel" }
  targets:
    files: "**/Dockerfile*"
    extract:
      regex: '(?mi)^[ \t]*FROM(?:[ \t]+--platform=\S+)?[ \t]+(?:[A-Za-z0-9._-]+(?::[0-9]+)?/)*(?:library/)?rust:([0-9]+\.[0-9]+\.[0-9]+)(?:-[A-Za-z0-9._-]+)*(?:@sha256:[a-f0-9]{64})?(?:[ \t]+AS[ \t]+\S+)?[ \t]*$'
  relation: equals
  allow_missing_target: true
  level: warning
  message: "Rust Docker base release differs from rust-toolchain.toml."
  fix: { sync_from: {} }
```

### `toolchain-rust-github-actions-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_rust_anchor_exact`
- **fix**: `sync_from` (applied by `alint fix`)

> GitHub Actions Rust toolchain differs from rust-toolchain.toml.

```yaml
- id: toolchain-rust-github-actions-coherent
  when: facts.toolchain_rust_anchor_exact
  kind: cross_file
  source:
    file: rust-toolchain.toml
    extract: { toml: "$.toolchain.channel" }
  targets:
    files: ".github/workflows/*.{yml,yaml}"
    extract:
      yaml: "$.jobs.*.steps[?match(@.uses, '^(?:dtolnay/rust-toolchain|actions-rust-lang/setup-rust-toolchain|actions-rs/toolchain)@.+') && match(@.with.toolchain, '^(?:[0-9]+[.][0-9]+[.][0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:[+][0-9A-Za-z]+[0-9A-Za-z.-]*)?|nightly-[0-9]{4}-[0-9]{2}-[0-9]{2})$')].with.toolchain"
  relation: equals
  allow_missing_target: true
  level: warning
  message: "GitHub Actions Rust toolchain differs from rust-toolchain.toml."
  fix: { sync_from: {} }
```

### `toolchain-python-anchor-exact`

Python -----------------------------------------------------------

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `warning`
- **when**: `facts.toolchain_python_anchor_present`

> .python-version must contain one exact Python version.

```yaml
- id: toolchain-python-anchor-exact
  when: facts.toolchain_python_anchor_present
  kind: file_content_matches
  paths: .python-version
  pattern: '\A[ \t]*v?[0-9]+\.[0-9]+\.[0-9]+(?:(?:a|b|rc)[0-9]+)?(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?[ \t]*(?:\r?\n)?\z'
  level: warning
  message: ".python-version must contain one exact Python version."
```

### `toolchain-python-declarations-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_python_anchor_exact`
- **fix**: `sync_from` (applied by `alint fix`)

> Python toolchain declaration differs from .python-version.

```yaml
- id: toolchain-python-declarations-coherent
  when: facts.toolchain_python_anchor_exact
  kind: cross_file
  source: { file: .python-version, extract: { regex: '\A[ \t]*v?([0-9]+\.[0-9]+\.[0-9]+(?:(?:a|b|rc)[0-9]+)?(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?)[ \t]*(?:\r?\n)?\z' } }
  targets:
    - file: .tool-versions
      extract:
        regex: '(?m)^[ \t]*python[ \t]+v?([0-9]+\.[0-9]+\.[0-9]+(?:(?:a|b|rc)[0-9]+)?(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?)[ \t]*(?:#.*)?$'
    - file: runtime.txt
      extract:
        regex: '\A[ \t]*python-v?([0-9]+\.[0-9]+\.[0-9]+(?:(?:a|b|rc)[0-9]+)?(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?)[ \t]*(?:\r?\n)?\z'
    - file: Pipfile
      extract: { toml: "$.requires.python_full_version" }
      matches: '^[0-9]+[.][0-9]+[.][0-9]+(?:(?:a|b|rc)[0-9]+)?(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:[+][0-9A-Za-z]+[0-9A-Za-z.-]*)?$'
  relation: equals
  allow_missing_target: true
  level: warning
  message: "Python toolchain declaration differs from .python-version."
  fix: { sync_from: {} }
```

### `toolchain-python-dockerfiles-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_python_anchor_exact and facts.toolchain_python_anchor_stable`
- **fix**: `sync_from` (applied by `alint fix`)

> Python Docker base release differs from .python-version.

```yaml
- id: toolchain-python-dockerfiles-coherent
  when: facts.toolchain_python_anchor_exact and facts.toolchain_python_anchor_stable
  kind: cross_file
  source: { file: .python-version, extract: { regex: '\A[ \t]*v?([0-9]+\.[0-9]+\.[0-9]+)[ \t]*(?:\r?\n)?\z' } }
  targets:
    files: "**/Dockerfile*"
    extract:
      regex: '(?mi)^[ \t]*FROM(?:[ \t]+--platform=\S+)?[ \t]+(?:[A-Za-z0-9._-]+(?::[0-9]+)?/)*(?:library/)?python:([0-9]+\.[0-9]+\.[0-9]+)(?:-[A-Za-z0-9._-]+)*(?:@sha256:[a-f0-9]{64})?(?:[ \t]+AS[ \t]+\S+)?[ \t]*$'
  relation: equals
  allow_missing_target: true
  level: warning
  message: "Python Docker base release differs from .python-version."
  fix: { sync_from: {} }
```

### `toolchain-python-github-actions-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_python_anchor_exact`
- **fix**: `sync_from` (applied by `alint fix`)

> actions/setup-python version differs from .python-version.

```yaml
- id: toolchain-python-github-actions-coherent
  when: facts.toolchain_python_anchor_exact
  kind: cross_file
  source: { file: .python-version, extract: { regex: '\A[ \t]*v?([0-9]+\.[0-9]+\.[0-9]+(?:(?:a|b|rc)[0-9]+)?(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?)[ \t]*(?:\r?\n)?\z' } }
  targets:
    files: ".github/workflows/*.{yml,yaml}"
    extract:
      yaml: "$.jobs.*.steps[?match(@.uses, '^actions/setup-python@.+') && match(@.with['python-version'], '^[0-9]+[.][0-9]+[.][0-9]+(?:(?:a|b|rc)[0-9]+)?(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:[+][0-9A-Za-z]+[0-9A-Za-z.-]*)?$')].with['python-version']"
  relation: equals
  allow_missing_target: true
  level: warning
  message: "actions/setup-python version differs from .python-version."
  fix: { sync_from: {} }
```

### `toolchain-ruby-anchor-exact`

Ruby -------------------------------------------------------------

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `warning`
- **when**: `facts.toolchain_ruby_anchor_present`

> .ruby-version must contain one exact Ruby version.

```yaml
- id: toolchain-ruby-anchor-exact
  when: facts.toolchain_ruby_anchor_present
  kind: file_content_matches
  paths: .ruby-version
  pattern: '\A[ \t]*v?[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?[ \t]*(?:\r?\n)?\z'
  level: warning
  message: ".ruby-version must contain one exact Ruby version."
```

### `toolchain-ruby-declarations-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_ruby_anchor_exact`
- **fix**: `sync_from` (applied by `alint fix`)

> Ruby toolchain declaration differs from .ruby-version.

```yaml
- id: toolchain-ruby-declarations-coherent
  when: facts.toolchain_ruby_anchor_exact
  kind: cross_file
  source: { file: .ruby-version, extract: { regex: '\A[ \t]*v?([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?)[ \t]*(?:\r?\n)?\z' } }
  targets:
    - file: .tool-versions
      extract:
        regex: '(?m)^[ \t]*ruby[ \t]+v?([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?)[ \t]*(?:#.*)?$'
    - file: Gemfile
      extract:
        regex: '(?m)^ruby[ \t]*["'']v?([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?)["'']'
    - file: Gemfile.lock
      extract:
        regex: '(?m)^[ \t]+ruby[ \t]+v?([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?)(?:p[0-9]+)?[ \t]*$'
  relation: equals
  allow_missing_target: true
  level: warning
  message: "Ruby toolchain declaration differs from .ruby-version."
  fix: { sync_from: {} }
```

### `toolchain-ruby-dockerfiles-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_ruby_anchor_exact and facts.toolchain_ruby_anchor_stable`
- **fix**: `sync_from` (applied by `alint fix`)

> Ruby Docker base release differs from .ruby-version.

```yaml
- id: toolchain-ruby-dockerfiles-coherent
  when: facts.toolchain_ruby_anchor_exact and facts.toolchain_ruby_anchor_stable
  kind: cross_file
  source: { file: .ruby-version, extract: { regex: '\A[ \t]*v?([0-9]+\.[0-9]+\.[0-9]+)[ \t]*(?:\r?\n)?\z' } }
  targets:
    files: "**/Dockerfile*"
    extract:
      regex: '(?mi)^[ \t]*FROM(?:[ \t]+--platform=\S+)?[ \t]+(?:[A-Za-z0-9._-]+(?::[0-9]+)?/)*(?:library/)?ruby:([0-9]+\.[0-9]+\.[0-9]+)(?:-[A-Za-z0-9._-]+)*(?:@sha256:[a-f0-9]{64})?(?:[ \t]+AS[ \t]+\S+)?[ \t]*$'
  relation: equals
  allow_missing_target: true
  level: warning
  message: "Ruby Docker base release differs from .ruby-version."
  fix: { sync_from: {} }
```

### `toolchain-ruby-github-actions-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_ruby_anchor_exact`
- **fix**: `sync_from` (applied by `alint fix`)

> ruby/setup-ruby version differs from .ruby-version.

```yaml
- id: toolchain-ruby-github-actions-coherent
  when: facts.toolchain_ruby_anchor_exact
  kind: cross_file
  source: { file: .ruby-version, extract: { regex: '\A[ \t]*v?([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?)[ \t]*(?:\r?\n)?\z' } }
  targets:
    files: ".github/workflows/*.{yml,yaml}"
    extract:
      yaml: "$.jobs.*.steps[?match(@.uses, '^ruby/setup-ruby@.+') && match(@.with['ruby-version'], '^[0-9]+[.][0-9]+[.][0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:[+][0-9A-Za-z]+[0-9A-Za-z.-]*)?$')].with['ruby-version']"
  relation: equals
  allow_missing_target: true
  level: warning
  message: "ruby/setup-ruby version differs from .ruby-version."
  fix: { sync_from: {} }
```

### `toolchain-jvm-anchor-exact`

JVM --------------------------------------------------------------

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `warning`
- **when**: `facts.toolchain_jvm_anchor_present`

> .java-version must contain one exact three-component JVM release.

```yaml
- id: toolchain-jvm-anchor-exact
  when: facts.toolchain_jvm_anchor_present
  kind: file_content_matches
  paths: .java-version
  pattern: '\A[ \t]*(?:[A-Za-z][A-Za-z0-9_-]*-)?[1-9][0-9]*\.[0-9]+\.[0-9]+(?:[-+_][0-9A-Za-z._+-]+)?[ \t]*(?:\r?\n)?\z'
  level: warning
  message: ".java-version must contain one exact three-component JVM release."
```

### `toolchain-jvm-declarations-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_jvm_anchor_exact`
- **fix**: `sync_from` (applied by `alint fix`)

> JVM release declaration differs from .java-version.

```yaml
- id: toolchain-jvm-declarations-coherent
  when: facts.toolchain_jvm_anchor_exact
  kind: cross_file
  source: { file: .java-version, extract: { regex: '\A[ \t]*(?:[A-Za-z][A-Za-z0-9_-]*-)?([1-9][0-9]*\.[0-9]+\.[0-9]+)[^ \t\r\n]*[ \t]*(?:\r?\n)?\z' } }
  targets:
    - file: .tool-versions
      extract:
        regex: '(?m)^[ \t]*java[ \t]+(?:[A-Za-z][A-Za-z0-9_-]*-)?([1-9][0-9]*\.[0-9]+\.[0-9]+)[^ \t#\r\n]*[ \t]*(?:#.*)?$'
    - file: .sdkmanrc
      extract:
        regex: '(?m)^[ \t]*java[ \t]*=[ \t]*(?:[A-Za-z][A-Za-z0-9_-]*-)?([1-9][0-9]*\.[0-9]+\.[0-9]+)[^ \t#\r\n]*[ \t]*(?:#.*)?$'
  relation: equals
  allow_missing_target: true
  level: warning
  message: "JVM release declaration differs from .java-version."
  fix: { sync_from: {} }
```

### `toolchain-jvm-dockerfiles-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_jvm_anchor_exact`
- **fix**: `sync_from` (applied by `alint fix`)

> JVM Docker base release differs from .java-version.

```yaml
- id: toolchain-jvm-dockerfiles-coherent
  when: facts.toolchain_jvm_anchor_exact
  kind: cross_file
  source: { file: .java-version, extract: { regex: '\A[ \t]*(?:[A-Za-z][A-Za-z0-9_-]*-)?([1-9][0-9]*\.[0-9]+\.[0-9]+)[^ \t\r\n]*[ \t]*(?:\r?\n)?\z' } }
  targets:
    files: "**/Dockerfile*"
    extract:
      regex: '(?mi)^[ \t]*FROM(?:[ \t]+--platform=\S+)?[ \t]+(?:[A-Za-z0-9._-]+(?::[0-9]+)?/)*(?:eclipse-temurin|amazoncorretto|azul/zulu-openjdk|openjdk):([1-9][0-9]*\.[0-9]+\.[0-9]+)(?:[-_+][A-Za-z0-9._+-]+)*(?:@sha256:[a-f0-9]{64})?(?:[ \t]+AS[ \t]+\S+)?[ \t]*$'
  relation: equals
  allow_missing_target: true
  level: warning
  message: "JVM Docker base release differs from .java-version."
  fix: { sync_from: {} }
```

### `toolchain-jvm-github-actions-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_jvm_anchor_exact`
- **fix**: `sync_from` (applied by `alint fix`)

> actions/setup-java release differs from .java-version.

```yaml
- id: toolchain-jvm-github-actions-coherent
  when: facts.toolchain_jvm_anchor_exact
  kind: cross_file
  source: { file: .java-version, extract: { regex: '\A[ \t]*(?:[A-Za-z][A-Za-z0-9_-]*-)?([1-9][0-9]*\.[0-9]+\.[0-9]+)[^ \t\r\n]*[ \t]*(?:\r?\n)?\z' } }
  targets:
    files: ".github/workflows/*.{yml,yaml}"
    extract:
      yaml: "$.jobs.*.steps[?match(@.uses, '^actions/setup-java@.+') && match(@.with['java-version'], '^[1-9][0-9]*[.][0-9]+[.][0-9]+$')].with['java-version']"
  relation: equals
  allow_missing_target: true
  level: warning
  message: "actions/setup-java release differs from .java-version."
  fix: { sync_from: {} }
```

### `toolchain-dotnet-anchor-exact`

.NET -------------------------------------------------------------

- **kind**: [`json_path_matches`](/docs/rules/structured-query/json_path_matches/)
- **level**: `warning`
- **when**: `facts.toolchain_dotnet_anchor_present`

> global.json sdk.version must be an exact .NET SDK version.

```yaml
- id: toolchain-dotnet-anchor-exact
  when: facts.toolchain_dotnet_anchor_present
  kind: json_path_matches
  paths: global.json
  path: "$.sdk.version"
  matches: '^[0-9]+[.][0-9]+[.][0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:[+][0-9A-Za-z]+[0-9A-Za-z.-]*)?$'
  level: warning
  message: "global.json sdk.version must be an exact .NET SDK version."
```

### `toolchain-dotnet-declarations-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_dotnet_anchor_exact`
- **fix**: `sync_from` (applied by `alint fix`)

> .NET SDK declaration differs from global.json sdk.version.

```yaml
- id: toolchain-dotnet-declarations-coherent
  when: facts.toolchain_dotnet_anchor_exact
  kind: cross_file
  source:
    file: global.json
    extract: { json: "$.sdk.version" }
  targets:
    - file: .tool-versions
      extract:
        regex: '(?m)^[ \t]*(?:dotnet|dotnet-core)[ \t]+v?([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?)[ \t]*(?:#.*)?$'
    - file: global.json
      extract: { json: "$.tools.dotnet" }
      matches: '^[0-9]+[.][0-9]+[.][0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:[+][0-9A-Za-z]+[0-9A-Za-z.-]*)?$'
  relation: equals
  allow_missing_target: true
  level: warning
  message: ".NET SDK declaration differs from global.json sdk.version."
  fix: { sync_from: {} }
```

### `toolchain-dotnet-dockerfiles-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_dotnet_anchor_exact and facts.toolchain_dotnet_anchor_stable`
- **fix**: `sync_from` (applied by `alint fix`)

> .NET SDK Docker image differs from global.json sdk.version.

```yaml
- id: toolchain-dotnet-dockerfiles-coherent
  when: facts.toolchain_dotnet_anchor_exact and facts.toolchain_dotnet_anchor_stable
  kind: cross_file
  source:
    file: global.json
    extract: { json: "$.sdk.version" }
  targets:
    files: "**/Dockerfile*"
    extract:
      regex: '(?mi)^[ \t]*FROM(?:[ \t]+--platform=\S+)?[ \t]+mcr\.microsoft\.com/dotnet/sdk:([0-9]+\.[0-9]+\.[0-9]+)(?:-[A-Za-z0-9._-]+)*(?:@sha256:[a-f0-9]{64})?(?:[ \t]+AS[ \t]+\S+)?[ \t]*$'
  relation: equals
  allow_missing_target: true
  level: warning
  message: ".NET SDK Docker image differs from global.json sdk.version."
  fix: { sync_from: {} }
```

### `toolchain-dotnet-github-actions-coherent`

- **kind**: [`cross_file`](/docs/rules/cross-file/cross_file/)
- **level**: `warning`
- **when**: `facts.toolchain_dotnet_anchor_exact`
- **fix**: `sync_from` (applied by `alint fix`)

> actions/setup-dotnet SDK version differs from global.json sdk.version.

```yaml
- id: toolchain-dotnet-github-actions-coherent
  when: facts.toolchain_dotnet_anchor_exact
  kind: cross_file
  source:
    file: global.json
    extract: { json: "$.sdk.version" }
  targets:
    files: ".github/workflows/*.{yml,yaml}"
    extract:
      yaml: "$.jobs.*.steps[?match(@.uses, '^actions/setup-dotnet@.+') && match(@.with['dotnet-version'], '^[0-9]+[.][0-9]+[.][0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:[+][0-9A-Za-z]+[0-9A-Za-z.-]*)?$')].with['dotnet-version']"
  relation: equals
  allow_missing_target: true
  level: warning
  message: "actions/setup-dotnet SDK version differs from global.json sdk.version."
  fix: { sync_from: {} }
```

## Facts

The `when:` clauses above read these facts. Each is resolved once per run; [`alint facts`](/docs/cli/facts/) prints what they resolved to in your repository.

```yaml
facts:
  # Node.js: .nvmrc is the explicit authority.
  - id: toolchain_node_anchor_present
    any_file_exists: .nvmrc
  - id: toolchain_node_anchor_exact
    file_content_matches:
      paths: .nvmrc
      pattern: '\A[ \t]*v?[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?[ \t]*(?:\r?\n)?\z'
  - id: toolchain_node_anchor_stable
    file_content_matches:
      paths: .nvmrc
      pattern: '\A[ \t]*v?[0-9]+\.[0-9]+\.[0-9]+[ \t]*(?:\r?\n)?\z'

  # Rust: rust-toolchain.toml [toolchain].channel is the authority.
  - id: toolchain_rust_anchor_present
    any_file_exists: rust-toolchain.toml
  - id: toolchain_rust_anchor_exact
    file_content_matches:
      paths: rust-toolchain.toml
      extract: { toml: "$.toolchain.channel" }
      pattern: '\A(?:[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?|nightly-[0-9]{4}-[0-9]{2}-[0-9]{2})\z'
  - id: toolchain_rust_anchor_stable
    file_content_matches:
      paths: rust-toolchain.toml
      extract: { toml: "$.toolchain.channel" }
      pattern: '\A[0-9]+\.[0-9]+\.[0-9]+\z'

  # Python: .python-version is the authority.
  - id: toolchain_python_anchor_present
    any_file_exists: .python-version
  - id: toolchain_python_anchor_exact
    file_content_matches:
      paths: .python-version
      pattern: '\A[ \t]*v?[0-9]+\.[0-9]+\.[0-9]+(?:(?:a|b|rc)[0-9]+)?(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?[ \t]*(?:\r?\n)?\z'
  - id: toolchain_python_anchor_stable
    file_content_matches:
      paths: .python-version
      pattern: '\A[ \t]*v?[0-9]+\.[0-9]+\.[0-9]+[ \t]*(?:\r?\n)?\z'

  # Ruby: .ruby-version is the authority.
  - id: toolchain_ruby_anchor_present
    any_file_exists: .ruby-version
  - id: toolchain_ruby_anchor_exact
    file_content_matches:
      paths: .ruby-version
      pattern: '\A[ \t]*v?[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?[ \t]*(?:\r?\n)?\z'
  - id: toolchain_ruby_anchor_stable
    file_content_matches:
      paths: .ruby-version
      pattern: '\A[ \t]*v?[0-9]+\.[0-9]+\.[0-9]+[ \t]*(?:\r?\n)?\z'

  # JVM: .java-version is the authority. Cross-vendor surfaces compare the
  # feature/interim/update release core, not vendor or build identity.
  - id: toolchain_jvm_anchor_present
    any_file_exists: .java-version
  - id: toolchain_jvm_anchor_exact
    file_content_matches:
      paths: .java-version
      pattern: '\A[ \t]*(?:[A-Za-z][A-Za-z0-9_-]*-)?[1-9][0-9]*\.[0-9]+\.[0-9]+(?:[-+_][0-9A-Za-z._+-]+)?[ \t]*(?:\r?\n)?\z'

  # .NET: global.json sdk.version is the authority.
  - id: toolchain_dotnet_anchor_present
    any_file_exists: global.json
  - id: toolchain_dotnet_anchor_exact
    file_content_matches:
      paths: global.json
      extract: { json: "$.sdk.version" }
      pattern: '\A[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+[0-9A-Za-z.-]*)?(?:\+[0-9A-Za-z]+[0-9A-Za-z.-]*)?\z'
  - id: toolchain_dotnet_anchor_stable
    file_content_matches:
      paths: global.json
      extract: { json: "$.sdk.version" }
      pattern: '\A[0-9]+\.[0-9]+\.[0-9]+\z'
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/toolchain-pins@v1
rules:
  - id: toolchain-node-anchor-exact
    level: off
  - id: toolchain-node-declarations-coherent
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/toolchain-pins@v1
    except: [toolchain-node-anchor-exact]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/toolchain-pins.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/toolchain-pins.yml) in the alint repo.
