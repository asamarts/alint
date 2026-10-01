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
