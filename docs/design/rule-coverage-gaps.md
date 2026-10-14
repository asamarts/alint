# Rule coverage gaps: detection classes alint does not yet cover

Status: Draft (research and analysis). Written 2026-09-09 alongside the auto-fix design, which
shipped in v0.17.0; revised 2026-10-14 after an adversarial review against `main` at v0.18.2
(see the review log in [`general-linter.md`](general-linter.md) section 15).
Decision IDs written `DEC-n` refer to [`general-linter.md`](general-linter.md) section 13; bare
letter-number IDs (A1, D1, J4) are the gaps in section 4 here.
Decisions: program-level decisions in [ADR-0019](../adr/0019-general-linter-program-and-plugin-architecture.md); individual gaps graduate to their own `docs/design/vX.Y/` design docs (and, where a gap is a new engine capability, an ADR) when scheduled. **Ordering:** the gap-to-substrate matrix in [`general-linter.md`](general-linter.md) section 11.8 is the canonical sequence for these gaps; section 8 here ranks value, not build order.
Demand evidence: cross-checked against Repolinter, OpenSSF Scorecard, syncpack / manypkg / knip, the version-bump tool family, markdownlint / yamllint / editorconfig-checker, REUSE / SPDX, and the 30-repo `examples/` corpus (whose analyses are v0.9.17 snapshots). In the 111-repo v0.12 study, version and toolchain-pin coherence (C1, C2) recur but were scored expressible today, and one i18n placeholder gap (D2-shaped) is recorded outside the locked backlog; C3, D1, and B1 do not appear as backlog items, so their demand rests on the outside tools; see [`general-linter.md`](general-linter.md) section 12.3.

> This is the detection-side companion to [`auto-fix.md`](auto-fix.md). That document asks
> "of the violations alint can already find, which can it mechanically fix?" This one asks
> the orthogonal question: "what classes of repository state should alint be able to find at
> all, that it cannot today?" The two share substrates: the format-preserving structured
> write-back engine shipped in v0.17.0, so the fixes for several gaps here ride existing
> operations (`set_value`, `sync_from`, `sort`, `insert_line`). The architectural program that
> generalizes the relational gaps is [`general-linter.md`](general-linter.md).

## Contents

