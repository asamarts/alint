---
destination: alint.org/releases.atom (site-repo build artifact at the public root)
status: drafting
blocks_on: build-time generation script (proposed below); CHANGELOG.md heading-format invariant (currently stable: `## [X.Y.Z] — YYYY-MM-DD`); decision on Astro/Starlight integration vs external script (recommend external script)
last_touched: 2026-05-06
---

# alint.org/releases.atom — content brief for the site repo

## Why

A machine-readable release feed at `/releases.atom` lets:

1. **RSS/Atom readers** (Feedly, Inoreader, NewsBlur, NetNewsWire) deliver alint releases to subscribers.
2. **Slack / Discord / Mattermost release-watch bots** post new releases to channels via standard feed integrations.
3. **Dependabot-style automation** track upstream releases without scraping HTML.
4. **The "subscribe to releases" CTA on the alint.org site** point somewhere standard.

GitHub already publishes a per-repo Atom feed at
`https://github.com/asamarts/alint/releases.atom`. So why a site-side feed?

- **Decoupling.** alint.org/releases.atom under our control means we can
  enrich entries (link to per-release blog posts, tag versions
  semantically, drop yanked releases) without depending on GitHub's
  schema.
- **Discoverability.** Most users land on alint.org first, not
  github.com. Putting `/releases.atom` on the marketing surface +
  surfacing it via `<link rel="alternate" type="application/atom+xml">`
  in the page head means readers' "subscribe" buttons just work.
- **Permanent URL.** GitHub's URL is owner-bound; if the project ever
  moves orgs, alint.org/releases.atom doesn't break.

CHANGELOG.md is the canonical source. We already maintain it carefully
(see release-time discipline in `RELEASING.md` + the per-release
patterns in the recent commit history). The build script just parses
it.

## Proposed feed shape

