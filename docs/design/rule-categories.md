# Many-to-many rule categories

Status: proposed (2026-07-07). CLI decision recorded in ADR-0009; this doc is the
full feature design. Authoring/gating pattern follows `spec-driven-development.md`;
the contract shape follows `facts-json.md`.

## Goal

Let a rule kind belong to more than one category, so users discover rules the way
they think about them. `no_bidi_controls` is both an Encoding rule and a Security
rule; `git_commit_gpg_signed` is both Git hygiene and Security; `filename_case` is
both Naming and Structure. Today each kind sits in exactly one family, which hides
these cross-cutting relationships on the site and in the CLI.

The change is many-to-many membership, a single source of truth, generated
everywhere, and gated against drift. The category vocabulary itself stays the current
set of families.

## The current model, and why it constrains us

There is no structured kind-to-category source anywhere in the codebase. "Family" is
encoded purely as physical placement: a kind's `###` heading sits under one `##`
family heading in `docs/rules.md`, and every consumer re-derives family from that one
fact.

- `xtask/src/facts.rs` `families()` = the non-meta `## ` headings; the `families`
  list and count in `facts.json` come from there.
- `xtask/src/docs_export.rs` slices `rules.md` H2 -> H3 into
  `rules/<family>/<kind>.md` pages plus a per-family index, and returns a singular
  `kind -> family-slug` map.
- `xtask/src/gen_model.rs` walks the same H2 -> H3 to build `rule-families.gen.c4`.
- alint.org `src/lib/docs-catalogue.ts` derives the sidebar, the browse table, the
  `/api/rules.json` contract, and `llms.txt` from the on-disk `rules/<family>/<kind>`
  directory layout.

A markdown heading has exactly one parent and a file lives in exactly one directory.
That single-parent nesting is what makes the whole chain one-to-one. Many-to-many
therefore needs an explicit membership channel that does not depend on physical
nesting, while keeping one canonical page per kind so no URLs break.

The CLI is unaffected by family today: `alint list`/`explain` are config-scoped and
never mention family. Making categories reachable from the CLI is a net-new
capability (see ADR-0009).

## Design decisions

1. **Vocabulary as a typed enum.** A closed `Category` enum in `alint-core` defines
   the categories, with `title()`, `slug()`, `order()`, and `all()`. This gives
   compile-time closure: a typo or rename is a build error, and every consumer
   imports one definition. It is a closed enum with helper methods, in the style of
   the existing `Level` (in `alint-core`) and `Format` (in `alint-output`) enums, and
   defines its own `title`/`slug`/`order`/`all` accessors.

2. **Associations authored in `docs/rules.md`.** Each `###` kind heading gains a
   machine-readable `**Categories:**` line (reader-facing in the SOURCE `rules.md` for
   contributors; stripped from the published page body, see below). This keeps a single
   authoring source (rules.md is already the family SSOT and is already parsed by the
   whole pipeline) and keeps association edits in the docs-speed layer.

3. **A generated, committed in-crate bridge for the CLI.** `alint rules` needs
   categories at runtime, but rules.md is a docs-tree file the binary never ships. So
   `xtask gen-categories` reads the `**Categories:**` lines, validates each token
   against the `Category` enum, and writes a committed in-crate table that the engine
   reads. This is the same generate-plus-commit-plus-`--check` pattern `gen-schema`
   uses for its in-crate schema copy (`crates/alint-dsl/schemas/v1/config.json`).

