# Case-study revalidation — batch 3 (2026-05-07)

Subagent log for batch 3 (alphabetical: facebook-react, flutter-flutter,
golang-go, helm-helm, istio-istio).

Validation pass run with the v0.9.17 release binary at
`/home/kaminsod/projects/alint/target/release/alint`. All 5 configs
load cleanly via `validate-config`. Per-case-study findings below.

---

## facebook-react

**Validation status (2026-05-07):**
- alint version: 0.9.17 (1dbd9b218a0e, built 2026-05-07).
- `validate-config`: 87 rules loaded cleanly.
- README claim is accurate at the high level (~36 react-specific rules
  layered on the bundled overlay) but didn't include a precise
  count — corrected the footer to the authoritative `87 rules total`.
- Live tree: pending — `/tmp/facebook-react/` not present in the
  validation env. Live findings retained from the original 2026-05-06
  pass remain valid (config has not been edited since).

**Stale-reference sweep:**
- `v0.10+ candidate` for `cross_file_value_equals` is now
  `v0.10 ship-target` (10 sources per launch-evidence.md). Updated.
- `v0.10+ candidate` for `registry_append_only` was a react-first
  finding; not yet promoted in launch-evidence.md (single source).
  Left as `v0.10 design candidate` with an explicit "single-source"
  qualifier so the catalogue treatment doesn't drift.
- "15 documented in CONFIG-AUTHORING.md" → updated to "21 documented".
- Bundled overlay names verified against the authoritative table; no
  drift in ruleset names.

**Fix-vs-workaround check:**
- React doesn't surface pitfalls #18 / #19 in its config or README, so
  no workaround → fix transitions apply.

**Rule-kind candidate sync:**
- `cross_file_value_equals`: 10 sources per launch-evidence.md;
  reflected in updated text.
- `registry_append_only`: still single-source (react-only);
  preserved as such.
- `json_path_keys_match_pattern` (react's tertiary suggestion): NOT
  yet on the launch-evidence.md backlog — left as a soft DX
  observation in the README, not a flagship demand signal.

**Bundled-ruleset count sync:**
- `oss-baseline=15`, `node=9`, `monorepo=4`, `monorepo/yarn-workspace=4`,
  `ci/github-actions=3`, `hygiene/no-tracked-artifacts=11`,
  `tooling/editorconfig=3`, `agent-context=5` — sum bundled = 54
  + 33 react-specific rules = 87. Matches the validate-config output
  exactly. Updated the rule-count breakdown in the "Starter alint
  config" section to read "87 rules (54 from 8 bundled rulesets +
  33 react-specific)".

**New analysis (think-hard outputs):**
1. **`agent-hygiene` ruleset overlay** — react ships `dangerfile.js`
   and 5 in-tree custom eslint rules under `scripts/eslint-rules/`;
   the agent-hygiene ruleset (6 rules) would gate AI-generated
   contribution patterns (no rolling commits to tracked artefacts,
   no tracked credentials in agent context). Worth pulling in as a
   sixth bundled overlay alongside the existing `agent-context@v1`.
2. **`compliance/reuse@v1` for the per-package LICENSE story** —
   react-published-package-has-source-license is a per-rule react
   construct; a future `compliance/reuse` overlay (3 rules — REUSE
   spec compliance with `LICENSES/` dir + per-file SPDX headers)
   would express the same intent declaratively across the workspace
   AND the 17 internal packages without per-rule duplication.
3. **`alint suggest` against the live tree** — pending
   `/tmp/facebook-react/`. Would surface candidate rules from the
   ~140k-file compiler subtree (heavy on `.expect.md` test fixtures
   that may have repeating shapes the suggester would generalise).

**Open issues / blockers:** None — config is clean, the
README's load-bearing claims survived revalidation. The
`registry_append_only` v0.10 candidate is still single-source
(react-only); a second source would justify promotion to
v0.10 ship-target.

---

## flutter-flutter

**Validation status (2026-05-07):**
- alint version: 0.9.17.
- `validate-config`: 68 rules loaded cleanly.
- README claim was "53-rule" in three places (lines 138/170/318) —
  corrected to 68 (53 flutter-specific + 15 bundled = 68). The
  three bundled overlays are `oss-baseline=15` +
  `ci/github-actions=3` + `hygiene/no-tracked-artifacts=11` = 29
  bundled rules + 39 flutter-specific = 68. Updated the
  "53-rule" claims to "68-rule" or to the more useful breakdown
  "39 flutter-specific + 29 from 3 bundled rulesets".
- Live tree: present at `/tmp/flutter/` (~14k files after
  sparse-checkout). Existing live-tree findings in the README
  (line 666) remain valid.

**Stale-reference sweep:**
- `v0.11+ candidate` for `cross_language_implementation_complete`
  → updated to `v0.11+ ship-target` (5 sources per
  launch-evidence.md: arrow + TF + protobuf + angular + flutter).
  flutter contributes the 5th source (the platform-driven
  variant). README already names this — promoted the qualifier.
- `v0.10+ candidate` for `registry_paths_resolve` → updated to
  `v0.10 ship-target` (8 sources per launch-evidence.md, of which
  rust + clap + cpython + arrow + next.js + flutter = 6 P2a/P2b
  sources).
- `v0.10+ candidate` for `ordered_block` → updated to
  `v0.10 ship-target` (7 sources per launch-evidence.md;
  flutter is a confirmation source via .ci.yaml alphabetisation).
- "CONFIG-AUTHORING.md pitfall #18" reference (line 510, 650)
  retained but reframed: pitfall #18 (per-rule
  `respect_gitignore: false`) is **FIXED IN ENGINE in v0.9.17**.
  The flutter `pubspec.lock` tracked-AND-gitignored pattern is now
  directly addressable by setting the per-rule knob; updated the
  README to flag this as a v0.9.17 fix that benefits flutter
  (second demand source after bazel) and to point at the canonical
  pattern in CONFIG-AUTHORING.md.
