# Rule coverage gaps: detection classes alint does not yet cover

Status: Draft (research and analysis; a companion to the auto-fix arc).
Decisions: none yet; individual gaps graduate to their own `docs/design/vX.Y/` design docs (and, where a gap is a new engine capability, an ADR) when scheduled.
Demand evidence: cross-checked against Repolinter, OpenSSF Scorecard, syncpack / manypkg / knip, the version-bump tool family, markdownlint / yamllint / editorconfig-checker, REUSE / SPDX, and the 30-repo `examples/` corpus.

> This is the detection-side companion to [`auto-fix.md`](auto-fix.md). That document asks
> "of the violations alint can already find, which can it mechanically fix?" This one asks
> the orthogonal question: "what classes of repository state should alint be able to find at
> all, that it cannot today?" The two share substrates (the format-preserving structured
> write-back engine unlocks the fixes for several gaps here), so they are designed together.

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
from the local files with no network, no language AST or type checker, no `node_modules`
metadata, no Git host API, and no cryptography. The non-goals from the README still hold
(alint is not a code/AST linter, not SAST, not a semantic IaC scanner, not a secret scanner),
and section 6 draws that line explicitly and honestly. Section 2 makes the boundary a theorem
rather than a taste.

Three axes classify every candidate:

- **Scope-fit.** IN-SCOPE (decidable from local files with alint's existing muscle, or a
  light stateful line-scanner that is not a real AST); BORDERLINE (needs one small
  self-contained new capability, for example a bundled SPDX id table, a semver-range algebra,
  a duplicate-aware structured parser, or local git-tag access, all deterministic and
  offline); OUT (needs a resolver, an AST, a registry/API, or crypto).
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
  tuples, path lists). Content matching is *exactly* the regular languages: the RE2-style `regex`
  engine has no backreferences, a precise ceiling, not an analogy, so a content rule cannot
  recognize `a^n b^n`, balanced brackets, or "the same identifier twice". Format recognition is
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
the local bytes, violating clause 2 and the determinism invariant). This grounds the intuitive
"between file-exists and resolve-the-dep-graph" band of section 1 as a theorem.

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
  `acyclic`. These are bundled with `acyclic` for ergonomics, not expressiveness.
- **Path-query expressiveness:** the navigational core of XPath is characterized as FO2 over trees
  (Marx and de Rijke 2005); JSONPath (RFC 9535) has no such published theorem, so FO2 is a grounded
  analogy, not a proof, for alint's `*_path_*` family. The family asserts a
  universal-over-a-selected-set predicate, which structurally cannot compare cardinalities across
  nodes, assert key-set equality, or see duplicate keys.
- **Counting:** comparing a count to a *fixed constant* (`max_files_per_directory`, and the size,
  depth, line, and path-length caps) is already plain **FO** (a threshold to a constant needs no
  counting quantifier). What the engine lacks is **general FO+COUNT**: comparing two counts, or an
  equality-of-counts over an extracted relation. (Parity is the classic witness that FO alone
  cannot count.) That general fragment, not the fixed caps, underlies several gaps below.

### 2.4 The unifying lens: a repository is a database with integrity constraints

Model the repository, plus the relations the recognition layer extracts from its structured files,
as a relational database. Then most cross-file rules and the version/key-consistency gaps are,
formally, **database integrity constraints** (Abiteboul, Hull, Vianu 1995):

- `unique_by` = a **functional dependency (FD) / key** (no two files share the key value);
  `no_case_conflicts` = a case-folded key.
