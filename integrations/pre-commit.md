---
title: pre-commit
description: Run alint as a pre-commit hook.
sidebar:
  order: 2
---

alint ships a [pre-commit](https://pre-commit.com/) hook definition. Add it to your `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: https://github.com/asamarts/alint
    rev: v0.9.6
    hooks:
      - id: alint
```

The `alint` hook runs `alint check` against the repo's `.alint.yml` on every commit, blocking commits whose changes introduce errors.

## Auto-fix on demand

A second hook id, `alint-fix`, applies fixers. It's registered under `stages: [manual]` so it does not run on every commit (fixers mutate the working tree). Invoke explicitly:

```bash
pre-commit run alint-fix --all-files
```

## Recommended config

Pin to a tagged release. Updating the `rev:` is how you upgrade alint:

```yaml
repos:
  - repo: https://github.com/asamarts/alint
    rev: v0.9.6
    hooks:
      - id: alint
        # Pass extra args here if you need to:
        args: ["--fail-on-warning"]
```

The hook uses `language: rust`, so pre-commit handles toolchain installation transparently — zero setup for pre-commit users.
