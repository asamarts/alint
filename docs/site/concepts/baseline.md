---
title: Baseline mode
description: "Grandfather a repo's existing violations so alint check gates only on new findings, using a fingerprint that keys on content rather than line number."
sidebar:
  order: 13
---

Turning a linter on for an established repo has a chicken-and-egg problem: the rules go red on thousands of pre-existing violations the current change never introduced. **Baseline mode** breaks it. You record today's violations into a committed baseline file, and from then on `alint check` reports and gates on only the findings that are *new* relative to that snapshot. It is the ratchet: stop the bleeding now, pay the backlog down on your own schedule.

<svg class="alint-base" viewBox="0 0 460 252" role="img" aria-labelledby="bl-t bl-d" xmlns="http://www.w3.org/2000/svg">
<title id="bl-t">A baseline suppresses recorded findings, lets new ones fail, and prunes stale ones</title>
<desc id="bl-d">This run's findings are checked against the recorded baseline by fingerprint. A recorded finding is suppressed even after its line moved; a finding not in the baseline is new and fails the gate; a recorded finding gone from this run is stale and pruned.</desc>
<style>
  .alint-base { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-base { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-base { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-base .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-base .tag { font:600 11px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-base .tx { fill:var(--tx); } .alint-base .mut { fill:var(--mut); } .alint-base .ac { fill:var(--ac); }
  .alint-base .row { fill:var(--card); stroke:var(--bd); stroke-width:1.2; }
  .alint-base .pulse { animation:blpulse 2.2s ease-in-out infinite; }
  @keyframes blpulse { 0%,100%{opacity:1} 50%{opacity:.5} }
  @media (prefers-reduced-motion:reduce){ .alint-base .pulse{animation:none} }
</style>
<text class="ui ac" x="18" y="16">this run, checked against the baseline</text>
<text class="ui mut" x="442" y="16" text-anchor="end">by fingerprint</text>
<rect class="row" x="20" y="26" width="420" height="40" rx="8"/><rect x="20" y="26" width="6" height="40" rx="2" fill="#22c55e"/><text class="tag tx" x="40" y="44">no-todo</text><text class="tag mut" x="40" y="59">api.ts (line moved)</text><text class="tag" x="424" y="50" text-anchor="end" fill="#22c55e">suppressed</text>
<rect class="row" x="20" y="74" width="420" height="40" rx="8"/><rect x="20" y="74" width="6" height="40" rx="2" fill="#ef4444"/><text class="tag tx" x="40" y="92">no-todo</text><text class="tag mut" x="40" y="107">new.ts</text><text class="tag pulse" x="424" y="99" text-anchor="end" fill="#ef4444">new: fails the gate</text>
<rect class="row" x="20" y="122" width="420" height="40" rx="8" stroke-dasharray="4 3"/><rect x="20" y="122" width="6" height="40" rx="2" fill="#94a3b8"/><text class="tag mut" x="40" y="140">lockfiles-only-one</text><text class="tag mut" x="40" y="155">fixed since</text><text class="tag mut" x="424" y="147" text-anchor="end">stale: pruned</text>
<text class="tag mut" x="230" y="196" text-anchor="middle">the fingerprint keys on content, not the line number</text>
<text class="tag mut" x="230" y="220" text-anchor="middle">a finding that only moved lines stays suppressed</text>
</svg>

## The two-command workflow

**1. Record the baseline.** Run once when you adopt alint (and again when you deliberately pay down or accept debt):

```sh
alint baseline
```

This runs the same whole-tree evaluation as `check`, then writes every current violation to `.alint-baseline.json`. Commit that file.

**2. Enforce the delta.** In CI and locally:

```sh
alint check --baseline .alint-baseline.json
```

Every violation whose fingerprint is in the baseline is suppressed (up to its recorded count); only new violations are reported, and only they drive the exit code. Persist the path in `.alint.yml` (`baseline: .alint-baseline.json`) so CI need not repeat the flag. A `--baseline` flag overrides the config key, and there is **no silent auto-detect**: suppression is always an explicit, committed decision.

## Fingerprints, not line numbers

The crux of a usable baseline is a stable identity for each violation. alint fingerprints a violation as a SHA-256 over its rule, its path, and a **content discriminator**, chosen in priority order: a rule may supply its own key (a structured-query rule keys on its JSONPath like `$.license`, a whole-file rule on the path alone), otherwise the **offending line's text** is used, and a path-bearing finding with no line keys on `(rule, path)` with the message deliberately left out of the hash. The line *number* is never part of it.

So inserting or deleting unrelated lines never churns the baseline. **Editing the offending line** re-keys a line-anchored finding, so it counts as new and the gate catches it, but a structured-query or whole-file finding keeps its identity across unrelated edits. The baseline survives ordinary refactoring without stale-entry noise, and never masks a genuinely new problem.

## The baseline file

`.alint-baseline.json` is **JSON Lines**: a header, then one sorted entry per grandfathered finding.

```
{"schema_version":1,"alint_version":"0.16.1"}
{"rule_id":"no-todo-comments","path":"src/legacy/api.ts","fingerprint":"<64-hex>","count":3,"message":"TODO without an owner"}
{"rule_id":"lockfiles-only-one","path":null,"fingerprint":"<64-hex>","count":1,"message":"Multiple lockfiles found"}
```

One entry per line (not a JSON array) is deliberately **merge-friendly**, and the sorted order makes an unchanged tree regenerate byte-for-byte. Only `fingerprint` and `count` are matched; `rule_id`, `path`, and `message` are advisory, there so a reviewer reading the diff can see what is being grandfathered. The `count` is a **budget**: identical findings collapse into one entry, and if the tree later holds *more* occurrences than recorded, the excess is reported as new; if *fewer*, the remainder is pruned as stale.

## Keeping the baseline honest

Re-running `alint baseline` on a repo that already has one **will not silently grandfather new debt**. If the re-run would add any new fingerprint (or a higher count), it refuses to write and tells you to fix them or opt in:

```
regenerating .alint-baseline.json would grandfather 2 new violation(s) (+2 / -1);
fix them, or pass --accept-new to accept them into the baseline
```

Pruning *stale* entries (findings you have since fixed) is always safe and happens without a flag, so a pure-cleanup re-run just rewrites the file. Accepting *new* debt is always explicit, with `--accept-new`. Separately, at enforcement time, `alint check --baseline` **warns** about stale entries by default, and `--strict-baseline` turns those warnings into a hard failure so a baseline cannot quietly rot.

## Output formats

Suppression **marks** violations rather than deleting them, so **sarif** emits suppressed results with `baselineState: "unchanged"` (keeping GitHub Code Scanning alerts open-but-dismissed instead of flapping) and **json** carries a `summary.baselined_suppressed` count. Only sarif and json are baseline-aware; the other formats receive the already-filtered live report. The `--show-baselined` flag lists the suppressed findings in any format, and the exit code is gated on the live (new) findings only, unless `--strict-baseline` also fails the run on stale entries.

## In practice

Adopt alint on a repo with a backlog of `no-todo-comments` hits, then gate on the delta:

```sh
alint baseline                                  # records today's violations, commit it
alint check --baseline .alint-baseline.json     # CI: only NEW findings fail
```

A pull request that adds one fresh TODO, while the legacy ones stay suppressed, fails on exactly that one:

```
error  no-todo-comments  TODO without an owner
```

## Going deeper

- [`baseline:` configuration key](/docs/configuration/#baseline) persists the path.
- [The walker and git](/docs/concepts/walker-and-gitignore/) is the whole-tree evaluation a baseline records; baseline mode rejects `--changed` at record time.
- [Suggesting rules](/docs/concepts/suggest/) pairs with baseline mode when adopting on an established repo.
