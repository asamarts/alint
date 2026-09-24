---
title: 'alint suggest'
description: 'alint suggest scans a repository for antipatterns and proposes rules and bundled rulesets that would catch them, without editing your config.'
---

`alint suggest` looks at what is actually in the tree and prints proposals for
you to review; it never edits `.alint.yml`. Run it after
[`alint init`](/docs/cli/init/), or on a repo that already has a config, to
find gaps.

## Examples

Proposals at medium confidence or higher (the default):

```bash
alint suggest
```

Only strong signals, each with the file-level evidence behind it:

```bash
alint suggest --confidence high --explain
```

## See also

- [Suggest](/docs/cookbook/suggest/): reading and acting on proposals