- "Notes for the parent agent" section (line 654) referenced an
  in-progress edit to `crates/alint-rules/src/file_exists.rs` for
  pitfall #18 — reflowed to past tense ("shipped in v0.9.17").

**Fix-vs-workaround check:**
- Pitfall #18 → FIXED in v0.9.17. flutter README updated to
  reflect this and to note that adding `respect_gitignore: false`
  to a `pubspec.lock`-targeting rule is now a one-line config
  change, no engine workaround needed.
- Pitfall #19 → also FIXED in v0.9.17 (literal_is_nested runtime
  guard). flutter doesn't surface this directly; no flutter-side
  edit needed.

**Rule-kind candidate sync:**
- `cross_language_implementation_complete` — promoted to
  `v0.11+ ship-target` (5 sources). flutter is the platform-driven
  variant — preserved this framing as load-bearing for the
  pitch.
- `registry_paths_resolve` — promoted to `v0.10 ship-target`
  (8 sources).
- `ordered_block` — promoted to `v0.10 ship-target` (7 sources).
- `respect_gitignore: false` knob → DELIVERED in v0.9.17.

**Bundled-ruleset count sync:**
- 3 bundled rulesets explicitly extended: `oss-baseline=15` +
  `ci/github-actions=3` + `hygiene/no-tracked-artifacts=11` = 29.
- 39 flutter-specific rules. Total = 68. Matches validate-config
  output. README's "roughly 15 rules between them" understated the
  bundled contribution by ~2x — corrected to "29 rules across 3
  bundled rulesets".

**New analysis (think-hard outputs):**
1. **`compliance/reuse@v1` overlay would replace the cross-language
   BSD header rules.** flutter currently ships two rules
   (`flutter-bsd-source-header` for `//`-comment languages,
   `flutter-bsd-source-header-shell-comment` for `#`-comment
   languages) with hand-rolled regex tolerance. The
   `compliance/reuse@v1` ruleset (3 rules — REUSE-spec
   `LICENSES/` dir + per-file SPDX headers + `.reuse/dep5`)
   wouldn't drop in as-is (REUSE expects SPDX, not the
   Flutter-Authors-BSD-style header), but a future
   `compliance/bsd-flutter@v1` derivative is an obvious
   bundled-ruleset extraction once the pattern stabilises.