- `pair`, `registry_paths_resolve`, `markdown_paths_resolve` = **inclusion dependencies (IND)**,
  i.e. referential integrity / foreign keys (Casanova, Fagin, Papadimitriou 1984);
  `registry_paths_resolve` with `orphans` is a two-way IND.
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
toolchain-pin family as *configurations* rather than bespoke kinds. It would **not** subsume
`unique_by`, which keys on a path-*template* (`key: "{stem}"`) over file paths rather than on any
content extraction, a form the `Extract` enum (`Structured` / `Lines` / `Regex` / `WholeFile`) does
not have; folding it in would need a new path-template extractor. Guardrail: **check only, never infer**, because FD+IND implication is undecidable
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
| Recognition upgrade (build a structure the parser drops or never builds) | B1 duplicate keys, B2 well-formed, E1/E3/E6 markdown grammar, G1 tabular, H2 Dockerfile, F2 SPDX-expression | a parser or scanner |
| Dependency over a *dynamically-extracted* relation | C3 version consistency, D1 key parity, D2 placeholder parity, F3 REUSE completeness | new constraint logic (the `constraint` kind of 2.4); the dependency shape is partly present already (`registry_paths_resolve` and `cross_file set_equals` are dynamic INDs), so the genuinely new pieces are key-*set* enumeration (D1, which JSONPath cannot express) and the extract-and-assert packaging |
| General FO+COUNT (equality-of-counts / threshold over an extracted relation) | C3 count-distinct, key-count parity, B4 mutually-exclusive (= 1), dead-pattern (= 0) | a `count` operator on the `constraint` kind |
| A small offline decision procedure | C4 semver-range algebra | a self-contained solver |
| Out of band (the computability boundary of 2.2) | Scorecard-via-API, dependency-graph resolution, code semantics, SAST, secrets, crypto | deliberately excluded |

## 3. What alint already covers

To keep the gap list honest, these are covered today and are **not** gaps (they are cited so
a reader does not re-propose them):

- **Community-health file presence** (README / LICENSE / CONTRIBUTING / CODE_OF_CONDUCT /
  SECURITY / SUPPORT / CODEOWNERS / GOVERNANCE / CHANGELOG / issue and PR templates /
  FUNDING) via `oss-baseline@v1` and the existence family. Repolinter's default set and
  Scorecard's `Security-Policy` / `License` / `Dependency-Update-Tool` reduce to this.
- **REUSE and Apache license headers** via `compliance/reuse@v1`, `compliance/apache-2@v1`,
  `apache/governance@v1`.
- **GitHub Actions SHA/digest pinning and workflow least-privilege `permissions:`** via
  `ci/github-actions@v1`, so Scorecard's `Token-Permissions` and the Actions slice of
  `Pinned-Dependencies` are covered by composition.
- **The whole structural pre-commit-hooks battery** (trailing whitespace, final newline,
  mixed line ending, BOM, merge-conflict, case conflict, illegal Windows names, large files,
  submodules, shebang/executable pairing, symlinks) maps onto existing hygiene, unicode,
  portable-metadata, and unix-metadata kinds.
- **Config-file schema validation** via `json_schema_passes` for any of the 8 formats.
- **The monorepo boundary / cycle / orphan / forbidden-edge cluster** (Nx, Turborepo,
  sheriff, dependency-cruiser) via `import_gate` + `file_graph`, with the documented
  regex-import caveat (dynamic and aliased edges are missed).
- **Duplicate-listing checks** via `unique_by`; **co-change gates** via
  `pair_changed_together` / `changeset_requires_path`; **value/version equality across known
  files** via `cross_file_value_equals` with `normalize:` bands (the dogfood
  `install-snippets-match-workspace-version` rule proves it); **naming parity** via
  `filename_case` / `filename_regex`.

Already on alint's own radar (the `examples/README` emerging-gaps list and ROADMAP
single-source candidates): `json_key_sort_order`, `column_alignment`, `not_executable`,
`directory_hash`, `case_collision_safe`, `dir_name_matches_field`, `balanced_delimiters`, and
the backlogged `duplicate_blocks` (copy-paste) and WASM plugins. The `detect: linguist` /
`detect: askalono` facts were planned for an early cut and **never shipped** (verified: no
`licensee` or `askalono` crate reference exists in `crates/` source; the remaining mentions are a
ROADMAP entry and a `detect: linguist` test comment).

## 4. The gap families

Ten families, ranked. Within each, the table columns are: gap (working kind name), what it
detects, scope-fit, expressibility, and default auto-fix tier. The standouts carry a
paragraph.

### Family A: config-as-SSOT / meta-conformance (highest leverage)

alint's differentiated move is "read the manifest that owns the truth" (`scope_filter` reads
Cargo/pnpm workspaces). These gaps extend that from build manifests to the config files that
already declare a repo's own hygiene policy, so the policy is asserted from its source of
truth instead of re-declared (and drifting) inside `.alint.yml`.

| Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|
| `editorconfig_conforms` | every file obeys the repo's own `.editorconfig` (indent, EOL, charset, trim, final newline, max line length, spaces-after-tabs) | IN | K | Safe |
| `gitattributes_valid` | `* text=auto` present; every committed `--check` artifact has an `eol=lf` line; valid `linguist-*` / `export-ignore`; contradictory attribute lines | IN | P+K | Suggestion (Safe for the eol-pin insert) |
| `ignore_consistency` | `.dockerignore` covers the high-risk set and is a superset of `.gitignore`; dead ignore patterns matching zero paths; duplicate entries | IN | P+K | Suggestion (Safe for dedup) |

**`editorconfig_conforms` is the single highest-leverage idea in the survey.** alint already
has every underlying hygiene primitive (`indent_style`, `line_endings`, `no_trailing_whitespace`,
`final_newline`, `line_max_width`, `no_bom`), and it already parses `.editorconfig` as an INI
document (so its values are queryable via `ini_path_*` today). What is missing is a rule that
treats `.editorconfig` as the **policy source** and dispatches the existing hygiene evaluators
per glob section, so a user declares the thresholds once (in the file every editor already
reads) rather than duplicating them in `.alint.yml` where they drift. editorconfig-checker is
a top-tier pre-commit hook, so the demand is proven; the fix reuses the existing hygiene
fixers (Safe), with tab/space conversion staying Unsafe per the auto-fix doc.

`gitattributes_valid` targets a genuinely un-owned niche: no widely adopted `.gitattributes`
linter exists, and the "generated `--check` artifact needs an `eol=lf` pin or Windows CI
reports it stale" failure is one alint hit in its own history.

### Family B: structured-data integrity (beyond querying a value)

The structured-query family reads values at paths. These check the **shape** of the document
itself, which a path query structurally cannot see.

| Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|
| `no_duplicate_keys` | the same key twice in one mapping, for JSON / YAML / dotenv / properties (silent data loss) | IN | K | Suggestion |
| `well_formed` (`parses_as`) | a file parses as valid X with no schema and no query; JSON strictness extras | IN | P+K | Never |
| `structured_key_sort` | keys within a parsed object are in a canonical order (distinct from `ordered_block`, which sorts lines) | IN | K | Safe |
| `*_path_casing` / `*_path_mutually_exclusive` | a queried value obeys a naming case; exactly one of two paths is present | IN | P+K (casing: `*_path_matches` with a case regex works today) / K (the XOR) | Unsafe / Never |

**`no_duplicate_keys` is a gap alint's own reference documents.** `docs/rules.md` states that
a "detect duplicate key" rule is only expressible for XML and INI (which array-collect the
duplicates) and TOML and HCL (which reject the file as a parse error), because for JSON / YAML /
dotenv / properties the `Format::parse -> serde_json::Value` pipeline silently keeps the last
duplicate and discards the earlier ones before any rule runs. A duplicate key
from a bad merge in a large Kubernetes, Ansible, or CI file silently changes behavior;
yamllint's `key-duplicates` is on by default. Closing this needs a duplicate-aware or spanned
parse, which is the same substrate the auto-fix structured bridge builds.

### Family C: cross-file value and version consistency (fresh white space)

The band between "file exists" and "resolve the dep graph" where `cross_file`,
`pair_changed_together`, and `json_schema_passes` are genuinely differentiated. alint has the
muscle; the gaps are packaging plus a few capabilities. The prior PROPOSAL gap analysis mined
ls-lint / Repolinter / Conftest but not syncpack / manypkg / knip / the version-bump family,
so this whole vein is unmined.

| Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|
| version-SSOT ruleset | one version identical across package.json / Cargo.toml / pyproject / VERSION / `__version__` / Chart appVersion / OpenAPI info.version / the top CHANGELOG entry, plus the git tag | IN (files) / BORDERLINE (tag) | P + K | Safe/Unsafe |
| toolchain-pins ruleset | one language/tool version across `.nvmrc` / `engines` / Dockerfile `FROM` / CI setup / `.tool-versions` (Node, Rust, Python, Ruby, JVM, .NET) | IN | P (exact pins); range pins fall to C4 | Unsafe |
| `dependency_version_consistency` | every instance of a dynamically-discovered dependency agrees across a glob of manifests | IN (assert) / BORDERLINE (pick highest) | K | Unsafe |
| `semver_range` awareness | range intersection, satisfaction, and `^`/`~`/exact policy consistency | BORDERLINE | K | Unsafe / Never |
| `dependabot_ecosystem_drift` | a manifest exists but no `updates[]` entry covers its ecosystem | IN | P+K | Suggestion |
| `git_tag_matches` / `git_tag_valid` | release tag equals the manifest version; valid SemVer; annotated; signed; monotonic | BORDERLINE (local git-tag access) | K | Never |

