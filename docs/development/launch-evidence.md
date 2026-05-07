# Launch evidence — engineering audit summary

What 30 production OSS repos surfaced when their structural-validation
tooling was inventoried, rebuilt as alint configs, and compared. This
doc is engineering-audit observations only — pitfalls, rule-kind
candidates, scale numbers, and saturation analysis. The strategic
launch plan that drove the validation pass is private; the per-repo
configs and writeups it produced are public under [`examples/`](../../examples/).

## Case-study inventory (30 of planned 40)

**P2a — single-language + diverse-ecosystem (20 of 20 done):**
kubernetes, rust-lang/rust, golang/go, python/cpython, nodejs/node,
apache/airflow, denoland/deno, tokio-rs/tokio, astral-sh/uv,
astral-sh/ruff, clap-rs/clap, microsoft/typescript, facebook/react,
prettier/prettier, pnpm/pnpm, helm/helm, pytorch/pytorch, vercel/turbo,
apache/arrow (P2a polyglot win), vercel/next.js (P2a polyglot win).

**P2b Wave 1 — scale + governance + flagship-visibility polyglot (5 of
5 done):** NixOS/nixpkgs, bazelbuild/bazel, tensorflow/tensorflow,
apache/spark, microsoft/vscode.

**P2b Wave 2 — platform-driven polyglot density (5 of 5 done):**
angular/angular, istio/istio, dotnet/runtime, protocolbuffers/protobuf,
flutter/flutter.

Each case-study directory under `examples/<owner>-<repo>/` carries the
working `.alint.yml`, an inventory of the repo's existing tooling, the
mapping to alint's primitives, and a gap catalogue (what alint can't
express today). 10 more polyglot repos remain queued from the original
P2b plan; saturation analysis (below) explains why launch did not
block on them.

## Pitfalls catalogued (21 distinct)

The full catalogue with canonical-correct YAML for each lives at
[`docs/development/CONFIG-AUTHORING.md`](CONFIG-AUTHORING.md). Brief
inventory by source:

