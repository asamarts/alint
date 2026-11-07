---
title: 'Variable interpolation ({{env.X}})'
description: 'Use {{env.NAME}} in string-valued configuration, add default fallbacks, and read environment variables from when conditions.'
sidebar:
  order: 9
---

alint resolves `{{env.NAME}}` references in your `.alint.yml` against the environment at config-load time. This lets one committed config adapt to its environment (a PR base SHA, a per-team registry host, an environment-driven path root) without templating the YAML file externally.

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

- `{{env.NAME}}`: substitute the value of environment variable `NAME`. If `NAME` is unset (or empty) and there is no default, config load fails with an error naming the field.
- `{{env.NAME | default('fallback')}}`: substitute `NAME`, or the quoted fallback when `NAME` is unset/empty. Single or double quotes are accepted.
- Interpolation composes with surrounding text: `"https://policy.{{env.TEAM}}.example.com/v1.yml"`.

The `| default(...)` filter follows the Jinja / Liquid / Nunjucks convention. It is the only filter in this release.

## Where it applies

Every string-typed **value** field is interpolated at load:

| Field | Interpolated | Why |
|---|---|---|
| `extends:` entry (URL `#sha256-...`) | yes | per-environment / per-team registry |
| `paths:` glob | yes | per-environment path roots |
| `pattern:` regex | yes | environment-driven matcher |
| `since:`, `changed_since:` | yes | the motivating case |
| `policy_url:` | yes | per-team policy URLs |
| `message:` | yes | `{{env.X}}` at load; `{{ctx.X}}` fills per violation (`{{vars.X}}` substitutes only inside `templates:`) |
| `content:`, `content_from:` | yes | templated fix bodies |
| `vars:` values | yes | lets `vars` themselves be env-driven |
| `id:`, `kind:`, `level:` | **no** | identity / type / severity must be stable across environments. Env-driven values break audit trails and reproducibility |

Interpolation only happens for **local config files**, the ones you author. Bundled rulesets are static, and a remote `extends:` ruleset is never interpolated against *your* environment.

## Type coercion

A value that is *fully* resolved by interpolation is re-typed, so an env-driven number or bool lands in a typed field correctly:

```yaml
subject_max_length: "{{env.MAX_SUBJECT | default('72')}}"   # validates as the integer 72
```

A consequence: a string-typed field whose env value is bare-numeric (`"72"`) or bare-bool (`"true"`) re-types too, and validation then rejects it with a type error. alint shows that error as `<redacted>`, because it would quote the value, which may be a secret.

## What alint shows of a value from the environment

A value read from the environment may be a secret, so alint keeps it out of what it prints:

