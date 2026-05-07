# Marketing-extraction batch 6

Per-case-study findings from extracting marketing/optics/positioning
content out of the public `examples/<repo>/README.md` files into the
private `alint.org` case-study collection.

The principle: **engineering findings stay in the public README;
positioning + headline-catch framing moves to alint.org**. Same fact,
different language — factual stays, promotional moves.

Batch scope (alphabetical batch 6 — the final batch, 5 case studies):

- `examples/rust-lang-rust/`
- `examples/tensorflow-tensorflow/`
- `examples/tokio-rs-tokio/`
- `examples/vercel-next.js/`
- `examples/vercel-turbo/`

## Per-case-study findings

### rust-lang/rust

- **Narrative:** orchestration-replacement (`src/tools/tidy/` is a
  bespoke ~5kLoC Rust binary; ~13 of ~32 tidy modules become
  declarative under alint).
- **Public README:** 299 → 294 lines (-5). Marketing pointer added.
  Recast "Recommendation for the launch story" → "Followup feature
  work surfaced (consolidated)" — moved the "Headline launch quote"
  framing + "second-strongest case study" + "polyglot
  monorepo / project-with-its-own-linter-binary audience" pitch +
  "we're not asking you to throw away your custom linter" framing
  to alint.org.
- **alint.org case study:** 103 lines. Frontmatter + Why this
  matters + Headline catch + Where alint earns its keep here +
  Future story angles. Headline puts the "30 % declarative subset
  in 18 lines of YAML" measurement up front.
- **Engineering preserved:** 32-tidy-module inventory tables,
  rule-kind candidate gap analysis (`ordered_block` /
  `registry_paths_resolve` / `file_pair_block_match` /
  `balanced_delimiters`), Future analysis section
  (`tidy@v1` bundled-ruleset suggestion + `scope_filter` refactor
  + `alint suggest` live-tree pass), Validation status (2026-05-07)
  footer with reconciled rule-count math.
- **Borderline calls:** the "second-strongest case study (behind
  kubernetes)" framing is an overt cross-case ranking — moved to
  alint.org. The "**v0.10 ship-target** — sortedness is the single
  most-requested missing rule kind across the ecosystem inventory
  passes" line in the table is a measurement of demand
  (engineering); kept. The narrative framing of *which case study
  earns flagship positioning* is positioning; moved.

### tensorflow/tensorflow

- **Narrative:** polyglot-wins (1,185 textproto API goldens
  demand-validate `cross_language_implementation_complete` at TWO
  topologies — per-source ↔ per-test within one language; core ↔ N
  bindings across languages).
- **Public README:** 775 → 740 lines (-35). Marketing pointer
  added. Replaced the "headline launch-pitch for alint on
  tensorflow" blockquote with a factual lead-in citing the
  validation commission. Renamed "Headline finding:" → "Live-tree
  findings (factual):". Toned the "**TF + arrow are now joined by
  protobuf + angular + flutter** as the 5 demand-driving repos —
  past saturation; `cross_language_implementation_complete` is the
  v0.11+ flagship ship-target" framing → kept the "5 demand-driving
  repos / saturated demand" measurement; moved the "flagship ship-
  target" framing to alint.org. Recast "Recommendation for the
  launch story" → "Followup feature work surfaced (consolidated)".
- **alint.org case study:** 114 lines (longest of the batch — TF
  has the richest cross-cutting positioning). Frontmatter + Why
  this matters + Headline catch (with the TWO-discipline-layers
  blockquote intact, since the framing IS the marketing) + Where
  alint earns its keep here + Future story angles.
