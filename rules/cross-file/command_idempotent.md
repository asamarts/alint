---
title: 'command_idempotent'
description: 'Run a user-declared formatter/checker in its **--check (idempotence) mode** once: exit 0 ⇒ the tree is formatter-clean (silent); non-zero ⇒.'
sidebar:
  order: 8
---

Run a user-declared formatter/checker in its **`--check`
(idempotence) mode** once: exit `0` ⇒ the tree is
formatter-clean (silent); non-zero ⇒ violation(s). The sibling
of `generated_file_fresh` — that rule diffs a *generator's*
captured stdout against a committed file; this trusts a
*checker's* own `--check` exit code. **alint never runs a
mutating formatter and never writes the tree.** With
`files_from` (`stdout`/`stderr`) the tool's own offender list is
parsed into one violation **per file** (optional `files_pattern`
regex, capture group 1 = path, for tools that wrap the path in a
message like `cargo fmt`'s `Diff in <path> at line N`); without
it, one violation for the whole invocation. A non-zero exit is
never swallowed into a pass. Single-shot, opt-in. Trust-gated
like `command` (see below): declarable only in your own
top-level config.

```yaml
- id: code-is-formatted
  kind: command_idempotent
  command: ["cargo", "fmt", "--all", "--", "--check"]
  workdir: "."
  files_from: stderr
  files_pattern: "Diff in (.+) at"
  level: error
  message: "run `cargo fmt` — code is not formatter-clean"
```

