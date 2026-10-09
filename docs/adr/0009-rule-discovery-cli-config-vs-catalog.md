---
status: accepted
date: 2026-07-07
decision-makers: asamarts
---

# 0009. Rule discovery in the CLI: `alint rules` (catalog) vs `alint list` (config)

## Status

Accepted. (One of: Proposed | Accepted | Rejected | Deprecated | Superseded by ADR-NNNN.)

Implemented: the `alint rules` catalog namespace (`list`, `categories`, later
`show`) shipped in v0.14.0 alongside the many-to-many rule categories, with
`alint rules show` and `--search` refinements in v0.15.0 (see CHANGELOG.md).

## Context

The many-to-many rule-category work adds a committed, in-crate kind-to-category
table, generated from `docs/rules.md` and validated against a `Category` enum in
`alint-core`. For the first time this makes a rule's categories readable by the
compiled binary, not just by the docs pipeline. That raises a CLI question: how
should a user discover rules by category (and, later, browse the catalog) from the
terminal?

Two forces complicate the answer.

1. **The CLI already has a config-scoped notion of "rules," and only that.**
   `alint list` lists the rules in this repo's effective config, after `extends:`
   and bundled rulesets resolve. `alint explain <rule-id>` describes one rule
   instance found in that config. Both read `.alint.yml`; neither can answer "what
   does alint offer?" A category browser is inherently catalog-scoped and
   config-independent, so it fits neither command.

2. **The command grammar is flat verbs.** All eleven subcommands at the time of
   this decision (`alint rules` made it twelve) (`check`, `list`,
   `explain`, `fix`, `baseline`, `facts`, `init`, `export-agents-md`, `suggest`,
   `validate-config`, `lsp`) are single-level verbs. There is no noun-namespace with
   sub-subcommands. Introducing discovery risks two failure modes: overloading
   `list` until "my configured rules" and "the whole catalog" blur into one
   confusing command, and scope creep into a broad `rules type|category|bundle` tree
   before those concepts are defined.

A prior critique (see More Information) concluded that the sharpest risk is exactly
this config-scoped versus catalog-scoped conflation, and recommended scoping the new
surface down to the motivated minimum.

## Decision

We will introduce a catalog-scoped noun-namespace, `alint rules`, and keep
`alint list` strictly config-scoped. The two are separated by one unambiguous,
tested rule: **`alint rules` never reads a config; `alint list` always does.**

Scope is the organizing distinction:

| | `alint list` | `alint rules list` |
|---|---|---|
| Scope | this repo's effective config (`.alint.yml` + `extends:` + bundles) | the full catalog of rule kinds alint ships |
| Answers | "what runs here, and how is it configured?" | "what does alint offer, what can I use?" |
| Unit | rule instances (id, kind, level, paths) | rule kinds (the types), config-independent |
| Reads a config? | yes (behaves outside a configured repo exactly as today) | no (works anywhere, including outside any repo) |
| `--category <slug>` | filters YOUR active rules | filters the CATALOG |

**Commands, initial scope** (`--format human|json` is the existing global flag,
inherited by these subcommands, not new per-command surface):

- `alint rules list [--category <slug>] [--search <term>]`
  Lists catalog rule kinds, optionally filtered by category and by a free-text
  search. In the first cut `--search` matches the kind NAME only: per-kind summaries
  live in `docs/rules.md` and are not compiled into the binary, so searching them
  requires the in-crate bridge to also carry summaries (deferred; see Data source).
  Aliases (for example `content_matches` for `file_content_matches`) are not listed
  as separate rows; each canonical kind carries its alias names as an annotation, and
  `--search` matches alias spellings.
- `alint rules categories`
  Lists the category vocabulary (slug, title, kind count) so the `--category` slugs
  are discoverable without leaving the terminal.