**Version SSOT has enormous, proven demand.** Every release tool ships a machine-readable
manifest of "this version lives in files X, Y, Z" (bump-my-version, commitizen
`--check-consistency`, cargo-release `shared-version`, release-please `extra-files`, knope,
version-sync), and **none is language-agnostic**. The file-to-file part is expressible with
`cross_file_value_equals` today; the gaps are a bundled `versioning@v1` ruleset, an ergonomic
way to ingest an existing bump-tool config as the rule source, and a git-tag kind for the tag
half. The fix is the auto-fix Phase 2 flagship (`set_value` to the SSOT).

**`semver_range` is the one class alint structurally cannot do today.** `normalize:` compares
version-band **equality**; range **intersection** and **satisfaction** (syncpack "Same Range",
manypkg `INTERNAL_MISMATCH`) need a small self-contained semver algebra. Deterministic and
offline, so BORDERLINE rather than OUT.

### Family D: cross-file key and placeholder parity

Family C compares values at known paths; Family D compares the **set of keys or tokens**
across files with dynamic membership, a shape no surveyed single-file linter does natively and
squarely alint's cross-file territory.

| Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|
| `key_parity` | every locale file (or `.env` vs `.env.example`) declares the same key set as a reference | IN | K | Suggestion (Safe for sort) |
| `placeholder_parity` | interpolation tokens (`%s`, `{name}`, ICU plural, `{{var}}`) match the source string per key | IN | K | Never |

**`key_parity` is a differentiator.** Missing-translation and drifted-`.env.example` are
common, ship-blocking bugs; eslint-plugin-i18n-json, i18n-tasks, and compare-locales each do
it for their ecosystem, and no general linter does it natively. `cross_file` set relations
compare extracted value lists, not the key sets of a whole object with dynamic membership, so
this is a clean new kind (reference file plus a glob of peers).

### Family E: docs, accessibility, and i18n structural

A markdown and docs-hygiene family. `markdown_paths_resolve` today handles only backticked
paths with a required prefix list; it does not parse markdown link / image / anchor grammar,
alt text, or heading structure. All of these are a light line-scanner (a fenced-code toggle
plus front-matter skip), never a real AST.

| Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|
| `markdown_links_resolve` | relative `[text](./path)` and `![alt](./img)` targets that miss on disk; `#anchor` links matching no heading slug; undefined reference labels | IN (relative/anchor); external URLs OUT | K | Suggestion |
| `markdown_images_have_alt` | an image with no alt text (WCAG / MD045) | IN | P+K | Suggestion |
| `markdown_required_headings` | a document's heading sequence matches a required ordered template with wildcards (MD043) | IN | K | Suggestion |
| `markdown_reference_integrity` | reference labels defined and used; no duplicate/unused definitions (MD052/MD053) | IN | K | Suggestion |
| `markdown_no_duplicate_headings` | colliding heading slugs that break deep links (MD024) | IN | K | Never/Suggestion |
| `frontmatter_schema` / `frontmatter_path_*` | a `---`-fenced front-matter block exists and has required keys / passes a schema | IN | K | Suggestion |

`markdown_required_headings` is the most alint-native rule of the set: a structural manifest
over an extracted heading list ("every ADR has `## Status` / `## Context` / `## Decision`"),
which alint's own `docs/adr@v1` approximates crudely today with `file_content_matches`.
`frontmatter_schema` has direct dogfood demand: alint.org is an Astro site whose content
collections fail the build on missing front-matter, and the structured-query family cannot
reach into a `---`-fenced block embedded in a `.md` file. A `docs/markdown@v1` bundled ruleset
also covers the (P) single-line markdownlint rules (first-line heading, single H1, bare URLs,
proper-name casing) with existing primitives; list-nesting rules (MD005/007/029/030/032) need
an AST and stay OUT.

