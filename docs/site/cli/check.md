---
title: 'alint check'
description: 'alint check lints a repository against its .alint.yml: the whole tree or only changed files, with exit codes and output formats built for CI.'
---

`alint check` is the default command: it walks the repository once, evaluates
every rule in the effective config (after `extends:`), and exits non-zero when
an error-level rule fails. Run it bare locally, with `--changed` as a fast
pre-commit or PR gate, and with a machine format in CI.

It reads `.alint.yml` from the directory it checks, honours `.gitignore` while
walking, and never modifies a file: repairs are [`alint fix`](/docs/cli/fix/)'s
job. `alint` with no subcommand runs the same check on the current directory.

## What it prints

A small Python project with a README and a `pyproject.toml` but no license,
checked against two bundled rulesets:

```yaml
version: 1
extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/python@v1
```

```bash
alint check
```

```text
─── Repository-level ───────────────────────────────────────────
  ⚠  warning  oss-license-exists
              An open-source repo should declare a license at
              the root.
  ℹ  info     oss-security-policy-exists
              Consider adding a SECURITY.md so vulnerability
              reporters know where to disclose.
  ℹ  info     oss-dependency-update-tool
              Consider configuring Dependabot or Renovate to
              keep dependencies and actions up to date.
  ℹ  info     oss-codeowners-exists
              Consider adding a CODEOWNERS file so PR reviews
              are auto-routed.
  ℹ  info     oss-code-of-conduct-exists
              Consider adding a CODE_OF_CONDUCT.md.
  ℹ  info     oss-gitignore-exists
              Most OSS repos should have a .gitignore to keep
              build artefacts and secrets out of git.
  ⚠  warning  python-has-lockfile
              A lockfile should be committed for reproducible
              installs (uv.lock / poetry.lock / Pipfile.lock /
              pdm.lock).

─── pyproject.toml ─────────────────────────────────────────────
  ℹ  info     python-pyproject-declares-requires-python
              `pyproject.toml` has no `project.requires-python`;
              declare a floor (e.g. `>=3.10`) so installs fail
              fast on unsupported interpreters.

Summary (8 violations):
  ⚠ 2 warnings   ℹ 6 info
  15 passing · 8 failing
```

(Captured with `--width 64 --no-docs`; by default each finding also carries a
`docs:` link to its rule kind's page.)

Findings are grouped by file. Checks about the repository as a whole, such as a
missing license, sit under "Repository-level". This run exits 0: there are
warnings and infos but no errors.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | No error-level findings (warnings and infos may be present) |
| 1 | At least one error-level finding, or a warning under `--fail-on-warning` |
| 2 | The config or the command line is invalid |
| 3 | Internal error |

The same run with `--fail-on-warning` exits 1 because of the two warnings.
Raising a rule to `level: error` in your config is the per-rule way to make it
block.

## Flags that matter most

- **`--changed`** limits per-file rules to changed files: staged, unstaged and
  untracked ones by default (the pre-commit shape), or the merge-base diff
  against `--base <ref>` (the PR shape; `--base` implies `--changed`).
  Existence and cross-file rules still see the whole tree.
- **`--format`** picks the output: `human` (the default; `--compact` makes it
  one line per finding), or `json`, `sarif`, `github`, `gitlab`, `junit`,
  `markdown` and `agent` for tools.
- **`--baseline <file>`** reports only findings that aren't recorded in a
  baseline, so a legacy repository can gate on new problems first.
- **`--only <rule-id>`** runs one rule (repeatable), handy while writing or
  debugging a rule.
- **`--fail-on-warning`** turns warnings into a failing exit code.

## Examples

Lint the repository in the current directory:

```bash
alint check
```

A fast PR gate, where per-file rules check only the files changed on this branch:

```bash
alint check --changed --base origin/main
```

SARIF for GitHub code scanning:

```bash
alint check --format sarif > alint.sarif
```

Fail only on violations that aren't in the committed baseline:

```bash
alint check --baseline .alint-baseline.json
```

One finding per line, for grep or an editor's quickfix list:

```bash
alint check --compact
```

```text
<repo>: warning: oss-license-exists: An open-source repo should declare a license at the root.
<repo>: info: oss-security-policy-exists: Consider adding a SECURITY.md so vulnerability reporters know where to disclose.
```

## See also

- [Changed mode](/docs/concepts/targeting/changed-mode/): what `--changed` narrows and what it doesn't
- [Severity and exit codes](/docs/concepts/start-here/severity-and-exit-codes/)
- [Output formats](/docs/reference/output-formats/)
- [GitHub Actions](/docs/integrations/github-actions/) and [pre-commit](/docs/integrations/pre-commit/)
