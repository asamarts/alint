---
title: 'Version 2 (experimental)'
description: 'Named selectors and relations in an experimental version 2 config, and their limits.'
sidebar:
  order: 10
---

A config whose top-level `version:` is `2` can name the files a policy reads (**selectors**) and
the rows of data it reads from them (**relations**), and refer to both by name, also from another
config file. Version 2 is everything version 1 is, plus `selectors:`, `relations:` and an `as:`
alias on an `extends:` entry.

Nothing reads a relation yet. The `constraint` rule kind, which will check a relation's rows (one
range per dependency, the same keys in every locale, toolchain pins that agree), comes in a later
phase. Until it lands, a version 2 config runs its rules exactly as version 1 would: its selectors
and relations are loaded and checked when the config loads, and do nothing else.

> **Experimental.** Version 2 may change in any release until one declares it stable, v0.24 at
> the earliest, and the [CHANGELOG](https://github.com/asamarts/alint/blob/main/CHANGELOG.md)
> lists each change. Keep production policies on `version: 1` until then. Loading a version 2
> config prints a warning once per run, naming the file:
>
> ```text
> alint: warning: `version: 2` is experimental; its syntax may change before a release declares it stable, v0.24 at the earliest (/repo/.alint.yml)
> ```
>
> `alint validate-config` names the version on its success line and, with `--format json`, puts
> the warning in `warnings` and the version in `config_version`. The language server loads a
> version 2 config without the warning.

## The version boundary

alint reads a config's `version:` before anything else in it and decodes the rest as that
version.

- Only the plain integer `2` selects version 2. A 2 written any other way (quoted as `'2'`,
  tagged, through a YAML alias, or as `+2`, `0x2`, `0o2` or `2.0`) is an error that says to write
  `version: 2`. A version read from the environment is version 1 and must equal 1, as before.
- A version 2 config accepts every version 1 key and rule kind with its version 1 meaning. A
  rule's `paths:` keeps its version 1 form; only relations read selectors.
- `selectors:`, `relations:` and `as:` in a version 1 config are errors that name the version
  they need (`` `selectors:` needs `version: 2`; this config is version 1 ``). An unknown key in
  a version 2 config is reported with the version 2 keys.

Only local files may be version 2: the top-level config, a local `extends:` target and an
`.alint.d/` fragment. A fragment without `version:` is read as its root's version. A remote
`extends:` source, a bundled ruleset, a nested config (`nested_configs:`) and the string
`alint_dsl::parse` reads must be `version: 1`, and a version 1 config cannot extend a version 2
one; each is an error that says so.

## Named selectors

A selector names a set of files. It takes the `paths:` and `scope_filter:` a rule takes and picks
files as a rule's scope does, inside the repository and never in `.git`.

```yaml
version: 2
selectors:
  manifests:
    paths: ["packages/*/package.json", "!packages/legacy/package.json"]
  crate-sources:
    paths: { include: "crates/*/src/**/*.rs", exclude: "**/generated/**" }
    scope_filter: { has_ancestor: Cargo.toml }
rules: []
```

| Key | Value |
|---|---|
| `paths` | required: a glob, a list of globs with `!` excludes, or `{ include, exclude }` |
| `scope_filter` | optional: `has_ancestor`, `include_manifest_paths` or `exclude_manifest_paths` |

A selector cannot use `changed_since`: the rules that will read selectors judge the whole
repository, under `--changed` too, so what a selector picks must not depend on the diff. A
manifest `source:` stays inside the repository, as a rule's does.

Wherever a relation names a selector (`from:`), it may write one inline instead, as a map:
`from: { paths: package.json }`. A string there is always a selector's name, never a glob.

## Relations

A relation reads the files a selector picks and emits rows with named fields. It has `fields:`
and exactly one of `entries:`, `read:`, `union:` and `join:`. `transform:` and `where:` are
optional.

### `entries:` rows

`entries:` emits one row per member of each object an `at:` path selects in a JSON, YAML or TOML
file, or one per leaf below it with `leaves: true` (a key with no key below it; an array is a
value, so its elements are never keys). `at:` is a JSONPath or a list of them, read in order.
A file that lacks an `at:` path gives no rows from that path.

```yaml
version: 2
selectors:
  manifests:
    paths: "packages/*/package.json"
relations:
  dependency-ranges:
    from: manifests
    entries:
      format: json
      at: ["$.dependencies", "$.devDependencies"]
    fields:
      manifest: path
      field: entry.parent
      dependency: entry.key
      range: entry.value
rules: []
```

| Key | Value | Default |
|---|---|---|
| `format` | `json`, `yaml` or `toml` | required |
| `at` | a JSONPath, or a non-empty list of them | required |
| `leaves` | `true` or `false` | `false` |

### `read:` rows

`read:` extracts values as `cross_file` does, one row per value. It takes exactly one of:

- `json:`, `yaml:` or `toml:`, a JSONPath;
- `regex:`, a regex whose first capture group is the value. It runs over the whole file, so `^`
  is the file's start unless the regex sets `(?m)`;
- `lines: {}` or `lines: { comment: "//" }`, one value per non-blank line, trimmed, skipping
  lines that start with the comment prefix (`#` by default);
- `whole_file: {}`, the file's content as one value.

A `read:` row's value is a string. A file where the read matches nothing gives one row without a
value, unless the relation sets `optional: true`, which gives none. As in `cross_file`, a value
written as `${VAR}` is skipped with a note, and a match that is not a string (a table, a number)
gives one note for the file naming its kinds, while the file's strings are still rows. A join, or
a `where:` list that reads the relation, keeps these notes. `read:` cannot take `xml`,
`ini`, `hcl`, `dotenv` or `properties` in this build: dotenv and properties values stay redacted,
and the others have no value rows yet.

```yaml
version: 2
relations:
  package-names:
    from: { paths: "packages/*/package.json" }
    read: { json: "$.name" }
    fields: { name: value, dir: dir }
  node-version:
    from: { paths: .nvmrc }
    read: { regex: "^v?(\\S+)" }
    optional: true
    fields: { version: value }
rules: []
```

### `union:` rows

`union:` emits the rows of two or more parts, in the order written. Each part has its own `from:`,
its own `entries:` or `read:`, and its own `optional:`; the relation's `fields:` apply to every
part, so each part must be able to bind every field.

### `join:` rows

`join:` combines the rows of two relations whose `on:` fields are equal: `on:` maps a field of
`left:` to a field of `right:`. The join is inner: a row without a match on the other side is
dropped. `unique:` is required and names the side, `left` or `right`, that holds at most one row
per join key; a repeated key there is an analysis failure naming both rows. Output rows follow
the order of the other side. `fields:` names every output field as `left.<field>` or
`right.<field>`; nothing is copied implicitly. A join may read another join.

```yaml
version: 2
selectors:
  manifests:
    paths: "packages/*/package.json"
relations:
  dependency-ranges:
    from: manifests
    entries: { format: json, at: "$.dependencies" }
    fields: { manifest: path, dependency: entry.key, range: entry.value }
  workspace-versions:
    from: manifests
    read: { json: "$.version" }
    fields: { name: "bound: $.name", version: value }
  ranges-of-workspace-packages:
    join:
      left: dependency-ranges
      right: workspace-versions
      on: { dependency: name }
      unique: right
    fields:
      manifest: left.manifest
      dependency: left.dependency
      range: left.range
      version: right.version
rules: []
```

### Fields

`fields:` maps each field's name to its source, or to `{ source, sensitive }`:

| Source | Rows | Value |
|---|---|---|
| `path` | any but a join | the file's path |
| `dir`, `basename`, `stem`, `ext`, `parent_name` | any but a join | a part of the file's path |
| `entry.key` | `entries:` | the entry's own key |
| `entry.value` | `entries:` | its value, a scalar |
| `entry.parent` | `entries:` | the key of the object that holds it (null at the top level) |
| `entry.root` | `entries:` | the first key of its key path (`en` for `en.nav.home`) |
| `entry.path` | `entries:` | its key path from the top (`nav.home`) |
| `value` | `read:` | the extracted value |
| `bound: <JSONPath>` | `entries:`, a document `read:` | a value read once per file (`$.name`) |
| `left.<field>`, `right.<field>` | `join:` | a field of the matched row on that side |

A document `read:` is one with `json:`, `yaml:` or `toml:`; one with `regex:`, `lines:` or
`whole_file:` reads the file as text and binds no `bound:` field. In a `union:`, a part binds
`entry.*` only with `entries:`, `value` only with `read:`, and `bound:` as a relation of its one
part would. A `bound:` path must match exactly once in each file. `sensitive: true` keeps a
field's values out of messages, the JSON report and baselines.

### `transform:`

`transform:` maps a field to a list of transforms, applied in order before `where:`:

- `trim` and `lower`, on strings. `lower` trims too, and lowercases character by character, which
  is not Unicode case folding (`STRASSE` and `straße` stay different);
- `semver-major` and `semver-minor`, which keep the major, or the major and minor, part of a
  version (`^18.2.0` becomes `18` or `18.2`), as `cross_file`'s `normalize` does. They do not
  apply to a path, a path token or an `entry.path`, which name where a value is, nor to a join's
  copy of one;
- `strip-root`, which drops the first segment of an `entry.path` field (`en.nav.home` becomes
  `nav.home`), or of a join's copy of one that no transform has changed. It comes first in its
  list, and each file must have one top-level key.

### `where:`

`where:` maps a field to predicates that every row must meet:

- `in` and `not_in`: a list whose items are literals or `{ relation, field }` references, the
  values another relation's field holds;
- `matches` and `not_matches`: a regex, on strings.

A literal in `in` or `not_in` is a string or, for a field that may hold one, a boolean. Null and
numbers are refused: null is in no set, and a number compares by the text it is written with
(`1.10` is not `1.1`), which the config's YAML does not keep.

Every predicate tests every row, so the order they are written in does not matter. A row that any
predicate is false for is dropped. A row that none is false for, but that one cannot test (a
pattern on a number, a list of another type), is an analysis failure.

`in`, `not_in` and a join's `on:` compare the exact text of the values after their transforms.
Nothing normalizes Unicode, so an `é` written as one code point is not one written as `e` and a
combining accent, and only `lower` changes case.

```yaml
version: 2
selectors:
  manifests:
    paths: "packages/*/package.json"
relations:
  workspace-names:
    from: manifests
    read: { json: "$.name" }
    fields: { name: value }
  external-dependencies:
    from: manifests
    entries: { format: json, at: ["$.dependencies", "$.devDependencies"] }
    fields: { dependency: entry.key, range: entry.value }
    transform: { range: [trim] }
    where:
      range: { not_matches: "^(workspace|file|link):" }
      dependency: { not_in: ["@types/node", { relation: workspace-names, field: name }] }
rules: []
```

## Names, aliases and namespaces

A name, an alias and a field name start with an ASCII letter or `_` and hold only ASCII letters,
digits, `_` and `-`. A config file declares each name once, across its selectors, relations and
aliases. Names, aliases and references are literal: `{{...}}` in one is an error, since it must
name the same thing in every environment. Other values interpolate `{{env.X}}` as in version 1,
and an error that could quote a value read from the environment is redacted to the selector or
relation it is about, as a version 1 rule's is.

Each config file has its own namespace:

- A bare name (`from:`, a join's `left:` and `right:`, a `{ relation, field }` reference) names an
  object the same file declares.
- `alias.name` names an object declared by the file an `extends:` entry gives `as: alias`. It
  reaches only what that file declares itself: there is no re-export and no `a.b.c` chain.
- Nothing shadows: an extended file's names never mix with the extending file's.
- An `.alint.d/` fragment has a namespace of its own too, so it may declare a name the top-level
  config also declares.

`as:` takes a local `extends:` entry only; bundled and remote sources are version 1 and declare
nothing to reach.

Here `policies/deps.yml` declares a selector and a relation, the top-level config reaches them
through the alias `deps`, and an `.alint.d/` fragment declares a `manifests` selector of its own:

```yaml
# file: policies/deps.yml
version: 2
selectors:
  manifests:
    paths: "packages/*/package.json"
relations:
  ranges:
    from: manifests
    entries: { format: json, at: "$.dependencies" }
    fields: { dependency: entry.key, range: entry.value }
rules: []
```

```yaml
version: 2
extends:
  - url: ./policies/deps.yml
    as: deps
relations:
  workspace-versions:
    from: deps.manifests
    read: { json: "$.version" }
    fields: { name: "bound: $.name", version: value }
  ranges-of-workspace-packages:
    join: { left: deps.ranges, right: workspace-versions, on: { dependency: name }, unique: right }
    fields: { dependency: left.dependency, range: left.range, version: right.version }
rules: []
```

```yaml
# file: .alint.d/10-docs.yml
# No `version:`: read as the top-level config's version. Its namespace is its own.
selectors:
  manifests:
    paths: "docs/**/*.md"
rules:
  - id: docs-have-an-index
    kind: file_exists
    paths: docs/index.md
    level: warning
```

A name or alias that does not resolve is an error naming the file and the key, with the names the
file can reach or the nearest one:

```text
.alint.yml: `relations.titles.from`: no selector named `doc` in this config; the selectors it can reach are `docs`
```

`alint list` shows each named selector and relation after the rules, with the file that declares
it and the aliases that reach that file; `alint export-agents-md` lists them the same way.

## What the loader checks

The whole relation plan is checked when the config loads, before any file is read:

- every key, value type, field source, transform and predicate;
- every name and alias resolves, and relations that read each other are refused, naming the cycle
  (`` relation `a` reads itself: `a` -> `b` -> `a` ``);
- every field a join's `on:`, a `left.<field>` or `right.<field>`, a `transform:` or a `where:`
  names exists, a relation's rows bind each of its fields (no `bound:` field on a text `read:`),
  and every part of a `union:` binds every field;
- `strip-root` comes first on an `entry.path` field or a join's copy of one, and the version
  transforms are not applied to a path, a path token or an `entry.path`;
- each regex compiles, a `read:` regex has a capture group, and each JSONPath parses;
- the literals in `in` and `not_in` can match.

When the `constraint` kind reads a relation, what a relation cannot hold (an `entry.value` that
is an object, an array or a TOML date, a `bound:` path that does not match exactly once, a
repeated key on a join's `unique:` side, a comparison across types) is an analysis failure naming
the relation, the file and the key path, never a dropped row. So is reaching a row, join or
distinct-value limit, which is never truncation.

## Limits in this release

- Nothing reads a relation until the `constraint` kind lands, and a rule's `paths:` cannot name a
  selector.
- Only local files may be version 2 (see [the version boundary](#the-version-boundary)).
- A selector cannot use `changed_since`.
- Values are scalars: null, booleans, numbers (compared by the text they are written with) and
  strings. An `entry.value` that is an object, an array or a TOML date is an analysis failure,
  and it fails before `where:` runs, so no predicate can drop its row: narrow `at:` to the
  objects of scalars, or read the leaves with `leaves: true`.
- `entries:` reads JSON, YAML and TOML. `read:` reads JSON, YAML and TOML values, a regex, lines
  or a whole file; XML, INI, HCL, dotenv and properties are not readable.
- A join is inner, and one side must be unique on the join key: there is no many-to-many join.
- The transforms are the five listed; there are no user-defined ones.
- `in` and `not_in` cannot list null or a number.
- Reading is bounded by alint's row, join and distinct-value limits; reaching one is an analysis
  failure, never truncation.

## Trust

Version 2 is local only in this release: every file that declares a selector or a relation is the
top-level config, a local `extends:` target or an `.alint.d/` fragment. Each selector and relation
passes the trust gates of the file that declares it, with what it references, as an extended
file's rules do. A reference reaches only the same file or a file it extends, never a file that
extends it.

## The three policies

These are the relation halves of the three policies the `constraint` kind is designed against.
Each loads today; the rules that check them come with the `constraint` kind.

**One range per dependency** across a workspace, leaving out workspace packages and specifiers
that are not ranges:

```yaml
version: 2
selectors:
  workspace-manifests:
    paths: ["packages/*/package.json", "!packages/docusaurus/package.json"]
relations:
  workspace-names:
    from: workspace-manifests
    read: { json: "$.name" }
    fields: { name: value }
  dependency-ranges:
    from: workspace-manifests
    entries:
      format: json
      at: ["$.dependencies", "$.devDependencies", "$.optionalDependencies"]
    fields: { field: entry.parent, dependency: entry.key, range: entry.value }
    where:
      range: { not_matches: "^(catalog|npm|workspace):" }
      dependency: { not_in: ["@types/node", { relation: workspace-names, field: name }] }
rules: []
```

**The same keys in every locale**, over each locale's merged tree:

```yaml
version: 2
selectors:
  locale-files: { paths: "config/locales/**/*.yml" }
relations:
  locale-keys:
    from: locale-files
    entries: { format: yaml, at: "$", leaves: true }
    fields: { locale: entry.root, key: entry.path }
    transform: { key: [strip-root] }
    where: { key: { not_matches: "^(date|devise|views)\\." } }
rules: []
```

**Toolchain pins that agree**: one row per place a Node version is pinned. A part whose path a
file lacks gives a row without a value, which will differ from `.nvmrc`'s.

```yaml
version: 2
relations:
  node-pins:
    union:
      - { from: { paths: .nvmrc }, read: { regex: "^v?(\\S+)" } }
      - { from: { paths: package.json }, read: { json: "$.engines.node" } }
      - { from: { paths: package.json }, read: { json: "$.devEngines.runtime.version" } }
      - { from: { paths: package.json }, read: { json: "$.volta.node" } }
    fields: { source: path, version: value }
rules: []
```

## Editor support: the draft schema

A draft JSON Schema for version 2 ships at
[`schemas/v2/config.json`](https://github.com/asamarts/alint/blob/main/schemas/v2/config.json).
Its `$id` names the release, and it may change with version 2. Point an editor at it with a
modeline on the first line of a version 2 file:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/asamarts/alint/v0.23.0/schemas/v2/config.json
version: 2
selectors:
  docs: { paths: "docs/**/*.md" }
rules: []
```

Nothing associates a file name with either schema: an editor checks a config against the
schema its modeline names, as for the [version 1 schema](/docs/configuration/). A version 2
file needs the version 2 modeline, and so does a version 2 `.alint.d/` fragment without
`version:`; the schema then asks for `version: 2`, as the version 1 schema asks for
`version: 1`.

The draft checks what the loader checks of a single file: every key, value type, name,
reference, field source, transform, format, `read:` form and count. It cannot check:

- what needs every file loaded: names and aliases that resolve, re-exports, cycles, and the plan
  checks above (fields that exist, union parts that bind every field, transforms that apply, the
  literals in `in` and `not_in`);
- a name declared twice across selectors, relations and aliases;
- regexes, a `read:` regex's capture group and JSONPaths;
- which files may be version 2, and that a version 1 config does not extend a version 2 one;
- YAML tags, and a key YAML reads as a boolean or null (`true:`), which an editor reads as text;
- that `only:` and `except:` are not both set on one `extends:` entry, as in version 1.
