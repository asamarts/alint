---
title: 'alint test'
description: 'alint test discovers versioned policy-test YAML, runs each case in a hermetic temporary repository, and reports expectation, tree, and snapshot results.'
---

`alint test` is the policy-authoring loop. It runs the real alint loader, walker,
rules, and fixers against isolated repository fixtures without modifying the
fixture package. With no path it searches `alint-tests/**/*.yml` and
`alint-tests/**/*.yaml` below the current directory. Recursive discovery treats
only YAML mappings with a top-level `test_version` discriminator as policy
tests, so package-local configs and repository fixtures may contain ordinary or
malformed YAML. An explicit file argument is always validated as a policy test;
explicit directories use the same discriminator-aware recursion.

```bash
alint test
alint test tests/policy --tag fast --filter license
alint test --format json
```

Repeated `--tag` values are an AND filter. `--filter` is a case-sensitive
substring match against the policy-test name or normalized source path.
Discovery never follows directory symlinks, de-duplicates explicit paths, and
executes tests in source-path/name order. No discovered or selected tests is an
error rather than a successful empty run.

## Snapshots

A policy test may name a package-relative JSON `snapshot`. Normal runs compare
it after checking the inline expectations. `--update-snapshots` defers every
write until all selected tests pass their inline assertions; a failing test
therefore leaves every snapshot unchanged. Duplicate targets, symlink targets,
and paths outside the test package are rejected before execution.

```bash
alint test --update-snapshots
```

Snapshots supplement inline behavior assertions; they cannot replace them.

## Capabilities

Tests start with no host authority. A command-spawning rule or fixer, including
one nested in another rule's `require:`, requires both
`capabilities: { spawn: true }` in the document and `--allow-spawn` on the
command line. This grants native code execution with the invoking user's host
filesystem and network permissions; it is not an OS sandbox. Host-keyring
access similarly requires both `host_keyring: true` and
`--allow-host-keyring`. A custom fact needs the spawn capability too.
`--no-spawn` and `--allow-spawn` contradict each other, so `alint test`
rejects the pair.

All selected definitions, package paths, configs, and capability grants are
preflighted before any rule or user command executes.

A test's repository stays in its sandbox. A `given.tree` symlink must lead to
an entry of the tree as the file system resolves it, through any symlink it
reaches, and no tree entry may sit under a symlink. Every symlink in the
repository, from a tree or a fixture, must lead inside it, checked again after
each fix step. A fix never writes or reads a Git directory (`.git` in the
spellings Git's own checks name, such as `.GIT`, `.git.` and `git~1`; a short
name such as `GIT~2` or a hashed one passes), and no rule reads one.

`given.env` reaches config interpolation, `when:` conditions (`env.X`) and
granted commands only. A condition also sees the runner's own sandbox values:
`HOME` and `GNUPGHOME` (unless the host keyring is granted, when they are the
host's and a condition does not see them), `XDG_CONFIG_HOME`, `LC_ALL`, and the
`GIT_*` settings below (`GIT_CONFIG_NOSYSTEM`, `GIT_CONFIG_GLOBAL`,
`GIT_TERMINAL_PROMPT`, `GIT_OPTIONAL_LOCKS`, `GIT_CEILING_DIRECTORIES`). It
never sees the host's `PATH` or any other host variable. A granted command also
gets the host's `PATH` (and, on Windows, its system variables). The Git commands the runner runs itself, for `given.git` and for
rules that read Git, run with a fixed environment: a sandboxed home (the host's
home when the host keyring is granted), no system or global Git configuration,
no global excludes or attributes file, `PATH`, and the C locale. Each names the
repository `given.git` made (`--git-dir`, `--work-tree`), so Git never
discovers one: a fixture shaped like a bare repository stays content whatever
the Git version, and a test without `given.git` runs no Git at all, so it never
reads a repository the host's temporary directory is in.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Every selected test passed, or was explicitly skipped for the current platform |
| 1 | At least one executed test had an expectation, final-tree, or snapshot mismatch |
| 2 | Discovery, schema, package confinement, filtering, or capability grants were invalid |
| 3 | The runner could not safely establish or operate its sandbox, or a snapshot write failed |

`--format json` emits the stable
[`policy-test-report` schema](https://github.com/asamarts/alint/blob/main/schemas/v1/policy-test-report.json).
The report contains normalized sources, names, tags, ordered step observations,
structured mismatches, validation issues, and deterministic summary counts.

## See also

- [Rule authoring](https://github.com/asamarts/alint/blob/main/docs/development/rule-authoring.md): policy-test structure and bundled-ruleset conventions
- [alint check](/docs/cli/check/): run a policy against a real repository
