# v0.19: Fail-loud findings and the capability inventory

Status: Planned (2026-10-14). This is the first release of the general-linter program: Phase 0
and Phase 1a of [`../general-linter.md`](../general-linter.md) section 11, decided in
[ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md). Each work item
below gets its own design document in this directory (in the [`../TEMPLATE.md`](../TEMPLATE.md)
shape, including the false-positive section) before its code lands, and the item's open
questions are decided there, one at a time.

v0.19 is a minor release with deliberate behaviour changes (W2 and W3 turn some false passes into
failures, and W4 changes report text). Every behaviour change is listed in the changelog with
its migration note.

## Work items

| ID | Item | Decisions | Size | Depends on |
|---|---|---|---|---|
| W1 | `KindMeta` capability inventory | Phase 0 | S | none |
| W2 | Analysis failures become error findings | DEC-2 | M | W1 (for the reject-at-load half of W3) |
| W3 | Close the remaining fail-open sites | section 4.12 | M | W1, W2 |
| W4 | Notes in every output format | section 8.1 | S | none |
| W5 | Evidence redaction and the leak gate | DEC-3 | M | none |
| W6 | Related locations and ranges on findings | DEC-4, DEC-5 | L | W4 |
| W7 | `--as-of` input | DEC-11 | S | none |
| W8 | Duplicate-aware spanned extraction (substrate) | Q5 | M | W6 (span model) |
| W9 | Standalone detection kinds, as capacity allows | section 11.8 | per kind | W1 |
| W10 | Documentation, recipes, and roadmap updates | n/a | S | all |

Suggested order: W1, W4, W7 (small, independent) first; then W2 and W3; W5 alongside; W6, then
W8; W9 interleaved; W10 last.

### W1. `KindMeta` capability inventory

- Add a static `KindMeta` declared at registration for every kind: resource domain, dispatch
  class, accepted selectors (`paths`, `scope_filter`, `git_tracked_only`, change selectors),
  capabilities (spawn, Git, environment, time, host keyring), formats, and fix operations.
- Emit it into `facts.json` (a new section; the facts gate covers it).
- Derive the spawn and Git trust lists (`SPAWNING_RULE_KINDS`, the Git-required kind list) from
  it, keeping the existing audits as cross-checks during the transition.
- Add a coverage audit: every scenario-built rule instance behaves within its declared
  `KindMeta` (for example, a kind that declares no spawn capability never spawns).

Acceptance: `KindMeta` exists for all 95 kinds and 11 aliases; trust lists derive from it; the
new audit and the facts gate pass; no change to any rule's behaviour.

### W2. Analysis failures become error findings (DEC-2)

- Separate "the rule could not evaluate this resource" from violations in the engine's result
  type, at every site that today wraps an `Err` as `Violation::new("rule error: ...")`.
- A failure caused by repository content is reported as a finding that always counts as error,
  regardless of the rule's `level`, with a stable message naming the resource and the reason.
- `alint baseline` refuses to record such findings, and an existing baseline cannot suppress
  them.
- Configuration failures keep exit 2 and internal errors keep exit 3; no new exit code.
- Every format renders them (an ADR-0012 positive-completeness gate).

Acceptance: a warning-level rule over an unparseable target exits 1; `alint baseline` does not
record the failure; scenarios cover each former "rule error" site; changelog entry under
"Changed".

### W3. Close the remaining fail-open sites

From [`../general-linter.md`](../general-linter.md) section 4.12:

- **`scope_filter` on kinds that ignore it** (the `git_commit_*` family, `git_no_denied_paths`,
  `generated_file_fresh`, `command_idempotent`) is rejected at load and by `validate-config`,
  driven by W1; `include_manifest_paths` on such a rule fails in `validate-config`, not only at
  `check` time.
- **Manifest-derived scope parse failure** becomes an analysis failure (W2) instead of an empty
  member set or a silent fallback to full scope.
- **Git unavailable**: Git-aware kinds report an analysis failure when Git or the repository is
  unavailable, instead of no findings.
- **Non-string extraction results**: a typed value at a queried path is either compared by its
  lexeme or reported, never silently dropped (the exact rule is decided in this item's design
  document).

Acceptance: each site has firing scenarios proving the new outcome and a gate asserting the
invariant; changelog entries under "Changed".

### W4. Notes in every output format

- SARIF: notes become `level: note` results; truncated analysis becomes
  `toolExecutionNotifications`.
- GitHub, GitLab, and Markdown outputs: a summary line with the note count and a pointer to the
  full list; JUnit and agent formats get an equivalent.
- Human output keeps `--show-notes`.

Acceptance: an ADR-0012 positive-completeness gate proves a note reaches every format.

### W5. Evidence redaction and the leak gate (DEC-3)

- Values extracted from dotenv and properties files are redacted by default in every message
  and format (rendered as a short marker plus a digest).
- A new `sensitive: true` rule option opts any other rule into redaction.
- Baseline files never store raw values: the advisory message is redacted, and the fingerprint
  remains a hash of the identity. The design document must address that an unsalted hash of a
  low-entropy value can be brute-forced (for example, by hashing with a per-repository salt or
  by excluding redacted values from the fingerprint input).