### Family F: license, SPDX, and provenance (beyond the `compliance/*` bundles)

| Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|
| `license_detectable` | LICENSE has a conventional name and content matching a known license signature phrase (heuristic, not a legal classifier) | IN (heuristic); OUT for fuzzy best-match | K | Never |
| `spdx_identifier_valid` | an `SPDX-License-Identifier` value is a real, non-deprecated id; an SPDX expression parses | BORDERLINE | K | Suggestion |
| `reuse_license_completeness` | every used SPDX id has a `LICENSES/<id>.txt`, and none is unreferenced | IN | P+K | Suggestion |
| `manifest_license_valid` | a manifest `license` field is a valid SPDX value (not a typo or bare `UNLICENSED`) | IN (presence) / BORDERLINE (validity) | P / K | Never |

A single bundled, regenerable **SPDX id table** (roughly 600 ids plus exceptions and
deprecated flags) plus a small self-contained SPDX-expression parser turns this whole
BORDERLINE tier green with no network. `license_detectable` revives the deferred `detect:`
fact as an offline heuristic (conventional name plus a bundled corpus of signature phrases),
staying clear of licensee/askalono's statistical best-match, which is OUT. An SBOM-presence
ruleset (`bom.json` / `*.cdx.json` exists and declares its format) is pure composition.

### Family G: data-file integrity

| Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|
| `delimited_columns` | CSV/TSV rows all match the header column count; no duplicate/empty column names; no stray quotes | IN (structure); Table-Schema types BORDERLINE | K | Never/Suggestion |

Data, ML, and analytics repos commit CSV fixtures where a ragged row is a silent downstream
break, and no general repo linter checks tabular integrity. The structured-query family has no
tabular reader, so this is a new kind.

### Family H: container and CI structural (beyond `ci/github-actions@v1`)

| Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|
| `pinned_references` | one cross-system unpinned-ref rule: Actions `@tag`, CircleCI orbs/images, GitLab/Cloud-Build `image:`, pre-commit `rev`, Dockerfile `FROM tag` (want `@sha256:`), `ADD http(s)://` | IN (detect); fix network-gated | P+K | Suggestion / class 7 |
| `dockerfile_structural` | hadolint's non-shell subset: base image tagged and digest-pinned, `COPY` not `ADD`, unique `FROM` aliases, JSON `CMD`/`ENTRYPOINT`, last `USER` not root, absolute `WORKDIR` | IN (structural); RUN-shell rules OUT | P (most) / K (cross-instruction state) | Unsafe / Suggestion |

`pinned_references` generalizes the pinning discipline alint already ships for GitHub Actions
to Dockerfile digests and pre-commit `rev:` (both clean gaps; hadolint has no base-image
**digest**-pin rule), motivated by the March 2025 `tj-actions/changed-files` supply-chain
incident. The tag-to-SHA fix is network-gated (auto-fix class 7).

### Family I: portability (extend the portable-metadata family)

| Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|
| `max_path_length` | any tracked path longer than a limit (default 260, Windows MAX_PATH) | IN | K (small) | Never |
| `lfs_pointer_valid` | a file tracked as LFS is a valid pointer, not a raw binary (or the inverse) | IN | P+K | Never |
| `file_encoding` | a file is valid UTF-8 or a declared charset; flag UTF-16 / Latin-1 where unwanted | IN | P+K | Unsafe (transcode) |

These sit alongside `no_illegal_windows_names` / `no_case_conflicts`, plus the already-tracked
`case_collision_safe`, `not_executable`, and `destroyed_symlinks` emerging-gaps.

### Family J: governance validity (beyond existence)

| Gap | Detects | Scope | Expr | Fix |
|---|---|---|---|---|
| `codeowners_valid` | CODEOWNERS syntax, owner format, duplicate patterns, dead patterns, unowned files, shadowing order | IN (except owner existence, which is OUT) | P+K | Suggestion |
| `governance/schemas@v1` | CITATION.cff / FUNDING.yml / dependabot / issue-forms / SECURITY-INSIGHTS validate against bundled schemas | IN | P | Never/Suggestion |
| `field_date_valid` | `security.txt` `Expires:` is a valid future date; CHANGELOG dates are valid ISO | IN | K (small) | Never |