- [1. The scope boundary](#1-the-scope-boundary)
- [2. A completeness framework](#2-a-completeness-framework)
- [3. What alint already covers](#3-what-alint-already-covers)
- [4. The gap families](#4-the-gap-families)
- [5. Borderline: route to `command` or a WASM plugin](#5-borderline-route-to-command-or-a-wasm-plugin)
- [6. Clearly out of scope](#6-clearly-out-of-scope)
- [7. Auto-fix tie-in](#7-auto-fix-tie-in)
- [8. Prioritized shortlist](#8-prioritized-shortlist)
- [9. Strategic read and reusable substrates](#9-strategic-read-and-reusable-substrates)
- [10. References](#10-references)

## 1. The scope boundary

alint's fresh white space is the band **between "a file exists" (a commodity every repo
linter does) and "resolve the dependency graph" (which belongs to cargo-deny, knip, bazel,
and the language toolchains)**. Everything proposed here sits in that band: it is decidable
from the local files with no network, no programming-language AST or type checker, no
`node_modules` metadata, no Git host API, and no cryptography. Parsing data and document formats
(the eight config formats, CommonMark, tabular files) is inside the band; programming-language
syntax is not (see [`general-linter.md`](general-linter.md) section 5.7, decision DEC-10). The
non-goals from the README still hold (alint is not a code/AST linter, not SAST, not a semantic
IaC scanner, not a secret scanner), and section 6 draws that line explicitly. Section 2 gives
the boundary a precise argument rather than leaving it to taste; it describes the declarative
core, and the trusted `command` and `custom` escape hatches sit outside it by design.

Three axes classify every candidate:

- **Scope-fit.** IN-SCOPE (decidable from local files with alint's existing muscle, or a
  light stateful line-scanner that is not a real AST); BORDERLINE (needs one small
  self-contained new capability, for example a bundled SPDX id table, a semver-range algebra,
  a duplicate-aware structured parser, or local git-tag access, all deterministic and
  offline); OUT (needs a resolver, a programming-language AST, a registry/API, or crypto).
- **Expressibility.** (P) expressible today with existing primitives, so it ships as a
  bundled ruleset with no engine change; (K) needs a new rule kind or engine capability;
  (P+K) partly expressible, but a dedicated kind is materially cleaner.
- **Auto-fix applicability**, using the [`auto-fix.md`](auto-fix.md) four-state model: Safe,
  Unsafe, Suggestion, Never.

## 2. A completeness framework

"Is the rule set complete?" has no absolute answer; completeness is only ever **relative to a
fixed class**. This section fixes that class and locates the covered fragment and the gaps inside
it, which turns the family list below from a wishlist into the boundary of a defined fragment.

### 2.1 Two layers: recognition and assertion

A repository is a finite labeled structure (a path tree; the filesystem a partial map
`path -> bytes`; each structured file a further finite tree). alint is two composed machines, and
confusing them is the main source of loose reasoning about coverage:

- **Recognition layer (the Chomsky hierarchy).** From raw bytes it materializes *labels* (unary
  predicates: "matches regex R", "is valid UTF-8", "has +x") and *relations* (edges, key/value
  tuples, path lists). A single content regex is RE2-class: alint's own patterns use the `regex`
  crate, which has no backreferences, so one pattern cannot recognize `a^n b^n`, balanced
  brackets, or "the same identifier twice". Three shipped features compose past that ceiling:
  `for_each_match` with `require.equal` compares captures, `ordered_block` checks sortedness,
  and JSON Schema `pattern` keywords in `json_schema_passes` run on `fancy-regex` (backreferences
  and lookaround, bounded by its backtrack limit). Format recognition is
  roughly context-free (balanced-bracket well-formedness is the Dyck language; the honest caveat
  is that YAML is not context-free, so the real parsers sit around or just above CF). Duplicate-code
  detection is *fingerprinting* (winnowing over Rabin-Karp hashes; Schleimer, Wilkerson, Aiken
  2003), not parsing, which is why it stays in scope.
- **Assertion layer (finite model theory + dependency theory).** Over the resulting finite
  structure it *asserts* a predicate: a first-order combination, a bounded slice of monadic
  second-order or transitive closure, integrity constraints, and bounded counting.

Every gap is then precisely a **recognition gap** (the layer that builds the structure throws the
needed information away, or never builds it), an **assertion gap** (the structure is present but
no kind expresses the predicate over it), or **out of band** (not a decidable function of the
local bytes). Tagging each gap this way splits the doc's undifferentiated "needs a new kind" (K)
verdicts into "needs a parser or scanner" versus "needs new constraint logic", which are
different engineering.

### 2.2 The scope boundary is a computability statement

Every alint rule is a decidable, polynomial predicate over the given finite structure. A candidate
is OUT on exactly one of two grounds:

1. **Undecidable or semantics-dependent in general.** "Is this the same code?" is program
   equivalence, undecidable (a corollary of Rice's theorem, 1953); full type inference, dataflow,
   and taint need a language model alint deliberately lacks.
2. **Not a function of the local bytes.** Branch protection, CI outcomes, advisory status,
   registry existence, true lockfile resolution, and signature crypto depend on external or global
   state that is not present in the committed tree.

The IN band is therefore: predicates that are a decidable, deterministic function of the committed
bytes alone, **including the local `.git` object store**. That last clause is exactly why local
git-tag and commit-history checks are IN while the GitHub API is OUT, and why a date rule must take
a pinned `--as-of` clock rather than reading the wall clock (a wall-clock read is not a function of
the local bytes, violating clause 2 and the determinism invariant). The shipped `git_blame_age`
does read the wall clock today, so it is a known exception to this rule, scheduled for the
`--as-of` input ([`general-linter.md`](general-linter.md) decision DEC-11). This grounds the
intuitive "between file-exists and resolve-the-dep-graph" band of section 1 as a precise
statement about the declarative core.

### 2.3 What the assertion layer expresses, and the one primitive that escapes it

Classifying the current kinds by finite-model-theory level:

- **First-order (FO):** existence, naming, and hygiene (Gaifman-local, radius at most one), the
  per-file structured queries, and the one-level cross-file kinds (`pair`, `cross_file`,
  `registry_paths_resolve`, `import_gate`) that quantify over files and tuples with no recursion.
- **The one genuine jump to MSO / transitive closure:** `file_graph`'s `acyclic` mode.
  Reachability, connectivity, and acyclicity are provably **not** first-order-definable
  (Ehrenfeucht-Fraisse games; Gaifman locality 1982; Aho-Ullman 1979), so acyclicity cannot be
  desugared into the FO cross-file kinds. This is the theorem that earns `file_graph` its keep.
  Honest correction: only `acyclic` is non-FO; `no_dangling` / `forbidden_edges` are FO over the
  materialized edge relation, and `no_orphans` is FO under its in-degree-zero reading ("a file
  nothing references"); a reachability reading ("unreachable from any root") would be non-FO like
  `acyclic`. These are bundled with `acyclic` for ergonomics, not expressiveness. (The fifth
  mode, `fresh`, is a generated-file freshness check, not a graph property.)
- **Path-query expressiveness:** the navigational core of XPath is characterized as FO2 over trees
  (Marx and de Rijke 2005); JSONPath (RFC 9535) has no such published theorem, so FO2 is a grounded
  analogy, not a proof, for alint's `*_path_*` family. The family asserts a
  universal-over-a-selected-set predicate, which structurally cannot compare cardinalities across
  nodes, assert key-set equality, or see duplicate keys.
- **Counting:** comparing a count to a *fixed constant* (`max_files_per_directory`, and the size,
  depth, and line caps) is already plain **FO** (a threshold to a constant needs no
  counting quantifier). What the engine lacks is **general FO+COUNT**: comparing two counts, or an
  equality-of-counts over an extracted relation. (Parity is the classic witness that FO alone
  cannot count.) That general fragment, not the fixed caps, underlies several gaps below.

### 2.4 The unifying lens: a repository is a database with integrity constraints

Model the repository, plus the relations the recognition layer extracts from its structured files,
as a relational database. Then most cross-file rules and the version/key-consistency gaps are,
formally, **database integrity constraints** (Abiteboul, Hull, Vianu 1995):

- `unique_by` = a **functional dependency (FD) / key** (no two files share the key value);
  `no_case_conflicts` = a case-folded key.
- `pair`, `registry_paths_resolve`, `markdown_paths_resolve`, `markdown_links_resolve`, and
  `cross_file` `resolves` / `registered` = **inclusion dependencies (IND)**, i.e. referential
  integrity / foreign keys (Casanova, Fagin, Papadimitriou 1984); `registry_paths_resolve` with
  `orphans` is a two-way IND.
- `cross_file ... equals` / `identical` = **equality-generating dependencies (EGD)**; `set_equals`
  = a two-way IND; `forbidden_edges` / `import_gate` = an **exclusion dependency**.
- `file_graph acyclic` is the deliberate exception again: embedded dependencies are FO sentences,
  acyclicity is not, so it is not a dependency and correctly stays bespoke. Two independent lenses
  (FO-definability and dependency theory) agree on the same boundary.

The **coverage gaps are the same dependency classes over *dynamically-extracted* relations**
(schema-on-read, rather than a pinned file pair): `dependency_version_consistency` is an FD
`name -> version`; `key_parity` is a two-way IND on key sets; `reuse_license_completeness` is a
two-way IND on SPDX ids. This motivates a single, theory-grounded **`constraint` kind**: an
`extract` spec (pull tuples from a glob via JSONPath / `lines` / `regex`, tagged by source)
composed with a dependency to assert (`key`, `references`, `set_equals`, `equal`, `disjoint`,
optionally a `count` / `distinct` operator for the counting fragment). It would subsume `cross_file`
and `registry_paths_resolve` (both already extract-then-assert-a-dependency kinds built on the
shared `crate::extract` module) and unlock `key_parity`, `dependency_version_consistency`, and the
toolchain-pin family as *configurations* rather than bespoke kinds. It would **not** directly
subsume `unique_by`, which keys on a path-*template* (`key: "{stem}"`) over file paths rather than
on any content extraction, a form the `Extract` enum (`Structured` / `Lines` / `Regex` /
`WholeFile`) does not have. A path-templated source already exists elsewhere, though:
`cross_file`'s `relation: registered` takes a path-template `register_as:` (for example
`"{dir}"`; the tokens are `{path}`, `{dir}`, `{basename}`, `{stem}`, `{ext}`, `{parent_name}`), so the
extractor `unique_by` needs is a generalization of shipped code, not a new idea. **This section
states the theory; the canonical design of the `constraint` kind, including which existing
kinds stay native, is [`general-linter.md`](general-linter.md) sections 6.5 and 7.4.** There,
specialized kinds remain as ergonomic sugar and native kernels rather than being removed, and
the relational gaps ship as named kinds implemented on the shared relation layer (decision DEC-8).
Guardrail: **check only, never infer**, because FD+IND implication is undecidable
(Chandra-Vardi 1985); alint evaluates a declared constraint against an instance (polynomial), and
must never entail or minimize a constraint set.

Two caveats keep this a design vocabulary rather than an overclaim: the extracted "relations" are
computed by heuristic extractors (the regex import-edge caveat generalizes, so a satisfied
constraint is only as sound as its extractor), and classical dependency theory assumes a fixed
schema whereas alint's relations are discovered per run.

### 2.5 Covered versus missing, by class

- **Covered (the realized fragment):** FO over the file tree and statically-declared extraction
  points; regular content recognition (exactly); context-free format recognition into a queryable
  tree; the FD / IND / EGD / exclusion dependency classes with *fixed* relations; exactly one
  MSO/TC property (acyclicity); and fixed bounded counting.
- **Missing (the gap list is the boundary of that fragment):**

| Missing class | Gaps | Kind of work |
|---|---|---|
| Recognition upgrade (build a structure the parser drops or never builds) | B1 duplicate keys (spanned parsers exist; extraction flattens them), B2 well-formed, G1 tabular, H2 Dockerfile, F2 SPDX-expression | a parser or scanner |
| Assertion over a structure the shipped parser already builds | E2-E5, E7, E8 Markdown structure (the `pulldown-cmark` event stream used by `markdown_links_resolve` already yields headings, image alt text, list nesting, and metadata blocks); E6 front matter also needs a decoding decision (see G1) | new assertion logic only |
| Dependency over a *dynamically-extracted* relation | C3 version consistency, D1 key parity, D2 placeholder parity, F3 REUSE completeness | new constraint logic (the `constraint` kind of 2.4); the dependency shape is partly present already (`registry_paths_resolve` and `cross_file set_equals` are dynamic INDs, and flat `.env` / `.properties` key parity is already expressible with a `regex:` extract), so the genuinely new pieces are key-*set* enumeration for nested formats (D1, which JSONPath cannot express) and the extract-and-assert packaging |
| General FO+COUNT (equality-of-counts / threshold over an extracted relation) | C3 count-distinct, key-count parity, B4 mutually-exclusive (= 1), dead-pattern (= 0) | a `count` operator on the `constraint` kind |
| A small offline decision procedure | C4 semver-range algebra | a self-contained solver |
| Out of band (the computability boundary of 2.2) | Scorecard-via-API, dependency-graph resolution, code semantics, SAST, secrets, crypto | deliberately excluded |

## 3. What alint already covers

To keep the gap list honest, these are covered today and are **not** gaps (they are cited so
a reader does not re-propose them):

- **Community-health file presence.** Bundled in `oss-baseline@v1`: README, LICENSE, SECURITY,
  CODEOWNERS, CODE_OF_CONDUCT, `.gitignore`, and a Dependabot or Renovate config. Expressible
  with the existence family but not bundled: CONTRIBUTING, SUPPORT, GOVERNANCE, CHANGELOG, issue
  and PR templates, FUNDING. Repolinter's default set and Scorecard's `Security-Policy` /
  `License` / `Dependency-Update-Tool` reduce to this.
- **REUSE and Apache license headers** via `compliance/reuse@v1`, `compliance/apache-2@v1`,
  `apache/governance@v1`.
- **GitHub Actions pinning and workflow `permissions:`, partially,** via `ci/github-actions@v1`.
  `gha-pin-actions-to-sha` checks step-level `jobs.*.steps[*].uses` only (reusable-workflow
  `jobs.*.uses` and composite `action.yml` steps are unchecked), and
  `gha-workflow-contents-read` rejects missing permissions, `write-all`, and `contents: write`,
  but lets other write scopes through, which is not least privilege. Scorecard's
  `Token-Permissions` and the Actions slice of `Pinned-Dependencies` are therefore covered in
  part; H1 below closes the pinning holes.
- **Most of the structural pre-commit-hooks battery** (trailing whitespace, final newline,
  mixed line ending, BOM, merge-conflict, case conflict, illegal Windows names, large files,
  submodules, shebang/executable pairing, a symlink ban) maps onto existing hygiene, unicode,
  portable-metadata, and unix-metadata kinds. Exceptions: `check-symlinks` (dangling links) and
  `destroyed-symlinks` have no kind (I4 below), and `check-json` / `check-yaml` / `check-toml`
  are covered only incidentally, through structured rules' parse errors (B2).
- **Config-file schema validation** via `json_schema_passes` for any of the 8 formats.
- **The monorepo boundary / cycle / orphan / forbidden-edge cluster** (Nx, Turborepo,
  sheriff, dependency-cruiser) via `import_gate` + `file_graph`, with the documented
  regex-import caveat (dynamic and aliased edges are missed).
- **Duplicate-listing checks** via `unique_by`; **co-change gates** via
  `pair_changed_together` / `changeset_requires_path`; **value/version equality across known
  files** via `cross_file_value_equals` with `normalize:` bands (`trim`, `lower`, `semver-major`,
  `semver-minor`; proven by the `cross_file_normalize_semver_minor` e2e scenario). alint's own
  dogfood version check (`install-snippets-match-workspace-version`) is still a `command` rule
  over a shell script, because its pins are spread through prose snippets rather than
  structured fields, which is itself evidence for C1's packaging gap. **Naming parity** via
  `filename_case` / `filename_regex`; **executable-bit absence** via `executable_bit`
  `require: false` (with a `chmod` fix); **case-collision safety** via `no_case_conflicts` and
  `unique_by case_insensitive:`.
- **Markdown link integrity** via `markdown_links_resolve` (E1, shipped in v0.18.0):
  repository-relative links, images and reference definitions resolve against the file tree;
  explicit reference labels exist (markdownlint MD052); and trailing-slash documentation sites
  can forbid relative URLs or map rendered root-absolute routes back to source files. URL
  fragments (`file.md#section`) are deliberately not checked (E7 below).
- **Flat key parity** (`.env` against `.env.example`, `.properties` files) via `cross_file`
  `relation: set_equals` over a `regex:` extract such as
  `(?m)^(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=` on both sides. This works today but is
  undocumented as a recipe; nested-format key parity remains gap D1.

Already on alint's own radar (the `examples/README` emerging-gaps list and ROADMAP
single-source candidates): `json_key_sort_order`, `column_alignment`, `directory_hash`,
`dir_name_matches_field` (`dir_name_equals_field` in ROADMAP), `balanced_delimiters`, and the backlogged `duplicate_blocks`
(copy-paste) and WASM plugins. (`not_executable` and `case_collision_safe` from those lists are
covered, as noted above.) The `detect: linguist` / `detect: askalono` facts were planned for an
early cut and **never shipped** (verified: no `licensee` or `askalono` crate reference exists in
`crates/` source; the remaining mentions are ROADMAP's v0.2 entry, `ARCHITECTURE.md`, the v0.10
design README, and a `detect: linguist` test comment).

## 4. The gap families

Ten families, ranked. Within each, the table columns are: ID (cited throughout this document
and in [`general-linter.md`](general-linter.md) section 11.8), gap (working kind name), what it
detects, scope-fit, expressibility, and default auto-fix tier. The standouts carry a
paragraph. Rows marked *(added 2026-10-14)* came from the review that checked this list
against peer tools' structural checks.

### Family A: config-as-SSOT / meta-conformance (highest leverage)

alint's differentiated move is "read the manifest that owns the truth" (`scope_filter` reads
Cargo/pnpm workspaces). These gaps extend that from build manifests to the config files that
already declare a repo's own hygiene policy, so the policy is asserted from its source of
truth instead of re-declared (and drifting) inside `.alint.yml`.

| ID | Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|---|
| A1 | `editorconfig_conforms` | every file obeys the repo's own `.editorconfig` (indent, EOL, charset, trim, final newline, max line length, spaces-after-tabs) | IN | K | Safe |
| A2 | `gitattributes_valid` | `* text=auto` present (already shipped as `tooling-gitattributes-normalizes-line-endings` in `tooling/editorconfig@v1`); every committed `--check` artifact has an `eol=lf` line; valid `linguist-*` / `export-ignore`; contradictory attribute lines | IN | P+K | Suggestion (an end-of-file `file_append` of the pin can be Safe; a positional insert is Unsafe, because `.gitattributes` is last-match-wins, like the gitignore files for which the shipped `insert_line` is already Unsafe) |
| A3 | `ignore_consistency` | `.dockerignore` covers the high-risk set and is a superset of `.gitignore`; dead ignore patterns matching zero paths; duplicate entries | IN | P+K | Suggestion (Safe for dedup) |

**`editorconfig_conforms` is the single highest-leverage idea in the survey.** alint already
has nearly every underlying hygiene primitive (`indent_style`, `line_endings`,
`no_trailing_whitespace`, `final_newline`, `line_max_width`, `no_bom`), and it already maps
`.editorconfig` to the INI format automatically (so `$['*.{js,py}'].indent_size` is queryable via
`ini_path_*` today). Three properties have no primitive yet: `charset` (`latin1`, `utf-16be`,
`utf-16le`; `file_is_text` accepts UTF-16 with a BOM, so this depends on I3 `file_encoding`),
`insert_final_newline = false` (`final_newline` can only require a newline), and
`end_of_line = cr` (`line_endings` supports `lf` and `crlf`). What is missing is a rule that
treats `.editorconfig` as the **policy source** and dispatches the existing hygiene evaluators
per glob section, so a user declares the thresholds once (in the file every editor already
reads) rather than duplicating them in `.alint.yml` where they drift. editorconfig-checker is
a top-tier pre-commit hook, so the demand is proven; the fix reuses the existing hygiene
fixers (Safe), with tab/space conversion staying Unsafe per the auto-fix doc. One recorded
decision must be revisited first: [ADR-0010](../adr/0010-manifest-derived-rule-scope.md) says a
manifest value may decide which files a rule sees, never what it decides about them, and an
`.editorconfig` value decides exactly that ([`general-linter.md`](general-linter.md) section
5.6, decision DEC-12).

`gitattributes_valid` targets a genuinely un-owned niche: no widely adopted `.gitattributes`
linter exists, and the "generated `--check` artifact needs an `eol=lf` pin or Windows CI
reports it stale" failure is one alint hit in its own history.

### Family B: structured-data integrity (beyond querying a value)

The structured-query family reads values at paths. These check the **shape** of the document
itself, which a path query structurally cannot see.

| ID | Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|---|
| B1 | `no_duplicate_keys` | the same key twice in one mapping, for JSON / YAML / dotenv / properties (silent data loss); XML and INI duplicates are queryable today as arrays, and TOML and HCL reject the file | IN | K | Suggestion |
| B2 | `well_formed` (`parses_as`) | a file parses as valid X with no schema and no query; **strict JSON** for consumers that reject comments and trailing commas (alint's JSON parsing is JSONC-tolerant, so a `package.json` with a trailing comma passes every structured rule today: a masking problem, not an extra) | IN | P+K | Never |
| B3 | `structured_key_sort` | keys within a parsed object are in a canonical order (distinct from `ordered_block`, which sorts lines) | IN | K | Suggestion (recommended: a key reorder is a new structural operation; ADR-0017 limits Safe `set_value` to scalar-for-scalar, and a Safe reorder would need its own lens proof) |
| B4 | `*_path_casing` / `*_path_mutually_exclusive` | a queried value obeys a naming case; exactly one of two paths is present | IN | P+K (casing: `*_path_matches` with a case regex works today) / K (the XOR) | Unsafe / Never |
| B5 | `yaml_footguns` *(added 2026-10-14)* | yamllint's structural set: YAML 1.1 truthy scalars (`on`, `no`, the "Norway problem"), octal-looking numbers, unintended multi-document files (`multi_doc_mode` is an open emerging gap in `examples/README.md`) | IN | K | Suggestion |
| B6 | self-declared schema validation *(added 2026-10-14)* | check-jsonschema's main modes: validate a file against its own `$schema`, or against a SchemaStore-style catalog mapped by filename; `json_schema_passes` needs an explicit schema per rule | IN (bundled catalog) / OUT (fetching schemas) | P+K | Never |
| B7 | in-file reference integrity *(added 2026-10-14)* | references within one structured file resolve: OpenAPI and JSON Schema local `$ref`, GitHub Actions `needs:` naming a real job, Compose `depends_on`, YAML aliases with anchors | IN | K (an in-file join JSONPath cannot express) | Never |

**`no_duplicate_keys` is a gap alint's own reference documents.** `docs/rules.md` states that
a "detect duplicate key" rule is only expressible for XML and INI (which array-collect the
duplicates) and TOML and HCL (which reject the file as a parse error), because for JSON / YAML /
dotenv / properties the `Format::parse -> serde_json::Value` pipeline silently keeps the last
duplicate and discards the earlier ones before any rule runs. A duplicate key
from a bad merge in a large Kubernetes, Ansible, or CI file silently changes behavior;
yamllint's `key-duplicates` is on by default. Closing this needs duplicate-aware extraction over
spanned parses, and most of that substrate shipped in v0.17.0 and v0.18.2: the fix bridge
already parses JSON through `jsonc-parser`'s AST, TOML through `toml_edit`, and YAML and INI
through hand-written locators, and `adotenv` keeps every `.env` entry with spans (alint
flattens them last-wins, and the dotenv fixer already declines duplicate keys). The remaining
work is per-format duplicate enumeration plus the rule, and it belongs to the provenance work of
[`general-linter.md`](general-linter.md) Phase 1a rather than inside the kind.

### Family C: cross-file value and version consistency (fresh white space)

The band between "file exists" and "resolve the dep graph" where `cross_file`,
`pair_changed_together`, and `json_schema_passes` are genuinely differentiated. alint has the
muscle; the gaps are packaging plus a few capabilities. The earlier competitive gap analyses
(the v0.12 case-study backlog and the launch-era tool comparisons) mined ls-lint / Repolinter /
Conftest but not syncpack / manypkg / knip / the version-bump family, so this whole vein is
unmined.

| ID | Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|---|
| C1 | version-SSOT ruleset | one version identical across package.json / Cargo.toml / pyproject / VERSION / `__version__` / Chart appVersion / OpenAPI info.version / the top CHANGELOG entry, plus the git tag | IN (files) / BORDERLINE (tag) | P + K | Unsafe (the shipped `sync_from` on a `cross_file equals` rule) |
| C2 | toolchain-pins ruleset | one language/tool version across `.nvmrc` / `engines` / Dockerfile `FROM` / CI setup / `.tool-versions` (Node, Rust, Python, Ruby, JVM, .NET) | IN | P (exact pins); range pins fall to C4 | Unsafe |
| C3 | `dependency_version_consistency` | every instance of a dynamically-discovered dependency agrees across a glob of manifests | IN (assert) / BORDERLINE (pick highest) | K | Unsafe |
| C4 | `semver_range` awareness | range intersection, satisfaction, and `^`/`~`/exact policy consistency | BORDERLINE | K | Unsafe / Never |
| C5 | `dependabot_ecosystem_drift` | a manifest exists but no `updates[]` entry covers its ecosystem | IN | P+K | Suggestion |
| C6 | `git_tag_matches` / `git_tag_valid` | release tag equals the manifest version; valid SemVer; annotated; signed; monotonic | BORDERLINE (local git-tag access) | K | Never |

**Version SSOT has enormous, proven demand.** Every release tool ships a machine-readable
manifest of "this version lives in files X, Y, Z" (bump-my-version, commitizen
`--check-consistency`, cargo-release `shared-version`, release-please `extra-files`, knope,
version-sync), and **none is language-agnostic**. The file-to-file part is expressible with
`cross_file_value_equals` today; the gaps are a bundled `versioning@v1` ruleset, an ergonomic
way to ingest an existing bump-tool config as the rule source, and a git-tag kind for the tag
half. The fix already exists: `sync_from` on a `cross_file equals` rule (shipped in v0.17.0,
Unsafe by default) rewrites each drifted value from the declared source.

**`semver_range` is the one class alint structurally cannot do today.** `normalize:` compares
version-band **equality**; range **intersection** and **satisfaction** (syncpack "Same Range",
manypkg `INTERNAL_MISMATCH`) need a small self-contained semver algebra. Deterministic and
offline, so BORDERLINE rather than OUT.

### Family D: cross-file key and placeholder parity

Family C compares values at known paths; Family D compares the **set of keys or tokens**
across files with dynamic membership, a shape no surveyed single-file linter does natively and
squarely alint's cross-file territory.

| ID | Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|---|
| D1 | `key_parity` | every locale file (or `.env` vs `.env.example`) declares the same key set as a reference | IN | P for flat `.env` / `.properties` (a `cross_file set_equals` recipe, section 3); K for nested JSON/YAML locale objects | Suggestion (insert stubs; ADR-0017 makes key insertion a Suggestion) |
| D2 | `placeholder_parity` | interpolation tokens (`%s`, `{name}`, ICU plural, `{{var}}`) match the source string per key | IN | K | Never |

**`key_parity` is a differentiator.** Missing-translation and drifted-`.env.example` are
common, ship-blocking bugs; eslint-plugin-i18n-json, i18n-tasks, and compare-locales each do
it for their ecosystem, and no general linter does it natively. For flat line formats a
`cross_file` `set_equals` over a `regex:` key extract already works. For nested formats,
`cross_file` set relations compare extracted value lists, not the key sets of a whole object
with dynamic membership, so this needs key enumeration. It is the proposed first user of the
shared relation layer ([`general-linter.md`](general-linter.md) Phase 2, decision DEC-7): a named
kind (reference file plus a glob of peers) implemented on that layer rather than as a
standalone evaluator.

### Family E: docs, accessibility, and i18n structural

A markdown and docs-hygiene family. `markdown_links_resolve` now covers live link / image /
reference destinations, undefined explicit reference labels, CommonMark-aware
code/comment/front-matter exclusion, source-relative resolution, and rendered-site URL policy.
`markdown_paths_resolve` remains the separate checker for backticked path claims. Alt text and
heading/front-matter structure remain gaps; they can extend the same parser event stream without
introducing a separate Markdown grammar. That stream (`pulldown-cmark` 0.13) already yields
headings, image alt text, nested list and item events, and metadata blocks, so every row below
is an *assertion* gap over a structure alint already builds, not a recognition gap.

| ID | Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|---|
| E1 | `markdown_links_resolve` | **shipped in v0.18.0**; listed so the IDs stay stable | covered | n/a | n/a |
| E2 | `markdown_images_have_alt` | an image with no alt text (WCAG / MD045) | IN | P+K | Suggestion |
| E3 | `markdown_required_headings` | a document's heading sequence matches a required ordered template with wildcards (MD043) | IN | K | Suggestion |
| E4 | `markdown_reference_definitions` | no unused or duplicate reference definitions (MD053); undefined labels (MD052) are already reported by E1 | IN | K | Suggestion |
| E5 | `markdown_no_duplicate_headings` | colliding heading slugs that break deep links (MD024) | IN | K | Never/Suggestion |
| E6 | `frontmatter_schema` / `frontmatter_path_*` | a `---`-fenced front-matter block exists and has required keys / passes a schema | IN | K | Suggestion |
| E7 | fragment resolution *(added 2026-10-14)* | a link's `#fragment` names a real heading anchor in the target file (lychee, remark-validate-links); E1 deliberately ignores fragments | IN | K (extends E1 with the E5 slug algorithm) | Never |
| E8 | `markdown_list_structure` *(added 2026-10-14)* | list indentation and marker consistency (MD005/007/029/030/032) | IN | K | Suggestion |

`markdown_required_headings` is the most alint-native rule of the set: a structural manifest
over an extracted heading list ("every ADR has `## Status` / `## Context` / `## Decision`"),
which alint's own `docs/adr@v1` approximates crudely today with `file_content_matches`.
`frontmatter_schema` has direct dogfood demand: alint.org is an Astro site whose content
collections fail the build on missing front-matter, and the structured-query family cannot
reach into a `---`-fenced block embedded in a `.md` file. A `docs/markdown@v1` bundled ruleset
also covers the (P) single-line markdownlint rules (first-line heading, single H1, bare URLs,
proper-name casing) with existing primitives. List-nesting rules (MD005/007/029/030/032) were
listed as OUT for needing an AST; the shipped CommonMark parser makes them decidable, so they
are now E8.

### Family F: license, SPDX, and provenance (beyond the `compliance/*` bundles)

| ID | Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|---|
| F1 | `license_detectable` | LICENSE has a conventional name and content matching a known license signature phrase (heuristic, not a legal classifier) | IN (heuristic); OUT for fuzzy best-match | K | Never |
| F2 | `spdx_identifier_valid` | an `SPDX-License-Identifier` value is a real, non-deprecated id; an SPDX expression parses | BORDERLINE | K | Suggestion |
| F3 | `reuse_license_completeness` | every used SPDX id has a `LICENSES/<id>.txt`, and none is unreferenced | IN | P+K | Suggestion |
| F4 | `manifest_license_valid` | a manifest `license` field is a valid SPDX value (not a typo or bare `UNLICENSED`) | IN (presence) / BORDERLINE (validity) | P / K | Never |
| F5 | `file_header_consistency` *(added 2026-10-14)* | license and copyright headers agree across files (holder, license id), and copyright years are current against an explicit `--as-of`; an open emerging gap in `examples/README.md` | IN | P+K | Suggestion |

A single bundled, regenerable **SPDX id table** (roughly 600 ids plus exceptions and
deprecated flags) plus a small self-contained SPDX-expression parser turns this whole
BORDERLINE tier green with no network. `license_detectable` revives the deferred `detect:`
fact as an offline heuristic (conventional name plus a bundled corpus of signature phrases),
staying clear of licensee/askalono's statistical best-match, which is OUT. An SBOM-presence
ruleset (`bom.json` / `*.cdx.json` exists and declares its format) is pure composition.

### Family G: data-file integrity

| ID | Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|---|
| G1 | `delimited_columns` | CSV/TSV rows all match the header column count; no duplicate/empty column names; no stray quotes | IN (structure); Table-Schema types BORDERLINE | K | Never/Suggestion |

Data, ML, and analytics repos commit CSV fixtures where a ragged row is a silent downstream
break, and no general repo linter checks tabular integrity. The structured-query family has no
tabular reader, so this is a new kind. If CSV becomes a `Format` (so the structured rules can
query it), it must go through the Format-as-SSOT cascade and the `Format::ALL` parity gate of
[`format-coverage.md`](format-coverage.md); a standalone kind avoids that cost. The same choice
applies to front matter (E6) and Dockerfiles (H2).

### Family H: container and CI structural (beyond `ci/github-actions@v1`)

| ID | Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|---|
| H1 | `pinned_references` | one cross-system unpinned-ref rule: Actions `@tag`, CircleCI orbs/images, GitLab/Cloud-Build `image:`, pre-commit `rev`, Dockerfile `FROM tag` (want `@sha256:`), `ADD http(s)://` | IN (detect); fix network-gated | P+K | Suggestion / class 7 |
| H2 | `dockerfile_structural` | hadolint's non-shell subset: base image tagged and digest-pinned, `COPY` not `ADD`, unique `FROM` aliases, JSON `CMD`/`ENTRYPOINT`, last `USER` not root, absolute `WORKDIR` | IN (structural); RUN-shell rules OUT | P (most) / K (cross-instruction state) | Unsafe / Suggestion |

`pinned_references` generalizes the pinning discipline alint already ships for GitHub Actions
to Dockerfile digests and pre-commit `rev:` (both clean gaps; hadolint has no base-image
**digest**-pin rule), motivated by the March 2025 `tj-actions/changed-files` supply-chain
incident. The tag-to-SHA fix is network-gated (auto-fix class 7). It should also close the two
holes in the shipped Actions rule (section 3): reusable-workflow `jobs.*.uses` and composite
`action.yml` steps.

### Family I: portability (extend the portable-metadata family)

| ID | Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|---|
| I1 | `max_path_length` | any tracked path longer than a limit (default 260, Windows MAX_PATH) | IN | K (small) | Never |
| I2 | `lfs_pointer_valid` | a file tracked as LFS is a valid pointer, not a raw binary (or the inverse) | IN | P+K | Never |
| I3 | `file_encoding` | a file is valid UTF-8 or a declared charset; flag UTF-16 / Latin-1 where unwanted | IN | P+K | Unsafe (transcode) |
| I4 | `symlinks_valid` *(added 2026-10-14)* | no dangling symlinks, and no symlink that escapes the root (pre-commit `check-symlinks` / `destroyed-symlinks`); `no_symlinks` only bans links and skips escaping ones | IN | K | Never |
| I5 | Unicode-normalization collisions *(added 2026-10-14)* | two paths that differ only by NFC/NFD normalization, a real macOS and Git portability break; `no_case_conflicts` folds case only | IN | P+K (a normalization option on `no_case_conflicts`) | Never |

These sit alongside `no_illegal_windows_names` / `no_case_conflicts`. The emerging gaps once
tracked here as `case_collision_safe` and `not_executable` are covered (section 3).

### Family J: governance validity (beyond existence)

| ID | Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|---|
| J1 | `codeowners_valid` | CODEOWNERS syntax, owner format, duplicate patterns, dead patterns, unowned files, shadowing order | IN (except owner existence, which is OUT) | P+K | Suggestion |
| J2 | `governance/schemas@v1` | CITATION.cff / FUNDING.yml / dependabot / issue-forms / SECURITY-INSIGHTS validate against bundled schemas | IN | P | Never/Suggestion |
| J3 | `field_date_valid` | `security.txt` `Expires:` is a valid future date; CHANGELOG dates are valid ISO | IN | K (small) | Never |
| J4 | `numeric_sequence` *(added 2026-10-14)* | numbered files (ADRs `NNNN-*`, migrations) form a gap-free, duplicate-free, monotonic sequence; `docs/adr@v1` explicitly defers this primitive | IN | K | Never |

`codeowners_valid` matters because GitHub silently skips invalid CODEOWNERS lines; alint today
only checks the file exists and is non-empty. `field_date_valid` carries a determinism caveat:
a "days since" verdict is time-dependent, which conflicts with constitution invariant 1, so it
must take a fixed "now" (a `--as-of` input or a per-run pinned clock) rather than reading the
wall clock. A Keep-a-Changelog structural ruleset and a `no_committed_binaries` ruleset round out
the (P) composition wins; the latter needs no new kind, because `file_is_text` over `**` with an
allowlist of excludes already detects binaries (with no fix), and `file_absent` with
`content_prefixes:` is a magic-byte scan that carries the shipped `git_untrack` fix (Unsafe by
default).

## 5. Borderline: route to `command` or a WASM plugin

These are real but better served by wrapping an existing tool via the `command` rule (or a
future WASM plugin) than by growing the core binary:

- **Dangerous-workflow taint** (Scorecard, zizmor, octoscan). The shallow regex patterns
  (`pull_request_target` plus untrusted checkout; `${{ github.event.* }}` interpolated into
  `run:`) are a reasonable bundled-content ruleset; the dataflow/taint core is CI-SAST and
  belongs to actionlint/zizmor via `command`.
- **codespell / common-misspellings** (a curated typo dictionary, not full spell-check): a
  large data dependency; wrap codespell.
- **Lockfile semantic in-sync-with-manifest**: only the resolver knows for real; route to
  `command` (`npm ci`, `cargo update --locked`, `pnpm --frozen-lockfile`).
- **Deep commit-history content grep** (Repolinter `git-grep-commits`): leans into
  secret-scanning, a stated non-goal. The adjacent shipped kinds do not scan history:
  `git_no_denied_paths` checks the currently tracked files, and `git_blame_age` dates lines in
  the current tree, so history content remains delegated.

## 6. Clearly out of scope

Named honestly, these belong to the tools the README already points at:

- **Scorecard checks needing a Git host API or release metadata:** Branch-Protection,
  Webhooks, Code-Review, Contributors, Maintained, CI-Tests, Signed-Releases, Packaging,
  CII-Best-Practices, Vulnerabilities (OSV), Fuzzing, SAST app-runs. These need a Git host API,
  release metadata, or an advisory database, none of which is in the committed tree.
- **Dependency-graph resolution:** knip symbol usage, dependency-cruiser reachability/license,
  true lockfile sync, cargo-deny advisories/licenses, cargo-udeps/machete.
- **Code semantics / programming-language AST:** ESLint / Clippy / ruff; hadolint RUN-shell
  rules; Python-AST pre-commit hooks. (Markdown list nesting moved in scope as E8, and the
  structural yamllint subset is B5, because the shipped parsers already build those structures.)
- **SAST / taint:** Semgrep, CodeQL, CI expression-injection dataflow.
- **Secret scanning:** gitleaks, trufflehog.
- **Semantic IaC:** Checkov, tfsec, KICS.
- **Fuzzy license classification, signature/attestation crypto, and numeric quality scores.**
- **Whole-file reformatting:** alint is not a formatter.

## 7. Auto-fix tie-in

Most high-value gaps map onto the [`auto-fix.md`](auto-fix.md) four-state model (Safe, Unsafe,
Suggestion, Never, as recorded in ADR-0017), and the highest-value new fixes ride operations
that shipped in v0.17.0. ADR-0017 Decision 2 bounds the tiers below: Safe `set_value` is only a
scalar replacing a scalar, and key insertion and object or array values are Suggestions. Fix
tiers for the relational gaps (D1, C3) apply only once fixes are enabled for the named kinds
built on the relation layer ([`general-linter.md`](general-linter.md) section 8.3).

| Gap | Tier | Note |
|---|---|---|
| A1 `editorconfig_conforms` | Safe | reuses the existing hygiene fixers, driven by the file's declared policy (tab/space conversion stays Unsafe) |
| A2 `gitattributes_valid` (eol pin) | Suggestion by default; Safe as an end-of-file `file_append`; Unsafe as a positional insert | `.gitattributes` is last-match-wins, so position is load-bearing |
| B3 `structured_key_sort` | Suggestion (recommended) | a key reorder is a new structural operation; a Safe tier would need its own lens proof |
| C1 version SSOT | Unsafe | the shipped `sync_from` on a `cross_file equals` rule |
| C2 / C3 toolchain and dependency drift | Unsafe | set the drifted pin to canonical (`sync_from` where one file is canonical) |
| B1 `no_duplicate_keys` | Suggestion | which duplicate to keep is ambiguous |
| D1 `key_parity`, E2-E4, E6, E8, C5 `dependabot_ecosystem_drift` | Suggestion | insert stubs / stanzas; a human confirms |
| E5, E7 | Never (E5 optionally Suggestion) | no unique correct heading or anchor |
| B2 `well_formed`, C6 `git_tag_*`, I1 `max_path_length`, F1 `license_detectable` | Never | no unique correct output |

The structured-value write-back engine shipped in v0.17.0 (`set_value`, `remove_value`,
`sync_from`, `sort`, `insert_line`, `insert_header` across the formats it supports), so it is no
longer a substrate to build: the version-SSOT, drift, and `*_path_equals`-backed gaps ride
existing operations.

## 8. Prioritized shortlist

This ranks value. Build order is the gap-to-substrate matrix in
[`general-linter.md`](general-linter.md) section 11.8, which places every gap in this document in
one of six buckets (ship now, needs local Git tag access, needs a Phase 1 substrate, needs
`--as-of`, build on the IR, standalone library).

1. `editorconfig_conforms` (A1): config-as-SSOT; nearly every primitive exists (`charset`
   depends on I3), zero conformance awareness; Safe auto-fix; needs decision DEC-12 on ADR-0010
   first. (K)
2. `no_duplicate_keys` (B1): a gap `docs/rules.md` documents; silent data loss; structural. (K)
3. `key_parity` (D1): cross-file key-set equality for i18n and `.env.example`; alint's
   territory, no general linter does it; the flat-format half is a recipe today, the nested half
   is the proposed IR pilot. (P+K)
4. version-SSOT ruleset + config-ingest (C1): huge proven demand; mostly expressible, needs
   packaging plus a tag kind. (P+K)
5. toolchain-pins ruleset (C2): under-served, because Dependabot ignores pin files. (P for exact
   pins; range pins are C4 `semver_range`)
6. `semver_range` awareness (C4): the one class alint structurally cannot do. (K)
7. `dependency_version_consistency` (C3): the syncpack/manypkg headline. (K)
8. `license_detectable` (F1): the deferred `detect:` fact; Scorecard/Repolinter/GitHub gate on
   it. (K)
9. `gitattributes_valid` (A2): an un-owned niche; alint hit the bug itself. (P+K)
10. `delimited_columns` (G1): data repos; no general linter covers tabular integrity. (K)
11. `markdown_required_headings` (E3): the most alint-native doc rule. (K)
12. `pinned_references` (H1): Dockerfile digest and pre-commit `rev` are clean gaps, and it closes
    the reusable-workflow and composite-action holes in the shipped Actions rule. (P+K)
13. `markdown_images_have_alt` (E2): a fresh accessibility angle. (P+K)
14. `dependabot_ecosystem_drift` (C5) and `codeowners_valid` (J1): governance validity beyond
    existence. (P+K)

Second tier: B2 `well_formed` (raise it if strict-JSON masking proves common), B3
`structured_key_sort`, B4, B5 `yaml_footguns`, B6 self-declared schemas, B7 in-file references,
E4, E5, E6 `frontmatter_schema`, E7 fragment resolution, E8 list structure, F2
`spdx_identifier_valid`, F3 `reuse_license_completeness`, F4, F5 header consistency, A3
`ignore_consistency`, D2 `placeholder_parity`, C6 `git_tag_*`, I1 `max_path_length`, I2
`lfs_pointer_valid`, I3, I4 `symlinks_valid`, I5, J2 the community-schema pack, J3
`field_date_valid`, J4 `numeric_sequence`, and H2 `dockerfile_structural`.

## 9. Strategic read and reusable substrates

Two clusters dominate the high-value gaps, and both are almost entirely un-owned by any single
language-agnostic tool:

1. **Config-as-SSOT / meta-conformance (Family A):** read the config that already declares the
   repo's policy (`.editorconfig`, `.gitattributes`, a bump-tool manifest) and assert it,
   instead of re-declaring thresholds in `.alint.yml`. The same "manifest owns the truth" move
   as `scope_filter`, applied to hygiene, subject to the ADR-0010 boundary (decision DEC-12).
2. **Cross-file value / key / version consistency (Families C and D):** the version-SSOT,
   toolchain-pin, translation-parity, and dependency-consistency vein, which the earlier
   competitive analyses never mined (they covered ls-lint / Repolinter / Conftest, not
   syncpack / manypkg / knip / the version-bump family).

Five reusable substrates unlock disproportionate coverage. One has shipped; the other four are
the substrates still to build:

- **Shipped: the format-preserving structured-value write-back engine** (v0.17.0). The
  version-SSOT, drift, and `*_path_equals`-backed gaps ride it today.
- **A bundled SPDX id table plus a small expression parser** turns the BORDERLINE license-validity
  checks (F2, and F4's validity half) green with no network, and underpins F3's REUSE completeness
  (an IN cross-file check).
- **Duplicate-aware, spanned extraction** over the parsers the fix bridge and `adotenv` already
  provide, which unlocks B1 and B3 and is part of the provenance work in
  [`general-linter.md`](general-linter.md) Phase 1a.
- **The shipped CommonMark event stream**, with new assertions over its headings, images, lists,
  and metadata blocks, which unlocks the remaining Family E checks without a second parser.
- **A shared relation layer with a `constraint` surface** (section 2.4), designed in
  [`general-linter.md`](general-linter.md) sections 6, 7.4, and 11.5. It unlocks the
  cross-file consistency vein (`dependency_version_consistency`, `key_parity`,
  `placeholder_parity`) as named kinds and configurations implemented on one layer rather than
  as bespoke evaluators, subject to the check-only-never-infer guardrail.

**Sequencing** is owned by [`general-linter.md`](general-linter.md) section 11.8. In short:
the native kinds and bundled rulesets that need no relation layer ship first (A1 after decision
D12, the Markdown structure kinds, the SPDX table, G1, H1, the version-SSOT and toolchain-pin
rulesets), B1 and B3 follow the Phase 1a extraction work, and the relational gaps (D1 nested,
C3, D2, F3, C5) are built on the relation layer, with `key_parity` as its pilot. Building those
relational gaps as standalone evaluators first would leave two spellings to support.

## 10. References

Repolinter rules; OpenSSF Scorecard checks, Allstar, Best Practices, OSPS Baseline;
CNCF / Apache maturity models; GitHub community health files; syncpack, manypkg, knip, Nx and
Turborepo boundaries, dependency-cruiser; the version-bump family (bump-my-version, commitizen,
cargo-release, release-please, knope, version-sync); markdownlint, yamllint,
editorconfig-checker, dotenv-linter, ls-lint, hadolint; REUSE and the SPDX license list;
eslint-plugin-i18n-json, i18n-tasks, compare-locales, gettext `msgfmt`; csvlint and the
Frictionless Table Schema; Keep a Changelog; SemVer 2.0.0; codeowners-validator;
remark-lint-frontmatter-schema; the pre-commit-hooks battery; lychee and remark-validate-links
(fragment checking); check-jsonschema and SchemaStore (catalog validation). The original
research notes behind this survey are not committed; the tools above are named so each claim
can be re-checked against the tool's own documentation.

The theoretical foundations behind section 2: Libkin, *Elements of Finite Model Theory*, 2004
(FO/MSO locality, Ehrenfeucht-Fraisse games, FO+COUNT); Gaifman 1982 (locality); Aho and Ullman
1979 (transitive closure is not first-order); Immerman, *Descriptive Complexity* (FO+TC captures
NL); Thatcher-Wright and Doner (MSO = regular tree languages), Courcelle 1990; Marx and de Rijke
2005 (the navigational core of XPath is FO2 over trees); IETF RFC 9535 (JSONPath); Chomsky 1956
(the hierarchy); Schleimer, Wilkerson, Aiken 2003 (winnowing) and Karp and Rabin 1987
(rolling-hash fingerprinting); Rice 1953 (program equivalence is undecidable); Abiteboul, Hull,
Vianu, *Foundations of Databases*, 1995 (FD / IND / EGD / TGD, the chase); Casanova, Fagin,
Papadimitriou 1984 (inclusion dependencies); Chandra and Vardi 1985 (FD+IND implication is
undecidable).