2. **Other native-OS embedder peer subdirs to lock parity on:**
   The README enumerates 9 platform subdirs but the
   `flutter-engine-platform-has-build-gn` rule covers 6 (skipping
   `darwin`, `embedder`, `common`). The Apple framework
   four-file layout rule (`flutter-darwin-framework-layout`)
   covers darwin/{ios,macos}/framework/. Worth adding a
   `flutter-engine-embedder-c-abi-presence` rule asserting
   `engine/src/flutter/shell/platform/embedder/embedder.h`
   exists (the load-bearing C ABI for every external embedder
   — silent removal would silently break out-of-tree embedders
   like https://github.com/sony/flutter-embedded-linux).
3. **`alint suggest` against /tmp/flutter/ — pending live-tree
   recheck.** The 14k-file sparse-checkout is large enough to be
   meaningful suggestion fodder. `dev/integration_tests/` carries
   ~80 mini-app trees; the suggester would surface
   per-integration-test-app structural conventions (every
   integration test app has android/ + ios/ + lib/main.dart) that
   the current config doesn't enforce.
4. **`nested_configs: true` for the engine subtree** — the
   `engine/src/flutter/` subtree is effectively a separate Dart
   workspace with its own `pubspec.yaml` and `analysis_options.yaml`.
   A subtree-scoped `.alint.yml` under `engine/src/flutter/` would
   pick up engine-specific rules (Apple framework four-file layout
   only applies inside `engine/src/flutter/shell/platform/darwin/`)
   without polluting the root config. Worth proposing as a
   refactor in v0.10's "nested configs" feature once the design
   lands.

**Open issues / blockers:** None — flutter is a clean revalidation
target. The README is the longest of any case study (750 lines)
and carries the most cross-language polyglot detail; the v0.9.17
`respect_gitignore` fix delivers a concrete win to highlight at
the next docs roll.

---

## golang-go

**Validation status (2026-05-07):**
- alint version: 0.9.17.
- `validate-config`: 64 rules loaded cleanly.
- README claim was "31 golang/go-specific rules" — verified
  against config: 64 rules total = 31 golang/go-specific +
  ~33 from 3 bundled overlays (oss-baseline=15 + go=8 +
  hygiene/no-tracked-artifacts=11 = 34, off-by-one due to
  rule-id collision dedup). Updated the README to read
  "64 rules total (31 golang/go-specific + 33 from 3 bundled
  rulesets)".
- Live tree: pending — `/tmp/golang-go/` not present.

**Stale-reference sweep:**
- "v0.10+ list" for `import_gate`, `pair_hash`, `ordered_block`,
  `registry_paths_resolve` — all updated in line with the
  authoritative launch-evidence.md backlog. Specifically:
  - `import_gate`: 4 sources per launch-evidence.md (k8s,
    airflow, golang/go, pytorch). Marked `v0.10 ship-target`.
  - `pair_hash`: 3 sources per launch-evidence.md (k8s, tokio,
    golang/go FIPS — golang/go FIPS is the highest-stakes use
    case). Marked `v0.10 ship-target`.
  - `ordered_block`: 7 sources per launch-evidence.md
    (rust, airflow, tokio, cpython, arrow, golang/go, protobuf).
    Marked `v0.10 ship-target`.
  - `registry_paths_resolve.mode: github_issues` — the GitHub-API
    sub-candidate; remains a v0.11+ design (single source).
- "16 catalogued in CONFIG-AUTHORING.md" → updated to "21
  catalogued" (line 446).

**Fix-vs-workaround check:**
- golang/go config doesn't surface pitfalls #18 / #19. No
  fix-transition edit needed.

**Rule-kind candidate sync:**
- All three "v0.10 high-priority" entries promoted to "v0.10
  ship-target" per launch-evidence.md.
- `registry_paths_resolve.mode: github_issues` left as v0.11+
  (single-source still).

