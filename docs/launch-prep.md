# alint Production Launch — End-to-End Plan

Living doc. Captures the path from the current state (v0.9.14 — feature-complete
v0.9 series, fully-automated bench-record CI, no public outreach yet) to a
public launch backed by real-repo case studies and a marketing site that earns
attention rather than just hosts documentation.

**Status: 2026-05-06.** P1 done; P2a pilot done (5 of 20 repos +
`docs/development/CONFIG-AUTHORING.md` + `coverage_audit_examples_parse.rs`);
v0.9.15 P1+P2 done (findings doc + examples-parse audit). Next: scale P2a to
the full 20 BEFORE v0.9.15 Phase 3-6, so the DX hardening fixes are informed
by the full pitfall set rather than just the pilot's 12.

## State of the world (audit at 2026-05-05)

| Surface | Current | Gap |
|---|---|---|
| **README.md** | Comprehensive, but reads like a spec, not marketing. References v0.9.6 (we're at v0.9.14). Agentic angle buried in long lists. | Hero rewrite around speed/agentic/extensible; refresh version refs; add 60-second quickstart near the top. |
| **alint.org** | Has structure (60 rule kinds, 19 rulesets, etc.) + OG image + Twitter cards. Same not-punchy framing as the README. | Hero rewrite, comparison page, examples gallery, public bench page, migration guides, SEO infra, AI/LLM discovery files. |
| **GitHub repo About** | `description: null`, `homepage: null`, `topics: []` | Set all three; topics target the discovery-search vocabulary. |
| **Discussions** | Disabled | Enable; this is the intended low-friction support channel for a launch. |
| **Issue / PR templates** | None | Add bug-report, feature-request, config-help templates + PR template. |
| **`CONTRIBUTING` / `CODE_OF_CONDUCT` / `SECURITY`** | All missing | Tablestakes for a public OSS launch. `SECURITY.md` is especially load-bearing for a build-tool — vulnerability disclosure path. |
| **`examples/` directory** | None | Becomes the home of P2 case studies — real configs from real repos. |
| **Comparison page** | None on README/alint.org | alint vs Repolinter, ls-lint, Megalinter, EditorConfig, custom-shell — direct table. |

---

## Phased plan

```
P1   Repo hygiene             ──┐ (DONE)
P1.5 v0.9.15 config DX        ──┼──► P3 Marketing refresh ──► P4 Launch ──► P5 Post-launch
     hardening (6 phases)       │              ▲                              │
P2a  Validation pass          ──┤              │                              │
     (20 repos, diverse)        │ (P2a findings inform P1.5 phase 3+5;        │
P2b  Validation pass          ──┘  P2 case studies become P3 evidence)        │
     (20 polyglot monorepos,                                                  │
     ongoing post-launch)        ───────────────────────────────────────────► │

Sequencing nuance: P2a-full (15 remaining repos) runs BEFORE
v0.9.15 Phase 3-6 (did-you-mean errors + JSON Schema +
validate-config subcommand) so the DX fixes target the full
pitfall catalogue rather than just the pilot's 12.
```

### P1 — Repo hygiene & community foundation (~1.5 days, DONE 2026-05-05)

Foundational; happens first because launch traffic is unpredictable and these need
to be live before the first link is shared.

- ✅ `CONTRIBUTING.md`
- ✅ `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1)
- ✅ `SECURITY.md` (PGP/contact + 90-day disclosure window)
- ✅ `.github/ISSUE_TEMPLATE/{bug-report,feature-request,config-help}.yml`
- ✅ `.github/pull_request_template.md`
- ✅ GitHub repo About: description, homepage, 10 topics
- ✅ Discussions enabled
- ✅ README hero rewrite + version refs refreshed to v0.9.14
- ✅ `examples/` directory scaffold

### P1.5 — v0.9.15 config DX hardening (~3-4 days, IN PROGRESS)

Surfaced by the P2a pilot — 12 schema/language pitfalls hit while writing
configs for the first 5 repos. Two layers of prevention:

1. **Editor / write-time** — JSON Schema for `.alint.yml` (Phase 5); ~80 % of
   pitfalls caught before save.
2. **Parser / load-time** — clearer errors with did-you-mean suggestions
   (Phases 3-4) for the residual 20 %.

Six sub-phases:

- **Phase 1** — `docs/development/CONFIG-AUTHORING.md` findings doc. ✅ DONE.
- **Phase 2** — `coverage_audit_examples_parse.rs` audit (every
  `examples/*/.alint.yml` MUST load + build cleanly). ✅ DONE.
  *(Already caught one bug on its first run — duplicate `level:` in airflow.)*
- **Phase 3** — Did-you-mean parse errors via custom serde Deserialize on
  rule Options structs. Levenshtein-suggested field renames; hand-curated
  high-drift overrides (`argv→command`, `secondary→partner`, `style→target`,
  `pattern→prefix`). ~1 day.
- **Phase 4** — Domain-specific error messages: `scope_filter.has_ancestor`
  basename constraint, `when:` operator-keyword guidance, JSONPath
  bracket-notation for dashed keys. ~half day.
- **Phase 5** — JSON Schema generation (`schemars` derive on every rule's
  `Options` + discriminated-union top-level → `schemas/v1/config.json`).
  Editor LSP autocomplete catches ~80 % of pitfalls at keystroke time.
  Biggest single payoff. ~2-3 days.
- **Phase 6** — `alint validate-config <path>` subcommand (parse-only, no
  tree walk). For editor LSP, pre-commit hooks, fail-fast CI. ~half day.

**Sequencing decision:** Phases 3-6 land AFTER P2a-full (the remaining 15
case studies). Reasons:
- The new examples-parse audit dropped iteration cost per case study; doing
  15 more is cheap.
- More repos surface more pitfalls — Phases 3-4 hand-curated suggestions
  benefit from the full set.
- Phase 5 JSON Schema work targets the right fields when the most-misused
  ones are known.

### P2a — First 20 repos, single-language + diverse-ecosystem (~10-15 days)

Diverse ecosystems + scales + tooling shapes. Becomes the case-study foundation
for P3 and the gap-catalogue for v0.10+.

**Pilot status (5 of 20 done, committed):** kubernetes, rust-lang/rust, deno,
airflow, turbo. Each has a per-repo case study + working `.alint.yml` at
`examples/<owner>-<repo>/`. The pilot iteration surfaced the 12 pitfalls now
documented in `docs/development/CONFIG-AUTHORING.md`.

**P2a-full Wave 1-3 (15 remaining):** 3 batches of 5 parallel subagents each.
Each subagent briefing includes the parse-validate requirement (Step 5
below) + a pointer to `docs/development/CONFIG-AUTHORING.md` so the
canonical-correct YAML is one click away.

| # | Repo | Ecosystem | Why |
|---|---|---|---|
| 1 | `rust-lang/rust` | Rust mega-monorepo | Has `src/tools/tidy` — a custom Rust binary doing exactly alint's job |
| 2 | `tokio-rs/tokio` | Rust workspace | Clean, well-curated, baseline case |
| 3 | `astral-sh/uv` | Rust + pyo3 | Modern multi-language Rust monorepo |
| 4 | `astral-sh/ruff` | Rust linter for Python | Direct comparable as a tool; we can dogfood-cross |
| 5 | `clap-rs/clap` | Rust workspace | Small, focused — quick win + baseline |
| 6 | `denoland/deno` | Rust + JS + TS | Multi-language, custom validation scripts |
| 7 | `microsoft/typescript` | TS mega-repo | Hand-rolled validation, lint-baseline files |
| 8 | `vercel/next.js` | TS monorepo | Highly conventional, pnpm-workspace |
| 9 | `pnpm/pnpm` | TS monorepo | pnpm itself; defines the workspace shape |
| 10 | `facebook/react` | JS/TS multi-package | Yarn-workspace, conventions per package |
| 11 | `prettier/prettier` | JS, well-curated | Mature, opinionated structure |
| 12 | `python/cpython` | Python + C | Make-driven, custom check scripts in `Tools/` |
| 13 | `apache/airflow` | Python plugin-heavy | Provider-package conventions, lots of structural rules |
| 14 | `kubernetes/kubernetes` | Go mega-monorepo | `hack/verify-*.sh` is *literally* this tool's use case |
| 15 | `golang/go` | Go canonical | Tightly-curated, minimal external tooling — ground-truth case |
| 16 | `helm/helm` | Go modular | Smaller Go monorepo, modular structure |
| 17 | `apache/arrow` | Multi-language (C++/Java/Python/Rust/Go) | Per-language subdir conventions; cross-language structural rules |
| 18 | `pytorch/pytorch` | C++/Python/CUDA | Massive multi-language; complex conventions |
| 19 | `vercel/turbo` | Rust monorepo orchestrator | Modern Rust + custom validation |
| 20 | `nodejs/node` | C++/JS, mature | Long-curated, deeply-conventional |

### P2b — 20 polyglot monorepos (~10-15 days, can run concurrent with P3 or post-launch)

Multi-language monorepos stress alint differently than single-language ones —
per-subtree conventions, polyglot bundle composition, scope-filter on
heterogeneous trees. These are the canonical use cases that motivate
`extends + nested_configs + scope_filter`, so they're the strongest stories
for the launch.

| # | Repo | Languages | Why |
|---|---|---|---|
| 21 | `bazelbuild/bazel` | Java + Go + C++ + Python | Canonical multi-language build system |
| 22 | `microsoft/vscode` | TS + C++ (native) + Python | One of the most-watched OSS repos; massive multi-language tree |
| 23 | `angular/angular` | TS monorepo + Bazel build | Tightly conventional, per-package rules, `ng-packagr` ceremony |
| 24 | `nrwl/nx` | TS monorepo | Nx ITSELF is a monorepo tool — interesting cross-comparison |
| 25 | `electron/electron` | C++ + JS + Python | Native + web hybrid; per-platform conventions |
| 26 | `tensorflow/tensorflow` | C++ + Python + Java + JS (TFJS) | ML mega-monorepo; ~80k files; perf stress test |
| 27 | `apache/spark` | Scala + Java + Python + R | Polyglot data engine; per-language module conventions |
| 28 | `apache/beam` | Java + Python + Go (+ TypeScript SDK) | *Explicitly* polyglot; cross-language SDK conventions |
| 29 | `prisma/prisma` | Rust (query engine) + TS (client) | Modern hybrid; rust-engine + ts-client subdirs |
| 30 | `temporalio/temporal` | Go core + per-SDK languages | Workflow orchestration; many sub-projects |
| 31 | `istio/istio` | Go + many control plane components | Service mesh; multiple Go modules in one tree |
| 32 | `grafana/grafana` | Go backend + TS frontend | Widely-deployed; clean backend/frontend split |
| 33 | `cockroachdb/cockroach` | Go (DB) + TS (UI) + C++ (libs) | Distributed DB; multi-tier monorepo |
| 34 | `directus/directus` | TS monorepo (API + admin app + extensions) | Headless CMS; pnpm-workspace conventions |
| 35 | `supabase/supabase` | TS + Go + Rust + Python | Modern Firebase alt; many engines in one repo |
| 36 | `NixOS/nixpkgs` | Nix + Python build scripts | **~150k+ files** — the largest non-trivial repo on this list; the scale stress-test candidate |
| 37 | `hashicorp/terraform` | Go + HCL + JS UI | Infrastructure-as-code; per-provider conventions |
| 38 | `flutter/flutter` | Dart + Java + Kotlin + Swift + C++ | Cross-platform UI framework; per-platform native dirs |
| 39 | `dotnet/runtime` | C# + C++ + native (per-arch) | Microsoft's CLR runtime; multi-arch + multi-language |
| 40 | `protocolbuffers/protobuf` | C++/Java/Python/Ruby/Go/JS/Obj-C/C#/PHP | Generated bindings for ~10 languages; per-language subdir conventions |

**Why P2b matters for launch positioning:** the "language-agnostic linter for
repository structure" pitch lands harder when paired with case studies showing
alint configs that work cleanly across `protobuf`'s 10 languages or `bazel`'s 4.
Single-language wins (P2a) prove correctness; polyglot wins (P2b) prove the
unique value prop.

### Per-repo workflow (2-4 hr per repo)

1. Shallow clone (depth=1)
2. **Inventory existing structural-check tooling** — grep for `hack/verify-*`,
   `scripts/lint-*`, `Makefile` lint targets, `.eslintrc` rules that aren't AST
   checks, `.editorconfig`, `.gitattributes`, custom shell pipelines in CI yml
3. **Categorise each check** — what shape of rule (filename / content /
   cross-file / structure)
4. **Build matching alint config** — start from the bundled rulesets that fit
   (`rust@v1`, `node@v1`, etc.), add per-repo custom rules
5. **Parse-validate the config** — `./target/release/alint check --config
   examples/<owner>-<repo>/.alint.yml examples/<owner>-<repo>/` MUST exit
   without a `building rule "..."` / `loading config` / `invalid options`
   error. Tool-not-on-PATH errors from `command:` rules ARE expected and
   indicate the rule structure is correct. **The kubernetes pilot iteration
   surfaced 8 schema-level bugs that wouldn't have shown up without this
   step.** Subagents writing configs against memory of the schema (vs.
   reading `crates/alint-rules/src/<kind>.rs::struct Options`) are the
   highest-failure-rate work — bake this validation in.
6. **Run + compare** — alint output vs the existing tool's output. Note: false
   positives, false negatives, perf delta
7. **Gap catalogue** — for each existing check alint can't express, write a
   one-line "needs rule kind X" note feeding the v0.10+ design
8. **Per-repo case study** — one markdown page in
   `examples/<owner>-<repo>/README.md` with the inventory + the alint config +
   the comparison

For P2b polyglot repos: one extra step — explicitly catalogue which conventions
are *cross-language* (e.g., "every language subdir has a README" →
`for_each_dir` rule) vs *per-language* (e.g., "Python files need a license
header" → `scope_filter: { has_ancestor: setup.py }`).

**Approach to scale:** Start with 5 representative repos to validate the
methodology, iterate the per-repo template based on what we learn, then dispatch
the remaining 15 in batches of 3-5 (possibly with subagents for the inventory
phase).

### P3 — Marketing refresh (~5-6 days; depends on P2a)

#### P3.1 Hero + content

Three concrete value props, evidence-backed from P2:

1. "Sub-second on 100K-file repos" (cite v0.9.13 100k bench: S3 1.13s)
2. "Agentic-aware: structured `agent` output format + `agent-hygiene` ruleset for AI-touched repos"
3. "60 rule kinds + 19 bundled ecosystem rulesets — zero plugins to install"

Pages to add:
- `alint.org/compare` — direct table: alint vs Repolinter (archived), ls-lint, Megalinter, custom shell scripts
- `alint.org/examples` — gallery of P2 case studies. "alint in production at: rust-lang/rust, kubernetes, deno, …"
- `alint.org/benchmarks` — public-facing version of HISTORY.md
- `alint.org/migrating-from/{repolinter,ls-lint,custom-bash-scripts}` — step-by-step

README hero — match alint.org messaging. 5-line punch + quickstart.
CLI demo — asciinema or animated GIF embedded in README + alint.org hero.

#### P3.2 SEO (~1.5 days)

| Item | Why |
|---|---|
| `sitemap.xml` (auto-generated) | Search Console + crawler discoverability |
| `robots.txt` with explicit AI crawler rules | Allow good crawlers; explicit posture |
| Canonical `<link rel="canonical">` per page | Avoids duplicate-content penalties |
| Per-page `<title>` + `<meta description>` | Each page ranks for its own keywords |
| H1/H2/H3 hierarchy audit (one H1 per page) | SEO + accessibility |
| Image alt text audit | A11y + image search |
| Schema.org JSON-LD: `SoftwareApplication`, `Article`, `BreadcrumbList` | Rich-result eligibility |
| Lighthouse pass + fix any < 90 | Page-quality signal |
| Internal linking (rule pages ↔ ruleset pages) | Topic-cluster authority |
| Submit `sitemap.xml` to Google Search Console + Bing | Faster indexing |
| Keyword-targeted landing pages: `/repolinter-alternative`, `/monorepo-linter`, `/agent-friendly-linter`, `/language-agnostic-linter`, `/repository-structure-linter` | High-intent + low-competition |

**Keyword strategy:**
- "repolinter alternative" / "repolinter replacement" — high intent (Repolinter archived 2026-02; users actively shopping)
- "monorepo linter" / "monorepo conventions enforcement"
- "language-agnostic linter" / "polyglot linter"
- "agent-friendly linter" / "AI-aware repository linter"
- "repository structure linter" / "filesystem linter"
- "ls-lint alternative"

#### P3.3 AI/LLM discovery (~1 day)

A coordinated story for every major way an LLM/agent finds documentation:

| File / endpoint | Purpose |
|---|---|
| **`/llms.txt`** | The [llmstxt.org](https://llmstxt.org/) standard. Single markdown file with H1 title + summary + H2 sections of bullet-list links to canonical content. LLMs ingest in one fetch instead of crawling. |
| **`/llms-full.txt`** | Companion: same content but with all linked docs inlined into one large markdown blob. For LLMs without browse-tool access. |
| **`/.well-known/security.txt`** (RFC 9116) | Standard vulnerability disclosure path. Important for a build-tool with supply-chain implications. |
| **`/.well-known/ai.txt`** | Spawning AI's emerging standard for opting in/out of AI training data. We *want* opt-in. |
| **`robots.txt` AI crawler rules** | Explicit allow/disallow for `GPTBot`, `ClaudeBot`, `anthropic-ai`, `CCBot`, `Google-Extended`, `PerplexityBot`, `Applebot-Extended`, `meta-externalagent`. We allow all. |
| **JSON-LD `SoftwareApplication`** | Schema.org structured data: name, version, license, install URL, supported OS. Both human SEO + agent ingestion. |
| **RSS/Atom feed for releases** (`/releases.atom`) | Both humans and agent monitoring poll feeds. |
| **Stable JSON endpoints**: `/api/rules.json`, `/api/rulesets.json`, `/api/versions.json` | Programmatic catalogue discovery. |

**Stretch (P5/post-launch): alint as an MCP server.** [Model Context
Protocol](https://modelcontextprotocol.io/) lets agents query tools directly.
An `alint` MCP server could expose `get_rule_doc(rule_kind)`,
`validate_config(yaml)`, `suggest_rules_for(repo_path)` — agent-native
integration. ~3-5 days of work.

### P4 — Launch (~2-3 days)

- **GitHub release for v0.9.14** (or hold for a v1.0 cut) — proper release notes, screenshots, migration guidance
- **Press kit** — `branding/` directory with logo SVGs, screenshots, GIFs, OG images
- **Pre-launch beta** — invite 5-10 people from the P2 case-study repos as beta testers (~1 week pre-public)
- **Launch posts drafted** — HN ("Show HN: alint, a fast linter for repo structure"), r/rust, Lobsters, dev.to. Each tailored to audience.
- **Launch day**: post → monitor Discussions/issues → respond fast for 24-48 hours
- **Day-after**: blog post or design-doc-style writeup of the v0.9.6→.10 silent-no-op bug class story (great content marketing)

### P5 — Post-launch (~ongoing)

- Privacy-respecting analytics on alint.org (Plausible)
- GitHub Sponsors button / `funding.yml`
- Star CTA banner in README (post-1k-stars)
- Newsletter / RSS for releases (RSS already in P3.3)
- Optional: `alint init` command that detects existing tooling and proposes a starter config
- **MCP server** (per P3.3 stretch)
- P2b case studies as ongoing content marketing — every polyglot case study becomes a blog post + dev.to article + social

---

## Other productionalization items

Worth doing but not blocking launch:

- **`alint --version` includes commit SHA + build date** (verify current state)
- **Crash-report path** — when alint panics, print a pre-filled `https://github.com/asamarts/alint/issues/new` URL with context
- **Schema URL for editor autocomplete** — `# yaml-language-server: $schema=https://alint.org/schemas/v1/config.json` at the top of `.alint.yml`. Schema is already published; just needs docs.
- **Reproducer machinery** — `alint debug bundle` that captures config + a minimal failing repo into a tarball for bug reports
- **Public roadmap page** (separate from internal `docs/design/ROADMAP.md`) — single "what's next" page per major version
- **Telemetry-free guarantee** — `SECURITY.md` or `PRIVACY.md` explicitly states alint sends nothing over the network except `extends: https://...` URLs the user wrote (and that's SRI-pinned)
- **Governance doc** — even a one-page "this is currently a single-maintainer project; here's how decisions get made" sets expectations
- **Search on alint.org** — once SEO + llms.txt land, internal search (Pagefind or similar static-site search) for the rule catalogue
- **Versioned docs** — alint.org currently shows current docs; a `/docs/v0.9/` switcher would let users on older versions land on accurate pages (especially after major API changes like v0.9.10's Scope refactor)
- **Translation strategy** — punt for now; English-only at launch; revisit if traction

---

## Timeline summary

```
Week 1:    ✅ P1 hygiene + P2a pilot (5 of 20 repos, +12 pitfalls catalogued)
                + v0.9.15 P1+P2 (findings doc + examples-parse audit)
Week 2:    P2a-full Waves 1-3 (15 remaining repos in 3 batches of 5)
                + v0.9.15 Phase 3-4 (did-you-mean errors + domain-specific messages)
Week 3:    v0.9.15 Phase 5-6 (JSON Schema + validate-config subcommand) → ship v0.9.15
Week 4:    P3.1 hero + content + P3.2 SEO + P3.3 AI/LLM discovery
Week 5:    P4 launch prep + beta
Week 6:    Launch
Week 7+:   P2b (polyglot monorepos) — runs as evidence-driven content marketing
            + P5 post-launch infra (MCP server, sponsors, analytics)
```

**Total to launch:** ~5-6 weeks. **Total to fully realised state** (40 case
studies + post-launch infra including MCP server): ~10-12 weeks.

---

## First concrete steps

1. ✅ **This doc** — committed for tracking
2. ✅ **P1 in one sitting** — repo launch-presentable (committed `52e7494f`)
3. ✅ **P2a pilot** with 5 repos — methodology validated (committed `e7451b95` + `481b32db`)
4. ✅ **v0.9.15 Phase 1+2** — findings doc + examples-parse audit (committed `ba7802fa`)
5. **P2a-full Wave 1** — 5 parallel subagents (tokio, uv, ruff, clap, typescript)
6. **P2a-full Wave 2** — next 5 (next.js, pnpm, react, prettier, cpython)
7. **P2a-full Wave 3** — final 5 (golang/go, helm, arrow, pytorch, nodejs/node)
8. **P2a aggregation** — update CONFIG-AUTHORING.md with new pitfalls; aggregate v0.10+ rule-kind candidate list
9. **v0.9.15 Phase 3-6** — DX hardening with full pitfall catalogue
10. **v0.9.15 release**
11. **P3 marketing refresh** — hero + SEO + AI/LLM discovery
12. **P4 launch**
13. **P5 post-launch** — concurrent with **P2b** (20 polyglot monorepos)

The plan is intentionally a living doc — every phase will surface adjustments.
Update this file as we learn.
