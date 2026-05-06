---
destination: alint.org/api/{rules,rulesets,versions}.json (3 site-repo build artifacts under /api/)
status: drafting
blocks_on: build-time generation script (proposed below); rule-registry source-of-truth confirmation (`crates/alint-rules/src/lib.rs::register_builtin`); ruleset-bundle source confirmation (`crates/alint-dsl/rulesets/v1/*.yml`); JSON-Schema decision (recommend ship without formal schema at v0.10; document shape in JSON `$comment` fields)
last_touched: 2026-05-06
---

# alint.org/api/{rules,rulesets,versions}.json — content brief for the site repo

## Why

Three stable JSON endpoints for programmatic discovery of alint's
catalogue. They serve four concrete use cases:

1. **Editor / LSP integrations.** A future LSP server (or a third-
   party VS Code extension) can fetch `/api/rules.json` to build
   autocomplete + hover docs without parsing the YAML rulesets.
2. **Documentation generators / aggregators.** Tools like
   "awesome-linters" lists or AI agents asked to "compare repo-
   linters' rule coverage" can consume the JSON directly without
   scraping HTML.
3. **alint.org itself.** The `/docs/rules/` and
   `/docs/bundled-rulesets/` pages currently auto-generate from
   markdown files in `docs-bundle`; over time, generating from the
   JSON endpoints (with markdown only for prose) reduces the
   sync surface.
4. **Migration tooling.** A "Repolinter → alint" config transformer
   can fetch `/api/rules.json` to validate that the target rule
   kinds exist + are spelled correctly.

These already exist as **machine-derivable data** in the alint repo:

- **`/api/rules.json`** — derived from `crates/alint-rules/src/lib.rs`
  (the `register_builtin` function enumerates all 60 rule kinds).
- **`/api/rulesets.json`** — derived from
  `crates/alint-dsl/rulesets/v1/*.yml` (19 ruleset YAML files).
- **`/api/versions.json`** — derived from git tags + CHANGELOG.md
  (already feeds `releases.atom` — share the parse).

This brief specifies the **build-time generation step** that turns
those sources into stable, versioned JSON at the site root.

## Proposed shapes

### `/api/rules.json`

```json
{
  "$schema": "https://alint.org/api/rules.schema.json",
  "$comment": "Catalogue of every rule kind alint ships. Generated from crates/alint-rules/src/lib.rs::register_builtin at build time. Stable across minor versions; breaking changes follow semver-major bumps of the api_version field.",
  "api_version": "v1",
  "alint_version": "0.9.16",
  "generated_at": "2026-05-06T00:00:00Z",
  "rules": [
    {
      "id": "file_exists",
      "family": "existence",
      "level_default": "error",
      "fix_available": false,
      "summary": "Asserts that the named file (or one of several) is present in the repo.",
      "doc_url": "https://alint.org/docs/rules/existence/file_exists/",
      "added_in": "v0.1.0"
    },
    {
      "id": "file_content_matches",
      "family": "content",
      "level_default": "warning",
      "fix_available": false,
      "summary": "Asserts that a file's content matches a regex pattern.",
      "doc_url": "https://alint.org/docs/rules/content/file_content_matches/",
      "added_in": "v0.1.0"
    },
    {
      "id": "trailing_whitespace",
      "family": "text-hygiene",
      "level_default": "warning",
      "fix_available": true,
      "fix_op": "trim_trailing_whitespace",
      "summary": "Flags trailing whitespace on any line; auto-fix removes it.",
      "doc_url": "https://alint.org/docs/rules/text-hygiene/trailing_whitespace/",
      "added_in": "v0.2.0"
    },
    {
      "id": "structured_path_equals",
      "family": "structured-query",
      "level_default": "error",
      "fix_available": false,
      "summary": "Asserts that the value at the given JSONPath (RFC 9535) equals the expected literal. Works on JSON, YAML, TOML.",
      "doc_url": "https://alint.org/docs/rules/structured-query/structured_path_equals/",
      "added_in": "v0.5.0"
    }
    // ... 60 entries total ...
  ]
}
```