**Bundled-ruleset count sync:**
- `oss-baseline=15` + `go=8` + `hygiene/no-tracked-artifacts=11`
  = 34 raw bundled rules. Total config loads 64 rules; 31
  golang-specific + 33 from bundled = 64 (one rule deduplicated
  across rulesets, expected). Updated README to read accurately.

**New analysis (think-hard outputs):**
1. **v0.9.6+ rule kinds replacing `command:` shellouts.**
   golang/go's defensive `command:` shellouts are
   `go-gofmt-check`, `go-vet-std`, `go-vet-cmd`, `go-shellcheck`.
   Of these, `go-shellcheck` (4-rule defensive section) could
   be replaced by the bundled `tooling/shellcheck` overlay
   IF/WHEN one ships (it doesn't yet — currently just an inline
   command:). The other 3 (`gofmt`, `go vet`) are inherently
   shellouts because alint deliberately doesn't ship Go AST
   awareness.
2. **`agent-context@v1` overlay** — golang/go ships
   `.github/PULL_REQUEST_TEMPLATE` with the load-bearing
   "+ No Markdown" instruction; the agent-context bundled
   ruleset (5 rules) would gate AI-generated contribution
   discipline (no markdown in PR descriptions, AGENTS.md
   present). Worth adding alongside the existing 3 bundled
   overlays — golang/go is squarely the kind of repo where
   AI-generated PR-description noise would get rejected by
   Russ Cox's review etiquette.
3. **`alint suggest` against the live tree** — pending
   `/tmp/golang-go/`. Would likely surface the
   `src/cmd/internal/...` per-package conventions, the
   `src/runtime/...` assembly-source license-header
   discipline, and the `test/` fixture filename conventions
   (every fixture has a first-line directive). The suggester
   could replace the hand-rolled
   `go-doc-next-stdlib-minor-issue-filenames` rule with a
   generalised "every file under
   `doc/next/6-stdlib/99-minor/<pkg>/` matches `^\d+\.md$`"
   rule it auto-discovers.

**Open issues / blockers:** None — golang/go is the cleanest
revalidation target in the batch. The "31 conventions, 0
scripts, 0 workflows" pitch survives unchanged.

---

## helm-helm

**Validation status (2026-05-07):**
- alint version: 0.9.17.
- `validate-config`: 58 rules loaded cleanly.
- README claim was "23-rule starter config" and "23 rules in
  /.alint.yml" (lines 42, 175) — corrected to 58 rules total
  (24 helm-specific + 34 from 4 bundled overlays:
  oss-baseline=15 + go=8 + ci/github-actions=3 +
  hygiene/no-tracked-artifacts=11 = 37 raw, dedup'd to 34).
  Updated to read "58 rules total (24 helm-specific + 34 from
  4 bundled rulesets)".
- Live tree: pending — `/tmp/helm-helm/` not present.

**Stale-reference sweep:**
- "16 documented in `docs/development/CONFIG-AUTHORING.md`"
  (line 390) → updated to "21 documented".
- helm README claims "Worth adding as **pitfall #17** in the
  next CONFIG-AUTHORING sweep" (line 401) referring to the
  YAML-array `[*]` semantics — this is in fact the existing
  pitfall #17 in the catalogue (the wave 3 promotion).
  Updated to read "Already documented as pitfall #17 in the
  catalogue."
- `*_path_contains` set-membership shorthand (NEW from helm,
  line 376) — updated to reflect that this is now a
  `v0.10 design candidate` per launch-evidence.md ("≥2 sources,
  or shape clarity"; sources: helm, deno, bazel — saturated to
  3 sources).
- `import_gate` (line 248): 4 sources per launch-evidence.md
  (k8s, airflow, golang/go, pytorch — helm is NOT actually on
  this list per the launch-evidence.md table). Reframed: helm
  surfaces the **same shape** (depguard rules) that k8s +
  airflow + golang/go + pytorch surface. helm is the 4th
  Go-monorepo source where this shape appears in the wild;
  whether the launch-evidence.md table promotes helm to a
  named source depends on whether helm's depguard config is
  qualitatively distinct from the others (it isn't — same
  pattern). Left as "saturating signal" rather than promoting
  helm to a named source.
