---
title: The agent surface
description: "How alint feeds a coding agent: the agent output format with per-violation fix_command, export-agents-md writing an AGENTS.md section, and the two bundled agentic rulesets."
sidebar:
  order: 14
---

alint treats a coding agent as a first-class consumer. Your one rule set drives two feeds: `export-agents-md` renders the rules into an `AGENTS.md` section the agent reads at the start of a session, and `alint check --format agent` emits each violation as machine-actionable JSON with an exact `fix_command`. The rules become both the agent's standing instructions and its fix loop.

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

`alint check --format agent` emits one JSON envelope. It is a check-only format: `alint fix` accepts only `human`, `json`, or `markdown`, and rejects `--format agent`. The envelope has four top-level fields, stable behind `schema_version`:

- **`schema_version`** and a literal **`format: "agent"`**.
- **`summary`**: `total_violations`, `by_severity` (`error` / `warning` / `info`), `fixable_violations`, `passing_rules`, `failing_rules`.
- **`violations`**: a single flat array (unlike `--format json`, which nests by rule).

Each violation object carries `rule_id`, `severity`, the location (`file`, `line`, `column`, present only when the finding has one), `human_message` (the rule's message verbatim), an `agent_instruction` (a templated remediation sentence: `"<severity>: <message>. To resolve: edit <path>:<line>:<col>"`, or a repository-level phrasing for a path-less finding), `fix_available`, and, when fixable, a **`fix_command`**. That command is the argv **after** the `alint` program name, `["fix", "--only", "<rule-id>"]`, so an agent runs the fix without parsing English, and a CLI-parse test guarantees it only ever names flags the binary accepts.

## `export-agents-md`

`alint export-agents-md` renders the **active** rule set into a directive block, grouped by severity, shaped for an agent-instruction file. By default it prints to **stdout**; `--output <path>` writes a file; and `--inline` (the canonical workflow) splices the block between `<!-- alint:start -->` and `<!-- alint:end -->` markers in an existing file, creating them if absent, so re-running keeps just that section in sync and leaves the rest of the file untouched. `--section-title` sets the heading, `--include-info` adds info-level rules (excluded by default), and `--format json` emits the machine shape instead of the default markdown. Because agents read these files at the start of a session, the same config that gates CI becomes the agent's standing instructions.

## Bundled agentic rulesets

Two bundled rulesets target the agentic era, adopted through `extends:` like any other:

- **`alint://bundled/agent-context@v1`** lints the agent-instruction files themselves (`AGENTS.md`, `CLAUDE.md`, `.cursorrules`, `GEMINI.md`, `copilot-instructions.md`) for existence, stubs, bloat, and stale-path drift.
- **`alint://bundled/agent-hygiene@v1`** catches residue that is distinctly AI-shaped: versioned duplicate filenames, scratch-doc sprawl, AI-affirmation prose, debug residue, and model-attributed TODOs.

```yaml
version: 1
extends:
  - alint://bundled/agent-context@v1
  - alint://bundled/agent-hygiene@v1
```

## In practice

Run alint as an agent would, asking for the machine feed:

```bash
alint check --format agent
```

One fixable finding comes back as a flat object with a ready-to-run `fix_command`:

```json
{
  "schema_version": 1,
  "format": "agent",
  "summary": {
    "total_violations": 1,
    "by_severity": { "error": 0, "warning": 1, "info": 0 },
    "fixable_violations": 1,
    "passing_rules": 12,
    "failing_rules": 1
  },
  "violations": [
    {
      "rule_id": "no-trailing-whitespace",
      "severity": "warning",
      "file": "src/app.rs",
      "line": 12,
      "column": 80,
      "human_message": "trailing whitespace",
      "agent_instruction": "warning: trailing whitespace. To resolve: edit src/app.rs:12:80 — or run `alint fix --only no-trailing-whitespace` to apply the auto-fix",
      "fix_available": true,
      "fix_command": ["fix", "--only", "no-trailing-whitespace"]
    }
  ]
}
```

The agent runs `fix_command` and the loop closes without it ever reading human output. To keep the standing instructions current, splice them on every rules change:

```bash
alint export-agents-md --inline --output AGENTS.md
```

## Going deeper

- [Fixing](/docs/concepts/fixing/) is what a `fix_command` invokes under the hood.
- [Configuration](/docs/configuration/) covers the output-format flag and `extends:` for the bundled rulesets.
- [Rules](/docs/rules/) lists the `agent-context` and `agent-hygiene` rulesets and their kinds.