- A leak gate: a scenario plants a known secret in `.env` and asserts its bytes appear in no
  output format and in no baseline file.

Acceptance: the leak gate passes for all eight formats and the baseline; existing fingerprints
are unchanged for non-redacted rules (W6's golden gate).

### W6. Related locations and ranges on findings (DEC-4, DEC-5)

- Extend the finding model with a primary range and labeled related locations, with deterministic
  ordering and caps (at most a fixed number of related locations, then "and N more").
- Primary-location rule: the rule's declared report anchor, else the non-canonical side, else the
  first in canonical order; existing kinds keep today's anchor.
- Emit related locations in SARIF (`relatedLocations`) and the LSP (`relatedInformation`, only when
  the client advertises support) in v0.19. Relax `schemas/v1/check-report.json`'s
  `additionalProperties: false` in v0.19, announced in the changelog; the JSON report gains the
  new fields in the next release (DEC-4).
- Freeze baseline fingerprints: a golden file of every fingerprint over the whole scenario corpus,
  recomputed in CI; related locations never enter a fingerprint; the baseline schema stays at
  version 1 (DEC-5).
- Populate related locations first for `cross_file`, `registry_paths_resolve`, `unique_by`,
  `pair`, and `file_graph`.

Acceptance: the golden fingerprint gate is green with zero changes; SARIF and LSP gates prove
related locations reach both; the JSON schema relaxation ships with its changelog notice.

### W7. `--as-of` input (DEC-11)

- A global `--as-of <date>` flag and an `ALINT_AS_OF` environment variable.
- Without either, time-dependent kinds use the HEAD commit time; outside a Git repository they
  report an analysis failure (W2) unless `--as-of` is given.
- `git_blame_age` moves onto it; the reference time is shown in `explain` and in its findings.
- A superseding ADR records the time input in constitution invariant 1.

Acceptance: the same commit yields byte-identical `git_blame_age` output on different days; the
ADR is accepted; changelog entry under "Changed".

### W8. Duplicate-aware spanned extraction (substrate)

- Extraction gains ordered entry rows with spans for the formats whose parsers can provide them
  (dotenv through `adotenv`; JSON through the `jsonc-parser` AST; YAML, INI, and properties
  through the fix bridge's locators), instead of flattening to a last-wins object.
- This is the substrate for B1 `no_duplicate_keys` and B3 `structured_key_sort`
  ([`../rule-coverage-gaps.md`](../rule-coverage-gaps.md)); the kinds themselves may land in
  v0.19 or v0.20 depending on capacity.
- Question Q5 (how keys and duplicates are represented across the eight formats) is decided in
  this item's design document.

Acceptance: extraction exposes duplicates with spans for the listed formats; no change to
existing kinds' results.

### W9. Standalone detection kinds, as capacity allows

From the "ship now" bucket of [`../general-linter.md`](../general-linter.md) section 11.8, in
value order:

1. A1 `editorconfig_conforms`, after the policy-source-files ADR (DEC-12);
2. the Markdown structure kinds on the shipped CommonMark parser (E2 alt text, E3 required
   headings, E5 duplicate headings, E7 fragment resolution);
3. the SPDX id table with F2 `spdx_identifier_valid`;
4. H1 `pinned_references`, which also closes the reusable-workflow and composite-action holes in
   `ci/github-actions@v1`;
5. the C1 version-SSOT and C2 toolchain-pin bundled rulesets.

Each kind follows the normal cascade (rules.md section, schema, firing and silent scenarios,
rule-page fixtures, facts) and declares its `KindMeta`.

### W10. Documentation, recipes, and roadmap

- Document the flat key-parity recipe (`cross_file` `set_equals` over a `regex:` key extract for
  `.env` against `.env.example`).
- Update the README's counts only through `facts.json`; update ROADMAP's "Latest release" at
  release time.
- Close the remaining review follow-ups: the stale `register_as` doc comment in
  `crates/alint-rules/src/cross_file/spec.rs` (it lists only `{path}`, `{dir}`, `{stem}`).

## Parallel tracks during v0.19

- **Plugin contract compiler (P0).** The separate project of
  [`../plugin-architecture.md`](../plugin-architecture.md) section 6 starts in parallel as
  `witforge` (`asamarts/witforge`, private until alint's contract `0.x` consumes it; Q16,
  decided 2026-10-14).
- **Phase 1b design.** The design documents for `alint test` (DEC-6) and `explain --trace`
  (Q14) can be written during v0.19, for implementation in v0.20.

## Gates for the release

- every behaviour change listed in the changelog with a migration note;
- the frozen-fingerprint golden gate (W6) and the leak gate (W5) green;
- ADR-0012 completeness gates for notes, analysis failures, related locations, and redaction;
- the deterministic performance gate within the program budget (cumulative against v0.18.2,
  DEC-13), plus wall-clock benchmarks on the canonical bench host for W2, W3, and W6;
- the usual release flow in `RELEASING.md`, including the alint.org pin bump.