4. **One canonical page per kind; secondary categories cross-list.** The primary
   family (the kind's H2 nesting) keeps owning the canonical URL
   `/docs/rules/<primary>/<kind>/`. No URL breaks, no redirects, no duplicate pages.
   Secondary categories cross-list: the secondary family's Overview page links to the
   kind's canonical page, and the kind's page shows all its categories.

5. **Sidebar stays primary-only.** In the docs tree nav a kind appears once, under
   its primary family. Cross-category discovery happens through the browse table, the
   family Overview pages, `alint rules`, and search. Repeating a kind under every
   family group in the sidebar was rejected as extra drift surface for little gain.

These carry one accepted cost, inherited from decision 3 and CLI-visibility
(ADR-0009): a category edit regenerates a committed in-crate artifact, so it leaves
the docs-only CI fast lane, and CLI users see new categories only at the next
release. The site still reflects edits sooner, straight from rules.md.

## Data model and flow

Single authoring source (`docs/rules.md`) fans out to every consumer, each gated:

```
docs/rules.md  (**Categories:** line per H3)          <- the only place a human edits
      |
      |  parsed + validated against Category enum (alint-core)
      v
  +---------------------------+   +--------------------------+   +----------------------+
  | gen-categories            |   | gen-facts                |   | docs-export          |
  | -> in-crate bridge (.rs)  |   | -> facts.json            |   | -> rule-page front-  |
  |    kind -> [Category]     |   |    rule_categories map   |   |    matter categories |
  |    + alias -> canonical   |   |    (families list = 13)  |   |    + family cross-   |
  +------------|--------------+   +------------|-------------+   |    listing pages     |
               |                               |                 +----------|-----------+
        engine / CLI                       alint.org  <-- docs-bundle sync ----+
        (alint rules,                      (browse table, /api/rules.json,
         list --category)                   llms.txt, per-kind cross-links)
```

`gen_model` also reads the rules.md tree, but it is deliberately NOT a Categories
consumer: its `rule-families.gen.c4` taxonomy stays primary-only (see gen-model below).

### The `Category` vocabulary (alint-core)

The 13 current families become enum variants. Title and slug match what the engine's
`slugify()` (`xtask/src/docs_export.rs`) already generates for family directory names,
and hence the site URLs, so nothing changes:

| Variant | title() | slug() |
|---|---|---|
| Existence | Existence | existence |
| Content | Content | content |
| StructuredQuery | Structured query | structured-query |
| Naming | Naming | naming |
| TextHygiene | Text hygiene | text-hygiene |
| SecurityUnicodeSanity | Security / Unicode sanity | security-unicode-sanity |
| Encoding | Encoding | encoding |
| Structure | Structure | structure |
| PortableMetadata | Portable metadata | portable-metadata |
| UnixMetadata | Unix metadata | unix-metadata |
| GitHygiene | Git hygiene | git-hygiene |
| CrossFile | Cross-file | cross-file |
| PluginTier1 | Plugin (tier 1) | plugin-tier-1 |

The variants are DECLARED in display order (the table above), which equals the rules.md
H2 sequence, and `order()` returns that position. The enum declaration order is the
single display-order source. `facts.json`'s `families` list is alphabetically sorted (a
`BTreeSet`), so it is NOT the display-order source; a gate asserts that the rules.md H2
sequence matches the enum order (see Gates). Adding or renaming a category is a
deliberate enum edit plus a title/slug review, not an accidental string.

### The association SSOT: the `**Categories:**` line

```markdown
### `no_bidi_controls`

**Categories:** Security / Unicode sanity, Encoding

<prose ...>
```

Rules:

- The line lists ALL categories the kind belongs to, by `title()`, comma-separated.
- The primary family is the H2 the heading sits under; it MUST be listed FIRST on the
  line. So the line is the complete membership set with the primary at position 0, and
  the derived `rule_categories` and bridge entries preserve that order. Consumers
  without the directory (the CLI, the `/api/rules.json` URL builder) treat position 0
  as the primary.
- Every token must resolve to a `Category` variant (validated at generation time).
- Single-membership kinds carry a one-entry line (their current family). Introducing
  the line with only current families is behavior-neutral (Phase 1).
- The norm is at most two categories per kind; a `gen-categories` gate caps membership
  at three (e.g. `file_is_ascii` = Content + Encoding + Security). The authoritative,
  drift-gated membership is the generated in-crate bridge (`categories_gen.rs`, checked
  by `gen-categories --check`) and `facts.json.rule_categories` — NOT the illustrative
  counts in this design doc, which are a point-in-time snapshot and are not gated, so
  treat the bridge as the source of truth. The full curated set is documented in
  `docs/design/rule-categories-assignments.md`.

Today six xtask files parse `rules.md` independently, so this design adds a small
shared parse helper for the `**Categories:**` line (and, opportunistically, the H2/H3
walk) that `facts.rs`, `gen_model.rs`, `docs_export.rs`, and `gen-categories` adopt, so
the line has one interpretation. Consolidating the existing per-file parsers is net-new
work scoped into Phase 1, not a precondition that already holds.