**Fields per rule:**

| Field | Source | Notes |
|---|---|---|
| `id` | The string registered via `registry.register("<id>", …)` in `crates/alint-rules/src/lib.rs` | The canonical rule-kind name |
| `family` | Per-rule comments + the family-grouping in `lib.rs` (`// Structured-query family — JSONPath queries over …`) | One of: existence, content, naming, text-hygiene, structured-query, security, encoding, structure, portable, unix, git-hygiene, cross-file, plugin |
| `level_default` | Each rule's `Default` impl (or hardcoded in builder) | `error` / `warning` / `info` |
| `fix_available` | Presence of a `Fixer` impl in `crates/alint-rules/src/fixers.rs` that targets this kind | bool |
| `fix_op` | Name of the fixer enum variant (only when `fix_available: true`) | e.g. `trim_trailing_whitespace`, `normalize_line_endings` |
| `summary` | First sentence of the rule's docstring (`//! …`) in its module | Human-readable, ~150 char |
| `doc_url` | Constructed as `https://alint.org/docs/rules/<family>/<id>/` | Per-rule doc page (must exist) |
| `added_in` | The first version tag where this rule kind appears in `register_builtin` | Derived by walking git history; cached in the build script. If derivation is too expensive, ship without and add later. |

### `/api/rulesets.json`

```json
{
  "$schema": "https://alint.org/api/rulesets.schema.json",
  "$comment": "Catalogue of every bundled ruleset. Generated from crates/alint-dsl/rulesets/v1/*.yml at build time.",
  "api_version": "v1",
  "alint_version": "0.9.16",
  "generated_at": "2026-05-06T00:00:00Z",
  "rulesets": [
    {
      "name": "oss-baseline",
      "version": "v1",
      "extends_url": "alint://bundled/oss-baseline@v1",
      "summary": "OSS-hygiene baseline: LICENSE, README, CONTRIBUTING, CODE_OF_CONDUCT, SECURITY. Covers Repolinter's rule catalogue.",
      "doc_url": "https://alint.org/docs/bundled-rulesets/oss-baseline/",
      "rule_count": 15,
      "rule_ids": [
        "oss-license-exists",
        "oss-readme-exists",
        "oss-readme-has-headings",
        "oss-contributing-exists",
        "oss-code-of-conduct-exists",
        "oss-security-exists",
        "oss-changelog-exists"
        // ... 15 entries total ...
      ],
      "facts": [
        { "id": "is_oss_repo", "summary": "Always true; oss-baseline applies broadly." }
      ],
      "ecosystem": "general"
    },
    {
      "name": "rust",
      "version": "v1",
      "extends_url": "alint://bundled/rust@v1",
      "summary": "Hygiene checks for Rust projects: Cargo.toml shape, Cargo.lock, target/ ban, snake_case modules.",
      "doc_url": "https://alint.org/docs/bundled-rulesets/rust/",
      "rule_count": 18,
      "rule_ids": [
        "rust-cargo-toml-exists",
        "rust-cargo-lock-exists",
        "rust-toolchain-pinned"
        // ...
      ],
      "facts": [
        { "id": "has_rust", "summary": "True if any Cargo.toml exists in the tree." }
      ],
      "ecosystem": "rust"
    }
    // ... 19 entries total ...
  ]
}
```

**Fields per ruleset:**

| Field | Source | Notes |
|---|---|---|
| `name` | Filename minus `.yml` (e.g. `rust.yml` → `rust`); for nested files, the directory + name (`monorepo/cargo-workspace.yml` → `monorepo/cargo-workspace`) | |
| `version` | The `v1` suffix in the bundled URL (currently always `v1`) | |
| `extends_url` | Constructed as `alint://bundled/<name>@v<version>` | The string a user puts in their `extends:` |
| `summary` | First sentence of the YAML's leading `#`-comment block | |
| `doc_url` | Constructed as `https://alint.org/docs/bundled-rulesets/<name>/` | Per-ruleset doc page |
| `rule_count` | Length of the ruleset's `rules:` list | |
| `rule_ids` | The `id` field of each rule in the ruleset | Lets a consumer enumerate the surface |
| `facts` | The `facts:` block from the YAML (id + first-sentence summary derived from the `#`-comment above it, or null if absent) | |
| `ecosystem` | Manual mapping: `rust` / `node` / `python` / `go` / `java` / `general` / `agent` / `monorepo` / `ci` / `compliance` / `tooling` / `docs` | Derived from the directory + filename pattern |

