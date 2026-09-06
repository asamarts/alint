---
title: The agent surface
description: "How alint feeds a coding agent: the agent output format with per-violation fix_command, export-agents-md writing an AGENTS.md section, and the bundled agentic rulesets."
sidebar:
  order: 14
---

alint treats a coding agent as a first-class consumer. Your one rule set drives two feeds: `export-agents-md` writes the rules into an `AGENTS.md` section the agent reads at the start of a session, and `alint check --format agent` emits each violation as machine-actionable JSON with an exact `fix_command`. The rules become both the agent's standing instructions and its fix loop.

<svg class="alint-agent" viewBox="0 0 460 320" role="img" aria-labelledby="ag-t ag-d" xmlns="http://www.w3.org/2000/svg">
<title id="ag-t">One rule set feeds an agent as an AGENTS.md section and as per-violation fix commands</title>
<desc id="ag-d">Active rules feed two paths. export-agents-md writes an AGENTS.md section the agent reads at session start; alint check --format agent emits per-violation JSON with agent_instruction and fix_command. Both reach the coding agent.</desc>
<style>
  .alint-agent { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-agent { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-agent { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-agent .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-agent .tag { font:600 11px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-agent .tx { fill:var(--tx); } .alint-agent .mut { fill:var(--mut); } .alint-agent .ac { fill:var(--ac); }
  .alint-agent .card { fill:var(--card); stroke:var(--bd); stroke-width:1.3; }
  .alint-agent .key { fill:var(--card); stroke:var(--ac); stroke-width:1.7; }
  .alint-agent .flow { fill:none; stroke:var(--ac); stroke-width:2; stroke-dasharray:6 6; opacity:.7; animation:agflow 1s linear infinite; }
  @keyframes agflow { to { stroke-dashoffset:-12; } }
  @media (prefers-reduced-motion:reduce){ .alint-agent .flow{animation:none;stroke-dasharray:none} }
</style>
<rect class="key" x="160" y="20" width="140" height="28" rx="8"/><text class="tag ac" x="230" y="38" text-anchor="middle">active rules</text>
<path class="flow" d="M 200 48 C 160 62, 120 62, 120 76"/>
<path class="flow" d="M 260 48 C 300 62, 340 62, 340 76"/>
<rect class="card" x="20" y="78" width="200" height="30" rx="7"/><text class="tag tx" x="120" y="97" text-anchor="middle">export-agents-md</text>
<rect class="card" x="240" y="78" width="200" height="30" rx="7"/><text class="tag tx" x="340" y="97" text-anchor="middle">check --format agent</text>
<path class="flow" d="M 120 108 V 130"/>
<path class="flow" d="M 340 108 V 130"/>
<rect class="card" x="34" y="132" width="172" height="46" rx="7"/><text class="tag tx" x="120" y="151" text-anchor="middle">AGENTS.md</text><text class="tag mut" x="120" y="168" text-anchor="middle">read at session start</text>
<rect class="card" x="254" y="132" width="172" height="46" rx="7"/><text class="tag tx" x="340" y="151" text-anchor="middle">agent_instruction</text><text class="tag ac" x="340" y="168" text-anchor="middle">+ fix_command</text>
<path class="flow" d="M 120 178 C 120 210, 210 210, 230 220"/>
<path class="flow" d="M 340 178 C 340 210, 250 210, 230 220"/>
<rect class="key" x="120" y="222" width="220" height="46" rx="10"/><text class="ui ac" x="230" y="242" text-anchor="middle">the coding agent</text><text class="tag mut" x="230" y="259" text-anchor="middle">reads the rules, runs each fix_command</text>
<text class="tag mut" x="230" y="298" text-anchor="middle">the rules are the agent's instructions and its fixes</text>
</svg>

## The `agent` output format

`alint check --format agent` (and `alint fix --format agent`) emits one JSON envelope with a `format`, a `summary`, and a `violations` array. Each violation carries an **`agent_instruction`**, a templated English sentence of the shape `"<severity>: <message>. To resolve: edit <path>:<line>:<col>..."`, and, when the rule is auto-fixable, a **`fix_command`**: the exact argv an agent should run, such as `["alint", "fix", "--only", "readme-exists"]`. Splitting the machine command out of the English means an agent never has to parse prose to act. The format is read-only output; it never runs anything itself.

## `export-agents-md`

`alint export-agents-md` renders the **active** rule set into an `AGENTS.md` section (or JSON, parallel to `suggest`). Because agents read `AGENTS.md` at the start of a session, this makes the rules the agent's standing instructions: the same config that gates CI also tells the agent what "good" looks like before it writes a line. Regenerate it whenever the rules change so the two never drift.

## Bundled agentic rulesets

alint also ships bundled rulesets from the agentic era (v0.6+) that lint the agent-instruction files themselves, checking that an `AGENTS.md` or `CLAUDE.md` is present and shaped the way your team expects. They compose with the rest of your config through `extends:` like any other bundled ruleset.

## In practice

Run alint as an agent would, asking for the machine feed:

```bash
alint check --format agent
```

Each fixable violation arrives with a ready-to-run command:

```json
{
  "rule": "readme-exists",
  "level": "error",
  "agent_instruction": "error: README.md is required. To resolve: create it, or run `alint fix --only readme-exists`.",
  "fix_command": ["alint", "fix", "--only", "readme-exists"]
}
```

The agent runs `fix_command`, and the loop closes without it ever reading human output.

## Going deeper

- [Fixing](/docs/concepts/fixing/) is what a `fix_command` invokes under the hood.
- [Configuration](/docs/configuration/) covers the output-format flag and `extends:` for the bundled rulesets.
- [Rules](/docs/rules/) lists the agentic-era bundled rulesets and their kinds.
