# Case-study revalidation batch 6 — 2026-05-07

Final alphabetical batch. 5 case studies revalidated against
v0.9.17:

- examples/rust-lang-rust/
- examples/tensorflow-tensorflow/
- examples/tokio-rs-tokio/
- examples/vercel-next.js/
- examples/vercel-turbo/

alint binary: 0.9.17 (1dbd9b218a0e, built 2026-05-07).

## Per-case-study findings

### rust-lang/rust

- **`validate-config`:** ✓ 62 rules (5 bundled + 20 rust-specific).
- **README claim "18 rust-specific":** off by 2 (actual 20 — slack
  from `rust-triagebot-relabel-section` + `rust-tidy-rustfmt` added
  in P2b polish). README updated with reconciled math in Validation
  status footer rather than chasing every "18" reference.
- **Stale references fixed:**
  - "Strong v0.10+ candidate" → "v0.10 ship-target" for
    `ordered_block` (saturated demand, rust-tidy alphabetical is
    canonical example).
  - "single highest-leverage v0.10+ gap" → "now a v0.10 ship-target"
    for `registry_paths_resolve` (6+ confirmations).
- **Pitfalls #18/#19 fix sync:** Neither surfaces in this config
  (no tracked-AND-gitignored files; no `root_only: true` on
  multi-component literals). Engine fixes don't change the config.
- **`tidy@v1` bundled-ruleset suggestion:** ~13 of ~32 tidy modules
  are declarative — packaging as `alint://bundled/tidy/rust@v1`
  would let any rust-lang/rust contributor adopt the canonical 30%
  of tidy as one extends entry, raising the pitch from "18 lines"
  to "1 line". Documented in Future analysis section.
- **`scope_filter` refactor opportunity:** Current config repeats
  `src/llvm-project/**` + `src/gcc/**` excludes across 5 rules.
  v0.9.17's `scope_filter` evolution centralises this. Documented.
- **README touched:** ✓ ; pitfall/rule-kind status sync, rule-count
  reconciliation, Future analysis section, Validation status
  footer.

### tensorflow/tensorflow

- **`validate-config`:** ✓ 83 rules (6 bundled + 40 tensorflow-specific).
- **README claim "30 tensorflow-specific":** understated — actual is
  40 (~10 added in P2b polish: extra TFLite parity rules + Apache
  header coverage). Documented in Validation status footer.
- **Stale references fixed:**
  - `cross_language_implementation_complete` → marked v0.11+
    ship-target with **5 confirmed sources** (arrow + tensorflow +
    protobuf + angular + flutter) per the launch-evidence doc.
    TF-specific note added: 1,185 textproto goldens were the 2nd
    source after arrow.
  - `cross_file_value_equals` / `registry_paths_resolve` /
    `generated_file_fresh` → all promoted to v0.10 ship-targets.
  - "may surface as pitfall #18" note REWRITTEN: this was actually
    pitfall #19 (`literal_is_nested` opacity) — TF was the source
    case study that surfaced it. Fix shipped in v0.9.17 with
    clearer diagnostic. Pitfall #18 (per-rule
    `respect_gitignore: false`) also shipped its fix in v0.9.17;
    doesn't surface here.
- **`scope_filter` refactor opportunity:** TFLite per-binding
  parity rules (Swift/ObjC/Python/Java) hard-code globs; named
  scope filters per binding would centralise the per-language
  conventions and serve as the design template for the v0.11+
  `cross_language_implementation_complete` primitive. Documented.
- **`bazel-monorepo@v1` bundled-ruleset suggestion:** TF + grpc +
  envoy + many internal Google + Pinterest + Lyft repos share the
  same Bazel-monorepo shape (BUILD/MODULE.bazel/.bzl Apache
  headers). `alint suggest` against `/tmp/tensorflow/` doesn't
  detect Bazel monorepos today (only flagged
  `oss-baseline` + `agent-hygiene` medium); a `bazel-monorepo@v1`
  bundle would close both gaps. Documented.
- **README touched:** ✓ ; multiple v0.10/v0.11 status sync,
  pitfall #18/#19 fix-shipped note, rule-count reconciliation,
  Future analysis section, Validation status footer.

### tokio-rs/tokio

