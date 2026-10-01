---
title: 'alint rules'
description: 'alint rules browses the catalog of rule kinds alint ships: list and search them, see the categories, or show one kind. It never reads a config.'
---

`alint rules` is the rule kind catalog on the command line. It never reads a
config, so it works in any directory, including before you write your first
`.alint.yml`.

## Examples

Every rule kind, with its categories:

```bash
alint rules list
```

Narrow the list:

```bash
alint rules list --category naming
alint rules list --search shebang
```

The categories and how many kinds each holds:

```bash
alint rules categories
```

One kind: summary, categories, aliases and docs link. An alias resolves to its
kind:

```bash
alint rules show content_forbidden
```

## Reference

```
Browse the catalog of rule kinds alint ships (config-independent).

Use `alint list` for the rules configured in THIS repo; `alint rules` never reads a config and works
anywhere.

Usage: alint rules [OPTIONS] <COMMAND>

Commands:
  list        List rule kinds in the catalog, optionally filtered. Reads no config
  categories  List the rule categories: slug, title, and how many kinds each holds
  show        Show one rule kind: its summary, categories, aliases, and docs link. Accepts an alias
              (resolves to the canonical kind)
  help        Print this message or the help of the given subcommand(s)
```

The [global options](/docs/cli/#global-options) apply to `alint rules` too, where they are relevant.

## See also

- [The rule catalog](/docs/rules/), browsable and filterable
- [Kinds, families and categories](/docs/concepts/start-here/kinds-families-categories/)
