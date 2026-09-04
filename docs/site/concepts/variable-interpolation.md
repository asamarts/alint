---
title: 'Variable interpolation ({{env.X}})'
description: 'Reference environment variables from any string-typed config field with {{env.NAME}}, with a default(...) fallback. Resolved once at config load. The when: language gains an env.X namespace.'
sidebar:
  order: 9
---

alint resolves `{{env.NAME}}` references in your `.alint.yml` against the environment at config-load time. This lets one committed config adapt to its environment — a PR base SHA, a per-team registry host, an environment-driven path root — without templating the YAML file externally.

```yaml
rules:
  - id: pr-conventional-commits
    kind: git_commit_message
    pattern: '^(feat|fix|chore|docs|refactor|test)(\(.+\))?!?: '
    subject_max_length: 72
    since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
    level: error
```

## Syntax

- `{{env.NAME}}` — substitute the value of environment variable `NAME`. If `NAME` is unset (or empty) and there is no default, config load fails with an error naming the field.
- `{{env.NAME | default('fallback')}}` — substitute `NAME`, or the quoted fallback when `NAME` is unset/empty. Single or double quotes are accepted.
- Interpolation composes with surrounding text: `"https://policy.{{env.TEAM}}.example.com/v1.yml"`.

The `| default(...)` filter follows the Jinja / Liquid / Nunjucks convention. It is the only filter in this release.

## Where it applies

Every string-typed **value** field is interpolated at load:

| Field | Interpolated | Why |
|---|---|---|
| `extends:` entry (URL `#sha256-…`) | yes | per-environment / per-team registry |
| `paths:` glob | yes | per-environment path roots |
| `pattern:` regex | yes | environment-driven matcher |
| `since:`, `changed_since:` | yes | the motivating case |
| `policy_url:` | yes | per-team policy URLs |
| `message:` | yes | adds `{{env.X}}` alongside `{{vars.X}}` / `{{ctx.X}}` |
| `content:`, `content_from:` | yes | templated fix bodies |
| `vars:` values | yes | lets `vars` themselves be env-driven |
| `id:`, `kind:`, `level:` | **no** | identity / type / severity must be stable across environments — env-driven values break audit trails and reproducibility |

Interpolation only happens for **local config files** — the ones you author. Bundled rulesets are static, and a remote `extends:` ruleset is never interpolated against *your* environment.

## Type coercion

A value that is *fully* resolved by interpolation is re-typed, so an env-driven number or bool lands in a typed field correctly:

```yaml
subject_max_length: "{{env.MAX_SUBJECT | default('72')}}"   # validates as the integer 72
```

A consequence: a string-typed field whose env value is bare-numeric (`"72"`) or bare-bool (`"true"`) re-types too; if the field truly wants those characters as a string, the subsequent validation surfaces a clear type error.

## `env.X` in `when:` expressions

The `when:` expression language gains `env` as a third namespace alongside `facts` and `vars`:

```yaml
- id: ci-only-rule
  kind: file_exists
  paths: [coverage.xml]
  when: env.CI == "true" or env.GITHUB_ACTIONS == "true"
  level: warning
```

Unlike string interpolation (load-time), `when: env.X` resolves at **evaluation time** — but since the environment is constant during a run, the result is the same. An unset variable evaluates to `null` (falsy), matching the "missing fact is falsy" rule. `env` is value-only — there are no callable methods on it.

**env values are always strings.** Compare against string literals: `when: env.PORT == "8080"`, not `== 8080`. A bare-integer comparison is a type mismatch that evaluates silently to `false` (the rule then never applies), so quote the right-hand side.

## `{{...}}` is shared — foreign templates pass through

alint does not own the `{{...}}` namespace. Go templates (`{{json .}}`, `{{end}}`), cookiecutter (`{{cookiecutter.slug}}`), Jinja, and others appear legitimately in `command:` args and `pattern:` regexes. alint only acts on spans it is confident are its own — `env.`, `vars.`, `ctx.` (and close typos of `env`/`vars`, which it flags). Everything else passes through verbatim:

```yaml
rules:
  - id: lint-workflows
    kind: command
    command: ["actionlint", "-format", "{{json .}}"]   # Go template — left untouched
```

## Migrating from the v0.9.21 `${VAR}` syntax

v0.9.21 shipped POSIX-style `${VAR}` / `${VAR:-default}` interpolation on `git_commit_message.since:` only. That syntax still works but emits a deprecation warning at config load, with the canonical rewrite shown inline:

```text
alint: warning: rule "pr-conventional-commits": `since: ${ALINT_BASE_SHA}` uses the
  deprecated v0.9.21 `${VAR}` interpolation syntax. The canonical form is
  `{{env.ALINT_BASE_SHA}}`; the `${VAR}` form will be removed in v1.0.
```

Rewrite `${VAR}` → `{{env.VAR}}` and `${VAR:-default}` → `{{env.VAR | default('default')}}`. The `${VAR}` path is removed in v1.0.

## Security

`{{env.X}}` reads via the standard environment lookup — no shell evaluation, no `system()`. The `extends:` case is the one to be aware of: an env var referenced in an `extends:` entry could change where a config load fetches from. Interpolation covers the whole entry string (including any `#sha256-…` hash), so it does **not** by itself pin the host.

The real trust boundary is your local `.alint.yml`: it is trusted source. Influencing an `extends:` URL through interpolation requires the ability to (a) write `{{env.X}}` into your config **and** (b) control the environment — and (a) alone is already a full compromise (an attacker who can edit your config can just write the malicious URL directly). For configs whose `extends:` URLs are *not* env-driven, the author-fixed `#sha256-…` hash continues to pin the content exactly as before.
