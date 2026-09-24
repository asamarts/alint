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

## See also

- [The rule catalog](/docs/rules/), browsable and filterable
- [Kinds, families and categories](/docs/concepts/start-here/kinds-families-categories/)