### `/api/versions.json`

```json
{
  "$schema": "https://alint.org/api/versions.schema.json",
  "$comment": "Released versions with date + GitHub Release URL. Generated from CHANGELOG.md at build time. Newest first.",
  "api_version": "v1",
  "generated_at": "2026-05-06T00:00:00Z",
  "latest": "0.9.16",
  "versions": [
    {
      "version": "0.9.16",
      "date": "2026-05-06",
      "release_url": "https://github.com/asamarts/alint/releases/tag/v0.9.16",
      "changelog_url": "https://github.com/asamarts/alint/blob/main/CHANGELOG.md#0916---2026-05-06",
      "atom_entry_url": "https://alint.org/releases.atom#alint-v0.9.16",
      "yanked": false,
      "headline": "Config DX hardening release."
    },
    {
      "version": "0.9.14",
      "date": "2026-05-05",
      "release_url": "https://github.com/asamarts/alint/releases/tag/v0.9.14",
      "changelog_url": "https://github.com/asamarts/alint/blob/main/CHANGELOG.md#0914---2026-05-05",
      "yanked": false,
      "headline": "..."
    },
    {
      "version": "0.8.1",
      "date": "2026-04-29",
      "release_url": "https://github.com/asamarts/alint/releases/tag/v0.8.1",
      "yanked": true,
      "yank_reason": "Partially published — superseded by v0.8.2",
      "headline": "..."
    }
    // ... all releases, newest first ...
  ]
}
```

**Fields per version:**

| Field | Source | Notes |
|---|---|---|
| `version` | The `[X.Y.Z]` from the CHANGELOG heading | SemVer string, no `v` prefix |
| `date` | The `— YYYY-MM-DD` from the heading | ISO-8601 date |
| `release_url` | Constructed | `https://github.com/asamarts/alint/releases/tag/v{X.Y.Z}` |
| `changelog_url` | Constructed | `…/blob/main/CHANGELOG.md#0916---2026-05-06` style anchor |
| `atom_entry_url` | Constructed | The `<id>` of the corresponding `<entry>` in `releases.atom` |
| `yanked` | Heading parenthetical contains "superseded" or "yanked" | bool |
| `yank_reason` | The parenthetical itself (only when `yanked: true`) | |
| `headline` | First sentence of the release body | Same source as the Atom feed's `<summary>` |

## Build-time generation strategy

Mirrors the `releases.atom` strategy (one xtask per artefact, all
called from the docs-bundle sync step):

```
xtask/src/bin/build-api-rules.rs       → emits api/rules.json
xtask/src/bin/build-api-rulesets.rs    → emits api/rulesets.json
xtask/src/bin/build-api-versions.rs    → emits api/versions.json
```

Or one consolidated binary `build-api.rs` that emits all three.
Recommend **one binary** — they share parsing (CHANGELOG.md is read
by both `versions` and `releases.atom`; rule-registry walking is
specific to `rules`) and emitting them together keeps the build step
atomic.

### Sketch — `xtask/src/bin/build-api.rs`