- **Engineering preserved:** 34 structural-validation surfaces
  inventory + 17/6/11 mapping breakdown, the per-language parity
  layer-1/layer-2 detail tables, 1,185 textproto goldens count,
  TFLite Swift / ObjC / Java / Python coverage tables, gap-catalogue
  (`cross_language_implementation_complete` /
  `cross_file_value_equals` / `registry_paths_resolve` /
  `generated_file_fresh` / `markdown_template_match`), Future
  analysis (`scope_filter` for TFLite + `bazel-monorepo@v1` bundled
  ruleset + `alint suggest` detector breadth), Validation status
  with reconciled 30→40-rule count.
- **Borderline calls:** the headline launch-pitch blockquote (TWO
  discipline layers stacked) is the single most-charged piece of
  positioning in the README. I kept the factual TWO-layers
  observation + Layer 1 / Layer 2 explanation but moved the
  "tensorflow is the cleanest single-repo demonstration of the
  pattern" framing to alint.org. The brief was explicit on this
  case: keep TWO topologies measurement (engineering), move
  "demand-validates v0.11+ flagship" framing (positioning) — done.

### tokio-rs/tokio

- **Narrative:** convention-without-checks (zero hand-rolled
  scripts; alint catches the 15 conventions tokio's pipeline
  silently assumes — the flagship "convention without checks"
  case study).
- **Public README:** 367 → 360 lines (-7). Marketing pointer
  added. Recast the "**The pitch is: tokio's CI assumes the repo
  state is sane; alint asserts the assumptions.**" framing in the
  Performance comparison section → kept the engineering point
  (CI assumes sanity; alint asserts it) but moved the headline
  framing. Replaced the "headline is *not* alint replaces ad-hoc
  shell scripts" blockquote — kept the FACTUAL "15 conventions
  tokio's pipeline silently assumes" measurement; moved the
  "flagship convention-without-checks case" framing to alint.org.
  Recast "Recommendation for the launch story" → "Followup feature
  work surfaced (consolidated)".
- **alint.org case study:** 108 lines. Headline catch keeps the
  kubernetes-vs-tokio launch-counterpoint blockquote (which IS the
  marketing). Lists the 15 silent conventions explicitly.
- **Engineering preserved:** workflow inventory table, the
  6-mappable + 15-defensive + 5-new-primitive + 1-out-of-scope
  breakdown, gap-catalogue (`cross_file_value_equals` /
  `ordered_block` / `pair_hash` / `toml_path_equals` typed
  comparison), Methodology notes (`file_starts_with` requires
  non-empty prefix), Future analysis (`agent-context` /
  `docs/adr` adoption + `alint suggest` against fresh clone),
  Validation status with reconciled 27→28-rule count.
- **Borderline calls:** the "well-curated Rust workspace" framing
  is a positioning label; moved. The "defense-in-depth, not
  replacement" framing is the strongest piece of positioning in
  the README; moved to alint.org. Engineering point that "tokio's
  CI assumes the repo state is sane; alint asserts the
  assumptions" is factual; kept.

### vercel/next.js

- **Narrative:** polyglot-wins (first hybrid pnpm + Cargo
  dual-workspace win — drift no per-language linter catches because
  each linter only sees half the tree).
- **Public README:** 528 → 506 lines (-22). Marketing pointer
  added. Recast "Headline finding:" → "Live-tree findings
  (factual):" — kept the FACTUAL "first hybrid pnpm + Cargo
  dual-workspace" measurement + the "3 of 19 / 4 of 63" per-package
  drift counts; moved the "tightest fit in the case-study
  catalogue" + "win" framing to alint.org. Recast "Recommendation
  for the launch story" → "Followup feature work surfaced
  (consolidated)".
- **alint.org case study:** 106 lines. Headline keeps the "first
  hybrid pnpm + Cargo dual-workspace win" lead. Per-package drift
  numbers in Headline catch.
- **Engineering preserved:** 34 structural-validation surfaces
  inventory + 18/7/9 mapping breakdown, the full hand-rolled
  `scripts/check-*.{js,mjs,sh}` table, gap-catalogue
  (`cross_file_value_equals` / `registry_paths_resolve` /
  `dir_name_matches_field` extension with unscoping), Pitfall #16
  surface (now in CONFIG-AUTHORING.md catalogue), Future analysis
  (`scope_filter` for dual workspace + `compliance/reuse` /
  `agent-hygiene` adoption), Validation status with the
  exact-match 59-rule count + 130-rule post-resolution total.