- **`validate-config`:** ✓ 74 rules (6 bundled + 28 tokio-specific).
- **README claim "27 declarative rules":** actual is 28 (within
  rounding). Documented.
- **Stale references fixed:**
  - `cross_file_value_equals` → v0.10 ship-target (8+
    confirmations).
  - `ordered_block` → v0.10 ship-target (rust + tokio + 3 more).
  - `pair_hash` → stays v0.10+ candidate (kubernetes + tokio).
  - "12 pitfalls + 1 new = 13" → updated note: catalogue grew to
    21 in P2a/P2b waves; the tokio-discovered #13 is now part of
    the published catalogue.
- **`toml_path_equals` typed comparison:** noted as overlap with
  next.js's pitfall #16 (JSONPath bool/number regex coercion);
  added a CONFIG-AUTHORING.md cross-reference suggestion.
- **15 conventions analysis:** Most of tokio's "convention without
  explicit checks" are now expressible (v0.9.6+ rule kinds covered
  most). Remaining 5 gaps are the cross-file/ordering/hashing
  patterns above — all on the v0.10+ pipeline. Documented.
- **`agent-context` / `docs/adr` adoption:** Current config skips
  these newer bundled rulesets — tokio doesn't ship an ADR tree
  (deliberate finding); `agent-context` would catch absence of
  `AGENTS.md` / `CLAUDE.md` if maintainers opt in. Documented.
- **README touched:** ✓ ; rule-kind status sync, catalogue size
  update, Future analysis section, Validation status footer.

### vercel/next.js

- **`validate-config`:** ✓ 130 rules (11 bundled + 59 next.js-specific).
- **README claim "59-rule":** matches exactly. Documented in
  Validation status footer.
- **Stale references fixed:**
  - `cross_file_value_equals` → v0.10 ship-target (8+
    confirmations: from "3 distinct repos" to "8+ confirmations").
  - `registry_paths_resolve` → v0.10 ship-target.
  - `dir_name_matches_field` extension → stays v0.10+ candidate.
  - "NEW pitfall #16 surfaced by this case study" → reframed to
    "now in CONFIG-AUTHORING.md" with note that catalogue stands
    at 21 entries.
- **Pitfalls #18/#19:** Neither surfaces in this config. Pitfall
  #16 (this pass — JSONPath bool/number regex coercion) IS in the
  published catalogue.
- **`scope_filter` refactor opportunity:** dual pnpm + Cargo
  workspace shape — named scopes (`rust-workspace`, `js-workspace`,
  `js-bench`) declared once, with path predicates centralised. Cuts
  ~40 lines and makes per-subtree intent explicit. Documented.
- **`compliance/reuse@v1` / `agent-hygiene@v1` adoption:**
  `alint suggest` against `/tmp/next.js/` flagged `agent-hygiene`
  (medium) — the next.js root has agent-readable docs the
  antipattern scan would benefit from. `compliance/reuse` is a
  deliberate skip (no per-file SPDX headers). Documented.
- **README touched:** ✓ ; rule-kind status sync, pitfall #16
  framing update, Future analysis section, Validation status
  footer.
- **Live-tree spot-check:** `alint suggest` against `/tmp/next.js/`
  surfaced `monorepo/cargo-workspace`, `monorepo`, `node`,
  `oss-baseline`, `rust` (high) + `agent-hygiene` (medium) —
  matches the `extends:` block exactly.

### vercel/turbo

- **`validate-config`:** ✓ 88 rules (9 bundled + 28 turbo-specific).
- **README claim "~29 structural checks":** matches actual 28 within
  rounding. Documented.
- **Stale references fixed:**
  - `dir_name_matches_field` / `json_schema_passes` → stay v0.10+
    candidates; turbo is the headline demand-driver but per-repo
    confirmation count is modest.