```rust
//! Generate alint.org/api/{rules,rulesets,versions}.json from in-repo data.
//!
//! Usage: `cargo xtask build-api --out api/`
//!
//! Sources:
//! - rules.json: walks `crates/alint-rules/src/lib.rs::register_builtin`
//! - rulesets.json: parses every YAML under `crates/alint-dsl/rulesets/v1/`
//! - versions.json: parses CHANGELOG.md headings (shared with releases.atom)

use anyhow::Result;
use std::path::PathBuf;

fn main() -> Result<()> {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().to_path_buf();
    let out_dir = std::env::args()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("api"));

    std::fs::create_dir_all(&out_dir)?;

    write_rules_json(&workspace_root, &out_dir)?;
    write_rulesets_json(&workspace_root, &out_dir)?;
    write_versions_json(&workspace_root, &out_dir)?;

    Ok(())
}

fn write_rules_json(_root: &PathBuf, _out: &PathBuf) -> Result<()> {
    // Approach A: parse register_builtin via syn (full Rust parse). Most robust.
    // Approach B: regex-extract `registry.register("<kind>", …)` lines. Faster, fragile.
    // Recommend A — register_builtin is stable + small, syn parse is ~50 LOC.
    // For each kind: read the source module's docstring for the summary,
    // check fixers.rs for the fix_op, default-level from builder/Default impl.
    todo!()
}

fn write_rulesets_json(root: &PathBuf, out: &PathBuf) -> Result<()> {
    use serde_yaml::Value;
    let rulesets_dir = root.join("crates/alint-dsl/rulesets/v1");
    let mut rulesets = Vec::new();
    for entry in walkdir::WalkDir::new(&rulesets_dir) {
        let entry = entry?;
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "yml") {
            rulesets.push(parse_ruleset(entry.path(), &rulesets_dir)?);
        }
    }
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "api_version": "v1",
        "alint_version": env!("CARGO_PKG_VERSION"),
        "rulesets": rulesets,
    }))?;
    std::fs::write(out.join("rulesets.json"), json)?;
    Ok(())
}

fn write_versions_json(_root: &PathBuf, _out: &PathBuf) -> Result<()> {
    // Reuses the CHANGELOG parser from build-releases-atom.
    todo!()
}
```

### docs-bundle pipeline integration

```bash
# In the docs-bundle sync step:
cargo xtask build-api --out docs-bundle/api/
# Site repo's Astro picks up docs-bundle/api/*.json into public/api/.
```

## API stability commitment

This section goes into `docs/site/api/index.md` (a new docs page that
points at the three endpoints):

- The `api_version` field is `"v1"` for the v0.10 launch. Field
  additions within v1 are non-breaking. Renames or removals require
  bumping to `"v2"` and shipping `/api/v2/{rules,rulesets,versions}.json`
  alongside the v1 endpoints for a deprecation window of at least one
  minor release.
- `rules.json` field stability: `id`, `family`, `level_default`,
  `fix_available`, `summary`, `doc_url` are committed. `added_in`
  is best-effort and may be missing for early rules.
- `rulesets.json` field stability: `name`, `version`, `extends_url`,
  `rule_ids`, `summary`, `doc_url` are committed. `facts` and
  `ecosystem` may evolve.
- `versions.json` field stability: `version`, `date`, `release_url`,
  `yanked` are committed. `headline`, `atom_entry_url`,
  `changelog_url` are best-effort.

## JSON Schema (deferred)

Each endpoint includes a `$schema` URL pointing to a formal schema:

- `https://alint.org/api/rules.schema.json`
- `https://alint.org/api/rulesets.schema.json`
- `https://alint.org/api/versions.schema.json`

**Recommend NOT shipping the schemas at v0.10.** The `$schema` field
sits there as a forward-looking pointer; consumers can read the
`$comment` field for shape documentation in the meantime. Schema
files land at v0.11 once the field set has stabilised across one or
two real consumers.

## Open questions

1. **Build cost vs. cache.** Walking git history for `added_in`
   could be slow (~30s per release on a CI runner). Recommend:
   pre-compute `added_in` once per release and cache it in
   `xtask/src/data/rule-history.json` (committed). Build script
   reads the cache; CI job updates the cache when a new rule is
   registered.
