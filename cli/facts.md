---
title: 'alint facts'
description: 'alint facts prints the resolved value of every facts: entry in the effective config, the quickest way to debug a when: clause.'
---

Rules can be gated on facts about the repository (for example "is there a
`Cargo.toml`?") through `when:` clauses, and bundled rulesets declare their
own facts. When a rule runs or skips unexpectedly, `alint facts` shows what
each fact resolved to.

## Examples

Resolved value of every fact in the effective config:

```bash
alint facts
```

Against another checkout:

```bash
alint facts path/to/repo
```

## Reference

```
Evaluate the config's `facts:` entries and print resolved values.

A debugging aid for `when:` clauses: prints the resolved value of every `facts:` entry in the
effective config.

Usage: alint facts [OPTIONS] [PATH]

Arguments:
  [PATH]
          Root of the repository to evaluate facts against

          [default: .]
```

The [global options](/docs/cli/#global-options) apply to `alint facts` too, where they are relevant.

## See also

- [Scoping](/docs/concepts/targeting/scoping/): `when:`, `paths:` and `scope_filter:`
