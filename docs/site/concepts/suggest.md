---
title: Suggesting rules
description: How `alint suggest` scans a repo for antipatterns and existing tooling, then proposes bundled rulesets and rule entries to adopt. Covers the signals, confidence levels, and review-only output.
sidebar:
  order: 10
---

The bundled rulesets cover the common ground, but every repo has conventions no generic ruleset knows about. `alint suggest` closes that gap: it scans your working tree for evidence that a rule *would* have caught something, and proposes the bundled rulesets and rule entries that fit. It is **review-only**: it never edits your `.alint.yml`; you copy across what you agree with.

Think of it as the counterpart to `alint init`: `init` scaffolds from ecosystem *markers* (a `Cargo.toml` → the Rust ruleset), while `suggest` reasons from *evidence in the tree* (three `console.log` calls in `src/` → a rule to forbid them).

## The workflow

```sh
alint suggest              # human-readable proposal table
```

Read the proposals, then adopt the ones you want, either by hand or by piping the paste-ready snippet into your config and editing it down:

```sh
alint suggest --format yaml >> .alint.yml   # then trim to what you accept
```

Re-run it periodically, or after onboarding a new area of the codebase, to catch conventions that have since accumulated evidence.

## What it looks for

Two kinds of signal, each shipping at a different confidence:

- **Ecosystem markers → bundled rulesets** (HIGH). A manifest or lockfile that maps to a bundled ruleset you don't already extend, e.g. a `go.mod` pointing at the `go` ruleset. Deduplicated against your existing `extends:` unless you pass `--include-bundled`.
- **Antipatterns in the tree → specific rules** (MEDIUM). Concrete residue a rule would catch, for example:
  - `console.log` / `console.debug` calls in production source → a rule that forbids them.
  - `.bak` files and scratch docs sitting at the repo root → `file_absent` rules.
  - TODO/FIXME markers older than 180 days → a `git_blame_age` rule that surfaces stale debt.

Each proposal carries its evidence (`3 console.log calls in src/`), so you can judge it at a glance.

## Confidence

`--confidence <low|medium|high>` sets the lower bound on signal strength:

```sh
alint suggest --confidence high   # only the strongest signals (ecosystem-marker hits)
alint suggest --confidence low    # broadest; useful when prospecting, but noisier
```

The default floor is **medium**: low-confidence hits are prospecting aids, not recommendations, so they stay hidden unless you ask. Proposals print in deterministic order (by confidence, then id), so the same tree always yields the same output.

## Output

- **human** (default): a grouped, colorized proposal table.
- **`--format yaml`**: a paste-ready `.alint.yml` snippet, header-commented with the scan timestamp and a "review each rule before adopting" reminder.
- **`--format json`**: a stable `{ proposals: [{ id, kind, evidence, confidence }, ... ] }` shape for agent consumption.
- **`--explain`**: print one-line, file-level evidence under each proposal so a reviewer can decide quickly.
- **`--include-bundled`**: also suggest bundled rulesets you already extend (off by default, since you presumably adopted those deliberately).

Progress on a large tree renders on stderr (`--progress` / `--quiet` control it); a `--format json` payload on stdout stays byte-clean.

## What it does *not* do

`suggest` **never writes to your config**: every output is review-only, so a scan can't surprise you with a rule you didn't vet. It also doesn't *run* the proposed rules against your tree; it proposes what would help, and you decide. Once you've adopted rules on an established repo, pair it with [baseline mode](/docs/concepts/baseline/) to grandfather the existing violations and gate only on new ones.

## See also

- [`alint suggest` CLI reference](/docs/cli/suggest/): every flag, captured from the binary
- [Baseline mode](/docs/concepts/baseline/): adopting on a repo with existing debt
- [Rules](/docs/rules/): the bundled rulesets and rule kinds `suggest` proposes