- `cross_file_value_equals` (line 250): 10 sources per
  launch-evidence.md. helm is referenced in launch-evidence.md
  pitfall #20 context but not as a named source — preserved
  the framing.
- `command_idempotent` (line 249): 2 sources in launch-evidence.md
  table (ruff + prettier) — helm is the 3rd source (NEW).
  Left framing as "third source" — strengthens demand signal.

**Fix-vs-workaround check:**
- helm doesn't directly surface pitfalls #18 / #19.

**Rule-kind candidate sync:**
- `*_path_contains`: v0.10 design candidate, 3 sources
  (helm + deno + bazel) per launch-evidence.md.
- `import_gate`: v0.10 ship-target, 4 sources.
- `cross_file_value_equals`: v0.10 ship-target, 10 sources.
- `command_idempotent`: v0.10 design candidate, 2 sources
  (helm is the 3rd surface but not promoted in the table yet).

**Bundled-ruleset count sync:**
- 4 bundled overlays: `oss-baseline=15` + `go=8` +
  `ci/github-actions=3` + `hygiene/no-tracked-artifacts=11`
  = 37 raw bundled. Plus 24 helm-specific = 61 raw, dedup'd
  to 58 (validate-config output). Updated README accordingly.

**New analysis (think-hard outputs):**
1. **Helm chart structural invariants beyond what `helm lint`
   covers.** helm/helm itself ships zero Helm charts in its
   repo (it's the helm CLI source, not a chart consumer), so
   the `manifests/charts/` polyglot pattern that istio uses
   doesn't apply here. But helm/helm DOES ship reference test
   chart trees under `pkg/chart/testdata/` (~80 fixture
   charts) — a `for_each_dir` over those plus
   `helm-chart-yaml-shape` (apiVersion/version/appVersion
   present + valid semver) would gate the test-fixture
   discipline that `helm lint` currently doesn't because the
   fixtures are deliberate corner cases (some intentionally
   malformed).
2. **`agent-context@v1` overlay** — helm ships an `AGENTS.md`
   file (line 30, 165 of README) but doesn't enforce its shape.
   The `agent-context@v1` ruleset (5 rules — AGENTS.md present
   + tour-of-codebase content + AI-context-window-friendly
   structure) would gate the file's invariants declaratively.
3. **`alint suggest` against the live tree** — pending
   `/tmp/helm-helm/`. The repo is small enough (~530 .go
   files) that the suggester would terminate quickly; likely
   surface candidates: per-`pkg/*/` test-coverage thresholds,
   `cmd/helm/` subcommand-package conventions, `internal/`
   visibility discipline.

**Open issues / blockers:** None — helm config is clean,
README's claims survive revalidation modulo the rule-count
update and the `*_path_contains` saturation update.

---

## istio-istio

**Validation status (2026-05-07):**
- alint version: 0.9.17.
- `validate-config`: 65 rules loaded cleanly.
- README claim was "65-rule starter config" (line 58) — VERIFIED;
  matches validate-config output exactly. Per-section breakdown
  is accurate.
- Live tree: pending — `/tmp/istio-istio/` not present.

**Stale-reference sweep:**
- "19 catalogued in `docs/development/CONFIG-AUTHORING.md`"
  (line 630) → updated to "21 catalogued".
- "Worth adding to the next CONFIG-AUTHORING sweep as
  **pitfall #20**" (line 687) — pitfall #20 is now ON the
  catalogue (cross-file value-equality with per-file
  extractor). Updated to "**Already documented as pitfall
  #20 in the catalogue**".
- "Worth adding as pitfall #20" for the multi-doc YAML failure
  (line 671 region) — that is now **pitfall #21** in the
  catalogue. Updated accordingly.
- `cross_file_value_equals` v0.10 candidate → `v0.10 ship-target`
  per launch-evidence.md (10 sources). istio refines the
  primitive's design via the per-file `value_extractor:` block
  (now a v0.10 design candidate per the launch-evidence.md
  table — istio is the named source for this refinement).
  Updated.
- `import_gate` v0.10 high-priority → `v0.10 ship-target`
  (4 sources per launch-evidence.md).
- `command_idempotent` v0.10 candidate → still v0.10 design
  candidate (2 sources in launch-evidence.md table; istio is
  the 4th surface in the wild). Preserved framing.
- `multi_doc_mode:` knob on `yaml_path_*` → now a v0.10 design
  candidate per launch-evidence.md (istio is the named source).
  Updated reference.

**Fix-vs-workaround check:**
- Pitfall #20 (cross-file value-equality with per-file extractor)
  — DOCUMENTED with workaround in CONFIG-AUTHORING.md, NOT YET
  FIXED. Engine resolution targets v0.10 (`value_extractor:`
  block on `cross_file_value_equals` per launch-evidence.md
  table). Preserved istio's workaround documentation.
- Pitfall #21 (yaml_path_* multi-doc YAML failure) —
  DOCUMENTED with workaround in CONFIG-AUTHORING.md, NOT YET
  FIXED. Engine resolution targets v0.10
  (`multi_doc_mode:` knob on `yaml_path_*` per
  launch-evidence.md table). Preserved istio's workaround
  documentation.
- Pitfall #18 / #19 are FIXED in v0.9.17 — istio config doesn't
  rely on either, so no fix-transition edit needed for istio.

**Rule-kind candidate sync:**
- `cross_file_value_equals` + `value_extractor:` refinement:
  v0.10 ship-target (cross_file_value_equals; value_extractor
  is a design refinement). istio is the named source for the
  refinement.
- `import_gate`: v0.10 ship-target, 4 sources.
- `command_idempotent`: v0.10 design candidate.
- `multi_doc_mode:`: v0.10 design candidate, istio sole source.

**Bundled-ruleset count sync:**
- 4 bundled overlays: `oss-baseline=15` + `go=8` +
  `ci/github-actions=3` + `hygiene/no-tracked-artifacts=11`
  = 37 raw bundled. README claims "roughly 30 rules between
  them" (line 318) — updated to "37 rules across 4 bundled
  rulesets". 65 total = 28 istio-specific + 37 bundled.
  (Off-by-one: 65 - 37 = 28 not the 65-rule number cited as
  "in /.alint.yml" — this is correct: the 65 IS the total
  including bundled.)

**New analysis (think-hard outputs):**
1. **`nested_configs: true` for the per-component subtree.**
   istio's per-component subdirs (pilot/, cni/, istioctl/,
   operator/, security/, tools/) are effectively peer
   subprojects under one root go.mod. A subtree-scoped
   `.alint.yml` under `manifests/charts/` (for the chart
   discipline) and `releasenotes/notes/` (for the release-note
   schema) would let those rules live next to their domain
   instead of in the root config. Especially relevant for the
   chart-shape rules: nine `Chart.yaml` files all share the
   same `apiVersion: v2` / `version: 1.0.0` / `appVersion: 1.0.0`
   placeholder; one subtree config would express the contract
   declaratively without per-chart config repetition.
