---
title: 'alint list'
description: 'alint list prints the rules this repository enables after extends: resolution, with each rule''s level, kind and policy link.'
---

`alint list` answers "what does this repo actually enforce?": it resolves
`extends:` and prints every rule in the effective config. To browse the kinds
alint ships instead, use [`alint rules`](/docs/cli/rules/).

## Examples

Every enabled rule:

```bash
alint list
```

Only rules whose kind is in one category:

```bash
alint list --category naming
```

## Version 2 selectors and relations

For an [experimental version 2 config](/docs/configuration/version-2/), `alint list` also shows
each named selector and relation after the rules, with the config file that declares it. Then,
for each file that `extends:` aliases reach and that declares a selector or relation, one line
names the file and every alias that reaches it, with the config that declares the alias:

```text
selector base_docs  rules/base.yml
selector own  .alint.yml
aliases rules/base.yml  as base in .alint.yml
```

`--format json` adds a `sources` list (each file's `path` and its `aliases`, each an `alias` and
the index in `sources` of the config that declares it in `in`) and `selectors` and `relations`
lists whose entries give a `name` and the index of their `source`. A file chosen through an
`extends:` entry read from the environment, and every file it extends, is shown as
`<redacted>`.

## Reference

```
List the rules configured in this repo.

Reflects THIS repo's effective config (after `extends:` resolution). To browse the full catalog of
rule kinds alint ships instead, use `alint rules list`.

Usage: alint list [OPTIONS]

Options:
      --category <CATEGORY>
          Only rules whose kind is in this category (slug; see `alint rules categories`)
```

The [global options](/docs/cli/#global-options) apply to `alint list` too, where they are relevant.

## See also

- [Kinds, families and categories](/docs/concepts/start-here/kinds-families-categories/)
- [The config model](/docs/concepts/start-here/the-config-model/)
- [Version 2 (experimental)](/docs/configuration/version-2/)