2. **`fix_available` extraction.** The fixer registry in
   `crates/alint-rules/src/fixers.rs` doesn't have a clean
   "rule kind → fixer" map currently. Either:
   - **(a)** Add a comment-attribute convention (`// fixes:
     trailing_whitespace`) that the build script greps. Low engine
     impact, high build-script reliability.
   - **(b)** Wire fixers into a metadata table at registration
     time. Cleaner long-term but a small engine refactor.
   - **Recommend (a)** for v0.10; revisit (b) at v0.11.
3. **Per-rule summaries from docstrings.** The convention of "first
   sentence of the rule's `//! …` docstring is the summary" needs
   to be enforced (CI check). Currently the rule modules don't
   uniformly start with a one-sentence summary.
4. **Cache headers.** `/api/*.json` should have a short
   `Cache-Control` (5-15 min) so consumers see fresh data after
   each deploy without thrashing the CDN. Cloudflare Pages
   defaults are usually fine; verify.
5. **Versioned URLs.** The brief proposes a single
   `/api/{rules,rulesets,versions}.json` triplet with an internal
   `api_version: "v1"` field. The alternative is path-versioning:
   `/api/v1/rules.json`. **Recommend internal versioning** for
   v0.10 (one URL, simpler consumers); switch to path-versioning
   if/when we ship a v2.
6. **CORS headers.** `/api/*.json` should serve with
   `Access-Control-Allow-Origin: *` so browser-side consumers
   (third-party documentation sites) can fetch directly. Verify
   Cloudflare Pages config supports this.

## Pre-publish checklist

- [ ] `xtask/src/bin/build-api.rs` exists and produces all three
      JSON files.
- [ ] Each output file validates as well-formed JSON
      (`jq '.' < rules.json > /dev/null` exits 0).
- [ ] Each output file is reachable at the documented URL
      (`https://alint.org/api/rules.json` etc.).
- [ ] `rules.json` has 60 entries (matches the registered-kinds
      count); CI regression check.
- [ ] `rulesets.json` has 19 entries (matches the YAML files
      count); CI regression check.
- [ ] `versions.json` is sorted newest-first and `latest`
      matches `versions[0].version`.
- [ ] All `doc_url` values resolve (no 404s) — automated link check.
- [ ] CORS header set: `curl -I https://alint.org/api/rules.json`
      includes `Access-Control-Allow-Origin: *`.
- [ ] One real third-party consumer test (e.g., a fetch from a
      Codepen or local script) confirms end-to-end fetch + parse.
- [ ] `docs/site/api/index.md` exists documenting the three
      endpoints + stability commitment.
- [ ] STATE.md row for `alint.org/api/{rules,rulesets,versions}.json`
      flipped from `missing` to `live` with date + commit SHA.

## Estimated diff size

In the alint repo:
- 1 new file at `xtask/src/bin/build-api.rs`: ~300 lines.
- `xtask/Cargo.toml` additions (syn, walkdir, serde_yaml,
  serde_json if not present): ~5 lines.
- 1 new doc page at `docs/site/api/index.md`: ~80 lines (documents
  the three endpoints + stability commitment + example fetches).
- `RELEASING.md` update — add `cargo xtask build-api` to the
  release-process checklist: ~3 lines.
- (Optional) 1 cache file at `xtask/src/data/rule-history.json` for
  the `added_in` lookups: ~60 lines.

In the site repo:
- 1 build-step entry to invoke the xtask + place outputs in
  `public/api/`: ~5 lines.
- (Cloudflare Pages headers config) Cache-Control + CORS for
  `/api/*.json`: ~3 lines.

Total: ~395 lines (alint repo) + ~10 lines (site repo).

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `releases-atom.md` | Both parse CHANGELOG.md; share the parser. Bundle into one xtask binary or one shared library. |
| `llms-txt.md`, `llms-full-txt.md` | The LLM-discovery surfaces could link to `/api/*.json` as machine-readable companions. Add a "Programmatic catalogue" section to llms.txt once the endpoints ship. |
| Future LSP server | The LSP can fetch `/api/rules.json` + `/api/rulesets.json` instead of bundling its own copy. Keep field stability in mind from v0.10 onward. |
