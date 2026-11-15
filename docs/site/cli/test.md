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

All selected definitions, package paths, configs, and capability grants are
preflighted before any rule or user command executes.

`given.env` reaches config interpolation, `when:` conditions (`env.X`) and
granted commands only, and none of the host's other variables reach them. The
Git commands the runner runs itself, for `given.git` and for rules that read
Git, run with a fixed environment: a sandboxed home (the host's home when the
host keyring is granted), no system or global Git configuration, `PATH`, and
the C locale. They never read a fixture as a bare repository, and the host's
global excludes file hides nothing from a test.

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