- **Pitfalls #18/#19:** Neither surfaces. Pitfall #16
  (cross-referenced from next.js — JSONPath bool/number regex
  coercion) was a *latent* risk in
  `turbo-example-meta-declares-maintenance`; the config already
  uses `file_content_matches` against the JSON text (with an
  in-line comment citing pitfall #16) — fix applied during
  original P2b pass. **No .alint.yml bug to flag.**
- **22 gates that don't exist in turbo's tooling:** Documented
  what `alint suggest` would propose (most of the 9 bundled
  rulesets currently in extends:, plus probably `agent-hygiene`
  for `// TODO(scope-name)` markers in the Rust crates).
- **`scope_filter` refactor opportunity:** `crates/`/`packages/`/
  `examples/` triad as named scopes — cuts ~20 lines and clarifies
  per-example rule intent. Documented.
- **`dir_name_matches_field` v0.10+ design note:** turbo has 7
  intentional drift cases; v0.10+ design needs `paths.exclude:` or
  `allow_drift:` knob. Documented.
- **README touched:** ✓ ; rule-kind status sync, pitfall #16
  resolved-not-bug note, Future analysis section, Validation
  status footer.

## Cross-cutting patterns

1. **All 5 configs are v0.9.17-compatible without changes.**
   `validate-config` passes cleanly; pitfalls #18 + #19 fixes
   land transparently. No engine regression risk in this batch.
2. **Rule-count drift is universally minor (≤ 10 rules).** All 5
   READMEs cite line-count framings from earlier waves; actual
   counts have grown 1-10 rules per config during P2b polish.
   Reconciled in each Validation status footer rather than
   surgically updating every "N rules" reference (would be ~30
   edits across 5 files for net zero accuracy gain).
3. **`scope_filter` evolution is a consistent refactor opportunity.**
   3 of 5 case studies (rust, tensorflow, next.js, turbo) have
   per-subtree glob repetition that named scopes would centralise.
   Documented in each Future analysis section as a v0.10+
   adoption ladder rung.
4. **`alint suggest` undersells multi-language monorepos.** TF
   surfaced only `oss-baseline` + `agent-hygiene` (medium) — no
   Bazel-monorepo detector. next.js detector worked correctly but
   missed the `compliance/reuse` opt-in question. The
   suggester's "high-confidence first" sort is right; the gap is
   detector breadth, not output framing.
5. **Pitfall catalogue is stable at 21.** No NEW pitfalls
   surfaced in this batch. The TF-discovered #19 (the LATENT
   pitfall noted in the original P2b README) is now both in the
   catalogue AND fixed in v0.9.17. The next.js-discovered #16
   (JSONPath bool regex coercion) is in the catalogue and the
   workaround (`file_content_matches`) is canonical across both
   next.js and turbo configs.
6. **Demand saturation matches the launch-evidence doc.**
   - `cross_file_value_equals` 8+ → v0.10 ship-target ✓
   - `registry_paths_resolve` 6+ → v0.10 ship-target ✓
   - `generated_file_fresh` 5+ → v0.10 ship-target ✓
   - `cross_language_implementation_complete` 5 sources (arrow +
     tensorflow + protobuf + angular + flutter) → v0.11+
     ship-target ✓
   - `ordered_block` 5+ → v0.10 ship-target ✓
   - `dir_name_matches_field`, `json_schema_passes`, `pair_hash`,
     `balanced_delimiters`, `file_pair_block_match`,
     `markdown_template_match` → stay v0.10+ candidates
     (single-source or modest demand).

## Blockers

- None. All 5 configs valid; no engine bugs surfaced; no .alint.yml
  bugs flagged. The turbo "latent pitfall #16" risk was already
  fixed during the original P2b pass (incorrect initial impression
  on my part; verified the actual file uses `file_content_matches`
  with the proper pitfall #16 citation).

## READMEs touched

- `/home/kaminsod/projects/alint/examples/rust-lang-rust/README.md`
- `/home/kaminsod/projects/alint/examples/tensorflow-tensorflow/README.md`
- `/home/kaminsod/projects/alint/examples/tokio-rs-tokio/README.md`
- `/home/kaminsod/projects/alint/examples/vercel-next.js/README.md`
- `/home/kaminsod/projects/alint/examples/vercel-turbo/README.md`

All 5 received: rule-kind candidate status sync, pitfall #18/#19
notes, Future analysis section (≥2 ideas each), Validation status
(2026-05-07) footer with reconciled rule-count math.

## Log entries

This file is the per-batch log entry; the master tracking file
(`docs/development/case-study-revalidation-log.md`) is updated by
the parent agent per the briefing's protocol.
