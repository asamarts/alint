---
title: 'alint explain'
description: 'alint explain shows how one configured rule is defined: its kind, level, paths, options, and a link to the rule kind reference.'
---

`alint explain` takes a rule id from your effective config (including rules
pulled in by `extends:`) and prints its definition. Use it when a finding
names a rule you didn't write yourself.

## Examples

Find the id in `alint list` or in a finding, then explain it:

```bash
alint explain oss-readme-exists
```

## See also

- [`alint list`](/docs/cli/list/): every rule id in the effective config
- [`alint rules show`](/docs/cli/rules/): a rule kind, independent of any config
