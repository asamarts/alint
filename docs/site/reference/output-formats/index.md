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

## Streams: report on stdout, diagnostics on stderr

A machine format is the *only* thing written to **stdout** — so `alint check --format json > report.json` (or `sarif` / `gitlab` / `github` / `junit` / `agent`) captures byte-clean output with nothing to strip. Progress and any diagnostic warning (for example an empty `include_manifest_paths` set) are written to **stderr**. Redirect it separately (`2> alint.log`) or discard it (`2>/dev/null`) without touching the report. (In the `human` format, the `Summary` footer is part of the human report and stays on stdout with the findings.)

## Stable fingerprints

`sarif` and `gitlab` attach a stable per-finding fingerprint (SARIF `partialFingerprints`, GitLab `fingerprint`) to **every** run — not only when a `--baseline` is active. SARIF's is the canonical `violation_fingerprint`, the same identity the [baseline](/docs/concepts/baseline/) file records, so an alert keeps one identity across SARIF and the baseline. A finding with a unique fingerprint carries that same identity in GitLab too; GitLab additionally disambiguates genuine within-report duplicates (two findings with byte-identical content), because GitLab Code Quality drops entries that share a fingerprint. GitHub Code Scanning uses the SARIF fingerprint to correlate alerts across runs — dedupe, and track a finding as fixed or reopened — with no `--baseline` required.

## Baseline suppression

When a run is filtered through a [baseline](/docs/concepts/baseline/), suppression *marks* findings rather than deleting them, and only two formats surface those marks:

- **`sarif`** carries `suppressions: [{ "kind": "external" }]` and `baselineState` (`unchanged` for suppressed, `new` for live) on each result — so GitHub Code Scanning keeps grandfathered alerts open-but-dismissed instead of flapping fixed-then-reopened. (The stable `partialFingerprints` identity is emitted on every run, baseline or not — see above.)
- **`json`** omits suppressed findings from `results` and records a `summary.baselined_suppressed` count in the envelope.

The other six formats receive the already-filtered live report and are baseline-oblivious. The global `--show-baselined` flag lists the suppressed findings in full, in any format; the exit code is gated on the live (new) findings only, in every format.
