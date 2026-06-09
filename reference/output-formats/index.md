---
title: Output formats
description: "alint check renders findings in eight output formats (human, json, sarif, github, markdown, junit, gitlab, agent). How to pick one and what each is for."
sidebar:
  label: Overview
  order: 1
---

`alint check` renders the same findings in eight output formats. Select one with `--format <name>`.

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