Critically, the `**Categories:**` line sits inside the H3 body, which docs-export both
SUMMARIZES and RENDERS. `docs_export.rs::first_sentence` takes the first non-blank
paragraph as the kind's summary (feeding the family tables, the master index, and the
SEO meta description), and `emit_rule_page` copies the body verbatim into the page. So
the shared parser MUST strip the `**Categories:**` line from the body before summarizing
and before rendering; categories then surface only through the page frontmatter and the
generated cross-link block. Without the strip, every summary becomes the literal
`**Categories:** ...` string and the raw line double-renders against the block. A parity
test asserts no rendered page body or summary contains a residual `**Categories:**`.

### The generated in-crate bridge

`xtask gen-categories` emits a committed Rust artifact, for example
`crates/alint-rules/src/categories_gen.rs`, of the form:

```rust
// @generated by `cargo run -p xtask -- gen-categories`. Do not edit.
pub static KIND_CATEGORIES: &[(&str, &[Category])] = &[
    ("no_bidi_controls", &[Category::SecurityUnicodeSanity, Category::Encoding]),
    // ...
];
pub static ALIAS_TO_CANONICAL: &[(&str, &str)] = &[
    ("content_matches", "file_content_matches"),
    // ...
];
```

- Keyed by CANONICAL kind. Aliases map to their canonical via `ALIAS_TO_CANONICAL`
  (harvested from the `(alias: X)` annotations already in rules.md H3 titles), so a
  category applies to the canonical kind and its aliases share it.
- Carries categories only, NOT per-kind summaries (see Alias handling and the
  deferred-summary note below).
- `gen-categories --check` fails if the committed artifact drifts from rules.md, the
  same way `gen-schema --check` guards the in-crate schema copy.

Generated `.rs` (static slices, zero runtime parse) is preferred over an
`include_str!`'d JSON; the exact form is an open sub-decision.

### facts.json contract addition

`gen-facts` adds two fields: an ordered `categories` vocabulary and the `rule_categories`
associations. Per the `facts.json` convention, `format_version` is bumped on a shape
change, so this bumps it from 1 to 2. The bump is NON-breaking: the alint.org facts
loader reads known keys and does not gate on facts.json `format_version` (the site's
SUPPORTED-version checks apply to the bundle `manifest.json` and `roadmap.json`, not
facts.json), and facts.json is release-tag-pinned so v2 reaches the site only at a
release. The bump keeps the convention honest and lets a future consumer pin the schema.

```json
"categories": [
  { "slug": "existence", "title": "Existence", "order": 0 },
  { "slug": "content", "title": "Content", "order": 1 }
],
"rule_categories": {
  "no_bidi_controls": ["security-unicode-sanity", "encoding"],
  "file_hash": ["content", "security-unicode-sanity"]
}
```

The `categories` list makes the contract self-contained: it carries each category's
slug, title, and display order, so a standalone consumer can map a `rule_categories`
slug back to its title and order without re-slugifying, and it is the source the site's
`FAMILY_ORDER` (order plus labels) is validated against. The legacy `families` titles
list stays for back-compat; `counts.families` stays 13 and `counts.rule_kinds` is
unchanged (see Counting semantics).

### Rule-page frontmatter and cross-listing

`docs-export` emits `categories: [<slug>, ...]` into each rule page's frontmatter
(additive; the family is still implied by the page's directory for the canonical
URL). The family Overview generator (`family_index.rs`) lists, for each category,
every kind whose `**Categories:**` line includes it, linking multi-belonging kinds to
their canonical page. The per-kind page renders a "Categories: X, Y" cross-link block
from its frontmatter; the raw `**Categories:**` line is stripped from the page body
during slicing (see the association SSOT section), so the block is the only rendered
categories element and the summary extractor never sees the line.

### gen-model and the LikeC4 taxonomy

`gen_model` derives `rule-families.gen.c4` from the rules.md H2 -> H3 tree today. That
taxonomy stays PRIMARY-only: the LikeC4 view is a tree, so a kind appears under its
primary family, not once per category. Multi-membership lives in the reference pages,
`facts.json`, and the CLI, not the architecture diagram. `check_taxonomy_complete` keeps
gating H3-documentation completeness; the new "every canonical kind has a
`**Categories:**` line" check lives in `gen-categories`, its natural home, not in
`check_taxonomy_complete`.