`codeowners_valid` matters because GitHub silently skips invalid CODEOWNERS lines; alint today
only checks the file exists and is non-empty. `field_date_valid` carries a determinism caveat:
a "days since" verdict is time-dependent, which conflicts with constitution invariant 1, so it
must take a fixed "now" (a `--as-of` input or a per-run pinned clock) rather than reading the
wall clock. A Keep-a-Changelog structural ruleset and a `no_committed_binaries` discovery kind
(magic-byte scan plus allowlist, fix = git-untrack) round out the (P) composition wins.

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
  secret-scanning, a stated non-goal; the defensible slice (`git_no_denied_paths`,
  `git_blame_age`) is already covered.

## 6. Clearly out of scope

Named honestly, these belong to the tools the README already points at:

- **Scorecard checks needing a Git host API or release metadata:** Branch-Protection,
  Webhooks, Code-Review, Contributors, Maintained, CI-Tests, Signed-Releases, Packaging,
  CII-Best-Practices, Vulnerabilities (OSV), Fuzzing, SAST app-runs. (Scorecard's own
  `--local` client returns "unsupported" for every one of these.)
- **Dependency-graph resolution:** knip symbol usage, dependency-cruiser reachability/license,
  true lockfile sync, cargo-deny advisories/licenses, cargo-udeps/machete.
- **Code semantics / AST:** ESLint / Clippy / ruff; markdown list-nesting; yamllint value-type
  rules; hadolint RUN-shell rules; Python-AST pre-commit hooks.
- **SAST / taint:** Semgrep, CodeQL, CI expression-injection dataflow.
- **Secret scanning:** gitleaks, trufflehog.
- **Semantic IaC:** Checkov, tfsec, KICS.
- **Fuzzy license classification, signature/attestation crypto, and numeric quality scores.**
- **Whole-file reformatting:** alint is not a formatter.

## 7. Auto-fix tie-in

Most high-value gaps map onto the [`auto-fix.md`](auto-fix.md) four-state model, and the
highest-value new fixes ride substrates that document already plans:

| Gap | Tier | Note |
|---|---|---|
| `editorconfig_conforms` | Safe | reuses the existing hygiene fixers, driven by the file's declared policy |
| `gitattributes_valid` (eol-pin) | Safe | a presence-guarded `ReplaceRange` insert |
| `structured_key_sort` | Safe | format-preserving key reorder via the structured bridge (auto-fix Phase 2) |
| version SSOT | Safe/Unsafe | structured `set_value` to the SSOT (auto-fix Phase 2 flagship) |
| toolchain / dependency drift | Unsafe | set the drifted pin to canonical |
| `no_duplicate_keys` | Suggestion | which duplicate to keep is ambiguous |
| `key_parity`, markdown gaps, `dependabot_ecosystem_drift` | Suggestion | insert stubs / stanzas; a human confirms |
| `well_formed`, `git_tag`, `max_path_length`, `license_detectable` | Never | no unique correct output |

The flagship structured-value write-back engine (auto-fix Phase 2) is the enabler for the
highest-value new fixes here: version SSOT, structured key sort, and any `*_path_equals`-backed
gap all ride the same path-to-span bridge.

## 8. Prioritized shortlist

1. `editorconfig_conforms` (A1): config-as-SSOT; every primitive exists, zero conformance
   awareness; Safe auto-fix. (K)
2. `no_duplicate_keys` (B1): a gap `docs/rules.md` documents; silent data loss; structural. (K)
3. `key_parity` (D1): cross-file key-set equality for i18n and `.env.example`; alint's
   territory, no general linter does it. (K)
4. version-SSOT ruleset + config-ingest (C1): huge proven demand; mostly expressible, needs
   packaging plus a tag kind. (P+K)
5. toolchain-pins ruleset (C2): under-served, because Dependabot ignores pin files. (P for exact
   pins; range pins are C4 `semver_range`)
6. `semver_range` awareness (C4): the one class alint structurally cannot do. (K)
7. `markdown_links_resolve` (E1): `markdown_paths_resolve` does only backticks. (K)
8. `dependency_version_consistency` (C3): the syncpack/manypkg headline. (K)
9. `license_detectable` (F1): the deferred `detect:` fact; Scorecard/Repolinter/GitHub gate on
   it. (K)