- **Borderline calls:** the "**fourth tile** on alint.org/examples
  (after kubernetes, airflow, microsoft/typescript)" framing is a
  cross-case launch-tile order; moved to alint.org. The "no other
  tool composes ecosystem rules at this layer" framing is
  positioning; moved. The pitfall #16 surfaced-by-this-case-study
  documentation IS engineering (it captures HOW the pitfall
  was discovered, what the workaround is); kept in full.

### vercel/turbo

- **Narrative:** structural-floor (Rust monorepo orchestrator;
  alint adds 22 gates that don't exist).
- **Public README:** 320 → 311 lines (-9). Marketing pointer
  added. Recast "Headline finding:" → "Live-tree findings
  (factual):". Recast "Recommendation for the launch story" →
  "Followup feature work surfaced (consolidated)" — moved the
  "strongest evidence for monorepo-tier positioning" framing +
  the kubernetes-vs-turbo launch-hook contrast (`50 hand-rolled
  scripts → 17` vs. `zero hand-rolled scripts → 22 structural
  gates that don't exist`) + "Use this as evidence on
  alint.org/examples that monorepo conventions are under-checked
  even at top-tier-tooling repos" closer to alint.org.
- **alint.org case study:** 115 lines. Headline catch keeps the
  kubernetes-vs-turbo blockquote (it IS the marketing punch) +
  the "even Vercel-grade tooling has structural drift" closer.
  Live-tree findings list (60/61, 8/17, 9/52, 7-crate-name-drift,
  with-microfrontends + with-nextjs) repeated for the headline
  context; the public README still carries the same numbers in
  the Live-tree findings (factual) section.
- **Engineering preserved:** 5-place structural-validation
  inventory (pre-push hook + lint.yml + pr-title + js-test
  scripts + check-examples.ts), 22-rule mapping table,
  gap-catalogue (`dir_name_matches_field` + `json_schema_passes`
  + `alint pr-diff-check` sibling-mode), Future analysis (`alint
  suggest` for the 22 gates + `scope_filter` for the
  crates/packages/examples triad + `dir_name_matches_field`
  v0.10+ design note), Validation status with the
  29→28-rule reconciliation.
- **Borderline calls:** none material. The headline-catch
  blockquote was a clean positioning unit to lift; the
  engineering tables underneath stayed intact.

## Cross-cutting patterns

1. **The "Recommendation for the launch story" section is the
   uniform marketing-extraction target.** All 5 READMEs had this
   section; all 5 became "Followup feature work surfaced
   (consolidated)" with the positioning prose moved to alint.org.
   The rule-kind candidate list at the END of each section is
   engineering and stays.
2. **"Headline finding:" → "Live-tree findings (factual):"** is
   the consistent recasting label. This pattern was already
   established in the angular case study (batch earlier) and it
   transfers cleanly. The label change preserves the engineering
   measurements but drops the marketing tone.
3. **Cross-case rankings ("flagship", "second-strongest",
   "fourth tile") are the cleanest signal of positioning content.**
   They appear nowhere in the alint.org files except as
   factual placement notes ("position as the second polyglot tile"
   inside Future story angles); they appear nowhere in the
   public READMEs.
4. **The kubernetes contrast is the single most-recurring launch
   hook.** kubernetes-vs-tokio (defense-in-depth), kubernetes-vs-
   rust-lang/rust (sprawl-vs-binary), kubernetes-vs-turbo
   (sprawl-vs-zero-scripts) — all three contrasts moved to
   alint.org. The kubernetes case study itself anchors them
   from the marketing side.
5. **Pitfall documentation is engineering, not marketing.**
   next.js's pitfall #16 surface, tokio's pitfall #13 + the
   `file_starts_with` empty-prefix surface, TF's pitfall #19
   (latent surfaced + fixed in v0.9.17), turbo's pitfall #16
   cross-reference — all stayed in the public READMEs. The
   "what pitfall did this case study surface, what's the
   workaround, where does it live in CONFIG-AUTHORING.md now"
   reads as engineering documentation, not positioning.
6. **The rule-count reconciliation footer (Validation status
   2026-05-07) is engineering.** All 5 READMEs preserve their
   "validate-config: ✓ N rules / README claim X / actual Y /
   bundled-overlap dedup explanation" footer untouched. The
   alint.org files mirror only the *headline* count in the
   frontmatter `rules:` field.

## Borderline calls (consolidated)

- **TF's TWO-discipline-layers blockquote.** Engineering or
  positioning? The brief said keep "TWO topologies" measurement
  (engineering), move "demand-validates v0.11+ flagship" framing
  (positioning). Resolution: kept Layer 1 / Layer 2 description +
  the topology distinction in public README; moved the
  "tensorflow is the cleanest single-repo demonstration" closer
  to alint.org. The TWO-layers detail per se is rule-kind design
  intent — that's engineering for the v0.11+ ship.
- **next.js's "Headline finding" lead.** "first hybrid pnpm +
  Cargo dual-workspace" is factual (it really is the first such
  case study in the catalogue); "win" / "tightest fit" is
  positioning. Resolution: kept the factual measurement; moved
  the framing.