## Alias handling

Aliases are registered as separate registry keys pointing at the same builder
(`content_matches` and `file_content_matches` both build the same rule), and
`known_kinds()` returns both. Without a rule they would either duplicate rows or
vanish. The design resolves this explicitly:

- The catalog lists CANONICAL kinds only. Each row annotates its aliases (from
  `ALIAS_TO_CANONICAL`).
- Categories are a property of the canonical kind; aliases inherit them.
- `alint rules list --search content_matches` matches via the alias map and shows the
  canonical row.

This mirrors how the site already treats aliases (its `/api/rules.json` keeps a
hand-maintained alias fallback map; the generated `ALIAS_TO_CANONICAL` replaces that).
The map cannot silently miss a registered alias: `schema.rs` asserts `all_kinds.yaml`
and the live `known_kinds()` match bidirectionally, and `check_taxonomy_complete`
asserts every registered kind is either an H3 or an `(alias: ...)` annotation, so every
alias is present to harvest.

## Counting semantics

Many-to-many changes only per-category membership, not the headline counts. One
pre-existing subtlety must be surfaced so the catalog is not mistaken for a drift:

- `counts.rule_kinds` = 105, unchanged by categorization. That figure counts distinct
  `kind:` spellings in `all_kinds.yaml`, which INCLUDE the 11 alias spellings, so the
  canonical kinds number 94.
- The catalog (`alint rules list` and the family Overview pages) lists the 94 CANONICAL
  kinds, each annotating its aliases, so its row count is intentionally lower than the
  105 headline. The gap is exactly the 11 aliases, not missing rules.
- `counts.families` = number of categories = 13, unchanged.
- The sum of per-category kind counts now exceeds the canonical count (that is the
  point) and is not a headline number.
- The claim "N rule kinds across 13 families" still holds.

## CLI

Full decision and rationale in ADR-0009. In brief:

- `alint rules list [--category <slug>] [--search <term>]` and
  `alint rules categories` provide catalog discovery from the in-crate bridge, with
  no config read. `--search` matches the kind name (and alias spellings) in the first
  cut; summary search is deferred (would require the bridge to carry summaries).
- `alint list --category <slug>` filters the user's active rules by category. This
  needs new plumbing: `RuleEntry` does not retain the kind and the `Rule` trait has no
  `kind()`, so the kind (available from `spec.kind` at load time) must be stored on
  `RuleEntry` and mapped to categories through the bridge.
- The `subcommands` headline count goes 11 -> 12 (`rules` is one top-level variant;
  its sub-subcommands are not counted).

## Site (alint.org)

- `src/lib/docs-catalogue.ts`: `getRuleRows()` gains `categories: string[]` per kind,
  ADDED from the synced `facts.json.rule_categories`. The `url`, primary `family`, and
  `familySlug` still come from the kind's canonical directory (its primary family),
  which is unchanged; the unknown-subdir drift guard is extended.
- `RulesTable.astro`: `data-family` becomes `data-families`; the chip filter matches
  membership; the Category column shows all categories.
- `/api/rules.json`: `schema_version` bumps to 2; each entry ADDS `categories: []`
  (primary first) and builds `docs_url` from the primary (`categories[0]`), so the URL
  still resolves to the one canonical page; the hand-maintained `FAMILY_OF` alias
  fallback is dropped (covered by the bridge). The `check-internal-links.mjs`
  `family: "unknown"` gate becomes "categories non-empty and all known."
- `llms.txt` / `llms-full.txt`: list each kind under all its categories. Per-kind page:
  the "Categories" cross-link block. The sidebar stays primary-only and
  directory-derived (`getRuleFamilies`); the multi-membership surfaces are the browse
  table, the ENGINE-generated family Overview pages (which cross-list), and search.
- A cross-repo parity check (see Gates) covers associations, vocabulary, and order.

## Zero-drift gates

Engine:

- Vocabulary closure: every `**Categories:**` token resolves to a `Category` variant
  (a compile-time closed enum plus a generation-time check).
