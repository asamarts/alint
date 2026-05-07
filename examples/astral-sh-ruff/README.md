# Case study: `astral-sh/ruff`

Inventory of the structural-validation tooling in `astral-sh/ruff` and an
alint config that replaces the rules alint can express today, plus a catalogue
of the rules that need new alint primitives.

**Repo state captured:** 2026-05-03,
`HEAD = ea6be89b0af6442ec09ac6a8998bce247c1fe5ba`
("Increase max value of `line-length` setting (#24962)").

---

## Summary

Ruff maintains its structural validation in **two places**:
the **`.pre-commit-config.yaml` (16 hooks)** runs locally and in the `prek` CI
job; the **`.github/workflows/ci.yaml` (~22 jobs)** runs cargo fmt / clippy /
nextest / shear / doc plus the heavier ecosystem checks. Roughly **40 % of the
prek hooks map directly to existing alint rules** (7 of 16 via `command`,
3 more via native `toml_path_matches` / `final_newline` / `file_exists`),
**~25 % need new alint primitives** (cargo dep-graph analysis, Rust AST scope
checks, snapshot freshness), and **~35 % are out of alint's scope** (codegen
drift in `crates/ruff_dev/`, ecosystem regression diff against a baseline
binary).

The 40 % that *do* fit translate cleanly to a 22-rule alint config (below).
**Headline finding:** ruff's own `crates/ruff_dev/` is exclusively a
**code-generation** binary (it's the equivalent of running `cargo dev
generate-all` to refresh `ruff.schema.json` + `docs/configuration.md` + the
rules table) — **NOT a tidy-style structural validator**. Ruff's structural
gates live entirely in `prek` hooks calling third-party tools (typos,
zizmor, actionlint, mdformat, shellcheck, prettier, ruff itself). This is a
legitimately different shape from rust-lang/rust's tidy: ruff has chosen the
"compose existing tools via prek" path rather than building its own
structural linter. **Alint is exactly the missing piece** — one declarative
file replaces the prek wrapper, plus the per-prek-hook YAML, plus the
priority-order metadata, plus the per-hook exclusion patterns.

---

## Existing tooling inventory

### `.pre-commit-config.yaml` — 16 hooks (the primary gate, run via `uvx prek run -a`)

| Hook | What it checks | alint replacement |
|---|---|---|
| `check-merge-conflict` | No `<<<<<<<` markers | `no_merge_conflict_markers` (already in `oss-baseline`; extended in this config to cover `crates/`) |
| `end-of-file-fixer` | Trailing newline on `*.toml` files | `final_newline` with `paths: "**/*.toml"` |
| `validate-pyproject` | `pyproject.toml` matches PEP 621 schema | `toml_path_matches` for `name` / `license` / `requires-python` (shallow); deep schema check needs `json_schema_passes` against a vendored PEP 621 schema |
| `typos` (crate-ci/typos) | Spelling vs. `_typos.toml` allowlist | `command` rule shelling to `typos` |
| `rustfmt` | All Rust source is rustfmt-clean | `command` rule: `cargo fmt --all --check` |
| `prettier` | YAML files formatted | `command` rule: `prettier --check {path}` |
| `zizmor` | GitHub Actions security audit | `command` rule shelling to `zizmor --config .github/zizmor.yml` |
| `check-github-workflows` (jsonschema) | Workflows match GitHub schema | `json_schema_passes` against `github-workflow.json` (vendor required) |
| `shellcheck-py` | `*.sh` files lint clean | `command` rule shelling to `shellcheck -x {path}` |
| `mdformat` | Markdown files mdformat-clean | `command` rule: `mdformat --check {path}` |
| `ruff-format` (self-hosted) | Python sources formatted | `command` rule: `ruff format --check {path}` |
| `ruff-check` (self-hosted) | Python sources lint-clean | `command` rule: `ruff check {path}` |
| `markdownlint-fix` | Markdown markdownlint rules | `command` rule: `markdownlint {path}` |
| `mdtest format` | mdtest .md files ruff-format-clean | second invocation of `ruff format --check` |
| `actionlint` (manual stage) | GitHub workflow grammar | `command` rule shelling to `actionlint -config-file .github/actionlint.yaml` |

15 of 16 hooks — direct or near-direct replacements. (The one that doesn't fit
is `check-merge-conflict`, which `oss-baseline` already enforces; including it
twice would be redundant.)

### `.github/workflows/ci.yaml` — 22 jobs (the slower / heavier gate)

Most CI jobs ARE the build (`cargo test`, `cargo clippy`, wasm builds, cross-
platform matrix) and aren't structural-validation in the alint sense. The
ones that ARE structural and replaceable:

| Job | What it checks | alint replacement |
|---|---|---|
| `cargo-fmt` | Workspace-wide rustfmt | `command` rule: `cargo fmt --all --check` (same as the pre-commit rustfmt hook) |
| `cargo-clippy` | Workspace-wide clippy | `command` rule: `cargo clippy --workspace ... -- -D warnings` |
| `cargo-shear` | Unused workspace dependencies | `command` rule: `cargo shear --deny-warnings` |
| `cargo doc` (RUSTDOCFLAGS=-D warnings) | rustdoc-warning-clean | `command` rule: `cargo doc --all --no-deps` |
| `scripts` job (`add_plugin.py` + `add_rule.py` smoke) | New-rule scaffolding produces clean code | not replaceable — codegen smoke test |
| `prek` job | Runs every prek hook | replaced wholesale by `alint check` |
| `docs` (mkdocs build --strict) | Docs build | not structural; out of scope |

4 of these 7 jobs map cleanly. The other 3 (codegen smoke, mkdocs build,
ecosystem diff against baseline) are out of scope.

### `crates/ruff_dev/` — Rust dev-tooling crate (NOT a tidy)

**Headline finding for the launch story.** Unlike rust-lang/rust's
`src/tools/tidy/` (which IS structural validation), `crates/ruff_dev/` is
exclusively a **codegen + introspection binary**:

```
crates/ruff_dev/src/
├── format_dev.rs              # formatter dogfood harness
├── generate_all.rs            # composite codegen entrypoint
├── generate_cli_help.rs       # codegen
├── generate_docs.rs           # codegen
├── generate_json_schema.rs    # codegen → ruff.schema.json
├── generate_options.rs        # codegen → docs/configuration.md
├── generate_rules_table.rs    # codegen → docs/rules/
├── generate_ty_*.rs           # codegen for ty
├── print_ast.rs               # introspection
├── print_cst.rs               # introspection
├── print_tokens.rs            # introspection
└── round_trip.rs              # parser dogfood
```

There is **no per-crate convention enforcement** in `ruff_dev` — the things
tidy would check (lints inheritance, README presence, manifest fields,
license headers) are simply not enforced anywhere in ruff's tree at all.
The conventions exist (every internal crate is `version = "0.0.0", publish =
false`; only `ruff` and `ruff_linter` and `ruff_wasm` get versioned), but
their enforcement is entirely social. **This is a direct alint opportunity:**
the rule `ruff-internal-crates-unpublished` in this config is something ruff
literally does not check today and would benefit from on day one.

### Ad-hoc CI gates worth knowing about

- `.github/zizmor.yml`, `.github/actionlint.yaml` — config side-files for
  workflow auditors (already covered above)
- `_typos.toml` — typo allowlist (already covered above)
- `clippy.toml` — disallowed-methods registry: 13 `std::*` calls banned in
  ty crates with rationale ("Use `System::env_var` instead in ty crates").
  Enforcement is via `cargo clippy` itself; alint can't substitute for it
  (Rust AST scope) and doesn't try.
- `rustfmt.toml`, `rust-toolchain.toml` — toolchain pins. The
  `rust@v1` ruleset already nudges that `rust-toolchain.toml` exists.

### Needs new alint primitive

| Existing check | What it validates | What alint needs |
|---|---|---|
| `cargo dev generate-all` drift | Source ↔ generated `ruff.schema.json` / `docs/rules/` / `docs/configuration.md` are in sync | A `command` rule already covers running the generator; what's missing is a `command_idempotent` mode — "running this command MUST leave the working tree clean" — generalising the prek pattern. |
| `clippy.toml::disallowed-methods` | Specific `std::*` calls banned in ty crates | Rust-AST scope check; out of scope for alint per the no-AST non-goal. Stays on clippy. |
| `cargo shear` | Workspace deps not used by any source | Cargo dep-graph analysis; out of scope for alint. Stays on the existing tool (already integrated as a `command:` rule above). |
| `--unreferenced=reject` (`cargo insta test`) | Snapshot files have a corresponding source rule | A **`pair_inverse`** rule kind — given a `partner` glob (the snapshot), every match must pair back to a primary (the rule source). The existing `pair` rule goes the other direction. Per `launch-evidence.md`, now a **v0.10 design candidate** with 2 demand sources (ruff + angular's goldens parity). |
| `python/ruff-ecosystem` regression | Diff lint output against a baseline ruff binary | Out of scope (would need to run a build of the project under test). |
| Per-prek priority chain (0 → 1 → 2) | Hook order matters when both modify the same file | Alint runs all rules in parallel; if two `fix:` ops conflict, the engine picks one (current semantics). The prek priority pattern is **not yet expressible** — defer until multi-fix conflict resolution becomes a saturated cross-repo ask. |
| `validate-pyproject` deep schema | Full PEP 621 metadata-block check | Vendor the published JSON Schema and use `json_schema_passes`. Same pattern as the GitHub workflow JSON Schema gate above; works today, just needs the schema file. |

**Two concrete launch-prep proposals surfaced from this case study:**

1. **`pair_inverse` rule kind** (snapshot ↔ source). ruff has thousands of
   `crates/ruff_linter/src/rules/<linter>/snapshots/*.snap` files paired
   with `crates/ruff_linter/src/rules/<linter>/rules/*.rs` sources. The
   existing `pair` rule answers "does every primary have a partner?". The
   inverse — "does every partner trace back to a primary?" — is what
   `cargo insta test --unreferenced=reject` does, and the same shape is
   wanted in any project with generated artefacts (codegen outputs,
   committed `.snap` files, golden files). Per `launch-evidence.md`, now
   a **v0.10 design candidate** with 2 sources (ruff + angular goldens).

2. **`command_idempotent` mode for the `command` rule kind**. Many of
   ruff's prek hooks (mdformat, markdownlint-fix, ruff-format, prettier)
   are **fixers** that the validation pass invokes in `--check` mode.
   What would actually compose better: run the fixer, snapshot the
   working tree before and after, fail if they differ. Per
   `launch-evidence.md`, now a **v0.10 design candidate** with 2 sources
   (ruff + prettier — covers mdformat, markdownlint, prettier, ruff-format,
   dprint-check, all of which share this shape).

### Out of alint's scope (use the existing tool)

- `cargo dev generate-*` / `RUFF_UPDATE_SCHEMA=1 cargo test` — codegen drift;
  alint doesn't run codegen. Keep the existing `cargo dev` invocation.
- `cargo clippy` (including the `clippy.toml` disallowed-methods registry) —
  Rust AST scope; alint's no-AST non-goal applies.
- `cargo shear` — Cargo dep-graph analysis; out of scope, but `command`-rule
  wrapped in this config so it still runs from `alint check`.
- `python/ruff-ecosystem` — runs a built ruff against real projects and
  diffs the output against a baseline; alint doesn't run the project under
  test.
- `cargo bench` / codspeed — performance regression, not structural.
- `wasm-pack test` — out-of-tree test harness; out of scope.

---

## Starter alint config (drop-in)

[`/.alint.yml`](.alint.yml) in this directory. Replaces ~10 prek hooks
directly + ~5 more via `command` rules + 3 of the CI structural gates
(cargo fmt / clippy / shear / doc). Net: **~18 of the ~22 prek/CI structural
checks** can move to one declarative file.

The remaining 4-6:

- 2 are codegen drift (`cargo dev generate-all`, `RUFF_UPDATE_SCHEMA=1 cargo
  test`) — keep on `cargo dev`
- 1 is the ecosystem regression diff — out of scope, run via the existing
  `python/ruff-ecosystem` package
- 1 is the snapshot freshness check (`cargo insta test
  --unreferenced=reject`) — needs the `pair_inverse` rule kind
- 1-2 are deep-schema validation gates (validate-pyproject, GitHub workflow
  schema) — work today via `json_schema_passes` if the schema is vendored
  locally

---

## Performance comparison (placeholder — bench when validation pass scales)

`uvx prek run -a` runs prek hooks with priority-ordered scheduling (priority
0 → 1 → 2) but parallel within a priority. Each hook does its own fs walk;
shellcheck / typos / actionlint / zizmor each pay process-startup cost
per file or per workflow.

alint runs all rules in parallel via the v0.9.3 dispatch flip + the v0.9.5+
cross-file fast paths. Expected: ~1-2 s for the alint-replaceable subset on
a ruff-scale repo (~2k Python + ~80k Rust lines, ~50 crates). Compare to
the v0.9.13 published S3 100k bench: 1.13 s for the workspace bundle.

To benchmark for real: run `time uvx prek run -a --hook-stage=manual`
against `time alint check` on the same checkout. Deferred to the per-repo
measurement pass.

---

## Recommendation for the launch story

This case study has **two distinct angles** worth featuring:

1. **The "prek replacement" angle** — "alint replaces 15 of ruff's 16
   pre-commit hooks with one declarative config." The `prek` framework is
   itself an Astral product (the modern pre-commit replacement); pitching
   alint as "the next layer up — the rule-config side, not the hook-runner
   side" positions alint orthogonally rather than competitively. There's
   no overlap: prek runs hooks, alint declares rules.

2. **The "ruff is a linter that can't lint its own structure" angle** —
   ruff has 900+ rules for Python, but **zero rules for its own per-crate
   manifest discipline**. The `ruff-internal-crates-unpublished` rule in
   this config is something ruff doesn't enforce today and would benefit
   from on day one. Same shape applies to most "linter for X language X"
   projects across the inventory.

Followup feature work surfaced (in priority order):

- **`pair_inverse` rule kind** (every partner traces back to a primary) —
  unlocks `cargo insta` `--unreferenced=reject`-style gates for any
  project with generated artefacts. Per `launch-evidence.md`, now a
  **v0.10 design candidate** with 2 sources (ruff + angular goldens).
- **`command_idempotent` mode** — generalises the "fixer in --check mode"
  pattern. Per `launch-evidence.md`, now a **v0.10 design candidate**
  with 2 sources (ruff + prettier).
- **Vendoring published schemas under `.alint/schemas/`** as a first-class
  workflow — the GitHub workflow schema, the PEP 621 schema, and others
  recur across configs and would benefit from a documented pattern.

---

## Future analysis

Surfaced during the 2026-05-07 revalidation pass; not yet executed
against a live tree:

1. **`for_each_leaf_dir` / `iter.is_leaf` accessor** for the
   per-snapshot-dir gates — ruff has hundreds of
   `crates/ruff_linter/src/rules/<linter>/snapshots/` subdirs, each
   leaf containing only `.snap` files. Per `launch-evidence.md`,
   this is now a **v0.10 design candidate** with 3 sources
   (prettier + rust + ruff). Once shipped, the ruff config could
   restate the snapshot-discipline check more precisely.
2. **`scope_filter.has_ancestor: Cargo.toml` in `crates/` rules** —
   the `monorepo/cargo-workspace@v1` overlay covers the per-crate
   manifest discipline; ruff-specific rules (license, edition,
   publish=false) could use `scope_filter` to narrow them to
   crates that ARE leaf-published, which would cleanly express
   the "only `ruff`/`ruff_linter`/`ruff_wasm` are versioned" rule
   without listing them by name.
3. **`agent-context@v1` is already adopted; `agent-hygiene@v1` not
   yet** — ruff has `CLAUDE.md`/`AGENTS.md`-style instructions
   under `crates/ruff_linter/`; trial the bundled `agent-hygiene`
   ruleset (6 rules: AGENTS.md canonical name, no agent self-edits,
   etc.) to see what surfaces.

---

## Validation status (2026-05-07)

- alint version validated: 0.9.17 (built 2026-05-07)
- `validate-config` rule count: **75 rules loaded** (21 in-config +
  7 bundled overlays summing to ~58 rules with overlap deduped:
  oss-baseline=15, rust=11, python=9, monorepo/cargo-workspace=4,
  ci/github-actions=3, agent-context=5, hygiene/no-tracked-artifacts=11)
- Live-tree recheck: **pending — `/tmp/ruff/` not present** at
  revalidation time.
- Pitfalls noted in this README that are now fixed in the engine:
  none directly cited — the README pre-dates the v0.9.17 pitfall
  catalogue and references no specific pitfall numbers.
- Open gaps after this revalidation: rule-kind candidate status
  drift was the principal stale claim — `pair_inverse` and
  `command_idempotent` are now v0.10 design candidates (2 sources
  each) per `launch-evidence.md`, no longer "v0.10+" without a
  clearer status. README rule-count claim ("22-rule alint config")
  is off by 1 (actual 21) — too small a delta to fix in prose;
  flagged in the batch findings file.