- A configuration error that could quote the value shows `<redacted>` in its place. alint repeats the failed step with every such value masked (a string replaced, a number moved one step, a boolean flipped) and shows the real error only when the masked run fails with the same message, which then cannot quote one of them. An unknown option or an unsupported field stays readable this way, except in a rule whose `fix:` block or manifest-path `scope_filter:` reads the environment: alint cannot mask those, so that rule's errors stay redacted.
- A rule with any field read from the environment, other than `policy_url`, reports its findings with redacted evidence, as [`sensitive: true`](/docs/configuration/#sensitive-all-rules-v019) does. When the field chooses files (`paths:`, `scope_filter:`, a selector such as `select:`, or a nested `require:`), the paths are `<redacted>` too, in reports and in the warnings `alint fix` prints.
- A `pattern:` read from the environment, or in a `sensitive: true` rule, gets no warning about its `^` or `$` anchors: the warning would say how the pattern starts or ends.
- A value in a top-level field (`extends:`, `ignore:`, `facts:`, `baseline:`, `version:`) is replaced with `<redacted>` wherever an error or warning prints it, including printed as a path or as a remote URL without its credentials.
- A selector with a dotted name (`targets.files`, `source.file`) and a nested rule's `when_iter:` choose files too, including a `when_iter:` that reads `env.X` as it runs or a var that holds a value from the environment, and `alint explain` shows the `fix:` line of a rule with redacted findings as `<redacted>`, since a fixer's description can quote its other fields.
- A redaction marker (`<redacted:...>`) leaves out a `<redacted>` path, so it says nothing about which file a finding is in, and the warning `alint baseline`, `check --baseline`, `fix` and the language server print for a file they cannot fingerprint shows such a path as `<redacted>`.
- From v0.22, the fingerprint of a finding with a `<redacted>` path leaves the path out, in the baseline file and in SARIF (`partialFingerprints`) and GitLab output, so someone holding one of them cannot test a guessed path against it. A rule's findings with `<redacted>` paths share one fingerprint, and a baseline grandfathers them by count. v0.22 still matches a baseline file v0.21 wrote for them; v0.23 will not, so re-run `alint baseline` before upgrading to it.
- A `JSONPath`, or a `when_iter:` or nested rule's `when:` expression, is masked with one that still parses, so an unrelated error after it stays readable.
- `alint baseline`'s confirmation, and `alint facts` for a fact value (a `custom:` command given the value as an argument can print it), show `<redacted>` in its place.

Two kinds of value are still printed: a boolean, and a value an error prints in fewer than four characters. Removing every occurrence of those would garble the message, and they are too short to be much of a secret. A value for an option with a fixed set of choices (`style: tabs`) cannot be masked with another valid choice, so an unrelated error after it in the same rule stays redacted.

These are outside what alint recognizes:

- A `since:` written in the deprecated `${VAR}` form is expanded when the rule runs, not at load, so it does not count as read from the environment and the rule's findings are not redacted. Its range error shows the reference as written. The [`{{env.X}}` form](#migrating-from-the-v0921-var-syntax) has neither gap.
- `alint facts` recognizes a value as it was read. A `custom:` command that prints it changed (encoded, split, or in another case) shows it.

## `env.X` in `when:` expressions

The `when:` expression reads four namespaces: `facts`, `vars`, `env`, and `iter` (the last only inside iteration contexts, such as a `for_each_*` rule's `when_iter:`). `env` exposes environment variables:

```yaml
- id: ci-only-rule
  kind: file_exists
  paths: [coverage.xml]
  when: env.CI == "true" or env.GITHUB_ACTIONS == "true"
  level: warning
```

Unlike string interpolation (load-time), `when: env.X` resolves at **evaluation time**, but since the environment is constant during a run, the result is the same. An unset variable evaluates to `null` (falsy), matching the "missing fact is falsy" rule. `env` is value-only: there are no callable methods on it.

**env values are always strings.** Compare against string literals: `when: env.PORT == "8080"`, not `== 8080`. A bare-integer comparison is a type mismatch that evaluates silently to `false` (the rule then never applies), so quote the right-hand side.

A `when:` (or `when_iter:`) from an `https://` `extends:` entry that is not listed under `trusted_extends:` may not read `env.*`: which of its rules fire would reveal your environment one comparison at a time. Such a config fails to load with an error naming the URL, the rule, and the variable. The same applies when a rule from that source fills a `{{vars.*}}` placeholder in one of your templates' `when:`, and a template from that source may not put a placeholder in `when:` at all. Your own config, local `extends:` files, and bundled rulesets are unaffected.

A var whose value you interpolate from the environment counts as the environment. With `vars: { token: "{{env.NPM_TOKEN}}" }` in your config, a drop-in, or a local `extends:` file (any spacing, with or without `| default(...)`), an untrusted remote's `when:` that reads `vars.token` is refused the same way, and the error shows the var's source text (`{{env.NPM_TOKEN}}`), never its value. The check uses the var's final value: overriding it later with a literal makes it readable again. Vars with literal values stay readable by every ruleset. Likewise, when a rule of yours passes an environment-derived value to a template through its own `vars:`, an untrusted remote that defines or adds to that template (or to the rule) may not have it substitute that `{{vars.*}}` anywhere, for example into a `message:`.

## `{{...}}` is shared: foreign templates pass through

alint does not own the `{{...}}` namespace. Go templates (`{{json .}}`, `{{end}}`), cookiecutter (`{{cookiecutter.slug}}`), Jinja, and others appear legitimately in `command:` args and `pattern:` regexes. alint only acts on spans it is confident are its own: `env.`, `vars.`, `ctx.` (and close typos of `env`/`vars`, which it flags). Everything else passes through verbatim:

```yaml
rules:
  - id: lint-workflows
    kind: command
    command: ["actionlint", "-format", "{{json .}}"]   # Go template, left untouched
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

`{{env.X}}` reads via the standard environment lookup: no shell evaluation, no `system()`. The `extends:` case is the one to be aware of: an env var referenced in an `extends:` entry could change where a config load fetches from. Interpolation covers the whole entry string (including any `#sha256-...` hash), so it does **not** by itself pin the host.

The real trust boundary is your local `.alint.yml`: it is trusted source. Influencing an `extends:` URL through interpolation requires the ability to (a) write `{{env.X}}` into your config **and** (b) control the environment; (a) alone is already a full compromise (an attacker who can edit your config can just write the malicious URL directly). For configs whose `extends:` URLs are *not* env-driven, the author-fixed `#sha256-...` hash continues to pin the content exactly as before.