- Primary membership: the kind's H2 family appears in its `**Categories:**` line.
- Completeness: every canonical kind has a `**Categories:**` line, enforced in
  `gen-categories` (which reads both the lines and the registry). `check_taxonomy_complete`
  keeps enforcing H3-documentation completeness as today.
- Order parity: the `Category` declaration order equals the rules.md H2 sequence (and
  the docs-export `family_order`); a gate fails on divergence. `facts.json.categories`
  is emitted from the enum, so the site `FAMILY_ORDER` order and labels are validated
  against it by the cross-repo parity gate (below).
- `gen-categories --check`, `gen-facts --check`, and docs-export `--check` (frontmatter
  and cross-listing) gate the generated artifacts.

Site:

- Extended `docs-catalogue.ts` drift guard.
- `check-internal-links.mjs` categories gate.
- Cross-repo parity: the site's category ASSOCIATIONS equal engine `rule_categories`,
  AND the site `FAMILY_ORDER` (order plus labels) equals engine `facts.json.categories`
  (slug, title, order). Associations, vocabulary, and order are all gated.

## Phased rollout

Following the project's one-commit-per-phase convention with a forward pointer.

- **Phase 0.** This design doc, ADR-0009, and the curated assignment table
  (`docs/design/rule-categories-assignments.md`): 94 canonical kinds, 32 multi-category,
  62 single, with the editorial calls settled.
- **Phase 1.** `Category` enum (with the order-parity gate); add the shared rules.md
  parse helper and adopt it in the generators; add single-membership `**Categories:**`
  lines (each kind's current family only); `gen-categories` plus the in-crate bridge;
  `facts.json` gains `categories` + `rule_categories` (with the `format_version` 1 -> 2
  bump); rule-page frontmatter plus the `**Categories:**`-line strip; `family_index`
  switches to categories-based membership (behavior-neutral while single-membership);
  `rule-families.gen.c4` stays primary-only; all gates. Behavior-neutral, ships safely.
- **Phase 2.** CLI: `alint rules list`/`categories`, `alint list --category` (with the
  `RuleEntry.kind` plumbing), `--help` snapshots, generated CLI-reference sections, and
  the 11 -> 12 subcommand-count move across `counts.rs`, the coverage audit, `facts.json`,
  AND the README claim (gated by `counts_match_readme_claims`).
- **Phase 3.** Flip the data to real many-to-many (populate secondary memberships from
  Phase 0). Regenerate: family Overview pages cross-list automatically; `alint rules
  list --category security` returns the cross-cutting kinds.
- **Phase 4.** Site consumes the mapping: `docs-catalogue.ts`, `RulesTable`,
  `/api/rules.json` v2, `llms.txt`, per-kind cross-links, cross-repo parity.
- **Phase 5.** Polish: optional browse-by-category facet, LikeC4 `catalogueOverview`
  multi-membership, a "set the kind's `**Categories:**`" step in the rule-authoring
  doc.

## Risks and open sub-decisions

- **In-crate artifact form**: generated `.rs` (zero-parse static slices) versus a
  `.json` embedded with `include_str!`. Leaning `.rs`.
- **Summaries in the bridge**: including each kind's one-line summary would enable a
  description column and summary search in `alint rules`, but summary prose edits would
  then also leave the docs-only CI fast lane. Deferred; default is categories-only.
- **API back-compat**: replace `family` with `categories` (clean, `schema_version: 2`)
  versus keep `family` as the primary alongside `categories` for one release. Leaning
  clean replacement with the version bump.
- **`list --category` plumbing**: store `kind` on `RuleEntry` (localized) versus add
  `kind()` to the `Rule` trait (touches every impl). Leaning `RuleEntry` field.
- **The assignments themselves**: which kinds gain which secondary categories is
  editorial and reviewed in Phase 0.

## Related

- ADR-0009 (rule discovery CLI: `alint rules` versus `alint list`).
- ADR-0001 (spec-driven development; the generate-and-gate pattern reused here).
- ADR-0007 (release-aware documentation; why CLI-visible categories are release-gated
  while the site is not).
- `docs/design/rule-categories-assignments.md` (the Phase 0 curated kind-to-category table).
- `docs/design/facts-json.md` (the contract this extends).
- `docs/development/rule-authoring.md` (gains a categories step in Phase 5).