| Source | Pitfalls | Examples |
|---|---|---|
| P2a pilot (5 repos) | 12 (#1–#12) | `command:` argv-vs-command, JSONPath dashed-key bracket-notation, `scope_filter.has_ancestor` basename-only |
| P2a Wave 1 (5 repos) | 3 (#13–#15) | `file_content_matches` regex anchoring (`^`/`$` are file-not-line by default), YAML `\n` regex literal, `file_starts_with.prefix: ""` |
| P2a Wave 2 (5 repos) | 1 (#16) | `*_path_matches` against bool fields silently fires "not a string" |
| P2a Wave 3 (5 repos) | 1 (#17) | `*_path_equals` against `[*]` flips intent from "any" to "every" |
| P2b Wave 1 (5 repos) | 2 (#18–#19) | `.gitignore`-masked tracked files (the bazel `.bazelversion` pattern), `root_only: true` + multi-component literal |
| P2b Wave 2 (5 repos) | 2 (#20–#21) | Cross-file value-equality across structurally-different files, `yaml_path_*` multi-doc YAML failure |

Pitfalls #18 and #19 are fixed in the engine as of v0.9.16/v0.9.17
(per-rule `respect_gitignore: false` knob; literal-path runtime guard);
the rest are documented with workarounds + canonical-correct YAML.

7 silently-broken structured-path rules in committed pilot + Wave 1+2
configs (6 bool-match + 1 array-semantics) were caught and fixed
in-flight by the validation pass — not by the test suite. The
v0.9.16-shipped smoke-test fixture audit
(`crates/alint-e2e/tests/coverage_audit_smoke_fixtures.rs`) closes
that audit gap going forward.

## Rule-kind candidates surfaced

Aggregated from per-repo `examples/<owner>-<repo>/README.md` gap
catalogues. Demand counted as the number of distinct repos that
surface the same need (saturation signal).

**v0.10 ship-targets** (broad applicability, ≥4 sources or critical
infra-validation):

| Candidate | Demand | Notes |
|---|---|---|
| `registry_paths_resolve` (every path/key in a registry file resolves to an on-disk artefact) | 8 sources (rust, clap, cpython×2, next.js, arrow, pytorch, nodejs/node, NixOS×3) | Highest-leverage gap in P2a |
| `cross_file_value_equals` (incl. `cross_file_field_equals` variant) | 10 sources (airflow, tokio, clap, uv, react, pnpm, nodejs/node, pytorch, vscode, istio) | Past-saturation demand. istio surfaces the per-file-extractor refinement (pitfall #20) |
| `ordered_block` (lines between marker pairs sorted unique under configurable comparator) | 7 sources (rust, airflow, tokio, cpython, arrow, golang/go, protobuf failure_lists) | Ties with `registry_paths_resolve` at top of v0.10 backlog |
| `xml_path_matches` / `xml_path_equals` | 2 sources (spark 49 pom.xml, dotnet/runtime ~2,300 XML manifests at one OOM bigger scale) | Completes the structured-query family (JSON/YAML/TOML/XML) |
| `import_gate` (forbid imports of pattern X in path scope Y) | 4 sources (k8s, airflow, golang/go, pytorch) | Recurring shape |
| `generated_file_fresh` (run a generator, diff output against on-disk file) | 6 sources (uv, cpython, pytorch, bazel, TF, spark) | Tension: alint's deliberate non-goal is running codegen — propose as opt-in primitive |
| `pair_hash` (computed property of file A appears at offset Y in file A) | 3 sources (k8s, tokio, golang/go FIPS) | golang/go FIPS is the highest-stakes use case (CMVP submission references the file format) |

**v0.11+ ship-target:**

| Candidate | Demand | Notes |
|---|---|---|
| `cross_language_implementation_complete` (every type in a schema spec has a per-language test fixture) | 5 sources (arrow, TF, protobuf, angular, flutter) | Densest source: protobuf (10 in-tree language bindings + 1 spun-out, ~45 cross-language assertions one rule would express). 3 distinct topologies — data-format-driven (arrow, TF, protobuf), within-language source↔golden (angular), platform-driven (flutter's 6 native-OS embedders) |

**v0.10 design candidates** (≥2 sources, or shape clarity):

| Candidate | Demand | Notes |
|---|---|---|
| `*_path_contains` (set-membership shorthand for "value X is present in array at JSONPath Y") | helm, deno, bazel | Resolves pitfall #17 directly |
| `pair_inverse` (every partner traces back to a primary; reverse of `pair`) | ruff, angular | Snapshot freshness; covers `cargo insta --unreferenced=reject` and angular goldens parity |
| `command_idempotent` mode (run tool in --check mode, fail if working-tree would change) | ruff, prettier | mdformat, markdownlint, prettier, ruff-format, dprint-check all share this shape |
| `for_each_leaf_dir` / `iter.is_leaf` accessor | prettier, rust, ruff | Leaf-walk variant of `for_each_dir`. Extends existing rather than new kind |
| `balanced_delimiters` + `file_pair_block_match` | rust, cpython×2 | tidy::rustdoc_css_themes + cpython Argument Clinic block markers |
| `json_schema_passes` config-shape mode (validate config file against inline JSON Schema) | k8s, turbo | Replaces hand-rolled `argv:`-shape checks |
| `dir_name_matches_field` (directory basename matches a field inside a manifest in that directory) | turbo, next.js | per-package `name` field in package.json must equal directory name |
| `file_hash_not` / hash-denylist | Repolinter migration | Repolinter's `file-hash-not` axiom; current alint workaround is `file_content_forbidden` against known-bad substring |
| `multi_doc_mode:` knob on `yaml_path_*` (`error` / `first` / `every`) | istio | Resolves pitfall #21 |
| `value_extractor:` block on `cross_file_value_equals` (per-file-pattern extractor) | istio | Resolves pitfall #20 |

**Bundled-ruleset candidates:**

| Ruleset | Source | Status |
|---|---|---|
| `apache/governance@v1` (LICENSE+NOTICE+KEYS+RAT discipline) | arrow + spark + airflow (3 Apache TLPs converge on 9 of 12 governance artefacts) | v0.10 ship-target |
| `dotnet@v1` | dotnet/runtime | v0.10 ship-target. Adopter surface: every dotnet/* + every Azure SDK + every microsoft/* .NET project |
| `python/pep-621-shape@v1` | uv | v0.10 design |
| `rust/cargo-release-conventions@v1` | clap | v0.10 design |
| `cncf/owners@v1` (OWNERS file shape per k8s sig conventions) | helm | v0.10 design |
| `ruby@v1` / `swift@v1` / `objective-c@v1` / `erlang@v1` / `elixir@v1` | Repolinter migration | v0.10 design |

## Saturation analysis (when to stop adding repos)

Pitfall discovery rate by wave: pilot 12 → P2a Wave 1 (3) → P2a Wave 2
(1) → P2a Wave 3 (1) → P2b Wave 1 (2) → P2b Wave 2 (2). The runtime-
semantics class (#13–#21) is distinct from the schema/regex class
(#1–#12), suggesting the smoke-test fixture audit closes the right
gap.

Rule-kind candidate saturation: by P2a Wave 3, ~80 % of new
candidates surfaced were single-source or refinements of existing
ones; ≥3-source candidates stopped appearing after P2a Wave 2. P2b
Waves 1+2 added zero new ≥3-source candidates — every Wave finding
either reconfirmed an existing v0.10 candidate with deeper data
(`xml_path_*` promoted from "v0.10 candidate" to "v0.10 ship-target"
via dotnet stress; `cross_language_implementation_complete` saturated
to 5 sources), or refined an existing one (istio's `value_extractor:`
shape is a refinement of `cross_file_value_equals` rather than a new
candidate).

**Implication:** the remaining 10 polyglot repos in the original P2b
plan would optimise for *new narrative shapes* (additional
multi-language stress, additional scale stress) rather than *new
rule-kind candidates*. They're queued for post-launch evidence
expansion rather than launch-blocking.

## Scale validation

| Repo | Files | Wall-clock | Rules |
|---|---|---|---|
| NixOS/nixpkgs | 39,101 (+ 20,678 by-name pkg dirs) | 273 ms | 79 |
| Per the public bench corpus | up to 1M | ~12 s | varies |

NixOS at 39k files / 20k by-name pkg dirs is the largest validated
single-repo case. The full 79-rule pass — including a `for_each_dir`
over the by-name tree — completes in 273 ms wall-clock. Combined with
the existing per-release public bench corpus (up to 1M files), "any
size repo" claims are empirically defensible.

## Per-repo case-study contribution workflow

Adding a 31st (or 41st) case study follows this shape:

1. **Sparse-clone the repo** at a captured commit SHA. Heavy test
   trees / vendor dirs excluded via `git sparse-checkout`. Record the
   commit SHA + working-tree size in the case-study README header so
   the writeup is reproducible.
2. **Inventory the existing tooling.** What lints today? Pre-commit
   hooks, `Makefile` validation targets, custom verify scripts, GHA
   workflows, language-specific tooling. Counted both in number of
   distinct surfaces and in number of discrete checks.
3. **Map to alint primitives.** For each existing check, which alint
   rule kind / bundled ruleset replaces it cleanly? Which need a
   `command:` shellout? Which can't be expressed at all today? The
   gaps become rule-kind candidates.
4. **Write the `.alint.yml`.** Ship the magic comment
   (`# yaml-language-server: $schema=…`). Use bundled rulesets where
   they apply; fall through to per-rule entries for the repo-specific
   conventions.
5. **Validate.** `alint validate-config <path>` for parse-time
   correctness. Against the live tree:
   `alint check --config <path> /path/to/repo` and triage the
   violation count (legitimate vs. false-positive vs. style
   disagreement).
6. **Write the README.** Headline finding (the catch that beats the
   repo's existing tooling). Structural inventory. % maps cleanly /
   % shellouts / % out of scope. Gap catalogue. Recommendation.
7. **Add to the ✅ checklist** in `examples/README.md` with a
   one-liner pointing at the headline finding.

The `crates/alint-e2e/tests/coverage_audit_examples_parse.rs` audit
fires at PR time to ensure every shipped `.alint.yml` loads + builds
cleanly. Drift in a bundled ruleset is caught before the case study
goes stale.

---

The strategic launch plan (timeline, marketing roadmap, hero copy
plans, P3-P5 phases) is private. This doc is intended to be the
public engineering record that the strategic plan referenced.
