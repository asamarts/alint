---
title: 'alint explain'
description: 'alint explain shows how one configured rule is defined: its kind, level, paths, options, and a link to the rule kind reference.'
---

`alint explain` takes a rule id from your effective config (including rules
pulled in by `extends:`) and prints its definition. Use it when a finding
names a rule you didn't write yourself.

A bundled ruleset contributes rules you never see in your own `.alint.yml`.
When `alint check` reports `oss-license-exists`, the id alone doesn't say which
files count as a license or whether nested copies are accepted. `alint explain`
answers that from the config alint actually loaded, after every `extends:`,
override and drop-in has been applied, so what it prints is what `check` runs.

## What it prints

For a rule from `alint://bundled/oss-baseline@v1`:

```bash
alint explain oss-license-exists
```

```text
id:         oss-license-exists
kind:       file_exists
categories: existence
summary:    Every glob match in paths must correspond to a real file.
docs:       https://alint.org/docs/rules/existence/file_exists/
level:      warning
paths:      LICENSE, LICENSE.{md,txt,TXT,rst}, license, license.{md,txt,rst}, LICENSE-APACHE, LICENSE-MIT, LICENSE-BSD, LICENSE-MPL, COPYING, COPYING.{md,txt}
options:    root_only: true
message:    An open-source repo should declare a license at the root.
policy_url: https://opensource.guide/legal/#which-open-source-license-is-appropriate-for-my-project
```

The fields, top to bottom:

- **`kind`**, **`categories`**, **`summary`** and **`docs`** describe the rule
  kind, the check this rule is an instance of. The `docs` link is that kind's
  reference page, with every option it accepts.
- **`level`** is the effective level. If your config overrides an inherited
  rule (`level: error`, say), the override is what shows here.
- **`paths`** and **`options`** are this rule's settings: here, any one of the
  listed names at the repository root satisfies it.
- **`when`** appears when the rule is gated on a fact, for example
  `when: facts.has_rust`. A gated rule that never reports anything is usually
  gated off; [`alint facts`](/docs/cli/facts/) shows what the fact resolved to.
- **`message`** and **`policy_url`** are what a finding prints and links to.

## Examples

Find the id in `alint list` or in a finding, then explain it:

```bash
alint explain oss-license-exists
```

Explain a rule as another config defines it, for example a stricter CI
config:

```bash
alint explain oss-license-exists --config .alint.ci.yml
```

An id that isn't in the effective config is an error (exit code 2), so a typo
is caught instead of explaining nothing:

```text
alint: no rule with id "oss-licence-exists" found in the effective config
```

## explain, list and rules show

Three commands describe rules from different angles:

| Command | Answers |
| --- | --- |
| `alint list` | Which rules does this config run? |
| `alint explain <id>` | How is this one configured rule set up? |
| `alint rules show <kind>` | What does this rule kind do, in any config? |

## See also

- [`alint list`](/docs/cli/list/): every rule id in the effective config
- [`alint rules show`](/docs/cli/rules/): a rule kind, independent of any config
- [`alint facts`](/docs/cli/facts/): why a gated rule did or didn't run