Per [Atom 1.0 spec (RFC 4287)](https://www.rfc-editor.org/rfc/rfc4287),
the feed is an XML document with one `<feed>` containing N `<entry>`
elements, one per release.

```xml
<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>alint releases</title>
  <subtitle>Fast, language-agnostic linter for repository structure, files, and content.</subtitle>
  <link href="https://alint.org/releases.atom" rel="self" type="application/atom+xml"/>
  <link href="https://alint.org/" rel="alternate" type="text/html"/>
  <id>https://alint.org/releases.atom</id>
  <updated>2026-05-06T00:00:00Z</updated>
  <author>
    <name>asamarts</name>
    <email>aliaksandr.samartsau@gmail.com</email>
  </author>
  <icon>https://alint.org/favicon.ico</icon>
  <generator uri="https://github.com/asamarts/alint" version="0.9.16">alint releases.atom build script</generator>

  <entry>
    <title>alint v0.9.16</title>
    <link href="https://github.com/asamarts/alint/releases/tag/v0.9.16" rel="alternate" type="text/html"/>
    <id>https://github.com/asamarts/alint/releases/tag/v0.9.16</id>
    <updated>2026-05-06T00:00:00Z</updated>
    <published>2026-05-06T00:00:00Z</published>
    <summary type="text">Config DX hardening release. Closes the launch-prep validation pass with seven-phase coverage of the 17 schema + runtime pitfalls surfaced during the P2a 20-repo case-study sweep, plus the deny_unknown_fields uniformity audit.</summary>
    <content type="html">
      &lt;p&gt;Config DX hardening release. Closes the launch-prep validation pass...&lt;/p&gt;
      &lt;p&gt;&lt;a href="https://github.com/asamarts/alint/blob/main/CHANGELOG.md#0916---2026-05-06"&gt;Read the full changelog →&lt;/a&gt;&lt;/p&gt;
    </content>
  </entry>

  <!-- ... one entry per release, newest first ... -->

  <entry>
    <title>alint v0.4.0</title>
    <link href="https://github.com/asamarts/alint/releases/tag/v0.4.0" rel="alternate" type="text/html"/>
    <id>https://github.com/asamarts/alint/releases/tag/v0.4.0</id>
    <updated>2026-04-21T00:00:00Z</updated>
    <published>2026-04-21T00:00:00Z</published>
    <summary type="text">Initial public-facing release.</summary>
    <content type="html">&lt;p&gt;Initial public-facing release.&lt;/p&gt;</content>
  </entry>
</feed>
```

### Entry fields per release

| Field | Source | Notes |
|---|---|---|
| `<title>` | CHANGELOG `## [X.Y.Z] — YYYY-MM-DD` heading | Format: `alint vX.Y.Z` |
| `<link href>` | Constructed | `https://github.com/asamarts/alint/releases/tag/v{X.Y.Z}` |
| `<id>` | Same as link | Atom requires a globally-unique IRI per entry; the GitHub release URL is permanent and unique |
| `<updated>` / `<published>` | The `— YYYY-MM-DD` suffix in the heading | Format as RFC 3339 with `T00:00:00Z` (CHANGELOG only has date precision; midnight UTC is conventional for release-date Atom feeds) |
| `<summary type="text">` | First sentence of the release body | Plain-text, ~200-char headline. The build script extracts the text up to the first `.` after the heading, skipping blank lines and section labels (`### Added`, etc.) |
| `<content type="html">` | First paragraph of the release body | HTML-escaped, with a "Read the full changelog →" link to the per-release CHANGELOG anchor |

### Skipped releases

- `## [Unreleased]` — never appears in the feed (it's not a release).
- `## [0.8.1] — 2026-04-29 (partially published — superseded by v0.8.2)`
  — the build script regex MUST tolerate the parenthetical suffix on
  the heading; this release should still appear in the feed (with the
  parenthetical preserved in the title) for completeness.

## Build-time generation strategy

Two approaches:

### Approach A — external Rust script (recommended)

Add a `xtask/src/bin/build-releases-atom.rs` to the alint repo that:

1. Reads `CHANGELOG.md`.
2. Parses each `## [X.Y.Z] — YYYY-MM-DD` heading + the body until the
   next heading.
3. Emits Atom XML to stdout.

Run as part of `cargo xtask build-releases-atom > public/releases.atom`
in the docs-bundle pipeline (the script that syncs alint repo content
to the alint.org site repo's Astro build).

**Why Rust:** the alint repo already has `xtask/`, the parsing logic is
straightforward (~100 LOC), and it co-locates with the CHANGELOG
schema invariants we already enforce in CI.

### Approach B — Astro/Starlight integration via Astro's RSS plugin

Astro ships an RSS plugin (`@astrojs/rss`) that can generate Atom
feeds from content collections. Would require defining a "releases"
content collection in the site repo, populated by the docs-bundle
sync step. Heavier integration; defer to v0.11+.

**Recommend Approach A.** Co-locates with CHANGELOG (one source of
truth, one parser); no new content-collection schema to maintain;
ships in 1-2 days.

### Sketch — `xtask/src/bin/build-releases-atom.rs`

```rust
//! Generate releases.atom from CHANGELOG.md.
//!
//! Usage: `cargo xtask build-releases-atom > releases.atom`
//!
//! Parses `## [X.Y.Z] — YYYY-MM-DD` headings; emits one Atom <entry>
//! per release with the GitHub release URL as the canonical link.
//! Skips `## [Unreleased]`. Tolerates parenthetical suffixes on the
//! heading line.

use std::path::PathBuf;
use anyhow::{Context, Result};

const REPO_URL: &str = "https://github.com/asamarts/alint";
const SITE_URL: &str = "https://alint.org";

#[derive(Debug)]
struct Release {
    version: String,    // "0.9.16"
    date: String,       // "2026-05-06"
    suffix: String,     // "" or " (partially published — superseded by v0.8.2)"
    summary: String,    // First sentence of body, ~200 char
    full_body: String,  // First paragraph, for <content>
}

fn parse_changelog(text: &str) -> Result<Vec<Release>> {
    // Heading regex: `^## \[(?P<ver>[\d.]+)\] — (?P<date>\d{4}-\d{2}-\d{2})(?P<suffix>.*)$`
    // Body extraction: everything between this heading and the next `^## ` or EOF.
    // Summary extraction: first prose sentence (skip ### Added/Changed labels).
    todo!()
}

fn render_atom(releases: &[Release]) -> String {
    // Emit XML matching the shape in the brief above.
    todo!()
}

fn main() -> Result<()> {
    let changelog_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()  // out of xtask/
        .join("CHANGELOG.md");
    let text = std::fs::read_to_string(&changelog_path)
        .with_context(|| format!("reading {}", changelog_path.display()))?;
    let releases = parse_changelog(&text)?;
    println!("{}", render_atom(&releases));
    Ok(())
}
```

Existing crates that simplify the work:

- `quick-xml` for XML emission (already in the workspace? — check
  `Cargo.lock`; if not, add to xtask only).
- `regex` for the heading-pattern parse.
- Or pure-string-handling — the parse is simple enough that no XML lib
  is strictly needed; a `format!` template + `htmlescape` for `<` `>`
  `&` in the content body is sufficient.

### docs-bundle pipeline integration

The site repo's docs-bundle sync step (per
`reference_alint-org-docs-pipeline.md`) runs `cargo xtask sync-docs`
or similar to copy `docs/site/` into the docs-bundle branch. Add a
sibling step:

```bash
cargo xtask build-releases-atom > docs-bundle/releases.atom
```

Site repo's Astro build then sees `releases.atom` in `public/` and
serves it verbatim.

## Feed-validation checklist

After generation, validate the feed with at least two of:

1. **W3C Feed Validator** — https://validator.w3.org/feed/ — the
   canonical Atom validator; flags spec violations.
2. **Atom-tools `atom-test`** — CLI validator, useful in CI.
3. **Manual subscribe in Feedly/Inoreader** — if a real reader
   accepts the feed, that's a strong end-to-end signal.

Failure modes to specifically check:

- **Date format.** Atom requires RFC 3339 (`2026-05-06T00:00:00Z`);
  if the script emits `2026-05-06` raw, validators reject.
- **Unique `<id>` per entry.** The GitHub release URL is unique by
  construction; verify the script doesn't accidentally double-emit
  an entry.
- **`<updated>` MUST be present** on both feed-level and entry-
  level. Feed-level `<updated>` should be the date of the most
  recent release.
- **HTML in `<content type="html">` MUST be escaped** (`<` →
  `&lt;`). The sketch above shows this; the implementation
  shouldn't forget.

## Site-side discoverability

Add a `<link>` element to alint.org's site `<head>` so feed-readers
auto-discover the URL:

```html
<link rel="alternate" type="application/atom+xml"
      title="alint releases" href="/releases.atom" />
```

In Astro/Starlight terms, this lives in the site's
`src/components/Head.astro` or in the Starlight config under
`head:` (depending on layout choice).

Also add a visible "Subscribe to releases" link in the site footer or
on `/docs/` landing — pointing at `/releases.atom` directly.

## Open questions

1. **Feed length / pagination.** Atom doesn't impose a length limit,
   but practice is to ship the last 20-50 releases in the main feed.
   alint has ~30+ releases now (v0.4.0 through v0.9.16). **Recommend:
   ship all releases initially** (file is still <100KB); add
   pagination via Atom-paging extensions if/when the file ever
   crosses 200KB.
2. **GitHub Release vs CHANGELOG.md as source of truth.** GitHub
   Releases auto-generate from tags but the description is the
   release notes (which we copy from CHANGELOG anyway). Choosing
   CHANGELOG.md keeps everything in one place and removes the GitHub
   API dependency from the build. Already locked in.
3. **Per-release `<author>`.** All releases are by `asamarts`; the
   feed-level `<author>` covers it; per-entry `<author>` is omitted.
   If we ever have multiple maintainers, revisit.
4. **Categories / tags.** Atom supports `<category>` per entry. We
   could tag releases as `bugfix`, `feature`, `breaking` based on
   CHANGELOG section headers. **Recommend defer** — no consumer use
   case yet.
5. **Time-of-day precision.** CHANGELOG only has date precision;
   the feed sets all release times to `T00:00:00Z`. Some readers
   sort by `<published>` time and may show all releases on the
   same day in arbitrary order. **Mitigation:** if it ever
   becomes annoying, add `T<HH:MM:SS>Z` derived from the git
   commit time of the version tag.
6. **GitHub Release URL existence.** Each release MUST have a
   corresponding GitHub Release page (not just a tag) for the link
   to resolve. v0.4.0 through v0.9.14 are tagged; verify GitHub
   Releases exist for each. **If not**, the build script should
   either (a) fall back to the tag URL (`/tree/v{X.Y.Z}`) or
   (b) emit a warning. Recommend (a).

## Pre-publish checklist

- [ ] `xtask/src/bin/build-releases-atom.rs` exists and runs
      cleanly: `cargo xtask build-releases-atom > /tmp/test.atom`
      produces well-formed XML.
- [ ] W3C Feed Validator passes (no errors; warnings OK if benign).
- [ ] Manual subscribe in Feedly succeeds + entries render correctly.
- [ ] All entry `<link href>` URLs resolve (no 404s) — automated
      check via curl/lychee in the build pipeline.
- [ ] docs-bundle sync step generates `releases.atom` and ships it
      to `public/releases.atom` on the site repo.
- [ ] Site's `<head>` includes the `<link rel="alternate">` for
      auto-discovery.
- [ ] Footer or docs-landing link to `/releases.atom` is visible.
- [ ] STATE.md row for `alint.org/releases.atom` flipped from
      `missing` to `live` with date + commit SHA.

## Estimated diff size

In the alint repo:
- 1 new file at `xtask/src/bin/build-releases-atom.rs`: ~120 lines.
- `xtask/Cargo.toml` additions (regex, htmlescape if not present):
  ~3 lines.
- `RELEASING.md` update — note that `cargo xtask
  build-releases-atom` is part of the release-process checklist:
  ~5 lines.

In the site repo:
- 1 build-step entry to invoke the xtask + place the output in
  `public/`: ~5 lines.
- 1 `<link rel="alternate">` in the head template: ~2 lines.
- 1 visible "Subscribe to releases" link in the footer or
  docs-landing: ~2 lines.

Total: ~120 lines (alint repo) + ~10 lines (site repo).

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `api-endpoints.md` | Both are build-time-generated artifacts derived from in-repo data (CHANGELOG, rule registry, ruleset YAML). Likely share the same xtask binary scaffolding pattern; ship coordinated. |
| `meta-descriptions.md` (P3.2) | The `<head>` `<link rel="alternate">` insertion is the same site-repo touchpoint. Bundle into one site PR. |
| `branding/` (P4) | If the feed includes `<icon>` and `<logo>`, those URLs depend on alint having logo assets. v0.10 ship: omit `<logo>`; keep `<icon>` pointing at favicon. |
