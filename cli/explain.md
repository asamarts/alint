---
title: 'alint explain'
description: 'alint explain shows one effective rule and can trace why one production evaluation passed, failed, skipped, or could not complete.'
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
capabilities: git(tracked_set) [when git_tracked_only is set]
selectors:   none
fix_ops:     file_create
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
- **`capabilities`** lists host access the kind can require and the condition
  that activates conditional access; **`selectors`** lists accepted
  kind-specific selector fields; **`fix_ops`** lists compatible fix operations.
- **`level`** is the effective level. If your config overrides an inherited
  rule (`level: error`, say), the override is what shows here.
- **`paths`** and **`options`** are this rule's settings: here, any one of the
  listed names at the repository root satisfies it.
- **`when`** appears when the rule is gated on a fact, for example
  `when: facts.has_rust`. A gated rule that never reports anything is usually
  gated off; [`alint facts`](/docs/cli/facts/) shows what the fact resolved to.
- **`message`** and **`policy_url`** are what a finding prints and links to.
- **`analysis_time`** appears when the configured rule—or a rule nested under
  its `require:` tree—uses deterministic time. It shows the UTC date and whether
  it came from an `explicit` input or `head`; an unavailable fallback includes
  the stable failure reason.

## Reference date

`alint explain <id> --as-of YYYY-MM-DD` shows the exact strict UTC date a
subsequent check would use. The flag overrides `ALINT_AS_OF`; without either,
alint reads the current HEAD commit's committer timestamp. Human output renders
`analysis_time: YYYY-MM-DD (explicit|head)`. With `--format json`, the
`analysis_time` object contains `date` and `source`; an unavailable fallback has
`date: null`, `source: "unavailable"`, and `error`.

## Trace one production evaluation

`--trace` walks the repository and evaluates exactly the selected effective
rule through the same engine path as `check`:

```bash
alint explain cargo-workspace-resolver --trace
alint explain cargo-workspace-resolver --trace --format json
```

The static definition stays first. Human output then lists ordered gate,
selection, extraction, cardinality, assertion, nested-evaluation, and outcome
stages that apply to the rule. JSON uses the experimental `kind: "rule-trace"`
envelope documented by
`schemas/experimental/rule-trace.json`; the stable static document remains
`kind: "rule"`.

A trace is diagnostic. A satisfied, violated, skipped, or analysis-failure
outcome exits 0 so scripts can inspect `result.status`. Invalid flags or config
exit 2; an internal invariant failure exits 3. A configured `level: off` rule
can be selected in trace mode and reports `skipped`.

Only the selected rule executes. This matters for process-backed rules:
tracing a `command` rule can run that rule's configured subprocess, while
unrelated command rules are never spawned. The production check context also
evaluates configured custom facts, which may execute their own subprocesses.
Plain `alint explain <id>` retains its static no-walk, no-evaluation behavior.

Use the same Git scope as a changed-only check:

```bash
alint explain my-rule --trace --changed
alint explain my-rule --trace --base origin/main
```

`--base REF` implies `--changed` and uses the merge-base diff
`REF...HEAD`. The JSON `execution` object records the effective controls.

### Bounded evidence preview

`--preview` adds deterministic structured-extraction rows and nested-child
outcomes where the selected rule exposes them:

```bash
alint explain package-version --trace --preview
alint explain package-version --trace --preview --preview-limit 5
```

The default limit is 20; accepted limits are 1–100. `total`, `shown`,
`omitted`, `redacted`, and `value_truncated` remain exact even when rows are
omitted. Retained rows are the lexicographically first rows by structural key,
not whichever parallel worker finishes first. Display values are bounded to
256 UTF-8 bytes; nested rows report each child instance's terminal outcome.

Environment-derived and explicitly sensitive option, message, policy URL,
condition, result, and configuration-error evidence is replaced with
`<redacted>` before trace storage or rendering. Dotenv and Java-properties
extracted values are sensitive by default. Rule ids and literal repository
source paths remain visible structural identity; environment-derived selector
paths and evidence-derived paths fail closed to `<redacted>`.

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

## Reference

```
Show a configured rule, optionally tracing one real selected-rule run

Usage: alint explain [OPTIONS] <RULE_ID>

Arguments:
  <RULE_ID>
          Rule id to describe

Options:
      --trace
          Walk the repository and evaluate only this effective rule. A traced command rule and any
          configured custom facts may execute subprocesses, matching `check`; unrelated command
          rules never run. Trace outcomes are diagnostic and exit 0, including violations and skips

      --preview
          Include bounded, redacted extraction or nested-child evidence in the trace

      --preview-limit <N>
          Maximum retained preview rows (1..=100; default 20)

      --changed
          Restrict the traced run to paths changed in Git

      --base <REF>
          Base ref for the traced changed-path run. Implies `--changed`
```

The [global options](/docs/cli/#global-options) apply to `alint explain` too, where they are relevant.

## See also

- [`alint list`](/docs/cli/list/): every rule id in the effective config
- [`alint rules show`](/docs/cli/rules/): a rule kind, independent of any config
- [`alint facts`](/docs/cli/facts/): why a gated rule did or didn't run
