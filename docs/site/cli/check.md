---
title: 'alint check'
description: 'alint check lints a repository against its .alint.yml: the whole tree or only changed files, with exit codes and output formats built for CI.'
---

`alint check` is the default command: it walks the repository once, evaluates
every rule in the effective config (after `extends:`), and exits non-zero when
an error-level rule fails. Run it bare locally, with `--changed` as a fast
pre-commit or PR gate, and with a machine format in CI.

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

## See also

- [Changed mode](/docs/concepts/targeting/changed-mode/): what `--changed` narrows and what it doesn't
- [Severity and exit codes](/docs/concepts/start-here/severity-and-exit-codes/)
- [Output formats](/docs/reference/output-formats/)
- [GitHub Actions](/docs/integrations/github-actions/) and [pre-commit](/docs/integrations/pre-commit/)