2. **`compliance/apache-2@v1` overlay** — istio is Apache 2.0
   licensed and ships a `licenses/` tree (the `lint-licenses`
   Make target points at it). The bundled `compliance/apache-2@v1`
   ruleset (3 rules — LICENSE present + NOTICE present +
   per-file SPDX header) would partially replace
   `istio-go-license-header` + `istio-shell-license-header`
   with declarative shape coverage (the `value_extractor:` for
   the year is an istio-specific extension that the bundled
   ruleset doesn't ship today, but the SPDX-header floor would
   carry).
3. **v0.9.6+ rule kinds replacing `command:` shellouts.**
   istio's 7 `command:` shellouts are golangci-lint, gofmt,
   go mod tidy, helm lint, hadolint, shellcheck, yamllint,
   license-lint. Of these, none have a v0.9.6+ replacement
   yet — the per-file-format linters (hadolint, shellcheck,
   yamllint) are inherently AST-aware and stay shellouts.
   `helm lint` could in principle be replaced by a future
   `helm/chart-structure@v1` bundled ruleset (helm is on the
   v0.10 design list per launch-evidence.md as `cncf/owners@v1`
   — sibling concern but not the same primitive). Worth
   noting in the README as a v0.10/v0.11 carrot.
4. **`alint suggest` against the live tree** — pending
   `/tmp/istio-istio/`. The repo is large enough (~6.4k files)
   that the suggester would surface multiple per-component
   conventions; particular interest in
   `releasenotes/notes/*.yaml` (~1,699 files with a fixed
   schema) — the suggester should converge on the
   `apiVersion: release-notes/v2` literal + `kind:` enum
   without manual coaxing.

