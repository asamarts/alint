---
title: Severity and exit codes
description: "How a rule's level (error, warning, info, off) maps to alint's process exit code, and how --fail-on-warning tightens the gate for CI."
sidebar:
  order: 5
---

Every rule carries a `level`, and every `alint check` returns an exit code. The level decides how loud a finding is; the exit code is what your CI gates on. The mapping is small and fixed.

<svg class="alint-sev" viewBox="0 0 460 292" role="img" aria-labelledby="sev-t sev-d" xmlns="http://www.w3.org/2000/svg">
<title id="sev-t">Severity levels mapped to exit codes</title>
<desc id="sev-d">A rule's level maps to an exit code. error exits 1 and fails the run. warning exits 0 by default, or 1 with the --fail-on-warning flag. info exits 0 and is reported only. off means the rule is skipped. A bad config or usage exits 2; an internal error exits 3.</desc>
<style>
  .alint-sev { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-sev { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-sev { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-sev .mono { font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-sev .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-sev .tx { fill:var(--tx); } .alint-sev .mut { fill:var(--mut); } .alint-sev .ac { fill:var(--ac); }
  .alint-sev .flow { fill:none; stroke:var(--ac); stroke-width:2; stroke-dasharray:5 5; opacity:.7; animation:sflow 1s linear infinite; }
  @keyframes sflow { to { stroke-dashoffset:-10; } }
  @media (prefers-reduced-motion:reduce){ .alint-sev .flow { animation:none; stroke-dasharray:none; } }
</style>
<text class="ui ac" x="18" y="16">a level maps to an exit code</text>
<rect x="20" y="42" width="90" height="28" rx="14" fill="#ef4444"/><text class="ui" x="65" y="61" text-anchor="middle" fill="#fff">error</text>
<path class="flow" d="M 116 56 H 138"/><path fill="var(--ac)" d="M 138 52 L 145 56 L 138 60 Z"/><text class="mono" x="146" y="61" fill="#ef4444" font-weight="700">exit 1</text><text class="mono mut" x="212" y="61" font-size="12">fails the run</text>
<rect x="20" y="86" width="90" height="28" rx="14" fill="#f59e0b"/><text class="ui" x="65" y="105" text-anchor="middle" fill="#fff">warning</text>
<path class="flow" d="M 116 100 H 138"/><path fill="var(--ac)" d="M 138 96 L 145 100 L 138 104 Z"/><text class="mono tx" x="146" y="105">exit 0</text><text class="mono mut" x="212" y="105" font-size="12">or 1 with --fail-on-warning</text>
<rect x="20" y="130" width="90" height="28" rx="14" fill="#3b82f6"/><text class="ui" x="65" y="149" text-anchor="middle" fill="#fff">info</text>
<path class="flow" d="M 116 144 H 138"/><path fill="var(--ac)" d="M 138 140 L 145 144 L 138 148 Z"/><text class="mono tx" x="146" y="149">exit 0</text><text class="mono mut" x="212" y="149" font-size="12">reported only</text>
<rect x="20" y="174" width="90" height="28" rx="14" fill="#94a3b8"/><text class="ui" x="65" y="193" text-anchor="middle" fill="#fff">off</text>
<path class="flow" d="M 116 188 H 138"/><path fill="var(--ac)" d="M 138 184 L 145 188 L 138 192 Z"/><text class="mono mut" x="146" y="193">skipped</text><text class="mono mut" x="230" y="193" font-size="12">rule never runs</text>
<line x1="20" y1="224" x2="440" y2="224" stroke="var(--bd)" stroke-width="1" opacity=".5"/>
<text class="mono mut" x="20" y="250" font-size="12">exit 2 = bad config or usage</text>
<text class="mono mut" x="20" y="272" font-size="12">exit 3 = internal error</text>
</svg>

## The four levels

- **`error`** is a hard failure. Any error-level violation makes `alint check` exit non-zero, which is what fails a CI job or blocks a commit.
- **`warning`** is reported but does not fail the run by default. Pass `--fail-on-warning` to promote warnings into failures when you want a stricter gate.
- **`info`** is advisory. It shows in the report and never affects the exit code.
- **`off`** disables the rule entirely: it is dropped at config load and never runs. Setting `level: off` on an inherited rule is how you switch off something a ruleset you `extends:` turned on.

## The exit codes

`alint check` returns one of four codes, so a pipeline can tell "clean" from "found problems" from "misconfigured":

- **`0`** clean: no errors, and no warnings under `--fail-on-warning`.
- **`1`** findings: at least one error, or a warning while `--fail-on-warning` is set.
- **`2`** a bad config or bad usage (an unknown field, a malformed `.alint.yml`, an invalid flag).
- **`3`** an internal error (a bug). Distinct from `2` so CI can tell "your config is wrong" apart from "alint fell over."

## In practice

A CI step that blocks on errors is just `alint check` (a non-zero exit fails the job). To also block on warnings during a hardening push:

```
alint check --fail-on-warning
```

And to keep a rule in the config but stop it from firing, without deleting it:

```yaml
rules:
  - id: legacy-header
    level: off      # inherited from a ruleset; silenced here
```

## Going deeper

- [The config model](/docs/concepts/the-config-model/) covers where `level` sits in the rule record and how a child config overrides it.
- [How alint works](/docs/concepts/how-it-works/) shows where the report and its exit code are produced.
