---
title: Drop-in configs (`.alint.d/*.yml`)
description: Auto-discovered alongside the top-level `.alint.yml`, merged alphabetically. The `/etc/*.d/` shape applied to alint configs — stage `00-base.yml` for ops defaults, `99-local.yml` for developer overrides.
sidebar:
  order: 7
---

When `.alint.yml` lives alongside a `.alint.d/` directory at the same path, alint discovers every `*.yml` (or `*.yaml`) file inside that directory and merges them into the effective config. Merge order is alphabetical by filename — the **last drop-in wins** on field-level conflict, mirroring the `/etc/*.d/` convention. Non-yaml files (`.gitkeep`, `README.md`) are silently skipped.

## Layout

```
.alint.yml
.alint.d/
    00-base.yml         # ops defaults
    50-team.yml         # team policies
    99-local.yml        # developer-local tweaks (gitignored)
```

Each drop-in is a complete alint config (with its own `version: 1`) that contributes to the merged result. They can declare new rules, override existing ones field-by-field by id (just like `extends:` overrides), add `extends:` entries of their own, or layer extra `facts:` / `vars:`.

## Trust posture

Drop-ins are **trust-equivalent to the main `.alint.yml`** — they live in the same workspace under the user's control, so they CAN declare `custom:` facts and `kind: command` rules without the trust gate that protects HTTPS-fetched and `alint://bundled/` extends. The mental model: anything you'd put in your own `.alint.yml`, you can put in a drop-in.

## Use cases

- **Ops/team layering.** A repo's `.alint.yml` declares the baseline; ops drops in `50-org-policies.yml` via the build / provisioning system; CI drops in `90-ci-strict.yml` to bump warnings to errors.
- **Per-developer customisation.** `99-local.yml` is gitignored and lets each contributor tweak severities for their own workflow without polluting the committed config.
- **Bundled-policy overlays.** A workplace ships an internal config bundle; sub-projects drop them in instead of duplicating an `extends:` line.

## What gets merged where

```yaml
# .alint.yml
version: 1
extends:
  - alint://bundled/rust@v1
rules:
  - id: my-custom-rule
    kind: file_exists
    paths: [VERSION]
    level: warning
```

```yaml
# .alint.d/50-team.yml
version: 1
rules:
  - id: my-custom-rule
    level: error             # bumps severity
  - id: extra-rule
    kind: no_trailing_whitespace
    paths: ["**/*.rs"]
    level: warning
```

After merge: `my-custom-rule` has `level: error` (drop-in won the field override), `extra-rule` is added, the `extends: alint://bundled/rust@v1` chain still applies. `alint list` shows the union with the resolved levels.

## Limits

- **Top-level only.** Only the root config gets `.alint.d/` discovery. Sub-extended configs (anything reached via `extends:`) don't get their own drop-ins — that would compound merge complexity beyond reason.
- **Alphabetical order is the only knob.** No explicit priority field, no "before this file" hooks. Use the `00-`, `50-`, `99-` convention to reserve ranges; that's how `/etc/*.d/` does it and it's enough.
- **Merge isn't recursive into nested fields.** Override semantics match `extends:` exactly — top-level keys on a rule mapping merge by id, but nested structures (`fix:` blocks, `paths:` `include`/`exclude` pairs) replace wholesale. Surprising in theory; the right call in practice (overriding `fix.file_create.content` partially is rarely what you actually want).