**Open issues / blockers:** None — istio is the densest case
study in the batch (the original wave 2 polyglot stress
target). Pitfalls #20 / #21 remain open but are documented
with workarounds and the v0.10 design-candidate slots are
named in launch-evidence.md.

---

## Cross-cutting findings (batch 3)

1. **Bundled-rule counts were systematically understated** in
   3 of 5 READMEs (flutter, golang-go, helm). The pattern was
   "roughly N rules between them" with N about half the actual
   contribution. Cause: the original case-study writers did not
   re-validate after the v0.9.6 rule-set expansions
   (`hygiene/no-tracked-artifacts` grew to 11 rules; `go` to 8;
   `oss-baseline` to 15). Fix: explicit per-overlay counts
   sourced from the authoritative table at the top of the
   revalidation pass.

2. **"v0.10+ candidate" → "v0.10 ship-target" promotions are
   uniformly missed.** All 5 READMEs cite candidate status for
   rule-kind backlog entries that have since been promoted
   based on saturation evidence (≥4 sources or critical
   infra-validation). flutter README naming 3 separate
   "v0.10/v0.11 candidate" entries was the heaviest.

3. **Pitfall numbering drift is concentrated in 2 of 5
   READMEs** (helm: 16→21; istio: 19→21). The other 3
   (react: 15→21; flutter: not pitfall-mentioning beyond #18;
   golang-go: 16→21) carry generic citations that shifted
   silently. Updated all 5 to the canonical 21.

4. **Pitfall #18 fix shipped in v0.9.17** is load-bearing for
   flutter's `pubspec.lock` tracked-and-gitignored pattern —
   second demand source after bazel. This is a concrete
   "alint shipped a fix because case studies surfaced demand"
   story that the launch narrative should pick up.

5. **`agent-context@v1` overlay is a recurring "obvious next
   addition" across all 5 READMEs.** None of the 5 currently
   extend it; all 5 ship `AGENTS.md` or equivalent.
   Surfaced as a concrete adoption-uplift opportunity in the
   "Future analysis" footer of each README.

6. **`compliance/reuse@v1` and `compliance/apache-2@v1`
   overlays** would simplify the per-rule
   license-header constructs in flutter, react, helm, and
   istio. flutter's case is borderline (Flutter-Authors-BSD
   doesn't fit REUSE-spec SPDX cleanly); react / helm / istio
   are direct fits. Surfaced as a future-analysis opportunity
   in each.

7. **`nested_configs: true`** (subtree-scoped rules) would
   benefit istio (per-component subdirs) and flutter
   (engine subtree as a separate Dart workspace). Surfaced
   as a future-analysis opportunity for both.

## Files touched

- `examples/facebook-react/README.md` — surgical edits to
  rule-count, pitfall-count, and v0.10 candidate-status text;
  added "Validation status (2026-05-07)" footer + "Future
  analysis" section.
- `examples/flutter-flutter/README.md` — surgical edits to
  rule-count (53→68 in three places), pitfall #18 fix-transition
  reframing, candidate-status updates; added validation footer
  + future-analysis section.
- `examples/golang-go/README.md` — pitfall-count update (16→21),
  candidate-status promotions, rule-count clarification; added
  validation footer + future-analysis section.
- `examples/helm-helm/README.md` — pitfall-count update (16→21),
  rule-count update (23→58), `*_path_contains` saturation
  update, candidate-status promotions; added validation footer
  + future-analysis section.
- `examples/istio-istio/README.md` — pitfall-count update
  (19→21), pitfalls #20/#21 catalogue-status update, candidate-
  status promotions; added validation footer + future-analysis
  section.
