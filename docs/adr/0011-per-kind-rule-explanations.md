---
status: accepted
date: 2026-08-06
decision-makers: asamarts
---

# 0011. Per-kind rule explanations via a generated summary bridge

## Status

Accepted (2026-08-09, after an adversarial audit of the original scaffold). The
Tier 2 follow-up to the `alint explain` enrichment: Tier 1 surfaced a rule's
config-instance detail (kind, paths, message, `when:`, fix); this closes the
remaining gap, kind-level explanation. The audit confirmed the decision and every
cross-reference, and corrected the scaffold's one overstated claim (that
`first_sentence` yields a terminal-ready summary as-is) - see Decision, the
extraction bullet.

## Context

`alint explain <rule>` and the `alint rules` catalog answer questions about a
rule, but neither can answer the most basic one: **what does this KIND check, and
why.** After the Tier 1 change, `explain` shows a rule's kind, paths, message,
`when:`, and fix (all config-instance data). It still cannot say what
`file_content_forbidden` *is*, because that prose exists only in `docs/rules.md`.

`docs/rules.md` is the single source of truth for per-kind prose.
`docs_export::first_sentence` extracts the opening sentence of each kind's prose,
but strictly for the WEBSITE, where every consumer post-processes it: family
tables run `ascii_dashes` + `escape_table_cell`, the SEO meta description runs
`meta_desc_clean`, and the master index renders as markdown. None of it is
compiled into the binary, and none of it is terminal-ready plaintext. The in-crate
kind-to-category bridge (`crates/alint-rules/src/categories_gen.rs`) deliberately
carries categories and **not** summaries; that omission is a documented deferral
in `docs/design/rule-categories.md` (the deferred-summary note) and ADR-0009's
Data-source section. So the terminal has no offline, config-independent way to
describe a kind, and `alint rules show <kind>` plus `list --search` over summaries
(both deferred in ADR-0009) have no data to read.

## Decision

We will compile a **per-kind one-line summary into the binary via a generated
bridge**, sourced from the `docs/rules.md` SSOT and drift-gated so it can never
diverge from the docs. Concretely:

- **A sibling generated artifact, `crates/alint-rules/src/kind_docs_gen.rs`**,
  emitted by the existing `gen-categories` pass (which already parses every H3
  body via `split_categories_line`), keyed by canonical kind. Aliases inherit
  their canonical kind's summary through the existing `ALIAS_TO_CANONICAL` map (no
  per-alias storage), exactly as `categories_for_kind` already resolves them. It
  is a SEPARATE file from `categories_gen.rs`, not a new field on it: summaries
  are free prose that churn on ordinary doc edits, whereas category associations
  are near-static, and summaries need a prose-cleaning gate that the typed
  category validator does not (see Consequences).

- **The extraction CLEANS and CAPS `first_sentence`; it must not compile the raw
  output.** The raw first sentence is website-source: across the 70 rule-kind H3
  sections it carries inline backticks (46), em/en-dashes (13), `**bold**` (5), is
  uncapped (up to 359 chars for `pair_hash`), and its naive first-`". "` split
  truncates an opening clause containing `e.g.`/`i.e.` (`no_case_conflicts` becomes
  "...differ only by case (e.g."). So the generator routes `first_sentence` through
  a terminal-cleaning pass modeled on the existing `meta_desc_clean` (strip
  backticks and `**bold**`, ASCII-fold or drop em/en-dashes, collapse whitespace),
  a hard length cap (target ~100 chars with sentence-aware backoff), and a splitter
  guard so an `e.g.`/`i.e.`/`vs.` abbreviation does not end the sentence early. The
  `--check` gate asserts the CLEANED, CAPPED artifact byte-for-byte.

- **`alint explain` prints a `summary:` line under its `kind:` line** and a `docs:`
  deep link to `https://alint.org/docs/rules/<family>/<kind>/` for the full prose.
  The `<family>` segment is DERIVED in-crate as `categories_for_kind(kind)[0]` (the
  primary category slug, which `gen-categories` guarantees equals the kind's family
  URL segment) - no new family field is needed. The `docs:` line honors `--no-docs`,
  exactly as the existing `policy_url` line does.

