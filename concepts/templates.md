---
title: Rule templates
description: Define a rule shape once via the top-level `templates:` block; instantiate it N times via `extends_template:` with `{{vars.X}}` substitution. Replaces N near-duplicate rules with one shape and N short instances.
sidebar:
  order: 6
---

A common monorepo pattern: every package directory should have a README; every service directory should have a README; every app directory should have a README. The rule body is identical, only the directory name changes. Before v0.5.10, that meant pasting the rule three times. Rule templates collapse that to one definition + three instances.

## How it works

A top-level `templates:` block defines reusable rule bodies. Each template carries an `id:` (its reference key) and any other rule-spec fields, with `{{vars.<name>}}` placeholders standing in for the values that vary per instance. Rules in the `rules:` block then reference a template by id and supply their own `vars:`:

```yaml
version: 1

templates:
  - id: dir-has-readme
    kind: file_exists
    paths: ["{{vars.dir}}/README.md"]
    level: warning
    message: "{{vars.dir}} is missing a top-level README"

rules:
  - extends_template: dir-has-readme
    id: packages-have-readme
    vars: { dir: packages }
  - extends_template: dir-has-readme
    id: services-have-readme
    vars: { dir: services }
  - extends_template: dir-has-readme
    id: apps-have-readme
    vars: { dir: apps }
```

That config produces three independent rules — `packages-have-readme`, `services-have-readme`, `apps-have-readme` — each scoped to the right directory, each with its own substituted message.

## Substitution

Substitution walks the template body recursively. Strings, lists, and nested mappings all get expanded:

```yaml
templates:
  - id: lang-source-rules
    kind: file_exists
    paths:
      - "{{vars.lang}}/Cargo.toml"
      - "{{vars.lang}}/Makefile"
    level: warning
    fix:
      file_create:
        path: "{{vars.lang}}/Cargo.toml"
        content_from: "templates/{{vars.lang}}.Cargo.toml"
```

Unknown placeholders are preserved literally — a typo in `{{vars.languge}}` shows up in the rule's output rather than silently blanking out a field.

## Field-level overrides on instances

An instance's own fields field-merge on top of the substituted body. Use this to bump severity for a specific instance, override the message, or attach a different `policy_url` while keeping everything else from the template:

```yaml
rules:
  - extends_template: dir-has-readme
    id: services-have-readme
    level: error                  # override: this one's a hard fail
    vars: { dir: services }
```

The instance's `id:` is required (templates don't carry an id into the instance — each instance owns its own); `vars:` and `extends_template:` are template-control fields that get stripped during expansion.

## Composition with extends

Templates merge through the `extends:` chain by id, the same way rules and facts do. A downstream config can replace an upstream template's body wholesale by re-defining the same id, or override individual fields. This lets shared rulesets (`extends: alint://bundled/...`) ship templates that consumers customise in their own `.alint.yml`.

## Leaf-only

A template can't itself reference `extends_template:` — the schema rejects it at config-load time with a clear "templates are leaf-only" error. This mirrors the bundled-rulesets restriction (which can't `extends:` themselves either): chained templates would invite cycles and silent depth explosions, both of which are awful to debug. If you need template hierarchies, build a thin pass-through rule that calls the inner template instead.

## When to reach for templates

Use templates when you'd otherwise write the same rule N times with one field varying. Common shapes:

- Per-directory existence / shape rules in monorepos (the `dir-has-readme` example above, scaled to packages / services / apps / docs)
- Per-language hygiene overlays driven by language facts
- Per-customer / per-environment config that swaps a name or URL

Don't reach for templates when only a single instance exists today — instantiating once is just indirection. And templates are no substitute for `extends:` for whole-config composition; they're for intra-config repetition.