- **tokio's "headline is *not* alint replaces ad-hoc shell
  scripts"** explanatory paragraph. The numerical count
  (15 conventions) is engineering; the editorialising about what
  the pitch ISN'T is marketing. Resolution: replaced the
  "headline is *not* …" rhetorical pivot with a plain factual
  statement; moved the kubernetes-counterpoint framing to
  alint.org.
- **rust-lang/rust's "Headline launch quote" blockquote.** Pure
  positioning unit; whole-block move to alint.org. The 30 % +
  18 lines + "four new rule kinds → 55 %" measurements are all
  preserved in the body of the public README's mapping/gap
  sections — the quote was just the marketing concentration of
  the same numbers.
- **turbo's "Headline finding" was almost entirely engineering
  numbers.** The label change to "Live-tree findings (factual):"
  was the only meaningful edit — the body content stayed.

## Blockers

- None. All 5 case studies extracted cleanly. No engine bugs
  surfaced (revalidation pass already established v0.9.17 health);
  no .alint.yml edits required (the brief was explicit on this).
  Public-README engineering content fully preserved; alint.org
  case studies follow the established frontmatter +
  4-section-body shape.

## Files touched

Public READMEs (5 — engineering content preserved, positioning
recast):

- `/home/kaminsod/projects/alint/examples/rust-lang-rust/README.md`
- `/home/kaminsod/projects/alint/examples/tensorflow-tensorflow/README.md`
- `/home/kaminsod/projects/alint/examples/tokio-rs-tokio/README.md`
- `/home/kaminsod/projects/alint/examples/vercel-next.js/README.md`
- `/home/kaminsod/projects/alint/examples/vercel-turbo/README.md`

alint.org case studies (5 — new files):

- `/home/kaminsod/projects/alint.org/src/content/case-studies/rust-lang-rust.md`
- `/home/kaminsod/projects/alint.org/src/content/case-studies/tensorflow-tensorflow.md`
- `/home/kaminsod/projects/alint.org/src/content/case-studies/tokio-rs-tokio.md`
- `/home/kaminsod/projects/alint.org/src/content/case-studies/vercel-next.js.md`
- `/home/kaminsod/projects/alint.org/src/content/case-studies/vercel-turbo.md`

Net delta: -78 lines from public READMEs (positioning trimmed),
+546 lines added on alint.org (case-study marketing pages).