- **`alint rules` gains a per-kind description and `rules show <kind>`.** `rules
  list` renders each kind's summary on an indented line under it; a new
  `RulesCommand::Show` prints a single kind's summary plus its deep link (honoring
  `--no-docs` like `explain`); and `list --search` extends its existing name/alias
  match to also match summary text - closing the ADR-0009 catalog deferrals with
  one data source.

- **Drift gate.** A `gen-<x> --check` step in the docs CI job (and under `cargo
  test`, like the others) fails if the committed `kind_docs_gen.rs` is stale,
  byte-for-byte, exactly like `gen-facts --check` and `gen-categories --check`:
  build in memory, compare to the committed file, `bail!` with the regen command.

Kinds sharing a multi-kind H3 heading (the `*_path_equals` / `*_path_matches`
quartets, `file_starts_with` / `file_ends_with`) share one summary; that is correct
(one prose sentence describes the group) and expected, not a defect.

## Consequences

Easier:

- `explain` becomes kind-aware: a user learns what a kind checks without leaving
  the terminal or reading YAML.
- The `rules` catalog gains descriptions and summary search - the ADR-0009 catalog
  deferrals resolve with one data source.
- Summaries stay honest: one SSOT (`docs/rules.md`), one extractor, one gate.

Harder, and accepted:

- **Doc-prose edits leave the docs-only CI fast lane.** Editing a kind's opening
  sentence in `docs/rules.md` now regenerates `kind_docs_gen.rs`, so the change is
  no longer docs-only and must ship in a release to reach CLI users (the site
  reflects it sooner). This is the exact tradeoff ADR-0009 named and deferred; we
  accept it, scoped to this artifact. The scope is fuzzier than "first sentence
  only" - with no cap, `first_sentence` can pull a long opening clause into the
  regen-triggering window - which is a further reason the length cap above matters,
  and why this artifact stays SEPARATE from the near-static category bridge (a
  category-only edit never regenerates the prose artifact, nor the reverse).
- A prose-cleaning pass + length cap are new code (small, modeled on
  `meta_desc_clean`), and one more generated artifact + `--check` gate to maintain,
  alongside facts.json and the categories bridge.

## Considered Options

- **Generated summary bridge from `docs/rules.md` (chosen).** One SSOT, reuses
  `first_sentence` behind a cleaning + cap pass, drift-gated, and unlocks the
  catalog deferrals.
- **Fold summaries into `categories_gen.rs` (rejected sub-option).** Fewer files,
  but drags the near-static category bridge into prose-churn regeneration and
  entangles a free-prose cleaning gate with the typed category validator; a sibling
  `kind_docs_gen.rs` isolates the accepted cost where ADR-0009 wanted it.
- **Leave summaries in docs only; `explain` just deep-links out.** No new artifact,
  but no offline/terminal kind description and no summary search - the status quo
  this ADR exists to end.
- **Hand-write a summary table in Rust.** Immediate, but a second source of truth
  that silently drifts from `docs/rules.md` - the drift class alint itself exists
  to prevent.
- **Compile each kind's full prose body into the binary.** Real kind help in the
  terminal, but tens of KB of doc prose in the binary and a large churn surface;
  the one-line summary is the 90% at a fraction of the cost.

## More Information

- Builds on the Tier 1 `explain` enrichment (config-instance detail) and its
  completeness gate (`explain_surfaces_configured_rule_detail`).
- The deferral this closes: `docs/design/rule-categories.md` (the deferred-summary
  note) and ADR-0009 (rule discovery; the `rules show` / `list --search` deferrals
  and the CI fast-lane tradeoff).
- Reused machinery: `docs_export::first_sentence` (extractor) and `meta_desc_clean`
  (the cleaning/cap model), the `gen-categories` parse pass (`categories.rs`), the
  `ALIAS_TO_CANONICAL` map + `categories_for_kind` (alias resolution + the family
  derivation), and `categories_gen.rs` (the generated-bridge + `--check` pattern).
- Every canonical kind has prose in `docs/rules.md` (enforced by
  `gen-categories`'s registry validation and docs-export's undocumented-kind gate),
  so the bridge is never missing a summary.