- `alint list [--category <slug>]` gains a config-scoped category filter ("which of
  my active rules are in category X"). This is NOT free reuse of the flag: a loaded
  rule does not expose its kind at runtime today (`RuleEntry` retains the built
  `dyn Rule` and its `when:` only, and the `Rule` trait has `id()` but no `kind()`),
  so `list --category` requires retaining the kind on `RuleEntry` (available from
  `spec.kind` at build time) and mapping kind to category through the same in-crate
  bridge. The catalog side (`alint rules list --category`) has no such gap: it reads
  the kind-to-category table directly.

**Deferred, out of scope for the first cut:**

- `alint rules --type <t>`: there is no clean, stable, user-facing "type" axis today.
  The only candidates are the internal `Rule` / `PerFileRule` trait shape (an
  implementation detail) or `level` (config, not intrinsic). We will not ship a
  taxonomy axis without an SSOT and a defined user-facing meaning.
- `alint rules --bundle <name>` and `alint rules show <kind>`: bundled rulesets are
  already discoverable (bundled-ruleset pages on the site, `extends:` in config), and
  single-kind detail is served by the site and by `explain` for configured rules.
  Revisit on demand.

**Data source.** `alint rules` reads the committed in-crate kind-to-category table;
the vocabulary is the `Category` enum. No config, no network, no docs tree at
runtime. The table carries categories only, not per-kind summaries (those stay in
`docs/rules.md`). Extending it to include each kind's one-line summary would enable a
description column and summary search, but would also mean summary prose edits
regenerate the in-crate artifact and so leave the docs-only CI fast lane. That trade
is deferred to the design doc.

**Separation is enforced, not merely intended:**

1. Config-independence is the defining behavioral difference above, and is covered by
   tests (running `alint rules` outside any repo succeeds; `alint list` still requires
   a config).
2. Scope-first help: each command's first help line states its scope and whether a
   config is required.
3. Distinct output headers: `alint list` prints "Active rules (N)"; `alint rules list`
   prints "Rule catalog (N kinds)".
4. Reciprocal cross-references: each command's help points at the other.

**Contracts:**

- The headline `subcommands` count stays defined as "top-level `enum Command`
  variants." A `Rules { ... }` variant counts as exactly one: the counter
  (`count_enum_variants` in `xtask/src/docs_export/counts.rs`) strips each variant's
  nested brace body before counting, and the separate `enum RulesCommand` holding the
  sub-subcommands is never scanned. So the count goes from eleven to twelve, and
  `counts.rs`, the matching coverage audit, `facts.json`, and the README move together.
- `alint rules --format json` follows the JSON envelope convention the CLI already
  uses: `{ "schema_version": 1, "kind": "<discriminator>", ... }`, exactly as `list`
  (`kind: "rule-inventory"`), `explain` (`kind: "rule"`), and `facts` (`kind: "facts"`)
  do today. The catalog output takes a new discriminator (for example
  `kind: "rule-catalog"`), so it is consistent with the existing contracts rather than
  a novel scheme, and it is kept in parity with the `/api/rules.json` site contract
  (both derive from the same categories source).

## Consequences

Easier:

- Category discovery reaches the terminal: offline, scriptable, single binary, no
  config required. This is the payoff of compiling the category bridge into the binary.
- The config versus catalog split becomes explicit and testable instead of latent, and
  `alint list --category` gives a genuinely useful "audit my own config by category"
  view.
- `alint rules` is an extensible home for future catalog discovery without further
  overloading the verb commands.

Harder, and accepted:

- `alint rules` is the first noun-namespace with sub-subcommands; it sets a grammar
  precedent the flat verb commands do not follow. We accept the inconsistency
  deliberately rather than retrofit every command into namespaces.
- New maintenance surface: `--help` snapshot fixtures, the generated CLI-reference
  sections, and the flag inventory all grow with the command tree. The JSON output is a
  new shape, but it reuses the established `schema_version` + `kind` envelope, so it
  adds a contract to maintain without introducing a new versioning scheme.
- Both `alint rules` and `alint list --category` need the kind-to-category bridge, and
  `list --category` additionally needs the kind retained on `RuleEntry`: neither the
  `Rule` trait nor a loaded rule exposes its kind today. This is small but real
  plumbing, not free reuse of an existing flag.
- The rule taxonomy is now surfaced in three places: the site catalog,
  `/api/rules.json`, and the CLI. All three must stay in parity. They share one source,
  but the CLI's labels and format are an additional drift surface with its own gate.
- Category edits regenerate a committed in-crate artifact, so they leave the docs-only
  CI fast lane, and CLI users see new categories only at the next release (the binary
  must ship). The site can still reflect edits sooner. This cost is inherited from the
  decision to make categories CLI-visible, not created here.
- `--category` takes a slug (for example `security-unicode-sanity`), which is not
  always guessable; `alint rules categories` exists as the discovery entry point for
  exactly this reason.

## Considered Options

- **New `alint rules` namespace, scoped to `list` plus `categories` (chosen).** The
  motivated minimum; a clean scope model; defers undefined axes.
- **Flags on existing commands** (for example `explain --category`, `list --catalog`).
  No new grammar, but it either overloads `list` across two scopes (the exact confusion
  this ADR removes) or bolts catalog behavior onto `explain`, which is single-instance
  and config-scoped.
- **Overload `alint list` with a `--catalog` switch to flip scope.** Rejected: one
  command silently changing between "my config" and "the whole catalog" is precisely the
  ambiguity this ADR exists to remove.
- **Full `rules type|category|bundle` tree now.** Rejected as scope creep: "type" has no
  definition or SSOT, and bundles are already discoverable.
- **No CLI discovery, site only.** Rejected: it contradicts the decision to make
  categories CLI-visible and forfeits the offline and scriptable use that motivated
  compiling the bridge.

## More Information

- Companion design doc (full many-to-many category plan: SSOT, the `Category` enum, the
  generated in-crate bridge, alias handling, and gates): `docs/design/rule-categories.md`.
- Related: ADR-0001 (spec-driven development; the generate-and-gate contract pattern this
  reuses) and ADR-0007 (release-aware documentation; why CLI-visible categories are
  release-gated while the site is not).
