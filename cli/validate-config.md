---
title: 'alint validate-config'
description: 'alint validate-config checks that an .alint.yml loads: it resolves extends:, builds every rule and parses every when: clause, without walking the tree.'
---

`alint validate-config` answers "does this config load?" in milliseconds,
without linting anything. It exits 0 when the config is valid and 1 when it
isn't, which makes it a cheap first CI step and a good pre-commit check for
the config file itself.

## Examples

The config in the current directory:

```bash
alint validate-config
```

A specific file:

```bash
alint validate-config path/to/.alint.yml
```

## Reference

```
Parse-validate an `.alint.yml` without walking the tree.

Resolves `extends:`, builds every rule, and parses every `when:`, reporting any errors. For editor
LSP, pre-commit hooks, and fail-fast CI steps that just want to know "is the config loadable?". Exit
0 on success; exit 1 on validation failure.

Usage: alint validate-config [OPTIONS] [PATH]

Arguments:
  [PATH]
          Path to the config file to validate. Defaults to the `.alint.yml` discovered upward from
          the current directory (same discovery rules as `alint check`)

Options:
  -f, --format <FORMAT>
          Output format. `human` prints a one-line success or a rich error trace; `json` emits a
          stable `{"valid": bool, "rule_count": N, "config_path": ..., "error": "...?"}` envelope
          for editor / CI consumption

          [default: human]
          [possible values: human, json]
```

The [global options](/docs/cli/#global-options) apply to `alint validate-config` too, where they are relevant. Its own `--format` above replaces the global one.

## See also

- [Configuration](/docs/configuration/): what goes in the config file
