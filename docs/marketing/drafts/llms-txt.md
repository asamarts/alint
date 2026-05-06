---
destination: alint.org/llms.txt (site-repo build artifact at the public root)
status: drafting
blocks_on: stable URL set on alint.org for the linked sections (cookbook, rules, bundled-rulesets, examples, compare, migrating-from); coordinated publish with `llms-full-txt.md`
last_touched: 2026-05-06
---

# alint.org/llms.txt — content brief for the site repo

## Why

[llmstxt.org](https://llmstxt.org/) proposes a single discoverable file
at the root of a site that lets LLMs (and the agents that use them)
quickly map the site's canonical content without crawling every URL.
The format is dead simple: a single H1 with the project name, a one-
paragraph summary, then H2 sections each followed by a bullet list of
`[Title](URL)` links to canonical content.

For alint specifically, llms.txt is high-leverage because:

1. **Our audience overlaps heavily with LLM users.** Maintainers and
   platform engineers reach for AI assistants when bootstrapping a
   linter config. A well-formed llms.txt means an LLM that's asked
   "set up alint for my repo" can find the cookbook + rule catalogue
   + bundled-ruleset list without reading the whole site.
2. **It's the cheapest possible AI-discovery investment.** Static
   markdown file, ~50 lines, zero runtime cost.
3. **It complements our opt-in AI-training posture (decided
   2026-05-06).** llms.txt is about *runtime* discovery (the LLM
   reads it when answering a question); ai.txt is about *training-
   time* opt-in. Both belong in the P3.3 set.

This brief produces a **complete llms.txt body ready to drop on the
site at `/llms.txt`** — no Astro routing or rendering layer needed,
just a static file at the public root.

## Proposed `/llms.txt` body

```markdown
# alint

> Fast, language-agnostic linter for repository structure, files, and
> content. Declare the shape your repo should have — required files,
> filename conventions, content patterns, values inside `package.json`
> / `Cargo.toml` / GitHub workflows, cross-file relationships — in a
> single `.alint.yml`. alint enforces it. One static Rust binary,
> 60 rule kinds across 13 families, 19 bundled ecosystem rulesets,
> 12 auto-fix ops, 8 output formats including a dedicated `agent`
> format with per-violation `agent_instruction` strings. Fills the
> active-maintenance gap left when Repolinter was archived in early
> 2026.

## Quickstart

- [Installation](https://alint.org/docs/getting-started/installation/): Homebrew, npm, curl, Docker, GitHub Action — pick your channel
- [Quickstart](https://alint.org/docs/getting-started/quickstart/): from `cargo install` to a passing `.alint.yml` in 60 seconds
- [Cookbook](https://alint.org/docs/cookbook/): copy-pasteable patterns for the 20+ most common repo-maintenance problems
- [Configuration reference](https://alint.org/docs/configuration/): the full `.alint.yml` surface — version, extends, facts, rules, output, ignore
- [CLI reference](https://alint.org/docs/cli/): `alint check`, `alint fix`, `alint validate-config`, all flags and exit codes

## Rule catalogue

- [All rules by family](https://alint.org/docs/rules/): 60 rule kinds across 13 families (Existence, Content, Naming, Text hygiene, Structured query, Security, Encoding, Structure, Portable, Unix, Git hygiene, Cross-file, Plugin)
- [Existence family](https://alint.org/docs/rules/existence/): file_exists, file_absent, dir_exists, dir_absent, dir_contains, dir_only_contains
- [Content family](https://alint.org/docs/rules/content/): file_content_matches, file_content_forbidden, file_starts_with, file_ends_with, file_header, file_footer, file_hash, file_max_size, file_min_size, file_max_lines, file_min_lines
- [Naming family](https://alint.org/docs/rules/naming/): filename_case, filename_regex, no_illegal_windows_names
- [Text hygiene family](https://alint.org/docs/rules/text-hygiene/): final_newline, trailing_whitespace, line_endings, no_tabs, no_bom, no_bidi, max_line_length
- [Structured-query family](https://alint.org/docs/rules/structured-query/): structured_path_exists, structured_path_matches, structured_path_equals, structured_path_in (RFC 9535 JSONPath over JSON/YAML/TOML)
- [Security family](https://alint.org/docs/rules/security/): commented_out_code, file_is_ascii, file_is_text
- [Encoding family](https://alint.org/docs/rules/encoding/): file_is_text, file_is_ascii, file_shebang, executable_has_shebang
- [Structure family](https://alint.org/docs/rules/structure/): file_in_dir, file_not_in_dir, dir_only_contains
- [Portable family](https://alint.org/docs/rules/portable/): no_illegal_windows_names, case-collision detection
- [Unix family](https://alint.org/docs/rules/unix/): executable_bit, executable_has_shebang, file_shebang
- [Git hygiene family](https://alint.org/docs/rules/git-hygiene/): commented_out_code, no_merge_conflict_markers
- [Cross-file family](https://alint.org/docs/rules/cross-file/): pair, for_each_dir, for_each_file, every_matching_has, unique_by
- [Plugin family](https://alint.org/docs/rules/plugin/): command (shell out to external CLIs)

## Bundled rulesets

- [All bundled rulesets](https://alint.org/docs/bundled-rulesets/): 19 rulesets shipped with the binary, addressed via `alint://bundled/<name>@v1`
- [oss-baseline@v1](https://alint.org/docs/bundled-rulesets/oss-baseline/): LICENSE, README, CONTRIBUTING, CODE_OF_CONDUCT, SECURITY — the OSS-hygiene baseline (Repolinter superset)
- [rust@v1](https://alint.org/docs/bundled-rulesets/rust/): Cargo.toml shape, Cargo.lock, snake_case modules, target/ ban, MSRV pinning
- [node@v1](https://alint.org/docs/bundled-rulesets/node/): package.json fields, lockfile presence, node_modules ban, scripts shape
- [python@v1](https://alint.org/docs/bundled-rulesets/python/): pyproject.toml shape, __init__.py, virtualenv ban, .pyc ban
- [go@v1](https://alint.org/docs/bundled-rulesets/go/): go.mod, go.sum, vendor/ posture, package layout
- [java@v1](https://alint.org/docs/bundled-rulesets/java/): pom.xml / build.gradle, target/build dir bans, Maven layout
- [agent-hygiene@v1](https://alint.org/docs/bundled-rulesets/agent-hygiene/): AGENTS.md, .cursorrules, .clauderules, .windsurfrules — agent-touched repo conventions
- [agent-context@v1](https://alint.org/docs/bundled-rulesets/agent-context/): agent context-pack hygiene (CLAUDE.md, .cursor/rules/, AI instruction files)
- [monorepo@v1](https://alint.org/docs/bundled-rulesets/monorepo/): per-package README, top-level workspace manifest, lockfile centralisation
- [monorepo/cargo-workspace@v1](https://alint.org/docs/bundled-rulesets/monorepo-cargo-workspace/): Cargo workspace member registration, MSRV consistency
- [monorepo/pnpm-workspace@v1](https://alint.org/docs/bundled-rulesets/monorepo-pnpm-workspace/): pnpm-workspace.yaml shape
- [monorepo/yarn-workspace@v1](https://alint.org/docs/bundled-rulesets/monorepo-yarn-workspace/): Yarn workspaces field shape
- [ci/github-actions@v1](https://alint.org/docs/bundled-rulesets/ci-github-actions/): workflow file shape, action-version pinning, secret-name conventions
- [hygiene/lockfiles@v1](https://alint.org/docs/bundled-rulesets/hygiene-lockfiles/): no nested lockfiles, root-only enforcement
- [hygiene/no-tracked-artifacts@v1](https://alint.org/docs/bundled-rulesets/hygiene-no-tracked-artifacts/): node_modules, target/, .DS_Store, dist/, build/ — common artifact bans
- [tooling/editorconfig@v1](https://alint.org/docs/bundled-rulesets/tooling-editorconfig/): consume `.editorconfig`, enforce in CI as a backstop
- [docs/adr@v1](https://alint.org/docs/bundled-rulesets/docs-adr/): docs/adr/NNNN-*.md naming and structure
- [compliance/apache-2@v1](https://alint.org/docs/bundled-rulesets/compliance-apache-2/): Apache-2.0 license headers, NOTICE file shape
- [compliance/reuse@v1](https://alint.org/docs/bundled-rulesets/compliance-reuse/): REUSE.toml presence and SPDX header coverage

## Case studies

- [All case studies](https://alint.org/examples/): 25 production OSS repos with working `.alint.yml` configs
- [kubernetes/kubernetes](https://alint.org/examples/kubernetes-kubernetes/): consolidated 17 of 50 `hack/verify-*.sh` scripts into declarative rules
- [apache/airflow](https://alint.org/examples/apache-airflow/): ~40% of 109 pre-commit hooks expressed as alint rules
- [python/cpython](https://alint.org/examples/python-cpython/): 12 validation surfaces consolidated into one config
- [apache/arrow](https://alint.org/examples/apache-arrow/): 6-language polyglot monorepo with 21 lint hooks across 14 tool repos
- [pytorch/pytorch](https://alint.org/examples/pytorch-pytorch/): `lintrunner` orchestration with alint as the structural floor (~86% of 57 adapters)
- [microsoft/typescript](https://alint.org/examples/microsoft-typescript/): TypeScript-native lint discipline + alint structural overlay
- [microsoft/vscode](https://alint.org/examples/microsoft-vscode/): apples-to-apples vs `build/hygiene.ts` (~75% coverage in one declarative config)
- [NixOS/nixpkgs](https://alint.org/examples/nixos-nixpkgs/): 39,101 files / 20,678 by-name package directories validated in 273 ms
- [tokio-rs/tokio](https://alint.org/examples/tokio-rs-tokio/): zero hand-rolled scripts; alint catches 15 implicit conventions
- [astral-sh/uv](https://alint.org/examples/astral-sh-uv/): 67-crate workspace conventions previously enforced nowhere

## Compare

- [alint vs other repo-level linters](https://alint.org/compare/): TL;DR routing table + 17-row feature matrix + per-tool deep dives + "use them together" patterns

## Migrating from

- [Migrating from Repolinter](https://alint.org/migrating-from/repolinter/): 24-entry mapping (17 full / 14 partial / 3 no-clean), starts with a 5-line `extends:` config
- [Migrating from ls-lint](https://alint.org/migrating-from/ls-lint/): 16 ls-lint primitives mapped (9 full / 6 partial / 1 none); when to stay on ls-lint
- [Migrating from custom bash scripts](https://alint.org/migrating-from/custom-bash-scripts/): 18 bash patterns catalogued, anchored by kubernetes 50-to-17 case study
```

## Implementation notes (for the site repo)

- **File location.** llms.txt MUST live at the public root, exactly
  `https://alint.org/llms.txt` — not under `/docs/` or anywhere else.
  Per the llmstxt.org spec, that's the only discoverable URL.
- **Build pipeline.** Astro/Starlight serves anything in
  `public/` verbatim at the site root. Drop the body above into
  `public/llms.txt` (no `.md` extension on the deployed file — the
  spec calls for `.txt` because clients fetch it as plain text and
  parse the markdown themselves).
- **Source of truth.** This brief contains the full body. Whoever
  applies it copies the markdown block above into `public/llms.txt`
  verbatim.
- **No Starlight route.** Don't create a sidebar entry or a
  `src/content/docs/llms.md` — the file is meant to be invisible to
  human navigation, discovered only by LLMs/agents that know to fetch
  it.
- **Content-Type.** Cloudflare Pages auto-serves `.txt` as
  `text/plain; charset=utf-8`. No header config needed.
- **Validation.** A draft validator exists at
  https://llmstxt.org/llms-validator/ — paste the body in to verify
  H1 + summary + H2-section structure parses cleanly.

## Open questions

1. **URL stability.** Several listed URLs (`/docs/rules/<family>/`,
   the per-bundled-ruleset URLs) need to resolve at publish time.
   Confirm each subroute exists in the docs-bundle build before
   shipping — the per-family rule pages exist (verified in STATE.md
   inventory), but the per-bundled-ruleset URLs may need a
   docs-bundle pipeline tweak to surface (currently they live under
   a single flat list at `/docs/bundled-rulesets/`).
2. **Examples gallery dependency.** `/examples/<owner>-<repo>/` URLs
   depend on `alint-org-examples-gallery.md` shipping. If the
   gallery is delayed, fall back to the GitHub-tree URL for each
   case study (`https://github.com/asamarts/alint/tree/main/examples/<owner>-<repo>`).
3. **Compare and migrating-from URLs.** Same dependency on the P3.1
   drafts publishing. If `/compare/` and `/migrating-from/*/` aren't
   live yet, hold this draft in `drafting` rather than shipping
   broken links.
4. **Per-family rule URLs vs. flat list.** The current
   `/docs/rules/` page groups by family but doesn't have per-family
   index pages. The links above assume per-family routes exist
   (e.g. `/docs/rules/existence/`). If those don't exist, collapse
   the "Rule catalogue" section to a single link
   `[All rules](https://alint.org/docs/rules/)`.

## Pre-publish checklist

- [ ] All listed URLs resolve (no 404s) — automated check via
      `lychee` or similar link checker.
- [ ] `public/llms.txt` exists at the site root and serves as
      `text/plain`.
- [ ] llmstxt.org validator parses the body cleanly.
- [ ] Body fits in one screen of an LLM's context window
      (~6KB target — current draft is well under).
- [ ] `llms-full-txt.md` companion is in `ready` state for
      coordinated publish (the two files reference each other
      conceptually; ship together).
- [ ] STATE.md row for `alint.org/llms.txt` flipped from `missing`
      to `live` with date + commit SHA.

## Estimated diff size on the site repo

- 1 new file at `public/llms.txt`: ~75 lines.
- No code changes, no config changes.

Total: ~75 lines (one file).

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `llms-full-txt.md` | Companion file; ship together so an LLM that fetches the small index can also fetch the inlined-content version. |
| `alint-org-hero.md` | The summary paragraph mirrors the hero block messaging — keep them in sync. |
| `alint-org-compare.md`, `alint-org-examples-gallery.md`, `migrate-from-*.md` | All linked from llms.txt; their URLs must resolve. |
| `well-known-ai-txt.md` | Conceptually paired (runtime discovery vs training-time opt-in). Different files, same posture. |
