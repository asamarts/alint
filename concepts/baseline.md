---
title: Baseline mode
description: Grandfather a repo's existing violations so `alint check` gates only on new findings — the fingerprint-based baseline file, the two-command workflow, and which formats are baseline-aware.
sidebar:
  order: 9
---

Adopting a linter on an established repo has a chicken-and-egg problem: turn the rules on and CI goes red on thousands of pre-existing violations, none of which the current PR introduced. **Baseline mode** breaks it. You record today's violations into a committed baseline file, and from then on `alint check` reports and gates on only the findings that are *new* relative to that snapshot. The legacy debt stays visible but non-blocking; the moment someone adds a *fresh* violation, the gate catches it.

This is the standard "ratchet": stop the bleeding now, pay the backlog down on your own schedule.

## The two-command workflow

**1. Record the baseline** — run once when you adopt alint (and again when you deliberately pay down or accept debt):

```sh
alint baseline
```

This runs the same whole-tree evaluation as `check`, then writes every current violation to `.alint-baseline.json`. Commit that file.

**2. Enforce the delta** — in CI and locally:

```sh
alint check --baseline .alint-baseline.json
```

Every violation whose fingerprint is in the baseline is suppressed (up to its recorded count); only new violations are reported, and only they drive the exit code. A one-line stderr summary tells you how many were suppressed.

Persist the path in `.alint.yml` so CI need not repeat the flag:

```yaml
baseline: .alint-baseline.json
```

`--baseline` on the command line overrides the config key. There is **no silent auto-detect** — a baseline suppresses findings only when you opt in explicitly (the config key or the flag), never because the file merely exists. Suppression is always a reviewable, committed decision.

## The baseline file

`.alint-baseline.json` is **JSON Lines** — a header line, then one sorted entry per grandfathered finding:

```
{"schema_version":1,"alint_version":"0.14.0"}
{"rule_id":"no-todo-comments","path":"src/legacy/api.ts","fingerprint":"<64-hex>","count":3,"message":"TODO without an owner"}
{"rule_id":"lockfiles-only-one","path":null,"fingerprint":"<64-hex>","count":1,"message":"Multiple lockfiles found"}
```

One entry per line (not a single JSON array) is a deliberate choice: it is **merge-friendly**. Two PRs that each grandfather a different finding don't collide on shared array brackets. Entries are sorted, so an unchanged tree regenerates byte-for-byte identically.

The `message` and `path` fields are **advisory** — they exist so a reviewer reading the baseline diff in a PR can see *what* is being grandfathered. Only the `fingerprint` is matched.

## Fingerprints, not line numbers

The crux of a usable baseline is a stable identity for each violation. alint fingerprints a violation as a SHA-256 over its rule, its path, and a **content discriminator** — for a line-anchored finding, the offending line's *text*, not its line *number*. So:

- Inserting or deleting unrelated lines elsewhere in the file **does not** churn the baseline (line numbers shift; fingerprints don't).
- **Editing the offending line itself re-triggers** the rule — the fingerprint changes, so the finding is treated as new and the gate catches it. You can't silently mutate grandfathered-but-broken code into something else that's broken.

This is why the baseline survives ordinary refactoring without a flood of stale-entry noise, yet never masks a genuinely new problem.

## Keeping the baseline honest

Re-running `alint baseline` on a repo that already has a baseline **will not silently grandfather new debt**. It prints `+N would be grandfathered / -M stale removed` and, if there is anything new (a new fingerprint, or a higher count on an existing one), refuses to write it unless you pass `--accept-new`:

```sh
alint baseline --accept-new     # deliberately accept the new debt
```

Pruning *stale* entries (findings you've since fixed) is always safe and happens without the flag. Accepting *new* debt is always explicit. This closes the footgun where a careless `baseline` re-run would rubber-stamp everything introduced since the last one.

Stale entries (present in the baseline but no longer firing) warn by default; `--strict-baseline` makes them a hard failure, so a baseline can't quietly rot.

## Output formats

Suppression **marks** violations rather than deleting them, so each formatter does the right thing for its consumer:

- **sarif** — suppressed results are emitted with `suppressions: [{ "kind": "external" }]` and `baselineState: "unchanged"`; new findings get `baselineState: "new"`. This keeps GitHub Code Scanning alerts *open-but-dismissed* instead of flapping fixed-then-reopened as findings resurface.
- **json** — suppressed findings are omitted from `results` (the gate sees only new), but the envelope carries a `summary.baselined_suppressed` count.
- **human** — suppressed findings are omitted; the count prints on stderr.

Only **sarif** and **json** are baseline-aware; the other formats receive the already-filtered live report. The global `--show-baselined` flag lists the suppressed findings in full, in any format. The exit code is gated on the live (new) findings only, always.

## What baseline mode does *not* do

- **`fix` does not take `--baseline`.** Auto-fixing is orthogonal to grandfathering; run `fix` normally, then refresh the baseline.
- **It is not `--changed`.** A baseline must be whole-tree, so `alint baseline` rejects `--changed`/`--base` — a changed-scope snapshot would capture only the diff and silently omit the rest of the legacy tree. Baseline mode and changed-file mode compose at `check` time, not at record time.

## See also

- [`baseline:` configuration key](/docs/configuration/#baseline)
- [Output formats](/docs/reference/output-formats/) — the full per-format behaviour
- [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) — what the whole-tree evaluation sees
