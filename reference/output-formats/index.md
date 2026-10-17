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

## Non-violation notes

Rules can emit informational notes when analysis skips an input without asserting a policy
failure. Notes do not affect pass/fail, severity counts, baseline suppression, fixes, or the exit
code. A note-only report therefore exits successfully.

| Format | Note projection |
| --- | --- |
| `human` | Writes the note count to stderr. `--show-notes` lists each note there. |
| `json` | Preserves the full per-rule `notes` array, including path, position, message, and structural `kind` (`informational` or `analysis_truncated`). The current emitter always includes `kind`; its absence in an older v1 report means `informational`. |
| `sarif` | Emits informational notes as `level: note` results. Emits `analysis_truncated` notes as `toolExecutionNotifications`, not policy results. |
| `github` | Emits one unlocated `notice` with the count and full-list pointer. |
| `gitlab` | Emits one valid Code Quality `info` issue with the count and full-list pointer. |
| `markdown` | Emits one blockquote with the count and full-list pointer. |
| `junit` | Emits one suite-level `system-out` line; notes add no tests, failures, or errors. |
| `agent` | Adds an optional top-level `notes` object containing `count` and `details_format: "json"`. |

Count-only formats use the exact pointer: `rerun the same check with --format json for the full
list.` Rerun the original command with that format so its config, path, `--only`, and `--changed`
arguments are preserved. Summary records never include repository-controlled note bodies.

## Analysis failures

An analysis failure means alint could not obtain the configured policy verdict: evaluation,
runtime-condition, resource-read, resource-parse, and repository-derived nested-rule build
failures are tracked separately from violations. They always make the run fail with exit `1`,
even when the owning rule is `warning`, `info`, or `off`. They have no canonical or baseline
fingerprint, cannot be baselined, and are never offered to a fixer; `alint baseline` refuses to
write while analysis is incomplete.

| Format | Analysis-failure projection |
| --- | --- |
| `human` | Error-styled `analysis failure` entry grouped by resource path. |
| `json` | Full per-rule `analysis_failures` records plus `summary.total_analysis_failures`. |
| `sarif` | An error result with `kind: "fail"` and `properties.analysisFailure`; the invocation has `executionSuccessful: false`. |
| `github` | An `error` annotation retaining path and position when available. |
| `gitlab` | A `critical` Code Quality issue with the failure class in its description and a deterministic schema-required transport fingerprint (not a baseline identity). |
| `markdown` | An error row labeled `analysis failure`. |
| `junit` | A testcase with `<error type="analysis_failure">`, distinct from a policy `<failure>`. |
| `agent` | A non-remediation `analysis_failures` record with no instruction, fix command, or edit. |

Violation and severity counts continue to count policy violations only. JSON, agent, and fix
reports expose failures through dedicated arrays and a separate total, so consumers do not need
to parse English messages. Fix runs preserve failures and exit `1` under `--fix-only`,
`--dry-run`, `--diff`, and `--baseline`; non-convergence retains its higher-priority exit `2`.

## Primary ranges and related locations

Findings may identify a half-open primary source range and labeled secondary locations. Existing
single `line` / `column` points remain valid and are used when no primary range is available.
Relationship rules use this metadata to keep their established report anchor while also pointing
to the source, target, partner, or colliding member that explains the failure.

- **`human`** lists each related label and path below the finding. A finding retains at most 20
  related locations; if producers supplied more, the output reports the omitted count.
- **`sarif`** projects the primary range into the result's first physical location and emits the
  secondary locations in `relatedLocations`, including regions when known. SARIF columns use
  Unicode code points.
- **LSP diagnostics** project the primary range into `Diagnostic.range` and related locations into
  `Diagnostic.relatedInformation`, with UTF-16 columns. alint sends related information only when
  the client advertises `publishDiagnostics.relatedInformation`.

The other check-format payloads do not gain related-location fields in v0.19. In particular, the
JSON report remains `schema_version: 1` with its previous emitted shape. Its schema now permits
additive fields so consumers can adopt the structured fields in the next release without an
atomic schema migration; required fields and known-field types remain enforced.

## Stable fingerprints

In `sarif`, every result has a location: a finding that isn't tied to one file (a missing `LICENSE`, a tree-level rule) is anchored on line 1 of the config file that loaded the rule, since GitHub Code Scanning drops results without a location. The run declares `columnKind: "unicodeCodePoints"`, because alint counts columns in Unicode characters (in fix regions too), and artifact URIs percent-encode `:` so a file like `a:b.txt` isn't read as a URI scheme. On Unix, URI encoding uses the original filename bytes, so distinct non-UTF-8 paths cannot alias. JSON and agent reports omit the optional `proposed_edit` field when an edit path has no exact JSON string representation; their safe `fix_command` remains available.

`sarif` and `gitlab` attach a stable per-finding fingerprint (SARIF `partialFingerprints`, GitLab `fingerprint`) to every **policy-violation** result — not only when a `--baseline` is active. SARIF's is the canonical `violation_fingerprint`, the same identity the [baseline](/docs/concepts/adoption/baseline/) file records, so an alert keeps one identity across SARIF and the baseline. A finding with a unique fingerprint carries that same identity in GitLab too; GitLab additionally disambiguates genuine within-report duplicates (two findings with byte-identical content), because GitLab Code Quality drops entries that share a fingerprint. GitLab analysis-failure issues receive only the deterministic transport identity its schema requires; SARIF analysis failures have no `partialFingerprints`. GitHub Code Scanning uses policy findings' SARIF fingerprints to correlate alerts across runs — dedupe, and track a finding as fixed or reopened — with no `--baseline` required.

## Baseline suppression

When a run is filtered through a [baseline](/docs/concepts/adoption/baseline/), suppression *marks* findings rather than deleting them, and only two formats surface those marks:

- **`sarif`** carries `suppressions: [{ "kind": "external" }]` and `baselineState` (`unchanged` for suppressed, `new` for live) on each result — so GitHub Code Scanning keeps grandfathered alerts open-but-dismissed instead of flapping fixed-then-reopened. (The stable `partialFingerprints` identity is emitted on every run, baseline or not — see above.)
- **`json`** omits suppressed findings from `results` and records a `summary.baselined_suppressed` count in the envelope.

The other six formats receive the already-filtered live report and are baseline-oblivious. The global `--show-baselined` flag lists the suppressed findings in full, in any format; the exit code is gated on the live (new) findings only, in every format.
