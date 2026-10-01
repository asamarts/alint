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

## Reference

```
Scan for antipatterns and propose rules that would catch them.

Prints proposals to stdout for review - never edits the user's config. Pairs naturally with `alint
init` for a smarter cold-start adoption flow.

Usage: alint suggest [OPTIONS] [PATH]

Arguments:
  [PATH]
          Root of the repository to scan. Defaults to the current directory

          [default: .]

Options:
  -f, --format <FORMAT>
          Output format. `human` (default) is colorised for terminals; `yaml` is a paste-ready
          config snippet; `json` is a stable shape suitable for agent consumption

          [default: human]
          [possible values: human, yaml, json]

      --confidence <LEVEL>
          Lower bound on signal strength for proposals. `low` is broadest (helpful when
          prospecting); `high` is strict (only ecosystem-marker hits and equivalents)

          [default: medium]
          [possible values: low, medium, high]

      --include-bundled
          Include bundled-ruleset suggestions even if the existing `.alint.yml` already extends them

      --explain
          Print one-line file-level evidence under each proposal so reviewers can decide quickly
```

The [global options](/docs/cli/#global-options) apply to `alint suggest` too, where they are relevant. Its own `--format` above replaces the global one.

## See also

- [Suggest](/docs/cookbook/suggest/): reading and acting on proposals
