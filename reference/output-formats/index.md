---
title: Output formats
description: "alint check renders findings in eight output formats (human, json, sarif, github, markdown, junit, gitlab, agent). How to pick one and what each is for."
sidebar:
  label: Overview
  order: 1
---

`alint check` renders the same findings in eight output formats. Select one with `--format <name>`.

A single `Report` fans out to each format:

<likec4-view view-id="outputFormats"></likec4-view>

| Format | Aliases | For |
| --- | --- | --- |
| `human` | `pretty`, `text` | The default. Colorized, grouped by file, for reading in a terminal. |
| `json` | | Stable machine shape behind `schema_version: 1`. General-purpose integration. |
| `sarif` | | SARIF 2.1.0, for GitHub code scanning and other SARIF consumers. |
| `github` | `github-actions` | GitHub Actions workflow commands (inline annotations on a run). |
| `markdown` | `md` | GitHub-flavored Markdown, for posting as a PR comment. |
| `junit` | `junit-xml` | JUnit XML, for CI test-report viewers. |
| `gitlab` | `gitlab-codequality`, `code-quality` | GitLab Code Quality report. |
| [`agent`](/docs/reference/output-formats/agent/) | `agentic`, `ai` | LLM-shaped JSON with a templated `agent_instruction` per violation. |

Each format is shown by example in the [quickstart](/docs/getting-started/quickstart/). The [`agent` format](/docs/reference/output-formats/agent/) has its own reference because its shape is purpose-built for AI coding agents.

## Baseline suppression

When a run is filtered through a [baseline](/docs/concepts/baseline/), suppression *marks* findings rather than deleting them, and only two formats surface those marks:

- **`sarif`** carries `suppressions: [{ "kind": "external" }]` and `baselineState` (`unchanged` for suppressed, `new` for live) on each result, plus `partialFingerprints` — so GitHub Code Scanning keeps grandfathered alerts open-but-dismissed instead of flapping fixed-then-reopened.
- **`json`** omits suppressed findings from `results` and records a `summary.baselined_suppressed` count in the envelope.

The other six formats receive the already-filtered live report and are baseline-oblivious. The global `--show-baselined` flag lists the suppressed findings in full, in any format; the exit code is gated on the live (new) findings only, in every format.