10. `gitattributes_valid` (A2): an un-owned niche; alint hit the bug itself. (P+K)
11. `delimited_columns` (G1): data repos; no general linter covers tabular integrity. (K)
12. `markdown_required_headings` (E3): the most alint-native doc rule. (K)
13. `pinned_references` (H1): Dockerfile digest and pre-commit `rev` are clean gaps. (P+K)
14. `markdown_images_have_alt` (E2): a fresh accessibility angle. (P+K)
15. `dependabot_ecosystem_drift` (C5) and `codeowners_valid` (J1): governance validity beyond
    existence. (P+K)

Second tier: `well_formed`, `frontmatter_schema`, `spdx_identifier_valid`,
`reuse_license_completeness`, `ignore_consistency`, `placeholder_parity`, `git_tag`,
`max_path_length`, `lfs_pointer_valid`, the community-schema pack, `field_date_valid`,
`structured_key_sort` (already on the radar).

## 9. Strategic read and reusable substrates

Two clusters dominate the high-value gaps, and both are almost entirely un-owned by any single
language-agnostic tool:

1. **Config-as-SSOT / meta-conformance (Family A):** read the config that already declares the
   repo's policy (`.editorconfig`, `.gitattributes`, a bump-tool manifest) and assert it,
   instead of re-declaring thresholds in `.alint.yml`. The same "manifest owns the truth" move
   as `scope_filter`, applied to hygiene.
2. **Cross-file value / key / version consistency (Families C and D):** the version-SSOT,
   toolchain-pin, translation-parity, and dependency-consistency vein, which the prior
   PROPOSAL gap analysis never mined (it covered ls-lint / Repolinter / Conftest, not
   syncpack / manypkg / knip / the version-bump family).

Four reusable substrates unlock disproportionate coverage, so they should be sequenced first:

- **The format-preserving structured-value write-back engine** (auto-fix Phase 2) turns the
  version-SSOT, structured-key-sort, and the whole `*_path_equals`-backed set of gaps fixable.
- **A bundled SPDX id table plus a small expression parser** turns the BORDERLINE license-validity
  checks (F2, and F4's validity half) green with no network, and underpins F3's REUSE completeness
  (an IN cross-file check).
- **A duplicate-aware / spanned structured parser** (shared with the auto-fix bridge) unlocks
  `no_duplicate_keys` (B1).
- **A markdown link / heading / front-matter scanner** (one light line-scanner with a
  fenced-code toggle) unlocks all of Family E.
- **A single `constraint` kind** (extract relations from a glob, then assert a dependency;
  section 2.4) generalizes `cross_file` and `registry_paths_resolve` (not `unique_by`, whose
  path-template keying needs a new extractor; 2.4) and unlocks the
  whole cross-file consistency vein (`dependency_version_consistency`, `key_parity`,
  `placeholder_parity`) as configurations rather than bespoke kinds, subject to the
  check-only-never-infer guardrail.

Sequencing suggestion: ship `editorconfig_conforms` and `no_duplicate_keys` first (highest
leverage, each a self-contained kind on an existing evaluator), package the version-SSOT and
toolchain-pins bundled rulesets (mostly (P), immediate value), then build the markdown scanner
(Family E) and the SPDX table (Family F) as shared substrates, with `semver_range` and the
cross-file consistency kinds following as engine capabilities.

## 10. References

Repolinter rules; OpenSSF Scorecard checks, Allstar, Best Practices, OSPS Baseline;
CNCF / Apache maturity models; GitHub community health files; syncpack, manypkg, knip, Nx and
Turborepo boundaries, dependency-cruiser; the version-bump family (bump-my-version, commitizen,
cargo-release, release-please, knope, version-sync); markdownlint, yamllint,
editorconfig-checker, dotenv-linter, ls-lint, hadolint; REUSE and the SPDX license list;
eslint-plugin-i18n-json, i18n-tasks, compare-locales, gettext `msgfmt`; csvlint and the
Frictionless Table Schema; Keep a Changelog; SemVer 2.0.0; codeowners-validator;
remark-lint-frontmatter-schema; the pre-commit-hooks battery. Full URLs are collected in the
research artifact this doc is distilled from.

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
