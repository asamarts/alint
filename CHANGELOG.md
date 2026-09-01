# Changelog

All notable changes to alint are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- New rule kinds `json_path_absent` / `yaml_path_absent` / `toml_path_absent` /
  `xml_path_absent`: assert a JSONPath query over the document matches nothing,
  firing one file-level violation when it does -- the existence sibling of the
  value-checking structured kinds (mirrors `file_absent` for a path). A root-level
  `$[?...]` filter that fans out over every top-level key still yields a single
  violation.
- **XML across every format-aware rule.** `json_schema_passes` gains `format: xml`
  (and auto-detects `.csproj` / `.props` / `.targets` / `.xml`), and the cross-file
  rules (`cross_file`, `file_graph`, `registry_paths_resolve`) accept
  `extract: { xml: ... }`. XML maps to the query tree via the xmltodict-style
  convention: attributes are `@name` keys, repeated sibling elements become an
  array, and every leaf value is a string.
- **dotenv (`.env`) as a config format.** New `dotenv_path_{equals,matches,absent}`
  rule kinds, `format: dotenv` in `json_schema_passes`, and `extract: { dotenv: ... }`
  in the cross-file rules. A `.env` maps to a flat `{ KEY: "value" }` object of
  literal strings with no `${VAR}` expansion (deterministic, environment-independent
  static analysis); `.env` / `.env.*` auto-detect by filename.
- **Java `.properties` as a config format.** New `properties_path_{equals,matches,absent}` rule kinds, `format: properties` in
  `json_schema_passes`, and `extract: { properties: ... }` in the cross-file rules.
  A `.properties` file maps to a flat `{ key: "value" }` object of literal strings
  (via `java-properties`; `${...}` placeholders are the application's job, kept
  verbatim); dotted keys stay one key, queried with bracket notation (`$['db.host']`).
  Auto-detects by the `.properties` extension.
- **INI / `.cfg` as a config format.** New `ini_path_{equals,matches,absent}` rule kinds,
  `format: ini` in `json_schema_passes`, and `extract: { ini: ... }` in the cross-file
  rules. An INI file maps to a 2-level `{ section: { key: "value" } }` object of literal
  strings (a small hand-rolled parser, zero new dependencies); pre-section keys hoist to
  the top level, values stay literal (quotes and inline `;`/`#` kept), a key repeated
  within one scope becomes a file-order array, and section names / keys are
  case-preserving. Multi-line values follow the configparser indentation-continuation
  convention, so `tox.ini` / `setup.cfg` / `pytest.ini` parse correctly. Query a
  section with bracket notation (`$['server']['port']`). Auto-detects by the
  `.ini` / `.cfg` extension.

### Fixed

- The bundled `ci/github-actions@v1` ruleset's `gha-pin-actions-to-sha` rule no
  longer flags **local composite-action references** (`uses: ./...`). Local actions
  live in the same repo (the same trust boundary) and cannot be SHA-pinned; the rule
  now exempts any `./`-prefixed `uses:` value, matching the exemption alint's own
  dogfooded copy already carried. Reported by @ivml.
  ([#208](https://github.com/asamarts/alint/issues/208))
- The same `gha-pin-actions-to-sha` rule no longer flags **digest-pinned Docker
  actions** (`uses: docker://img@sha256:<64-hex>`). A digest is an immutable pin, so it
  is exempt; a tag-pinned `docker://img:tag` still fires.
- The bundled `ci/github-actions@v1` `gha-workflow-contents-read` rule no longer
  false-fires on `permissions: read-all`, `permissions: {}`, `contents: none`, or
  per-job-only permissions. It now fires only when a workflow grants (or defaults
  to) write access to the GITHUB_TOKEN -- no permissions declared anywhere,
  `write-all`, or `contents: write` at the workflow level. Rebuilt on the new
  `yaml_path_absent` kind.

## [0.15.2] - 2026-08-22

A release-infrastructure patch: the alint binary and rules are unchanged from
0.15.1 (byte-identical). It moves npm publishing to keyless OIDC and hardens the
post-publish smoke test.

### Changed

- npm now publishes `@asamarts/alint` via **npm OIDC Trusted Publishing**
  (tokenless), retiring the `NPM_TOKEN` PAT, so a release can no longer fail on an
  expired npm token (as 0.15.1 did). The npm package now also carries a
  build-provenance attestation. Implements the ADR-0015 npm-OIDC decision.

### Fixed

- The advisory post-publish-smoke Homebrew leg no longer false-fails on Homebrew's
  "untrusted tap" gate in CI: it reads the formula from the local tap clone and
  respects a runner-set tap allowlist.

## [0.15.1] - 2026-08-21

A supply-chain-hardening release. The alint tool is unchanged from 0.15.0, but
every release is now cryptographically verifiable end to end: releases are
cosign-signed and carry GitHub build provenance, a CycloneDX SBOM and a
third-party license bundle ship with every artifact, `install.sh` verifies the
signature when `cosign` is present, and each published crate carries its license
texts. See [SECURITY.md](SECURITY.md#verifying-release-artifacts) to verify a
download. Implements ADR-0015.

### Added

- Supply-chain artifacts on every release (P1-a of the supply-chain-hardening
  arc; ADR-0015): a CycloneDX 1.5 SBOM (`alint.cdx.json`) and a third-party
  license bundle (`THIRD-PARTY-LICENSES.html`), both scoped to the shipped `alint`
  binary, attached to the GitHub Release and embedded in every binary tarball and
  the Docker image. A root `NOTICE` states the dual license and accompanies each
  distribution. The image now also carries `LICENSE-APACHE` / `LICENSE-MIT`, and
  each published crate ships its `LICENSE-*` + `NOTICE` in the crates.io archive.
- `install.sh` verifies the release signature when `cosign` (v3+) is present
  (best-effort: `ALINT_SKIP_VERIFY=1` skips, `ALINT_REQUIRE_VERIFY=1` fails closed
  on an untrusted network). Its header documents a tag-pinned installer URL for
  inspection before running.

### Security

- Releases are now signed and carry build provenance (P1-b of the
  supply-chain-hardening arc): `SHA256SUMS` is cosign-signed keyless
  (`SHA256SUMS.cosign.bundle`), every asset recorded in `SHA256SUMS` (tarballs,
  installer, SBOM, license bundle) gets a GitHub build-provenance attestation
  (verifiable with `gh attestation verify`), the CycloneDX SBOM is attested, and
  the container image is both attested (pushed to the registry) and cosign-signed
  by digest. All keyless via Sigstore / GitHub OIDC, so there is no signing secret
  to manage. Verify with cosign v3+ or `gh attestation verify`; see
  [SECURITY.md](SECURITY.md) for the commands.
- `SHA256SUMS` covers `install.sh` and the supply-chain artifacts, so the cosign
  signature over the manifest protects the installer and the attribution bundle,
  not only the tarballs (`MP-L8`).

## [0.15.0] - 2026-08-16

v0.15 makes rules legible and monorepos first-class. `alint explain <rule>` now
surfaces a rule's full configured detail — kind and categories, a one-line
`summary:`, the kind-specific `options:`, the author `message:`, the `when:`
clause, and the auto-fix — with a `docs:` deep link, and `alint rules show
<kind>` / `alint rules list --search` browse the catalog offline. A new
`scope_filter: include_manifest_paths:` / `exclude_manifest_paths:` (ADR-0010)
scopes a per-file rule by the paths a manifest declares — a workspace's `members`
(glob or literal, e.g. `crates/*`), or a package's `bin` entrypoints mapped back
to source via `derive_target:` — so the rule tracks the manifest that owns the
truth instead of a hand-maintained `paths.exclude` that drifts. Alongside,
`scope_filter` now resolves under `--changed`, applies on the rule-major kinds,
and diagnostics never corrupt machine-readable stdout.

### Added

- `scope_filter:` gains `include_manifest_paths:` / `exclude_manifest_paths:`
  (ADR-0010): scope a per-file rule by membership in a path set a manifest
  declares, instead of a hand-maintained `paths.exclude` that drifts from the
  manifest that owns the truth. Each takes a `source:` manifest, the shared
  `{json|toml|yaml|lines|regex}` `extract:`, and an optional
  `derive_target: {from, to}` that maps a declared build output (`package.json`
  `bin`'s `dist/cli.js`) back to source (`src/cli.ts`); `include_manifest_paths:`
  additionally accepts `expect_nonempty:` (default `true`) to warn when the
  resolved set is empty. Membership is
  directory-aware (a declared `workspace.members` entry, including a glob such as
  `crates/*`, scopes a rule to files under each matching directory); the set
  resolves once per run; a manifest value gates which files a
  rule sees, never what it decides about a file. `alint explain` prints the
  resolved set.
- `alint explain <rule>` now shows a one-line `summary:` of what the rule's KIND
  checks, plus a `docs:` deep link to the full reference. Both are compiled into
  the binary from `docs/rules.md` (so they work offline); `--no-docs` suppresses
  the link, and an alias links to its canonical docs page.
- `alint rules show <kind>` describes a single rule kind (summary, categories,
  aliases, docs link) and accepts an alias; `alint rules list` gains a per-kind
  description line; and `alint rules list --search` now matches summary text, not
  just the kind name and its aliases.

### Changed

- `alint explain <rule>` now surfaces the rule's full configured detail. It
  previously printed only `id`, `level`, and `policy_url`; it now also shows the
  rule's `kind` and categories, the `paths:` scope, the author's `message:`, the
  `when:` clause as authored (not the parsed AST), and, for a fixable rule, a
  one-line description of the auto-fix. `explain --format json` gains matching
  additive fields (`rule_kind`, `categories`, `paths`, `message`, `when`,
  `fix`); the existing fields are unchanged. The detail was already in the
  loaded config but was dropped when the rule was built, so most rules used to
  explain to two bare lines.
- `alint list` now shows each rule's `kind` and a `[fix]` marker in its human
  output, matching what `list --format json` already carried (the human
  inventory previously showed only level, id, `[when]`, and the policy URL).
- `alint export-agents-md` keeps a rule's scope in the generated directive when
  the rule sets no `message:` ("file_exists rule on README.md" rather than the
  bare "file_exists rule"), so a generated AGENTS.md stays actionable.
- `alint explain <rule>` additionally surfaces a rule's kind-specific options
  (the flattened fields such as `pattern` / `max_lines`), under an `options:`
  block in the human output and an `options` object in `--format json`. This
  completes the explain output: id, kind, categories, level, paths, options,
  message, policy_url, when, and fix.

### Fixed

- `scope_filter.changed_since:` now resolves under `--changed` / `--base`. Its
  per-rule diff set was cached only on the full index, so a `changed_since` rule
  dispatched against the `--changed` filtered index matched nothing (a silent
  false negative); the resolved diff is now copied onto the filtered index, the
  same way the manifest path sets are.
- `scope_filter` now applies on rule kinds dispatched in the rule-major partition
  (`filename_case`, `filename_regex`, `file_max_size`, `file_min_size`,
  `executable_bit`, `no_empty_files`, `json_schema_passes`, and others), not only
  per-file content rules. Those kinds applied their scope per file but didn't
  expose it for the engine to pre-resolve, and the scope caches were resolved
  after the rule-major dispatch — so a `changed_since:` predicate silently
  matched nothing on them (affected since v0.11). A per-file rule that applies a
  scope now exposes it (also enabling `--changed` to skip it), and the scope
  caches resolve before every dispatch path.
- Diagnostic warnings now write to stderr, never stdout, so a `warn!` (such as an
  empty `include_manifest_paths` set) no longer corrupts the machine-readable
  `--format json` / SARIF output that consumers read from stdout.

## [0.14.2] - 2026-08-06

A fixes-and-hardening patch on top of 0.14.1. The `hygiene-no-macos-junk`
bundled rule stops flagging Hadoop checksum sidecars (via a new
`content_prefix_hex` signature check any `file_absent` rule can use), SARIF
output carries a stable `partialFingerprints` identity on every run,
untagged-enum config errors read clearly, `alint --help` is legible on narrow
terminals, and every third-party CI action is SHA-pinned. Existing configs keep
working; the only change in what gets flagged is fewer false positives from
`hygiene-no-macos-junk`.

### Added

- `file_absent` (and the rules built on it) gained an optional
  `content_prefix_hex` option: a list of hex byte-signatures that a matching
  file's content must begin with to be flagged. It distinguishes real binary
  junk from unrelated files that merely share a name pattern. Unset (the
  default) keeps the historical name-only behaviour.
- `alint check --format sarif` now emits `partialFingerprints` (the canonical
  `violation_fingerprint`) on every run, not only when a baseline is active, so
  GitHub Code Scanning can correlate alerts across runs without `--baseline`.
  The fingerprint matches the GitLab Code Quality export and the baseline file,
  so a finding has one identity across every surface.

### Changed

- `alint --help` now wraps to the terminal width, with each command and global
  option summarised on one line; the full per-item detail moved to
  `alint <cmd> --help` (and `alint -h` shows the concise summary). Previously
  clap emitted the long descriptions unwrapped and the terminal soft-wrapped
  them under the command/option columns, which was unreadable on narrow screens.

### Fixed

- `hygiene-no-macos-junk` no longer reports Hadoop `._<name>.crc` checksum
  files as macOS junk. The rule matched `**/._*` by name, so a committed
  `._SUCCESS.crc` (whose content begins `crc\0`) looked like a macOS
  AppleDouble sidecar (`00 05 16 07`). The bundled rule now verifies the
  AppleDouble or `.DS_Store` ("Bud1") signature via `content_prefix_hex`
  before flagging, and `alint fix` removes only files that carry a real
  signature. Name-only matching is unchanged when `content_prefix_hex` is
  unset.
- Config-parse errors for untagged-enum rejections now describe the accepted
  forms (via serde `expecting`) instead of a bare "data did not match any
  variant" message, across the whole config surface.

### Security

- Every third-party GitHub Actions reference in CI is pinned to a full commit
  SHA, enforced by alint's own `gha-pin-actions-to-sha` dogfood rule (OpenSSF
  Scorecard Pinned-Dependencies hardening).

## [0.14.1] - 2026-07-24

A performance patch. v0.14.0's OOM/TOCTOU read cap inadvertently regressed
content-read throughput on read-heavy repositories by up to ~15% (worst on the
existence-and-content and per-file scenarios), because bounding every whole-file read
with `Take<File>` dropped the standard-library buffer preallocation that
`std::fs::read` performs — the read fell back to a grow-and-reread loop issuing extra
`read()` syscalls per file. v0.14.1 restores single-read behaviour by sizing the read
buffer from the file size the directory walk already knows, keeping the read cap and
its bound fully intact. Read-heavy runs return to (and slightly below) v0.13.0
timings. No configuration or behaviour changes.

### Fixed

- Whole-file content reads no longer grow-and-reread their buffer. The read helpers
  (`alint-core`'s `read_bounded` and `alint-rules`'s `read_capped_with`) preallocate
  from the walk-time file size, so a read completes in one syscall instead of several.
  This reverses a v0.14.0 wall-clock regression on read-heavy scenarios (up to ~15% on
  the most read-dominated) that was invisible to the deterministic instruction-count
  perf gate because the cost lives in syscalls, not guest instructions. The 256 MiB
  analysis cap and its TOCTOU-safe read bound are unchanged. Full investigation,
  reproduction, and the corrected write-up:
  `docs/benchmarks/investigations/2026-07-v0.14-s2-harness-artifact/`.

## [0.14.0] - 2026-07-18

The adoption release. **Baseline mode** (`alint baseline` + `alint check
--baseline`) grandfathers a repo's existing violations behind a
content-fingerprinted snapshot, so `alint check` reports and gates only on *new*
findings — alint becomes a blocking merge gate on an established codebase without
a flag-day cleanup, and for SARIF the suppressed findings are *marked, not
deleted*, so GitHub Code Scanning dismisses them instead of flapping
fixed-then-reopened. Rule kinds go **many-to-many over categories**, so a
cross-cutting kind like `no_bidi_controls` surfaces under both Encoding and
Security instead of hiding under one family, browsable with the new `alint rules
list` / `alint rules categories` catalog commands. Alongside: `--only <id>` to
run a single rule (the exact command the `agent` output format now emits per
fixable finding), a more readable `--compact` output, and a whole-repo security
and correctness cycle that closes an `extends:` trust-bypass, a YAML flow-scalar
DoS, a bidi/zero-width fail-open, a symlink path-confinement escape, and a FIFO
read hang.

### Added

- **Baseline mode** (`alint baseline` + `alint check --baseline <file>`): snapshot
  the current violations into a committed `.alint-baseline.json`, then gate only
  on NEW violations — the one-step way to adopt alint as a blocking merge gate on
  a large legacy repo. The baseline is matched on a content/structural fingerprint
  (not line numbers), so it survives benign code motion and re-triggers when the
  offending code is edited; per-fingerprint counts keep duplicates honest. A
  missing or unsupported-`schema_version` baseline is a hard error, never a silent
  no-op. `--show-baselined` lists the suppressed findings; `--strict-baseline`
  fails when the baseline has stale entries (fixed findings); `alint baseline`
  refuses to grandfather new debt on regeneration without `--accept-new`.
  Per-rule baseline identity is audited so a fingerprint can't silently mask a
  new finding: a path-bearing violation defaults to a `(rule, path)` identity
  (so threshold rules like `file_max_lines` stay grandfathered as the file grows
  instead of churning), while structured-query, cross-file, first-offender and
  multi-per-line rules declare a precise `baseline_key`; a coverage-audit
  collision-invariant (`coverage_audit_baseline_safety`) enforces the boundary
  for every kind. A `baseline: <path>` key in `.alint.yml` persists the baseline
  so CI need not pass `--baseline` (the flag overrides it); like
  `allow_out_of_root`, it is honored only from the user's own top-level config —
  never from an `extends:`'d or nested config — so a published ruleset can't
  choose what the gate suppresses, and a config key pointing at a missing file is
  the same hard error as a missing `--baseline` (never a silent ungated run).
  For **SARIF**, baselined findings are *marked, not removed* — re-emitted with
  `suppressions: [{ kind: "external" }]` + `baselineState: "unchanged"` +
  `partialFingerprints`, so GitHub Code Scanning dismisses (rather than
  closes-then-reopens) the alert; live findings carry `baselineState: "new"` + a
  fingerprint for run-to-run correlation. The `json` envelope gains a
  `baselined_suppressed` count (and the suppressed list under `--show-baselined`).
  Robustness: loading a baseline **sums** duplicate fingerprints (so a git-merged
  file suppresses the total, not the last writer's count); regeneration is
  byte-identical across runs and its guard counts new *occurrences* (a higher
  count on an existing finding is fresh debt too, not just new fingerprints).
  The baseline file is excluded from the walk by `check`, `baseline`, and
  `fix` alike, so a broad-glob content rule (`**/*.json`, `line_max_width`, …)
  can't lint alint's own JSON-Lines artifact as a new violation — the adopt-flow
  stays clean, regeneration doesn't see the artifact as fresh debt, and `fix`
  never rewrites it. The exclusion is root-anchored, so a same-named baseline in
  a subdirectory is still linted (its real violations aren't silently dropped).
  See `docs/design/baseline.md` / ADR-0006.
- `--only <RULE_ID>` on `check` and `fix` (repeatable): restrict the run to the
  named rule id(s) from the effective config. An id that matches no loaded rule
  is an error, so typos fail loudly. This is the command the `agent` output
  format emits per fixable violation (`alint fix --only <rule-id>`), which
  previously named a flag that did not exist.
- `fix_command` field on `check --format agent` violations: the exact argv
  (e.g. `["fix", "--only", "<id>"]`) to auto-fix a single violation, present
  iff `fix_available`. Lets an agent run the fix programmatically instead of
  parsing the command out of the English `agent_instruction`.
- **Many-to-many rule categories.** Every rule kind now belongs to one or more
  categories (primary first), so cross-cutting kinds surface where users look for
  them — `no_bidi_controls` is both Encoding and Security, `git_commit_gpg_signed`
  both Git hygiene and Security — instead of hiding under a single family. Two new
  config-independent catalog commands: `alint rules list` (browse every kind,
  optionally `--category <slug>` or `--search <text>`) and `alint rules
  categories` (the vocabulary: slug, title, and how many kinds each holds); and
  `alint list --category <slug>` filters *this repo's* effective config by
  category. The membership is one source of truth — a generated in-crate bridge
  (`gen-categories`), validated against `docs/rules.md` + the registry and
  drift-gated — published as `facts.json` `categories` (slug / title / order) and
  `rule_categories` (kind → category slugs), which alint.org renders and
  cross-checks so the site can't disagree with the tool. `docs/rules.md` gains a
  `Categories:` line per kind, and the site's family pages list membership by
  category. The category vocabulary stays the current 13 families; only the
  membership becomes many-to-many. Design: `docs/design/rule-categories.md`; CLI
  shape in ADR-0009.

### Changed

- **`--compact` output is more readable: the `:line:col` suffix is dropped for
  findings that have no specific location, and the path prefix is colored.**
  A repo-level or whole-file finding used to render `<repo>:0:0:` / `PLAN.md:0:0:`,
  where the `:0:0` was noise that also misleadingly implied line 0 / column 0.
  Located findings still emit the full `path:line:col:` an editor / grep jumps on;
  only findings with no location drop it (`PLAN.md: warning: …`). The location
  prefix is now magenta so each finding's start is visually distinct even without
  the grouped format's per-file separators. Color is TTY-only (SGR is stripped
  when piped), so `path:line:col` piping is unchanged.
- **`git_tracked_only` and `respect_gitignore` are now kind-specific options,
  rejected off the kinds that honor them (ADR-0008).** Both moved off the common
  `RuleSpec` into per-kind options: `git_tracked_only` into the four existence
  kinds (`file_exists` / `file_absent` / `dir_exists` / `dir_absent`), and the
  per-rule `respect_gitignore` override into `file_exists` alone (the only kind
  that honors it - the pitfall-#18 escape hatch). A rule that sets either field on
  a kind that does not honor it now fails to load with a clean error instead of
  silently ignoring it (the fail-loud the fields' doc-comments always promised).
  This is a stricter load: configs that set `git_tracked_only` on a non-existence
  kind, or `respect_gitignore` on any kind but `file_exists`, used to load and
  no-op and now error. The workspace-level `respect_gitignore:` walker setting is
  unaffected. The four existence kinds are now schemars-derived, which also fills
  their previously-empty option descriptions.

### Fixed

- **Auto-fixers no longer corrupt binaries or risk truncation on failure.**
  The byte-level fixers (`line_endings`, `no_bom`, `final_newline`, and the
  `file_prepend` / `file_append` content injectors) rewrote raw bytes with no
  binary guard, so a `paths: "**"` glob could corrupt a matched binary file
  (a CRLF rewrite inserting `\r` into a `.png`, a stray final `\n`, a stripped
  leading BOM-shaped byte). They now skip files detected as binary. Separately,
  every content-rewriting fixer wrote in-place via `std::fs::write`
  (open-truncate-then-write), so a crash or `ENOSPC` mid-write could leave the
  original truncated or lost; writes now go through a shared atomic helper
  (sibling temp + `fsync` + rename, preserving the file mode — and writing
  *through* an in-tree symlink so the link survives, not the rename clobbering
  it into a regular file). `final_newline`'s
  on-disk path is reconciled with its editor path (it no longer double-appends
  to an already-terminated or empty file). (H3)
- **`validate-config` rejects an output format it can't render.** `alint
  --format sarif validate-config` (or `validate-config --format sarif`) silently
  fell through to human output; it now fails loudly (exit 2) for any format
  other than `human` / `json`, regardless of flag position — matching how
  `list` / `facts` / `explain` already gate their formats. (M13)
- **`fix` rejects an output format it can't render, before touching files.**
  `alint fix --format sarif` (or `github` / `junit` / `gitlab` / `agent`)
  silently degraded to human output with a *success* exit code — those formats
  describe findings, not fixes, and have no fix-report renderer. It now fails
  loudly (exit 2), and because `fix` mutates the tree the gate runs *before* any
  fix is applied, so a rejected format never leaves a half-fixed repo. `fix`
  still renders `human` / `json` / `markdown`; the error points `agent` users at
  `check --format agent`, whose per-violation `fix_command` drives the agentic
  fix loop. (E2E sweep)
- **`json` / `agent` output survives a non-UTF-8 filename.** Both serialized the
  violation path through serde's strict `&Path` impl, which *errors* on a
  non-UTF-8 path (legal on Unix) — so a single oddly-named file made `alint
  check --format json` (or `--format agent`) abort with `exit 2: "path contains
  invalid UTF-8 characters"` while every other format rendered it fine. Both now
  render the path lossily (invalid bytes → U+FFFD), consistent with SARIF /
  GitLab / JUnit / GitHub / Markdown / human. Surfaced by a new weird-path ×
  output-format matrix that renders a violation path carrying each format's
  metacharacters, a control byte, and a non-UTF-8 byte through all eight
  formats. (E2E sweep)
- **Markdown output no longer splits a heading on a newline-in-path.** A file
  whose name contains a `\n`/`\r` (legal on Unix) broke out of the `## \`path\``
  inline-code heading, orphaning the rest onto a following line. `md_inline_code`
  now collapses those control chars to a space (inline code is single-line), and
  the weird-path matrix asserts every Markdown heading stays complete. (review
  follow-up)
- **Exit code `3` (internal error) is now actually produced.** The README
  documented `3` for an internal alint error vs `2` for a config/usage error,
  but every error funnelled to `2`, so a script could never tell "fix your
  config" from "file an alint bug." A new internal-error class is tagged at the
  genuinely-internal sites (a bundled ruleset shipped inside the binary failing
  to parse), and the CLI now returns `3` for those and `2` for everything a user
  can fix. (M11)
- **`no_symlinks` now flags directory symlinks.** It scanned only file
  entries, so an in-tree symlink whose target is a *directory* (indexed as a
  dir entry when the walk follows it) slipped through. It now scans all indexed
  entries and re-stats each. (A symlink whose target escapes the repo root is
  pruned by the walker before indexing and is still not flagged — recording
  those safely is a tracked follow-up.) (M4)
- **GitLab Code Quality no longer drops duplicate findings.** The
  `fingerprint` was `SHA256(rule_id|path|message)`, so two genuinely-distinct
  findings sharing all three (a generic-message per-line rule firing on
  several lines of one file) produced the same fingerprint — and the Code
  Climate spec GitLab consumes requires per-report uniqueness, so GitLab
  silently kept only one. The fingerprint now folds in a per-report
  `occurrence` discriminator. The line number is still deliberately excluded,
  so a finding that drifts up/down stays the same issue across runs; only true
  duplicates are disambiguated. (M10)
- **`--config` is honest about being single-valued.** The flag help promised
  "repeatable; later overrides earlier", but every consumer read only the
  first `-c`, so a second was silently dropped — and worse, was position-
  sensitive across the subcommand boundary, so `-c base.yml -c override.yml`
  could use `base`. A second `--config` is now a hard error pointing at
  `extends:` for composition, and the help text is corrected. (H4)
- **The GitHub Action's `config:` input matches the single-valued `--config`.**
  The action advertised "config file path(s), one per line" and emitted one
  `--config` per line — but the H4 fix above made a second `--config` a hard
  error, so any workflow passing two config paths broke (exit 2 from the
  binary). The input is now documented as a single path pointing at `extends:`
  for composition, and the action rejects more than one path itself with a clear
  GitHub-annotated `::error::` rather than surfacing a raw CLI stderr. New
  `action-selftest` jobs cover both the single-path happy path and the
  multi-path rejection. (E2E sweep)
- **A long flat `when:` chain fails loudly instead of aborting the process.**
  The expression parser capped `(`/`[`/call nesting at depth 64, but a *flat*
  chain — `a and a and …` (or `or`) — is parsed iteratively and never re-enters
  the guarded recursion, so it slipped past the cap. An adversarial ~100k-operator
  `when:` from an untrusted `extends:` ruleset built a deeply left-nested AST that
  overflowed the stack on its recursive `Drop`/eval — an uncatchable abort, the
  strongest determinism violation. Both chain loops now bound length the same way
  nesting is bounded, so the chain fails with a normal parse error. (H5)
- **The zero-width fixer strips the full set the rule flags.**
  `no_zero_width_chars` flags U+2060 (WORD JOINER) and U+180E (MONGOLIAN VOWEL
  SEPARATOR) alongside U+200B/C/D and body-internal U+FEFF, but the
  `file_strip_zero_width` fixer hard-coded only the latter four — so a file
  containing U+2060/U+180E was reported on every run yet never repaired, and
  `--fix` never converged. Both fix paths now defer to the rule's own
  `is_flagged_zero_width`, so detector and fixer can't drift apart again. (L1)
- **`file_header` / `file_footer` fixers no longer stack duplicates.** When a
  configured header/footer's content didn't satisfy the rule's own pattern,
  the violation never cleared, so each `--fix` re-prepended/appended it. The
  prepend/append fixers now skip when the file already begins/ends with that
  content (idempotent across runs). (L4)
- **`{token}` path templates no longer re-substitute.** `render_path` ran a
  sequence of `String::replace` passes, so a token that appeared in an earlier
  substitution's value (e.g. a file literally named `a{ext}.c`, stem `a{ext}`)
  was wrongly re-expanded by a later pass. It now does a single left-to-right
  scan, emitting each value once. (L8)
- **The JSONPath dashed-key hint no longer false-fires inside string
  literals.** A dash in a comparison literal (`@.x == 'a.dashed-value'`) used
  to trigger the "use bracket notation" hint; quoted spans are now masked
  before the check. (L11)
- **`custom` facts now honor a 30s timeout.** The doc promised that a timing-out
  custom-fact command resolves to the empty string, but the implementation used
  a blocking `Command::output()` with no timeout — a hanging command froze the
  run. It now drains output on a thread and kills the child at the deadline
  (matching the `command` rule's 30s default). (L6)
- **`when:` `matches` on a missing fact is falsy, not an error.** `null == "x"`
  was already falsy, but `<missing fact> matches "x"` hard-errored — an
  inconsistency. A missing (null) left-hand side now evaluates to `false`; a
  non-string *value* is still a config-type error. (L10)
- **`when:` string literals are decoded as UTF-8.** The lexer cast each byte to
  `char` (Latin-1), so a non-ASCII literal like `vars.x == "café"` was lexed as
  mojibake and could never match. Multi-byte scalars now lex correctly. (L3)
- **`no_case_conflicts` folds case the Unicode way.** It lowercased ASCII-only,
  so `É.txt`/`é.txt` (and other non-ASCII case pairs) slipped past — yet a
  case-insensitive filesystem folds them. It now uses Unicode `to_lowercase`,
  the strict portable default. (L2)
- **`filename_case` `--fix` converges on non-ASCII names.** The `lower`/`upper`
  conventions case-folded ASCII-only while the check was Unicode-aware, so a name
  like `RÉSUMÉ.md` "fixed" to a form the check still rejected and re-fired every
  run. `convert` now folds with Unicode `to_lowercase` / `to_uppercase`, so the
  fixed name is a fixed point of the check. (W4)
- **`command` argv is guarded against option-injection via filenames.** A repo
  file named like a flag (e.g. `--write`) rendered from `{path}` could turn a
  trusted `command: ["prettier", "--check", "{path}"]` into a destructive
  `--write`. A path token that would render to a leading dash is now prefixed
  with `./`; flags you wrote yourself are untouched. (L13)
- **Unreadable files are reported, not silently skipped.** A non-`NotFound`
  read error (permission denied, I/O) during the per-file walk was
  indistinguishable from "file absent." Such errors are now logged at `warn`
  (visible with `-v` / `RUST_LOG`); a genuinely-absent file still skips
  silently and the run stays resilient. (L7)
- **SARIF file paths are now valid URI references.** `artifactLocation.uri`
  emitted the raw OS path, so a `\`-separated path on Windows broke GitHub
  Code Scanning's repo-file mapping, and a path containing a space / `#` /
  `%` was non-conformant. Paths are now forward-slashed and percent-encoded
  per RFC 3986 (plain-ASCII paths are unchanged). (M9)
- **The baseline flags fail loudly off `check`.** `--baseline`,
  `--strict-baseline`, and `--show-baselined` are global but only `check`
  honored them; on `fix` / `list` / `baseline` / … they were silently
  ignored, breaking the flag's "a missing baseline is an error, never a
  silent no-op" contract. They're now rejected on any subcommand but
  `check` (the `baseline` subcommand writes via its own `--output`). (M12)
- **The `is_text` rule kind no longer silently accepts unknown options.** The
  `is_text` alias of `file_is_text` was registered without the
  `deny_unknown_options` wrapper its canonical name carries, so a typo'd option
  on an `is_text` rule was swallowed instead of failing the build (every other
  kind, the canonical `file_is_text` included, already rejected it). A
  registry-driven coverage test now probes **every** registered kind — aliases
  included — so an alias can't diverge from its canonical kind again.
- **`validate-config` (and `check`) now structurally validate nested `require:`
  rules.** `for_each_dir` / `for_each_file` / `every_matching_has` build their
  nested rules lazily per matched entry, so a nested rule with an unknown kind,
  an unknown option, or a missing required field passed `validate-config` clean
  and was caught by `check` only when the selector matched at least one entry
  (with a selector matching nothing, never). Each rule now dry-builds its nested
  specs at load, so these errors surface immediately — honoring
  `validate-config`'s "is the config loadable?" contract. The new validation
  immediately caught a real latent bug: the `examples/clap-rs-clap` config had a
  nested `toml_path_matches` with an invalid JSONPath
  (`$.package.metadata.docs.rs.rustdoc-args[*]` — the dashed `rustdoc-args` key
  needs bracket notation, `['rustdoc-args']`), now fixed.
- **A nested config declaring `allow_out_of_root:` is now rejected, not silently
  dropped.** Every other trusted root-only key (`extends`, `facts`, `vars`,
  `ignore`/`nested_configs`, `baseline`) already errored when set in a nested
  `.alint.yml`; `allow_out_of_root` parsed and was ignored without feedback. The
  out-of-root grant was never honored from a nested config (confinement always
  held), but it now fails loudly for parity, so a subtree can't appear to grant
  itself reads outside the repo root.
- **`alint baseline` now rejects a non-default `--format` instead of silently
  ignoring it.** `baseline` writes the baseline file plus a human summary and has
  no machine-readable report output, so `baseline --format json` (etc.) was a
  no-op; it now errors and points you to drop the flag.
- **`filename_regex` no longer doubles a redundant anchor in its violation
  message.** The rule auto-anchors with `^…$`, so a pattern written as `^test_`
  rendered as `/^^test_$/`; it now shows the effective `/^test_$/`.
- The `--only` flag's help said it was "a no-op for other subcommands"; it is
  actually rejected there, and the bare `alint --only <id>` form lints the
  current directory (for a path, use `alint check --only <id> <path>`). The help
  text now says so, and the `alint baseline` summary pluralizes "occurrence"
  correctly for a single occurrence.
- **`root_only:` now works on `file_absent` / `dir_exists` / `dir_absent`.** The
  option was documented as the existence-family counterpart of `file_exists`'s
  `root_only` and used in a bundled ruleset, but the three sibling rules silently
  ignored it. They now honor it — `root_only: true` restricts a glob that would
  otherwise match at any depth (e.g. `**/NOTES.md`) to paths directly at the
  repository root — with the option validated and published in the config schema,
  like `file_exists`. (The bundled usages pin bare filenames, which already match
  only the root, so their behavior is unchanged.)
- **LSP server robustness.** The `alint lsp` server held its shared state behind
  a `std::sync::Mutex` accessed via `.lock().unwrap()` at every site, so a panic
  in any one async handler poisoned the lock and wedged the whole session
  (every subsequent request panicked). It now uses a non-poisoning
  `parking_lot::Mutex`, so a single handler panic can't cascade into a dead
  server; the editor session survives.
- `alint list --format json` and `alint explain <id> --format json` now emit a
  stable JSON envelope (a rule inventory and a single-rule shape) instead of
  silently printing the human output with a success exit code. An unsupported
  `--format` for these commands is now an explicit error rather than a silent
  human fall-through.
- The `agent` output format no longer instructs agents to run
  `alint fix --only <id>` when no such flag existed; `--only` now exists, and a
  regression test parses every command the agent format emits against the real
  CLI so a dead command can't reappear.
- Corrected config examples that the loader (and the JSON schema) reject:
  `docs/design/ARCHITECTURE.md` (`query:`/`pattern:` → `path:`/`matches:`,
  map-keyed `rules:` → a list, a `sha256:`/`path:` `extends` mapping → SRI
  URL-fragment + bare-string form, phantom `detect:`/`primary_language`/
  `count_contributors` facts removed, `custom: {command:}` → `argv:`, WASM
  plugin tier relabelled v0.13 → v0.14) and `docs/rules.md`
  (`max:` → `max_width`/`max_depth`, `unique_by` `paths:` → `select:`).
- Added the `agent` format to the README `--format` example block (it listed
  seven of the eight formats) and removed the stale "Planned rulesets (v0.5)"
  section from `docs/rules.md` (all five shipped).
- The `*_path_equals` / `*_path_matches` rule reference pages no longer all
  show `kind: json_path_*`: each page's example now leads with its own kind,
  and the `*_path_matches` example gained the missing `toml_path_matches`
  variant.
- Fixed documentation links that dead-end for a reader browsing on GitHub:
  a broken `crates/alint-core/src/when.rs` link (now the `when/` module),
  the `/docs/rules/` cross-links in `ARCHITECTURE.md` (now absolute
  `https://alint.org` URLs), and a "full reference at alint.org" banner on
  the GitHub view of `docs/rules.md`.

### Security

- **Closed three RCE bypasses of the `extends:`/nested spawn gate.** The gate
  that confines process-spawning rule kinds (`command`,
  `command_idempotent`, `generated_file_fresh`) to the user's own top-level
  config inspected only `rules[].kind` at each inherited source, so three
  paths slipped past it: (1) an `extends:`'d ruleset could hide a spawning
  kind in a `templates:` block and reference it from a `kind`-less
  `extends_template:` rule that expands into a `command` rule *after* the
  gate runs — meaning a single SRI-pinned `extends:` line could run
  arbitrary code; (2) a nested `subdir/.alint.yml` (under
  `nested_configs: true`) was never spawn-gated at all, so any subtree
  config — addable via an untrusted monorepo PR, vendored dir, or submodule
  — could declare `kind: command` and execute on `alint check`; and (3) the
  `require:` block of `for_each_dir` / `for_each_file` / `every_matching_has`
  carries *nested* rule specs whose `kind` flattens into the parent rule's
  options, which neither the top-level check nor a post-`finalize` scan
  inspects (found in pre-merge review). Fix: a spawning kind may no longer
  appear in *any* `templates:` block (enforced source-agnostically in
  `finalize`, and earlier with the offending ruleset named); nested configs
  are now spawn-gated like `extends:` and may not declare `templates:`; and
  the gate recurses into `require:` blocks at the raw-mapping level (the only
  place that catches the nested kind, which never becomes a top-level rule).
  A spawning kind in a top-level `rules:` entry — the intended, allowed case
  — is unchanged. Seven regression tests encode the three PoCs plus the
  allowed paths. See `docs/design/v0.14/post_v0.13_audit.md` (C1, C2, and the
  `require:` vector).
- **Path confinement now follows symlinks for config-derived reads.**
  `normalize_confined` is purely lexical, so an in-repo symlink (`link ->
  /etc`) let a lexically-confined `link/secret` escape the repo root at
  read time and bypass the `allow_out_of_root` gate — a content/existence
  disclosure oracle reachable from an `extends:`'d ruleset (notably
  `json_schema_passes`, which echoes schema-compile errors into a
  violation). Every config-derived read (`json_schema_passes`, `pair_hash`,
  `cross_file`, `registry_paths_resolve`, and the `file_exists`
  gitignore-bypass stat) now resolves symlinks and re-verifies containment
  before reading; an escape is reported as out-of-root and honored only
  under a top-level `allow_out_of_root`. The Kani proof and its prose are
  corrected to scope the guarantee to the *lexical* policy (the filesystem
  layer is guarded by code + tests, not the proof). (H1)
- **`when:` expressions are depth-bounded.** A crafted `when:` from an
  untrusted `extends:` ruleset — deeply nested parens, or a long flat
  `a and a and …` chain — could overflow the parser or evaluator stack (an
  uncatchable abort). Both now fail loudly past a generous depth cap. (H5)
- **Structured-query rules no longer panic on a multibyte value.** The
  `*_path_*` error renderer byte-sliced an untrusted matched value at
  offset 80, which panics when a codepoint straddles that boundary —
  crashing the whole parallel `check` run (and the LSP server). It now
  truncates on a char boundary. (H2)
- **Remote `extends:` no longer follows redirects (SSRF hardening).** ureq's
  defaults (up to 10 redirects, downgrade to `http` allowed on a redirect hop)
  let a pinned-but-malicious HTTPS host `302` a fetch to
  `http://169.254.169.254/…` (cloud metadata) or any internal address. The
  fetcher now refuses redirects outright; an `extends:` URL is SRI-pinned to
  specific content, so it must point at the final resource and a redirect
  surfaces as a clear error. (M1)
- **Local `extends:` targets are confined to the config's directory.** A local
  `extends: ../../../../etc/shadow` (or an absolute path) in a shared ruleset
  committed to the repo was read off the host, and a YAML-parse error could
  echo file content back — a read/exfil oracle. The loader now rejects any
  local `extends:` target that resolves outside the top-level config's
  directory; both sides are canonicalized so `..` and symlinks (an in-tree
  symlink pointing out) are caught. A top-level `allow_out_of_root: true`
  lifts it for the whole chain (the same blanket escape that lifts per-rule
  read confinement); `extends:`'d and nested configs still cannot grant it. (M2)
- **A non-UTF-8 commit can no longer bypass commit linting.** `parse_commit_log`
  silently dropped any commit whose author name, email, or message was not
  valid UTF-8, so a contributor could dodge `git_commit_subject_matches` /
  author-allowlist / forbidden-pattern checks by using non-UTF-8 metadata.
  Commits are now lossily decoded and still linted. (M6)
- **`git_blame_age` no longer panics on a crafted author timestamp.** A commit
  carrying a 19-digit author-time (parses as `u64`, overflows `SystemTime` on
  `+`) aborted the run; the addition is now checked and a malformed timestamp
  is dropped. (M7)
- **`git_no_denied_paths` denylist patterns are anchored to any depth.** A
  bare denied *literal* (no `/`, no wildcard) like `id_rsa` was root-anchored by
  globset, so a tracked `secrets/id_rsa` evaded the denylist — a dangerous
  default for a secrets control. (A bare *wildcard* like `*.pem` already crosses
  `/` in globset, so it was never the gap — an earlier note over-generalised.)
  Bare patterns are now auto-anchored to `**/<pattern>` (matching any depth,
  root included — a no-op for wildcards, the real fix for literals); explicit-path
  patterns (`secrets/*.key`, `**/*.pem`) are taken as written. (M5)
- **Completed the Unicode control-character sets for the obfuscation rules.**
  `no_bidi_controls` now also flags the implicit directional marks U+061C
  (ALM), U+200E (LRM), and U+200F (RLM) — completing the Trojan-Source
  (CVE-2021-42574) set that rustc's own lint covers; `no_zero_width_chars`
  now also flags U+2060 (WORD JOINER) and U+180E. The strip fixers extend in
  lockstep. Note: U+200D (ZWJ) stays flagged even though it joins emoji
  sequences, so its strip fixer will break a literal emoji ZWJ sequence —
  scope the rule away from files that legitimately carry such emoji. (L1)
- **Per-file reads are bounded so a giant file can't OOM the run.** Only the
  cross-file rule kinds capped their reads; the per-file engine/rule loops and
  the structured-query kinds read whole files unbounded, so one committed
  multi-GB blob could exhaust memory. The 256 MiB analysis cap now lives in
  `alint-core` and gates every per-file read — the engine/rule loops skip an
  over-cap file up front using the walk-time index size (no extra `stat`) and
  log it at `warn`; the run stays resilient (one oversized file never aborts
  the lint). (M3)
- **Resource-exhaustion hardening (several spots).** A few unbounded
  operations on attacker-influenced input are now capped: the `extends:`
  chain has a depth cap (`MAX_EXTENDS_DEPTH = 64`) so a deeply-nested acyclic
  chain errors instead of overflowing the recursion stack (L5); the remote-
  `extends:` disk cache writes a PID-unique temp file instead of a fixed
  `<sri>.yml.tmp` (no concurrent-run race) (L5); the "did you mean" suggester
  skips length-mismatched candidates before building its edit-distance matrix,
  bounding work on a multi-kilobyte unknown field name (L9); and a spawned
  rule's captured stdout/stderr is capped at 64 MiB with the excess drained,
  so a runaway generator can't OOM the run (L12). (L5, L9, L12)
- **Human output neutralizes terminal escapes in untrusted text.** A repo
  file named with an `\x1b[…]` sequence — or a `kind: command` rule whose
  subprocess output carries ANSI codes — could clear the screen, hide
  findings below the fold, or forge an "all rules passed." banner when a
  human lints an untrusted repo (alint's `anstream` stream passes raw bytes
  through on a TTY, exactly where the escapes fire). The grouped, compact,
  and fix renderers now replace every control char (except the intentional
  newline) in attacker-controlled paths/messages with a visible `\xNN`
  escape. alint's own styling is unaffected, machine formats (JSON/SARIF/…)
  are unchanged, and clean output is byte-identical (no TTY-conditional
  divergence). (M8)
- **An inherited ruleset can no longer lift path-confinement before its own
  `extends:` resolves.** The loader derived `extends:`-target confinement from
  `allow_out_of_root: true` at every recursion level, but the rejection of that
  flag in a non-top-level config fired only *after* the sub-config had already
  resolved its own `extends:`. So an inherited (untrusted) ruleset could set the
  flag and read an out-of-root target — hang on a FIFO, slurp a large file, or
  probe host paths as an existence oracle — before the reject could fire. Only
  the user's own top-level config (and trust-equivalent `.alint.d/` drop-ins) may
  now open that gate. (W1)
- **Deeply-nested YAML flow collections can no longer hang the run.**
  `serde_yaml_ng`/libyaml has no nesting limit and is super-linear on `[[[…` /
  `{{{…`, so a crafted config, an `extends:`'d ruleset, or a `yaml_path_*` rule
  over crafted repo content could stall for seconds-to-forever. A cheap pre-parse
  flow-depth scan (shared in `alint-core`, cap 1024) now rejects such input
  before libyaml sees it, matching the existing JSON/TOML/XML depth guards; it
  counts only genuine flow opens, so a `[` inside a plain or quoted scalar never
  false-rejects ordinary YAML. (W2)
- **The Trojan-Source and zero-width rules no longer fail open on a non-UTF-8
  byte.** `no_bidi_controls` and `no_zero_width_chars` abandoned the whole file
  on the first invalid UTF-8 byte, so appending one stray `0xFF` suppressed
  detection of every bidi override / zero-width char in it — a one-byte bypass of
  a CVE-2021-42574 defense. Both now decode lossily and keep scanning the valid
  runs. (W3)
- **A direct read of an in-tree FIFO can no longer hang the run.** The walker
  skips named pipes at index time, but the direct-read helpers reached by
  shebang / prefix / suffix / structured rules on a config- or content-derived
  path opened them anyway, blocking `O_RDONLY` until a writer appeared. They now
  stat `is_file()` before opening, matching the walker. (W5)

### Internal

- Expanded the dogfood config (`.alint.yml`) to exercise alint's flagship rule
  families on its own tree (Dog1): structured-query (`toml`/`json`/`yaml_path_equals`
  over `Cargo.toml`, the VS Code `package.json`, and `action.yml`), cross-file
  (`registry_paths_resolve` on the `[workspace] members`), and git-hygiene
  (`git_no_denied_paths` for secret-shaped files). Each was negative-tested to
  confirm it fires when broken; the dogfood stays green (46/46).
- Split the two source files over the self-lint's `rust-file-max-lines`
  threshold so the dogfood is fully green (Dog2, no behavior change):
  `alint-dsl/src/lib.rs`'s test module moved to `src/tests.rs`, and
  `xtask/src/docs_export.rs` split its tests + the drift-gate count parsers into
  `docs_export/{tests,counts}.rs`. Verified the `docs-export` manifest counts
  are unchanged.
- The post-v0.12 audit fixes each ship with a regression test (a registry-driven
  unknown-option probe over every kind, nested-`require:` and nested
  `allow_out_of_root` rejection, the `baseline --format` guard, and the
  `filename_regex` message). Workspace line coverage is 91.05%, above the 85% CI
  floor.
- Corrected `docs/design/baseline.md`: the `gitlab.rs` fingerprint is documented
  as still using the legacy message-keyed scheme — the unification onto
  `violation_fingerprint` landed for the SARIF `partialFingerprints` but the
  GitLab migration is deferred (it needs precomputed fingerprints threaded to the
  formatter) — and `line_max_width` is reclassified as a first-offender rule.
- New drift gates so the doc-vs-schema, stale-status, generated-page, and
  dead-link classes can't recur: `coverage_audit_doc_examples` loads every
  fenced config example in the hand-written docs through the real config
  loader; `coverage_audit_planned_rulesets` fails if a "Planned" section
  names a ruleset already in `facts.json`; `structured_query_pages_lead_with_their_own_kind`
  asserts each generated structured-query page leads with its own kind; and
  `coverage_audit_doc_links` checks repo-doc links resolve (no broken
  relative links, no GitHub-dead root-absolute links).

## [0.13.0] - 2026-06-17

This release turns the documented surface into generated, drift-proof contracts.
The config JSON Schema is now derived from the Rust option types, powering a
per-rule `## Options` table on every reference page; `facts.json` and
`roadmap.json` publish the surface-area counts and the public roadmap as
machine-readable artifacts; the path-confinement boundary from v0.12.0 gains a
Kani-model-checked proof of its lexical containment policy plus proptest
properties; and the architecture is
documented as a single interactive LikeC4 model on alint.org, exported to Mermaid
for the GitHub repository.

### Added

- **`facts.json`: a machine-readable surface-area contract.** A generated,
  committed manifest (alint version; the rule-kind / family / bundled-ruleset /
  auto-fix-op / output-format / subcommand counts; and catalogue lists) shipped
  into the docs bundle at a stable URL, so the README, docs, and alint.org render
  these numbers from one source instead of restating them in prose. Generated and
  drift-gated by `xtask gen-facts --check`.
- **`roadmap.json`: a machine-readable public-roadmap contract.** A generated,
  committed phase list (version, title, kind, and a one-line blurb) parsed from
  the `roadmap-public` markers in `docs/design/ROADMAP.md` and shipped into the
  docs bundle, so alint.org's `/roadmap/` page renders a data-driven timeline
  instead of a hand-maintained one (status is derived by the consumer from the
  released version). Generated and drift-gated by `xtask gen-roadmap --check`,
  which also forbids AI-content signals (em dashes, smart quotes) in the
  published blurbs.
- **Interactive architecture diagrams on alint.org.** The system, container,
  component, and flow views are generated from a single
  [LikeC4](https://likec4.dev) model and rendered as interactive web components,
  with the crate dependency graph extracted from `cargo metadata`. The same model
  is exported to static Mermaid for the GitHub repository. CI gates the model
  against the code (crate set, config keys, rule catalogue, embedded view ids).

### Changed

- **Every rule reference page on alint.org now carries a generated `## Options`
  table** (name / type / required / default / description), derived from the
  type-driven JSON Schema rather than hand-maintained prose, so the documented
  options cannot drift from the engine.
- **The config JSON Schema is now generated from the Rust option types** (via
  `schemars`) instead of being hand-maintained, so the published schema and the
  engine cannot drift; a freshness gate keeps the committed schema in sync.

### Fixed

- **`cross_file` `SemverMajor` normalization is now idempotent**, so normalizing
  an already-normalized value is stable.
- **`file_graph` and `cross_file` violation messages now emit forward-slash
  paths**, for consistent output across platforms.
- **The GitHub Action emits its `sarif-file` output even when the check finds
  violations**, so a failing run still uploads SARIF to Code Scanning.

### Security

- **A Kani-model-checked proof and proptest properties now verify the lexical
  path-confinement policy** introduced in v0.12.0 (the untrusted-`extends:`
  threat model): the `..`/absolute-root normalization can't underflow or escape.
  Both cover the *lexical* layer only; the filesystem layer — an in-repo symlink
  that resolves outside the root — is a separate runtime canonicalize-and-recheck
  (`resolved_within_root`), covered by integration tests, not the proof. Turns
  the lexical guarantee into a machine-checked invariant.

## [0.12.0] - 2026-06-07

The case-study-driven rule-kind expansion, paired with a security cycle that
hardens path handling into a real boundary. New kinds: `file_graph`
(file-dependency-graph firewalls plus cycle / orphan / dangling-edge checks),
`for_each_match` (a per-line predicate quantifier), a unified `cross_file`
value-relation kind, `pair_changed_together` (a co-change gate), and
`generated_file_fresh` mutating / in-place mode — plus markerless
`ordered_block`, a `php@v1` bundled ruleset, JSONC-tolerant structured parsing,
and more `import_gate` language presets. On the security side, every
config-declared path a rule reads or resolves is now confined to the repo root
(the untrusted-`extends:` threat model), with `allow_out_of_root:` as the
explicit top-level opt-in, the file walker pruning symlinks that escape the
tree, and — most notably — a fixed **git argument-injection** in the `since:`
range mode that could write or truncate an arbitrary out-of-tree file and
affected released versions back to v0.9.21.

### Added

- **`allow_out_of_root:` — a top-level opt-in to read config-declared paths
  outside the repo root.** Path confinement is the secure default (a rule can
  never read or resolve a config-declared path outside the tree); this
  top-level-only key relaxes it for *reads* when a trusted config needs to
  reference an external file (a shared schema, a manifest in a sibling checkout).
  `allow_out_of_root: true` permits every rule; `{ kinds: [...], rules: [...] }`
  permits only the listed rule kinds / ids; absent or `false` keeps full
  confinement. **Rejected from `extends:`'d rulesets** — only the user's own
  top-level config may open the hatch (the same trust model as the spawning-rule
  gate), so adopting a ruleset can never grant it out-of-tree reads. Honored by
  the non-spawn-gated read kinds `registry_paths_resolve` (`source:`),
  `json_schema_passes` (`schema_path:`), and `pair_hash` (`target:`); a permitted
  read emits an informational note. Resolve/index checks, the spawn-gated
  `generated_file_fresh`, and (for now) `cross_file`/`file_graph` reads stay
  confined. Design: `docs/design/v0.12/allow_out_of_root.md`.

- **Macro bench scenario `S14` — comprehensive v0.12 featureset coverage.** One
  deliberately-mixed `xtask bench-scale` scenario exercises every file-shape
  rule kind/mode added in v0.12 over the synthetic macro tree (so a single bench
  row catches a regression anywhere in the v0.12 surface): `file_graph` +
  `no_dangling` over both `from_content` and `derive_target` edges,
  `for_each_match`, `cross_file` glob-union `source.files`, markerless
  `ordered_block`, and `generated_file_fresh` mutating mode. All six run silent
  on the clean tree, so the row measures dispatch work, not violation emission.
  Wired into `Scenario::all()` + the publish-grade `bench-record.yml` matrix
  (now S1-S14 = 112 cells). The git-extract kinds are excluded (they need a
  `since:` diff base the one-commit synthetic tree doesn't model — their
  git-dispatch cost is the S8 git-tree class).
- **`for_each_match` — the in-file line quantifier (new rule kind).** For each
  line matching `select:` (a regex), the line must satisfy the nested `require:`
  predicates: `matches` (the line matches **all** listed regexes), `forbid` (the
  line matches **none**), and `equal` (the listed named `select` captures are all
  **equal**). The dual of `ordered_block`'s `select:` — where `ordered_block`
  *orders* selected lines, this asserts a *conjunction of predicates* over each.
  It closes two shapes no `file_content_*` kind can express, both reproduced
  against the shipped binary first: a **per-line changelog grammar** ("every
  `* ` entry must *also* end with a linked PR ref" — `file_content_matches` is
  existence, `is_match`, not a per-line conjunction) and **intra-line capture
  equality** ("the display number must equal the `/pull/` URL number" — the Rust
  `regex` engine is RE2, with no backreferences). One violation per offending
  line; lines `select` does not match are ignored. Per-file (the `PerFileRule`
  fast path). Rule-kind count +1.
- **`file_graph` — the file-dependency-graph rule kind (new).**
  Assembles the repo's *file → file* reference graph from path-based
  edges and asserts a global structural property no 1-level kind can
  express. `nodes:` (a glob) selects the graph's files; the `edges:` block
  takes either `from_content` (extract one reference per match — reusing the
  `extract:` one-of: toml/json/yaml JSONPath, `lines`, or `regex` capture
  group 1 — then `resolve` it to a path, `relative_to_file` /
  `relative_to_repo_root`) or `derive_target` (a name template, source →
  generated file). Ships all five `require:` modes: `forbidden_edges` (the
  whole-repo layering firewall — no edge whose source matches `from` and
  whose resolved target matches `to`, e.g. domain code must not import infra;
  `import_gate` is the cheap per-file version), `acyclic` (no dependency
  cycle among the nodes, each reported once as a rotation-canonical path
  list — the clearest capability gap, since no current kind detects cycles),
  `no_dangling` (every path-shaped edge must resolve to a path that exists on
  disk — the generic doc-cross-link / `markdown_paths_resolve` integrity
  check), `no_orphans` (no node is unreferenced by another node, except those
  matching a `roots:` glob — the registry / staging orphan detector), and
  `fresh` (over `derive_target` edges: the generated file must embed the
  source's current content-hash, captured by a `marker` regex — the
  alint-native, content-hash form of `make gen && git diff`, no generator
  run and no mtime, reusing the `pair_hash` digest machinery). Bare module
  names, absolute paths, URLs, and computed references are dropped, not
  mis-resolved — nodes stay path-based (module-*name* resolution is the
  package-graph non-goal). Pure-parse and extraction-based: it never shells
  out, so it stays out of the spawning-rule trust gate. The #1 demand-ranked
  new kind of the 111-repo v0.12 case study (257 file-reference-graph edge
  sources across 56 repos).
- **`cross_file` — the unified cross-file value-relation kind (new).**
  One kind, parameterised by `relation:`, over the shared `extract:`
  (`crate::extract`) and `normalize:`. `relation: equals` (default) is the
  released `cross_file_value_equals` — now a **byte-compatible alias**
  (`relation` defaults to `equals`, so every existing config is unchanged).
  The new set relations compare the source's extracted set `S` to each
  target's set `T`: `subset` (`S ⊆ T`, a singleton `S` = membership — e.g.
  pnpm catalog refs ⊆ catalog keys), `superset` (`S ⊇ T` — a registry covers
  every use), and `set_equals` (`S == T` — rust `features` ↔ unstable-book,
  protobuf binding parity). The engine reports relation-specific diffs
  (`missing: {…}` / `extra: {…}`), not a generic "values differ". Realises
  architecture-synthesis primitive A and the whole `value_set_membership`
  demand in one kind; `requires_full_index` cross-file dispatch. Two further
  relations have a different shape (validated at load): `identical` compares
  whole files byte-for-byte (no `extract`; optional `skip_header_lines` to
  ignore a differing license/header — the README-mirror / `files_equal`
  case), and `resolves` checks that each path the source extracts exists on
  disk (`source.extract`, no `targets` — the forward half of
  `registry_paths_resolve`, which keeps its richer `base`/`must_contain`/
  `orphans` ergonomics). Only the `normalize:` promotion remains.
- **`git_commit_subject_matches` — subject-line grammar for the commit
  family (new).** Each commit's subject (the first line of its message)
  must match a `matches:` regex — the subject-grammar member alongside
  `git_commit_signed_off` / `_no_fixup` / `_author_allowlist` /
  `_gpg_signed`. The regex is anchored to the subject alone (so `^…$`
  describes the first line exactly), unlike `git_commit_message`'s
  whole-message `pattern:`; use that rule's `subject_max_length:` for a
  length cap. Enforces conventions like go / Gerrit's `pkg/path:
  lowercase summary`, node's `subsystem: description`, or
  conventional-commit types. Shares the family's `since:` /
  `include_merges:` semantics (HEAD-only when unset, `<since>..HEAD` when
  set), `{{env.X}}` interpolation, silent-outside-a-repo posture, and the
  shallow-clone hint on an unresolvable `since:`.
- **`changeset_requires_path` — "did you add a changelog entry?" diff gate
  (new).** The `<since>...HEAD` diff must ADD (git status `A`) at least one
  path matching `add_glob:` — the changeset / changelog-per-PR convention
  (prettier `changelog_unreleased/`, cpython `Misc/NEWS.d/next/`, pnpm
  `.changeset/*.md`). `since:` (the base ref) is required; an optional
  `when_changed:` gates the requirement on some other glob having changed (no
  changelog demanded for a docs-only PR), and with no gate any non-empty
  changeset triggers it. Builds on the same `<since>...HEAD` three-dot
  (merge-base) diff as `alint check --changed`, via a new
  `collect_changed_paths_filtered` git helper (`--diff-filter=A`). Silent
  no-op outside a git repo or when nothing relevant changed; a `since:` that
  fails to resolve hard-fails with a shallow-clone hint. Check-only.
- **`pair_changed_together` — the co-change gate (new).** If the
  `<since>...HEAD` diff changes any path matching `if_changed:`, at least one
  path matching `then_changed:` must change in the same range (rust's
  `rustdoc-json-types` `FORMAT_VERSION` must bump when the format struct
  changes; "`version.txt` and the lockfile change together" release guards).
  Both globs and `since:` (the base ref) are required. Directional — a
  `then_changed`-only change never triggers it; swap the globs in a second
  rule for a bidirectional pact. The `changeset_requires_path` sibling, on the
  same merge-base diff as `alint check --changed`. Silent no-op outside a git
  repo or when `if_changed` didn't change; a `since:` that fails to resolve
  hard-fails with a shallow-clone hint. Check-only.
- **`generated_file_fresh` — mutating / in-place mode (`outputs:`).** The
  shipped kind only diffed a generator's *stdout* against one committed
  `file:`; the common real pattern is a generator that rewrites files **in
  place**, after which CI runs `git diff --exit-code` (redis `make
  commands.def`, ruff `cargo dev generate-all`, pytorch `generate_ci_workflows`,
  symfony, postgres, protobuf, cpython `make regen-all`, … — the #1 residual of
  the post-build coverage re-analysis, ≈23 repos). The new `outputs:` field (a
  glob or list) selects this mode: alint **snapshots** the outputs, runs the
  generator, **diffs** (flagging each stale / newly-created / removed file), and
  **restores the snapshot** — so `alint check` leaves the tree byte-identical
  (the restore is panic-safe via a Drop guard). It preserves the kind's "never
  run codegen as a build step" non-goal: alint *verifies* freshness, it never
  *performs* it. Exactly one of `file:` / `outputs:` is required; `command:` /
  `workdir:` / `normalize:` / `timeout:` and the spawn trust-gate are shared
  with the stdout mode. A *mode* on the existing kind — rule count unchanged.
- **Selector tuning — `file_is_ascii` `allow:` + `ordered_block` `select:`
  (the C-tuning cluster from the v0.12 study; no new rule kinds).**
  `file_is_ascii` gains `allow:` — a list of permitted non-ASCII codepoints,
  each a single character (`"ö"`), a `U+XXXX` codepoint, or a `U+XXXX-U+YYYY`
  inclusive range — so a tree that keeps source ASCII can still allow `ö` in
  "Björn" (curl-proven; recurs across llvm / vscode / elixir). With `allow:`
  the file is decoded as UTF-8 and checked per character; without it the
  strict byte-level fast path is unchanged. `ordered_block` gains `select:` —
  a regex that restricts the sortable entries to matching lines, so other
  lines inside the block (comments, group headers) pass through untouched
  (the sectioned / keep-sorted-subset shape; rubocop / gradle / pandas).
- **`select:` on `for_each_dir` / `for_each_file` / `every_matching_has` now
  takes a list with `!`-excludes.** Previously a single glob; now a single
  glob or a list where `!`-prefixed patterns are excludes (e.g. `select:
  ["packages/*", "!packages/internal"]` — iterate every package except
  `internal`). Completes the C-tuning selector cluster (eslint's
  include/exclude shape); a `select:` with no include pattern is a load-time
  error. Single-glob configs are unchanged.
- **`cross_file` gains a glob-union `source.files:`.** The `source` may now be
  `{ files: <glob>, extract }` instead of `{ file, extract }`: every file the
  glob matches is read and its extracted values are **unioned into one set**,
  for the set relations (`subset` / `superset` / `set_equals`). This expresses
  symbol-set / cross-language parity — "the union of `*hl-X*` across every
  `runtime/doc/*.txt` must equal the `default link X` set in `src/highlight.c`"
  (vim hlgroups), protobuf cross-language binding parity, error-code completeness
  (react/neovim), JS barrel `index.js` re-export sets. A `files:` source is
  rejected with a non-set relation (it would yield many values), and `file` /
  `files` are mutually exclusive. Reuses the shipped set relations — no new
  dispatch class, rule-kind count unchanged.
- **`file_graph` `derive_target` edges now compose with `require: no_dangling`.**
  `derive_target` (a `from`→`to` regex-capture name template) was coupled to the
  `fresh` codegen-freshness mode; it now also works under `no_dangling`, where the
  **derived sibling must merely exist** (no content hash). This expresses
  "capture a name and require a rewritten sibling" — every `licenses/X-LICENSE.txt`
  needs an `X-NOTICE.txt` (elasticsearch `DependencyLicensesTask`), a `.proto` its
  `.pb.go`, a `.d.ts` its sibling. The derived edge is computed purely from the
  node *path* (no file read); a node the `from` regex doesn't match has no edge.
  This is the strict superset of a captured `pair.partner` for existence-pairing.
  The content-graph modes (`acyclic` / `no_orphans` / `forbidden_edges`) keep
  rejecting `derive_target` (a 1:1 name map isn't a reference graph). No new
  dispatch class, rule-kind count unchanged.
- **`ordered_block` markers are now optional.** `start` and `end` were both
  required; either may now be omitted — drop `end` to sort from `start` to EOF,
  drop both to sort the **whole file** (the markerless "this file is one sorted
  list" form: dictionaries, allow-lists, a fully-sorted `CODEOWNERS`). A block
  with an absent `end` runs to EOF by design (never reported `unclosed`); the
  fully-delimited start+end contract — including the `unclosed` check — is
  unchanged, so every existing config behaves identically. Reproduce-first: the
  v0.12 case study found ~7 repos whose whole-file sorted lists had no marker
  pair to hang the rule on.
- **`php@v1` bundled ruleset — PHP / Composer baseline (new).** The PHP
  ecosystem was the one with first-class corpus demand
  (composer/laravel/symfony/guzzle/phpstan) and no bundled ruleset (rust / node
  / python / go / java / dotnet all have one). `alint://bundled/php@v1` is gated
  with `when: facts.has_php` (any `composer.json`, so it is a silent no-op in
  non-PHP repos) and composed entirely of existing kinds — no engine change. Its
  heart is the **"Composer-fatals" invariants**: `registry_paths_resolve` checks
  that every `composer.json` `autoload.psr-4` directory, `autoload.files` entry,
  and `bin` script resolves on disk (Composer aborts at install/autoload time
  otherwise — the checks laravel and phpstan hand-roll), plus a `name`-format
  structured-query check and a no-committed-`vendor/` guard. 6 rules, non-blocking
  levels; bundled-ruleset count 21 → 22.
- **Structured-query JSON parsing is now JSONC-tolerant.** A `.json` file with
  `//` or `/* … */` comments or trailing commas (`tsconfig.json`,
  `.vscode/*.json`, and other JS/TS-ecosystem files that use a `.json`
  extension for JSONC content) now parses — so `json_path_*`, the `json:`
  extract (`cross_file` / `registry_paths_resolve`), and `json_schema_passes`
  work on them (astro, TypeScript, nix). Strict JSON is tried first and is
  byte-identical (plain JSON pays nothing); only on failure is a comment- and
  trailing-comma-stripped retry attempted (string-aware, so markers inside
  string values are preserved). A genuinely-malformed document still fails and
  reports the strict parser's error.
- **`unique_by` gains `case_insensitive:`.** When `true` the rendered `key` is
  folded to lowercase before grouping, so files that collide only under
  case-folding (`README.md` vs `readme.md`) are flagged — the
  case-insensitive-filesystem hazard (Windows / macOS; tensorflow's
  `full.bats` Windows-dup check, recurs in git). Default `false` keeps the
  exact-key behaviour.
- **`cross_file` `normalize:` promotion — `semver-minor` + composable lists.**
  `normalize:` now accepts an ordered list of transforms applied
  left-to-right (`normalize: [trim, semver-minor]`), not just a single
  transform, and adds `semver-minor` — the leading `MAJOR.MINOR` band, taking
  each token's leading digits with a non-digit prefix stripped. So `4.36-dev`,
  `4.36.0`, `pnpm@11.3.0` (→ `11.3`) and `>=22.13` all reconcile to one band
  (the protobuf / pnpm version-format case from the v0.12 study). `semver-major`
  keeps its exact released behaviour, and every existing scalar `normalize:`
  config — including `cross_file_value_equals` — is byte-compatible.
  `strip_prefix` / `strip_suffix` / `casefold` are deferred (not corpus-proven;
  `semver-minor`'s non-digit strip already covers the prefix cases).
- **`cross_file` `whole_file: {}` extract source.** A new `extract` source
  (alongside `toml`/`json`/`yaml`/`lines`/`regex`) that yields the entire file
  content as one value, so the value relations can compare whole-file content —
  e.g. `equals` between a `LICENSE` and a `LICENSE-MIT` copy — without dropping
  to `identical` (which forbids `extract` and `normalize`). Unlike the other
  sources, a `whole_file` value is compared verbatim: the non-literal skip
  (which drops interpolated *paths*) does not apply, so content embedding
  `${…}` / `{{…}}` is still compared. `whole_file` honours `normalize`. No new
  rule kind — an extract option on the existing cross-file kinds; rule count
  unchanged.
- **`import_gate` `language:` presets for Scala, Java, Dart, and Nix.**
  Joins the existing go/python/rust/js presets, so these ecosystems get
  a built-in import-line pattern instead of a hand-written
  `import_pattern` (the form spark / flutter / nixpkgs used). The Nix
  preset targets the `import` builtin (`import <nixpkgs>` /
  `import ./mod.nix`); the NixOS `imports = [ ... ]` module-list form
  still needs `language: generic` plus a custom pattern.

### Changed

- **`generated_file_fresh` reference clarifies it is stdout-only.** The
  declared generator must print canonical content to stdout; most real
  codegen rewrites files in place, for which `command_idempotent` (the
  tool's own `--check` mode) is the broadly-applicable form.
  Schema / reference text only; no behavior change.

### Fixed

- **The pass count ("All N rule(s) passed") now includes silently-passing
  per-file rules.** A per-file rule (`for_each_match`, `ordered_block`,
  `file_content_matches`, …) that ran over matching files and found nothing was
  dropped from the report, so `alint check` printed "All 0 rule(s) passed" (and
  JSON `passing_rules: 0`) even though the rule passed — only cross-file passing
  rules were counted. Per-file passing rules are now emitted as passing results,
  matching the cross-file path. Cosmetic / observability only — exit codes,
  violations, and the failing-rule report were always correct; the v0.12
  per-file kinds just made the miscount far more visible.
- **`pair_hash` `sums-line` now accepts a path-first manifest.** The
  parser handled only the coreutils / go-`.sum` `<hex>  <path>` order;
  the Go FIPS snapshot manifest (`lib/fips140/fips140.sum`) writes
  `<path> <hex>` (path-first), which silently never matched. The digest
  token is now identified by its shape (the algorithm fixes the hex
  length), so either order parses; an ambiguous line still assumes the
  hex-first default. No new option.
- **`no_merge_conflict_markers` false-fired on reST/Markdown setext
  headings.** A bare `=======` line is now treated as a conflict
  separator only when the file also carries an unambiguous anchor
  marker (`<<<<<<< ` / `>>>>>>> ` / `||||||| `, each followed by a ref
  that never appears in prose). On its own, a seven-character `=======`
  is identical to a setext H1 underline — "Changes" and "Git tag" are
  both exactly seven characters — and a real conflict always carries a
  `<<<<<<<` start anyway. This removes the need to exclude `docs/**`
  from the rule (flask, django, and other reST/MD-heavy repos hit it).
- **`import_gate language: js` matched `import(…)` inside comments.**
  The `js` preset's pattern is unanchored (it must catch dynamic
  `import("m")` and `require("m")` anywhere on a line), so it also
  matched a JSDoc type annotation like `@typedef {import("../x")}` — a
  type-only reference, not a real import (eslint, svelte). The preset
  now blanks `//` and `/* … */` comments (preserving string literals,
  since the import target is itself a quoted string, and newlines, so
  violation line numbers are unchanged) before matching. The anchored
  presets (`go`/`python`/`rust`/`scala`/`java`/`dart`/`nix`) can't
  match a comment line, so they are unaffected, as is a custom
  `import_pattern`.
- **`compliance/apache-2@v1` and `apache/governance@v1` over-fired on
  CNCF / codegen-heavy repos.** The source-header rules now accept the
  modern `SPDX-License-Identifier: Apache-2.0` form alongside the ASF
  short and long-preamble forms (CNCF projects such as helm, istio, and
  kubernetes carry an SPDX id rather than the ASF appendix text), and
  they exclude vendored trees (`third_party/`, `3rdparty/`) and
  generated-by-naming source (`*.pb.go`, `*_grpc.pb.go`,
  `zz_generated.*.go`, `*_pb2.py`, `*.pb.cc`, `*.pb.swift`,
  `*.generated.*`, and similar). `apache-gov-no-binaries-in-source`
  likewise excludes `third_party/`. Every large ASF/CNCF repo in the
  case-study corpus (airflow, helm, istio, kubernetes, tensorflow) hit
  one of these. This is a pure false-positive reduction, so the bundles
  stay `@v1`: a tree that passed before still passes.
- **`cross_file` build-validates its config (was silent / late).** A malformed
  `extract.regex` is now a clean config error at load — previously it built fine
  and surfaced as an error-level *eval-time* violation reading like a content
  failure. And options the chosen relation ignores are now rejected at build:
  `skip_header_lines` on any relation but `identical`, and `normalize` on
  `identical` / `resolves`. A config that previously loaded then misbehaved now
  fails fast with a clear message.
- **`ordered_block` flags an abandoned block in fully-delimited mode.** With both
  markers set, a second `start` before the `end` now emits an "unclosed"
  violation for the unterminated first block instead of silently swallowing it.
  (Start-only / markerless mode is unchanged — a repeated `start` there is the
  intended section delimiter.)
- **`file_graph` no longer double-reports a repeated edge.** `no_dangling` and
  `forbidden_edges` now emit one violation per *distinct* dangling/forbidden
  target (a node referencing the same target twice produced two identical
  violations), matching `acyclic` / `no_orphans`. A violation-count change.

### Security

- **A git rule's `since:` could write or truncate an arbitrary file (argument
  injection).** The commit-range / diff helpers interpolated the
  config-controlled `since:` (a revision ref) into a `git log` / `git diff`
  argument with **no `--end-of-options` separator**, so a `since:` beginning with
  `-` — e.g. `since: "--output=/path"` — was parsed by git as an *option*, letting
  it **create or truncate an arbitrary out-of-tree file** (git-log content via
  `git log --output=`; a 0-byte truncation via `git diff`). It is reachable from
  an untrusted `extends:`'d ruleset — the git kinds correctly aren't
  process-spawn gated (git is a fixed program), but the *argument* was
  unvalidated, so the spawn gate gave no cover. The git invocations now pass
  `--end-of-options` before the range, and a `since:` / `base` starting with `-`
  is rejected with a clear error. **Unlike the other v0.12 Security entries, this
  one affected released versions** — `git_commit_message`'s `since:` range mode
  shipped in v0.9.21 (and `git_commit_signed_off` / `_no_fixup` /
  `_author_allowlist` via the shared range helper); the v0.12
  `changeset_requires_path` / `pair_changed_together` diff kinds were affected on
  `[Unreleased]`.

- **`file_exists` with `respect_gitignore: false` no longer stats outside the
  repo root.** The per-rule gitignore-bypass (the bazel-style "tracked AND
  gitignored" case) stat'd a literal `paths:` entry on disk directly, so an
  absolute or `../` literal (e.g. `paths: "/etc/passwd"`) probed a host path — an
  *existence* oracle reachable from an `extends:`'d ruleset. The literal is now
  confined via `normalize_confined` before the stat (an escaping literal is
  treated as absent), matching `structured_path` / `for_each_dir`. Pre-existing
  since v0.9.16; existence-only (no content read); pre-release hardening — no
  released version is affected.

- **Walker — a committed symlink whose target escapes the repo root is no longer
  followed into the file index.** With `follow_links(true)` the walker indexed a
  symlink under its in-tree path while reading the link's *target*, so a symlink
  like `link -> /etc/passwd` committed to a linted tree was indexed as an in-tree
  file — and a symlink to a *directory* had its out-of-tree children descended
  and indexed too — letting any content rule read out-of-tree bytes. This is the
  untrusted-repo-*content* half of the path-confinement threat (it bites CI that
  lints fork PRs with a trusted config). The walker now prunes any symlink whose
  canonicalized target is not under the canonicalized repo root
  (`build_walk_builder`'s `filter_entry`); pruning a symlink-dir also stops
  descent, so its children are never indexed. In-tree symlinks are still followed
  (byte-compatible for trees without out-of-root symlinks); broken symlinks are
  pruned. Pre-existing since v0.1; pre-release hardening — no released version is
  affected. See `docs/design/v0.12/path-confinement.md`.

- **Path confinement — config-derived paths can no longer read or resolve
  outside the repo root.** Several rule kinds turn a config-author-controlled
  string into a filesystem path that is then read or resolved: `file_graph`
  (`fresh` reads the `derive_target` output; `derive_target`/`from_content`
  resolve edges), `cross_file` (`identical` + the value relations read
  `source.file` / `targets[].file`; `resolves` resolves extracted paths),
  `registry_paths_resolve` (resolves each declared entry **and reads the
  `source:` registry file**), `json_schema_passes` (reads `schema_path:`),
  `pair_hash` (reads `target:`), and `generated_file_fresh` (reads `file:`).
  The per-kind `normalise()` helpers had two escapes (both reproduced): an
  **absolute** path was read via `root.join(absolute)` (Rust discards `root`),
  giving an **out-of-tree read oracle** reachable from an untrusted `extends:`'d
  ruleset (these kinds are not spawn-gated); and **`..` double-dot
  cancellation** (`../../x`) collapsed to an in-tree path that a first-component
  escape check missed, resolving references to the wrong file. A single
  `crate::pathsafe::normalize_confined` now rejects absolute paths and every
  `..` escape (caught during the walk, not after); every read site refuses to
  read and emits an "escapes the repo root" violation, and every resolve site
  treats the path as unresolved. All config-derived path reads route through the
  one confiner — including `registry_paths_resolve`'s `source:`,
  `json_schema_passes`'s `schema_path:`, and `pair_hash`'s `target:`, which a
  pre-release re-audit found were still unconfined (each leaked an
  existence/size/parse oracle in its error message); `generated_file_fresh`'s
  `file:` is confined too as defense-in-depth (the kind is spawn-gated). Each
  newly-confined site has a "fires and is never read" regression test.
  Pre-release hardening — no released version is affected (all v0.12 work is
  `[Unreleased]`). Design: `docs/design/v0.12/path-confinement.md`.

## [0.11.1] - 2026-05-31

A JetBrains-plugin patch that clears JetBrains Marketplace moderation.
The v0.11.0 plugin read its own version through platform plugin-lookup
APIs (`PluginManagerCore` / `PluginManager`) that the Marketplace's
validator rejects as internal-API usage even though
`intellij-plugin-verifier` passes them; the version is now stamped into
a build-time classpath resource with zero platform-API surface, and a
new `verifyNoMarketplaceDeniedApis` bytecode gate fails the build on any
denied API so Marketplace moderation is no longer the first detector.
Linter behavior, the DSL, the CLI, and every other distribution channel
are unchanged.

### Fixed

- **JetBrains plugin:** stop calling platform plugin-lookup APIs to read
  the running plugin's own version. Marketplace validation rejects both
  `PluginManagerCore.getPlugin(PluginId)` and
  `PluginManager.getPluginByClass(Class)` as internal-API usage even
  though neither is `@ApiStatus.Internal` in released-IDE bytecode. The
  version is now stamped into a build-time classpath resource
  (`alint-lsp/version.txt` via `generateVersionResource` in
  `build.gradle.kts`) that `AlintNotifier.pluginVersion()` reads with
  its own classloader — zero platform-API surface. A
  `PluginVersionResourceTest` JUnit test locks the wiring (cross-checks
  the embedded content against the gradle build's version). See
  <https://plugins.jetbrains.com/docs/intellij/api-internal.html>.

### CI / pipeline

- **JetBrains plugin — Marketplace-rejection gate.** Added a
  `verifyNoMarketplaceDeniedApis` gradle task that scans the built
  jar's constant pool for an explicit deny-list of platform-internal
  class FQNs (initially `com/intellij/ide/plugins/PluginManagerCore`
  and `com/intellij/ide/plugins/PluginManager` — both classes'
  plugin-lookup methods are rejected by Marketplace moderation despite
  not being annotated `@ApiStatus.Internal` in IDE bytecode) and fails
  the build with a pointer to the public alternative. Wired as a
  `buildPlugin` finalizer so every path — local `./gradlew
  buildPlugin`, CI `./gradlew buildPlugin verifyPlugin`, AND the
  release-job `./gradlew publishPlugin` — runs it. Also opted the
  existing `verifyPlugin` task's `failureLevel` into the broader set of
  Marketplace-rejection-worthy problem classes (`INTERNAL_API_USAGES`,
  `OVERRIDE_ONLY_API_USAGES`, `NON_EXTENDABLE_API_USAGES`,
  `SCHEDULED_FOR_REMOVAL_API_USAGES`, `PLUGIN_STRUCTURE_WARNINGS`,
  `MISSING_DEPENDENCIES`). Closes the gap that let the v0.11.0
  `PluginManagerCore` call ship — Marketplace moderation should no
  longer be the first detector of this class of issue.

## [0.11.0] - 2026-05-28

The LSP + editor-integration release. Ships the `alint lsp` language
server (the new `alint-lsp` crate) with in-editor diagnostics,
hover-to-explain, and apply-fix quick actions, wired up across VS Code,
JetBrains, and Zed extensions plus Neovim / Sublime / Emacs / Helix
configs. Alongside it lands the v0.9.21-derived DSL polish: per-rule
`scope_filter.changed_since:` (lint only the files a PR touched), the
`git_commit_signed_off` / `_no_fixup` / `_author_allowlist` /
`_gpg_signed` commit-validation family, `{{env.X}}` interpolation across
config fields (with `env.X` in `when:` expressions), and an
informational-notes channel surfaced via `--show-notes`.

### Changed

- **LSP hardening (post-review).** The language server now: reloads on
  `.alint.yml` changes via `didChangeWatchedFiles`; surfaces tree-level
  findings (e.g. a missing required file) and config-load errors as
  diagnostics on `.alint.yml` instead of dropping/logging them;
  preserves cross-file diagnostics while typing (only per-file findings
  are replaced on each edit, the rest refresh on save); caches evaluated
  `facts:` on the file index so per-keystroke re-evaluation doesn't
  re-scan the tree (`Engine::run_for_file`); and honors the client's
  code-action `only` filter. Adds `Engine::is_per_file`.

### Added

- **Tier-2 editor configs (Neovim, Sublime Text, Emacs) + honorable
  mentions (Helix, Eclipse).** Config-only integrations that point each
  editor's LSP client at `alint lsp`: `editors/nvim/` (an `lsp/alint.lua`
  for Neovim 0.11+ / nvim-lspconfig), `editors/sublime/` (an `LSP`
  package client config), `editors/emacs/` (an `alint.el` Eglot
  registration + lsp-mode snippet), `editors/helix/` (a `languages.toml`
  snippet), and `editors/eclipse/` (LSP4E setup docs). Each documents
  the per-language attachment limitation (these clients have no
  "all files" wildcard). No packaged artifacts/release jobs — Neovim is
  upstreamed to nvim-lspconfig; the rest are documented snippets.
- **Zed extension (`editors/zed/`).** A Rust→wasm extension
  (`zed_extension_api`) that launches `alint lsp` for Zed, with the
  same binary resolution (settings → PATH → managed GitHub download).
  Completes the Tier-1 editor set (VS Code + JetBrains + Zed).
  Publishing is a manual PR to `zed-industries/extensions`. Note: Zed
  attaches language servers per-language, so the extension registers a
  broad common-language set (no all-files wildcard).
- **JetBrains plugin (`editors/jetbrains/`).** A Kotlin plugin
  (`intellij-platform-gradle-plugin` 2.x) that registers `alint lsp`
  with LSP4IJ, bringing alint's diagnostics, hover, and quick-fixes to
  the whole JetBrains suite (IDEA, PyCharm, GoLand, WebStorm, RustRover,
  CLion, Rider, Android Studio). Binary resolution mirrors the VS Code
  extension (`alint.path` setting → PATH → opt-in SHA-256-verified
  download). A tag-gated `publish-jetbrains` release job runs
  `gradle publishPlugin` to the JetBrains Marketplace
  (`JETBRAINS_MARKETPLACE_TOKEN` + signing secrets).
- **VS Code extension (`editors/vscode/`).** A TypeScript extension
  that launches `alint lsp` as a language server and surfaces alint's
  diagnostics, hover, and quick-fixes natively in VS Code (and forks
  via Open VSX). Resolves the `alint` binary from the `alint.path`
  setting, then `PATH`, then an opt-in download of the matching release
  (SHA-256 verified, mirroring the npm shim). A tag-gated
  `publish-vscode` release job publishes to both the VS Code
  Marketplace and Open VSX (`VSCE_PAT` / `OVSX_PAT` secrets).

- **`alint lsp` language server.** A new `alint-lsp` crate and `alint
  lsp` subcommand speak the Language Server Protocol over stdio for
  editor integrations. Open and save run the full `Engine::run` over
  the workspace (cross-file rules included); every (debounced) edit
  re-runs only the per-file rules for the changed file against the
  editor's in-memory bytes via the single-file hot path, so
  per-keystroke feedback stays cheap. Violations map to LSP diagnostics
  keyed by file (full-document sync). Hovering a violation shows the
  rule id, severity, message, and `policy_url` (as a link); the
  `policy_url` is also attached to each diagnostic so editors link it
  from the Problems panel. A violation whose rule declares a fixer
  offers an "Apply fix" quick-fix code action that returns a
  `WorkspaceEdit` (applied to the buffer, undoable) — content fixes as
  a full-document edit, file create/delete/rename as resource
  operations. (The "add rule to ignore" action is still deferred.)
- **`Fixer::fix_edit` → `FixEdit`.** A non-writing sibling of
  `Fixer::apply` that expresses a fix as data (`SetContent` /
  `CreateFile` / `DeleteFile` / `RenameFile`) so a caller can build an
  editor edit instead of mutating the filesystem. Powers the LSP
  "Apply fix" action above; `alint fix`'s disk-writing path is
  unchanged.
- **`Engine::run_for_file` — single-file re-evaluation.** Evaluates
  only the per-file rules in scope for one file, using caller-supplied
  bytes, at a cost proportional to that file rather than the whole
  tree. Cross-file rules are intentionally skipped. Returns the new
  `Error::FileNotInIndex` when the path is excluded from the walked
  tree. Powers the LSP change hot path above.
- **Informational notes channel + `--show-notes`.** Rules can now
  surface non-violation findings: `registry_paths_resolve` and
  `cross_file_value_equals` report the non-literal (interpolated /
  computed) entries they skip rather than silently dropping them.
  `alint check` prints a one-line note count on stderr by default
  (stdout stays clean) and lists them in full with `--show-notes`;
  the `json` output carries a per-result `notes` array (omitted when
  empty). Notes never affect pass/fail or the exit code.
- **`git_no_denied_paths` gains an optional `since:` ref** — when set,
  only denied paths changed in the `<since>...HEAD` diff are flagged
  (PR-scoped: catches a secret added in a PR even if HEAD still tracks
  an older one). Accepts the `{{env.X}}` interpolation; a bad ref
  hard-fails with a shallow-clone hint. Completes the diff-scoping
  pair with `scope_filter.changed_since:`.
- **`scope_filter.changed_since: <git-ref>`** — narrow a per-file rule
  to files in the `<ref>...HEAD` diff (the merge-base diff `alint check
  --changed` uses), so a rule can grandfather pre-existing files and
  fire only on what a PR touched. AND-composes with `has_ancestor:`;
  accepts the `{{env.X}}` interpolation; resolved once per run and
  cached on the file index. Matches nothing outside a git repo
  (silent); an unresolvable ref inside a repo hard-fails with a
  shallow-clone hint.
- **`git_commit_signed_off` rule kind** — assert every commit in scope
  carries a DCO `Signed-off-by:` trailer (the first of the v0.11
  commit-validation family). Takes `since:` + `include_merges:` like
  `git_commit_message`: unset checks HEAD, set checks `<since>..HEAD`
  (PR-CI shape). Defaults to the canonical DCO pattern; override
  `pattern:` for a stricter form. Silent outside a git repo; a bad
  `since:` ref hard-fails with a shallow-clone hint.
- **`git_commit_no_fixup` rule kind** — fail on residual `fixup!` /
  `squash!` / `amend!` commits left in scope (forgot to
  `git rebase --autosquash`). Same `since:` / `include_merges:` shape
  as the rest of the commit-validation family; no configuration knobs.
- **`git_commit_author_allowlist` rule kind** — assert every commit
  author matches an allowed `email_pattern:` and/or `name_pattern:`
  (at least one required; both = AND). For enterprise contributor-
  identity enforcement and catching sock-puppet / compromised-account
  commits. Same `since:` / `include_merges:` shape as the family.
- **`git_commit_gpg_signed` rule kind** — assert every commit has a
  verifying signature (`git verify-commit` exits 0); unsigned or
  unverifiable commits fire. Completes the v0.11 commit-validation
  family. Reflects git's own verdict (trust stays git's job). Same
  `since:` / `include_merges:` shape as the family.
- **Variable interpolation: `{{env.NAME}}` across the config.** Any
  string-typed config value can now reference an environment variable
  — `paths:`, `pattern:`, `since:`, `policy_url:`, `message:`,
  `extends:` URLs, `vars:` values, etc. — resolved once at config
  load. `{{env.NAME | default('fallback')}}` supplies a fallback;
  an unset variable with no default is a load error naming the field.
  Only local config files are interpolated (bundled rulesets are
  static; a remote `extends:` ruleset is never interpolated against
  your environment). A fully-resolved value re-types, so
  `subject_max_length: "{{env.MAX | default('72')}}"` validates as a
  number. alint only claims `env`/`vars`/`ctx` spans (and close typos
  of `env`/`vars`); foreign `{{...}}` templates — Go's `{{json .}}`,
  cookiecutter, etc. — pass through verbatim. See
  [variable interpolation](https://alint.org/docs/concepts/variable-interpolation/).
- **`env.X` namespace in `when:` expressions.** `when: env.CI == "true"`
  gates a rule on an environment variable, alongside the existing
  `facts.` / `vars.` namespaces. Resolved at evaluation time; an unset
  variable is `null` (falsy).

### Deprecated

- **The v0.9.21 POSIX `${VAR}` syntax on `git_commit_message.since:`.**
  It still works this minor (and is still expanded at evaluate time)
  but emits a one-line deprecation warning at config load with the
  canonical `{{env.VAR}}` rewrite shown inline. The `${VAR}` form will
  be removed in v1.0.

### Changed

- Bumped `workspace.dependencies` version pins `0.10.0` → `0.10.2`
  to match `workspace.package.version`. No behavior change — intra-
  workspace deps resolve via path; this only tightens the SemVer
  floor for external consumers pulling a single workspace crate by
  name.

## [0.10.2] - 2026-05-21 (asciinema-demo underline-extension fix)

Targeted follow-up to v0.10.1. The alint.org landing-page demo
rendered `docs:` link underlines extending past the URL text to
the end of the terminal row in `asciinema-player`. Root cause:
the human formatter emitted `\e[4m\e[34m{URL}\e[0m` around each
URL, and the player's `.ap-underline` class extended visually
across the row's trailing cells. Fix: when an OSC 8 hyperlink
wrap is also being emitted around the URL, drop the explicit
`\e[4m` — the OSC 8 already carries the link semantic and the
terminal handles the link affordance itself. The fallback path
(no OSC 8 detected) keeps `\e[4m` as the visual link cue.
Bundled in: a new `ALINT_FORCE_HYPERLINKS=1` env var that lets
screen-recording captures opt into OSC 8 emission even when
stdout isn't a TTY. No schema-version bump; `version: 1`
configs continue to work; safe upgrade for every consumer.

### Changed

- **Human output: `docs:` URLs drop the explicit underline
  escape when emitted alongside an OSC 8 hyperlink** (the OSC 8
  wrap already carries the link semantic; the terminal handles
  the link affordance itself — typically a hover-driven
  underline + pointer cursor). Emitting our own `\e[4m` on top
  caused some renderers — notably `asciinema-player` — to
  extend the underline past the URL text to the end of the
  terminal row, visible on the alint.org landing-page demo.
  Non-OSC-8 terminals (no `supports-hyperlinks` detection)
  keep `\e[4m` as the visual link cue; this is purely a
  conditional swap on the existing `opts.hyperlinks` flag.

### Added

- **`ALINT_FORCE_HYPERLINKS=1` environment variable** — forces
  OSC 8 hyperlink emission even when stdout isn't a TTY. The
  only intended use is screen-recording capture (e.g. the
  asciinema demo build script), where `alint check >
  /tmp/demo-outputs/01.txt` would otherwise see `is_tty=false`
  and skip OSC 8 entirely. Empty / `0` values do NOT force.

## [0.10.1] - 2026-05-20 (read_capped reach extension + post-release CI/docs hygiene)

Small post-release follow-up to v0.10.0. Headline change: the
`crate::io::read_capped` 256 MiB whole-file cap (introduced in
v0.10 for the cross-file / structured rule kinds) now also
covers two pre-existing read sites that pre-dated v0.10 and
were missed by the original sweep — `json_schema_passes` and
`for_each_dir`'s literal-path nested-rule bypass. Over-cap
files now emit a clear "too large to analyze" violation rather
than the previous silent skip. Bundled in: post-release CI
hygiene (bench-record PR-body template refreshed to reflect
S1-S13 + `xtask bench-gate` instead of the obsolete eyeball
checklist; pinned GitHub Actions runner agent bumped 2.332.0
→ 2.334.0 after GitHub's deprecation rotation), and the
Docker `<major>.<minor>` channel example refreshed from
`:0.9` to `:0.10` across the install docs. No schema-version
bump; `version: 1` configs continue to work; safe upgrade
for every consumer.

### Security

- **Whole-file reads bounded — extended reach.** The Phase 3
  v0.10 sweep brought every cross-file / structured rule-level
  whole-file read through `crate::io::read_capped` (256 MiB cap).
  Two pre-existing whole-file read sites that pre-dated v0.10
  were missed by that sweep and now also go through the cap:
  `json_schema_passes` (each matched file + the schema file at
  build time) and `for_each_dir`'s literal-path nested-rule
  bypass (`crates/alint-rules/src/for_each_dir.rs:371`).
  Over-cap files now emit a clear "too large to analyze
  (N bytes; 256 MiB cap)" violation rather than the previous
  silent skip (which masked an OOM-DoS surface on hostile /
  accidental multi-GB candidate files). `version: 1` unchanged.

## [0.10.0] - 2026-05-20 (case-study coverage push: 8 rule kinds + 2 bundled rulesets + pre-release hardening sweep)

The "case-study coverage push" minor. Eight new rule kinds and
two bundled rulesets aggregated from the 30-OSS-repo demand-
validation pass ([`docs/development/launch-evidence.md`](docs/development/launch-evidence.md)),
plus three pre-release security retirements that landed alongside.
Headline shape: cross-file (`registry_paths_resolve` /
`cross_file_value_equals` / `pair_hash`) makes manifest-driven
policy a first-class shape; per-file (`ordered_block` /
`import_gate`) covers the keep-sorted and architectural-import-
firewall patterns five-plus case studies wanted; single-shot
(`generated_file_fresh` / `command_idempotent`) extends the
`command`-rule trust tier to freshness and idempotency gates;
the XML arm of structured-query (`xml_path_equals` /
`xml_path_matches`) reaches the .NET ecosystem (the new
`dotnet@v1` bundled ruleset is the concrete payoff); Apache TLP
governance lands as `apache/governance@v1`. Security hardening
retired three audit-found gaps before tagging: spawn-trust-gate
now covers every spawning rule kind (not just literal
`kind: command`); XML parsing has a 256-deep recursion bound;
every cross-file / structured rule-level whole-file read is
bounded at 256 MiB. No schema-version bump; `version: 1`
configs continue to work.

### Added

- **`registry_paths_resolve` rule kind.** A manifest file
  enumerates path entries (Cargo `[workspace] members`,
  `package.json` `workspaces`, a plain line list, a regex
  capture, etc.); each must resolve to an on-disk artefact.
  `extract` selects the parse: structured-query `toml` / `json` /
  `yaml` (RFC 9535 JSONPath), `lines` (optional `comment`
  prefix), or `regex` (capture group 1). `expect`
  (`any` / `file` / `dir`) and `must_contain` constrain the
  resolved kind; `exclude_query` subtracts entries;
  `entries_are_globs` expands each entry as a glob; non-literal
  entries (interpolation / antiquotation) are skipped, not
  failed. Optional `orphans` adds the reverse-completeness
  check: on-disk artefacts under a `space` glob that no entry
  references (the "new crate not wired into the workspace"
  detector). Cross-file; O(1) per-entry existence via the
  v0.9.5 path index. First rule kind of the v0.10 case-study
  coverage push (top demand: 13 sources). Existing configs are
  unaffected; `version: 1` unchanged.
- **`cross_file_value_equals` rule kind.** A value extracted
  from one authoritative `source` file must equal a value
  extracted from one or more `targets`. `targets` is a
  `{ files: <glob>, extract }` map (one query per glob match —
  the per-file `value_extractor` shape) or a `{ file, extract }`
  list (heterogeneous pins: monorepo version lockstep, toolchain
  pins, SDK bands). `extract` is the same one-of as
  `registry_paths_resolve` (shared `crate::extract`); `normalize`
  (`none` / `trim` / `lower` / `semver-major`) relaxes the
  compare; non-literal values are skipped, not failed;
  `allow_missing_target` controls absent files/values. Cross-file;
  `version: 1` unchanged. Second rule kind of the v0.10
  case-study coverage push (12 demand sources).
- **`ordered_block` rule kind.** The lines between a `start` /
  `end` marker pair must stay sorted, and with `unique: true`
  free of duplicates, under `comparator` (`lexical` /
  `lexical-ci` / `numeric`). The generic form of the per-project
  keep-sorted scripts (protobuf `failure_lists`, sorted
  `.gitignore` / `CODEOWNERS` / dependency lists). Per-file:
  files without the `start` marker are silently fine; markers
  match the trimmed line; blank lines inside a block are
  ignored; one violation per out-of-order block. Detection-only
  (auto-fix is a follow-up); `version: 1` unchanged. Third rule
  kind of the v0.10 case-study coverage push (8 demand sources).
- **`generated_file_fresh` rule kind.** A committed `file` must
  equal the stdout of a declared `command` generator — a
  non-mutating freshness check (protobuf/buf stubs, `pip-compile`
  / `uv` lock outputs, cbindgen headers, cpython generated
  tables). alint does **not** run codegen as a build step: it
  only verifies, capturing the generator's stdout, and never
  writes the working tree — the same trust tier as the existing
  `command` rule, opt-in (no `command:` is a config error).
  Spawn failure, non-zero exit, and a missing committed file are
  each a distinct, clearly-worded violation; `normalize`
  (`none` / `trim` / `final-newline`) absorbs trailing-newline
  churn. Single-shot; opt-in `timeout:` (seconds, default 120 —
  a hung generator yields one timeout violation, not a hung
  run); `version: 1` unchanged. Fourth rule kind of the v0.10
  case-study coverage push (8 demand sources).
- **`import_gate` rule kind.** Forbid imports whose extracted
  target matches a `forbid` regex, within the `paths` scope — an
  architectural import firewall (k8s `staging/` layer
  isolation, airflow core/providers separation, `torch._C`
  private-API gates, prometheus-imports). Matches the *extracted
  import target*, not the raw line (a comment/string mentioning
  the path doesn't fire — the low-false-positive specialisation
  of `file_content_forbidden`). `language` (`go`/`python`/
  `rust`/`js`) supplies a built-in import-line pattern;
  `import_pattern` overrides it (capture group 1 = target;
  required for `generic`); `allow` globs exempt sanctioned
  files. One violation per offending import. Per-file;
  `version: 1` unchanged. Fifth rule kind of the v0.10
  case-study coverage push (5 demand sources).
- **`command_idempotent` rule kind.** Run a user-declared
  formatter/checker in its `--check` (idempotence) mode once:
  exit `0` ⇒ the tree is formatter-clean (silent), non-zero ⇒
  violation(s). The sibling of `generated_file_fresh` (that
  rule diffs a *generator's* captured stdout against a committed
  file; this trusts a *checker's* own `--check` exit code —
  `cargo fmt --check` / `gofmt -l` / `ruff format --check` /
  `prettier --check` / `dprint check` / `eslint --no-fix`).
  alint never runs a mutating formatter and never writes the
  tree. With `files_from` (`stdout`/`stderr`) + optional
  `files_pattern` (capture group 1 = path) the tool's own
  offender list is parsed into one violation per file; a
  non-zero exit is never swallowed into a pass. Single-shot,
  opt-in, trust-gated like `command`; opt-in `timeout:`
  (seconds, default 120 — a hung checker yields one timeout
  violation, not a hung run); `version: 1` unchanged. Sixth
  rule kind of the v0.10 case-study coverage push (5 demand
  sources).
- **`xml_path_equals` + `xml_path_matches` rule kinds.** The XML
  arm of the structured-query family, completing JSON / YAML /
  TOML / **XML** under one RFC 9535 JSONPath query language.
  XML maps to the same value tree via an xmltodict-style
  convention: attributes are `@name` keys, repeated child
  elements become an array (`dependency[*]`), a leaf element
  collapses to its text string, namespaces flatten to the local
  name (Maven `pom.xml`'s default namespace just works), and
  every XML leaf value is a string (quote `equals:`). Same
  option surface as the rest of the family (`paths` / `path` /
  `equals` | `matches` / `if_present`); not-well-formed XML is
  one parse-error violation per file. Demand-validated by spark
  (`pom.xml`) and dotnet/runtime (`.csproj` / `.props` /
  `.targets` at ~2,300-manifest scale); unblocks the planned
  `dotnet@v1` bundled ruleset. New dependency: `roxmltree`
  (MIT/Apache, read-only DOM). Per-file; `version: 1`
  unchanged. The seventh v0.10 case-study-coverage item — and
  the first that adds **two** rule kinds (2 demand sources).
- **`pair_hash` rule kind.** Cross-file: the `algorithm` digest
  (`sha256` default / `sha512`) of every file matching `source`
  must appear in the single `target` file — either as an
  embedded hex substring (`format: contains`, default) or a
  coreutils / go-`.sum`-style `<hex>  <path>` manifest line
  (`format: sums-line`, path token = the source's path; a
  leading `*` binary marker tolerated). One violation per source
  whose digest is absent or mismatched; a missing `target` is
  one violation. Raw bytes are hashed (an integrity pin — a newline
  change flips the verdict); detection-only (alint never
  regenerates the manifest, same posture as `file_hash`). The
  cross-file sibling of `file_hash` (literal-hash pin) and
  `generated_file_fresh`; golang/go `fips140.sum` is the
  canonical, highest-stakes use (k8s + tokio the same shape).
  Reuses the existing `sha2` dependency — no new crate.
  Cross-file; `version: 1` unchanged. The **eighth and final**
  v0.10 rule-kind item (3 demand sources) — the v0.10
  case-study coverage push is now rule-complete; the remaining
  items (#9 `apache/governance@v1`, #10 `dotnet@v1`) are bundled
  rulesets.
- **`apache/governance@v1` bundled ruleset.** A new
  `alint://bundled/apache/governance@v1` ruleset (composed of
  existing rule kinds — **not** a new rule kind; the rule-kind
  count is unchanged) covering the Apache Top-Level Project
  governance / release-artefact baseline that arrow + spark +
  airflow each re-implement by hand: LICENSE + NOTICE (incl. an
  ASF-attribution content check — bare or long form, `warning`
  — not merely existence) + KEYS +
  RAT discipline (source-license headers reusing
  `compliance/apache-2@v1`'s v0.9.18-broadened ASF-preamble
  pattern verbatim; no compiled binaries in the source tree) +
  README + changelog. Eight rules, ids namespaced `apache-gov-*`
  so it is safe to adopt alongside `compliance/apache-2@v1` (the
  governance superset vs. that ruleset's license-redistribution
  focus). Tiered levels (legally load-bearing artefacts `error`,
  release discipline `warning`, nice-to-have `info`); no fact
  gate. 3 demand sources (arrow + spark + airflow converge on
  9 of 12 governance artefacts). The ninth v0.10 case-study-
  coverage item and the first bundled ruleset of the cut;
  bundled-ruleset count 19 → 20.
- **`dotnet@v1` bundled ruleset.** A new
  `alint://bundled/dotnet@v1` ecosystem ruleset (top-level, like
  `rust`/`go`/`java`; composed of existing rule kinds — **not** a
  new rule kind, the rule-kind count is unchanged) for the .NET
  project baseline. Ecosystem-gated via `facts.has_dotnet`
  (`*.sln` / `**/*.csproj` / `.fsproj` / `.vbproj` /
  `global.json`) so it is a silent no-op in non-.NET repos and
  the non-.NET parts of a polyglot monorepo. Seven rules; three
  exercise the now-shipped structured-query family —
  `json_path_matches` on `global.json` (`$.sdk.version`),
  `xml_path_matches` + `xml_path_equals` on `.csproj` /
  `Directory.Packages.props` (SDK-style, `Nullable`, Central
  Package Management) — the concrete payoff that made this item
  depend on the v0.10 `xml_path_*` work. Deliberately does
  **not** require a per-`<PackageReference>` `Version` (CPM
  makes it absent by design — enforcing it would false-positive
  across dotnet/runtime); the structured-query rules are
  `if_present: true` and levels are non-blocking (no `error`)
  given the adopter surface (every `dotnet/*` + every Azure SDK
  + every `microsoft/*` .NET project). 1 demand source
  (dotnet/runtime) but a vast adopter surface. The tenth and
  **final v0.10 case-study-coverage item** — v0.10 is now
  content-complete (8 rule kinds + 2 bundled rulesets);
  bundled-ruleset count 20 → 21.

### Fixed

- **`pair_hash` `sums-line` false "not listed in manifest" on
  `./`-prefixed paths.** The `sums-line` path-token compare
  stripped a coreutils binary-mode `*` marker but not a
  `find`-style `./` prefix, so a manifest line `<hex>  ./path`
  (what `find … -exec sha256sum` and Go checksum tooling emit)
  did not match the source's repo-root-relative index path and
  produced a false "not listed in manifest" violation on a
  correctly-pinned file. Both `*` and a leading `./` are now
  normalised off before the compare. Gap was `[Unreleased]`-only
  (`pair_hash` never reached a release).
- **`registry_paths_resolve` / `cross_file_value_equals`
  silently skipped real literal paths containing `$`, `` ` ``,
  or `(.`.** The shared `is_non_literal` heuristic (which
  decides an entry is a computed/interpolated value to skip
  rather than check) over-matched: a bare `$`, backtick, or
  `(.` — all legal in real filenames — made a literal path get
  silently dropped (never checked, never reported: a false
  negative). Narrowed to the genuine interpolation /
  concatenation markers only: `${`, `$(`, `{{`, `+ `. Such
  entries are now checked. (The skip remains intentionally
  silent — `alint check` has no informational-finding channel;
  visibly surfacing skipped entries is a tracked v0.11 item.)
  Gap was `[Unreleased]`-only.

### Security

- **Spawn trust-gate generalised — closes a `generated_file_fresh`
  code-execution gap.** `alint_dsl::reject_command_rules_in` (the
  `extends:`-resolver gate that refuses process-spawning rules in
  any extended ruleset — local / HTTPS / `alint://bundled/`)
  matched only the literal `kind: command`. `generated_file_fresh`
  (added earlier in `[Unreleased]`) shells out identically but
  was never added to the gate, so an `extends:`'d ruleset could
  declare `kind: generated_file_fresh` with an arbitrary
  `command:` and execute code on every consumer. The gate now
  covers the full set of process-spawning kinds (`command`,
  `generated_file_fresh`, `command_idempotent`) via
  `SPAWNING_RULE_KINDS`, with a regression test asserting each is
  rejected in an extended config. The gap existed only within
  `[Unreleased]` (`generated_file_fresh` never reached a
  release), so no released version is affected.
- **XML parsing recursion bound — closes a deep-nesting
  process-abort (DoS).** The `xml_path_*` arm's
  `element_to_value` recursed once per nesting level with no
  depth limit; `roxmltree`'s default options bound node count
  but not nesting depth (the JSON/YAML/TOML parsers carry
  internal recursion limits the XML arm lacked). A crafted or
  accidental deeply-nested XML file (reachable from any
  passively-linted repo once an `xml_path_*` rule or the
  `dotnet@v1` ruleset, which targets `**/*.csproj`, is active)
  overflowed the stack and aborted the whole `alint` process —
  an unrecoverable abort, not a catchable panic. Now bounded by
  `MAX_XML_DEPTH` (256): past the bound the file yields one
  ordinary parse-error violation ("XML nesting exceeds the
  maximum supported depth"), per-file contained, no abort. (XXE
  / billion-laughs was already closed by `roxmltree`'s
  `allow_dtd: false` default.) The gap existed only within
  `[Unreleased]`, so no released version is affected.
- **Whole-file reads bounded — closes an OOM (DoS).** The
  cross-file / structured rules (`registry_paths_resolve`,
  `cross_file_value_equals`, `pair_hash`,
  `generated_file_fresh`) read a manifest / source / target /
  committed file fully into memory with no size cap, so a
  hostile or accidental multi-GB file in a linted repo OOM-ed
  the run. All such reads now go through
  `crate::io::read_capped` — a `metadata` stat-gate so the
  oversized bytes are never read, refusing anything over
  `MAX_ANALYZE_BYTES` (256 MiB) with a clear "too large to
  analyze" violation (never a silent skip — notably
  `pair_hash`'s per-source read, which previously skipped
  unreadable sources, now flags an over-cap source). Side
  effect: `registry_paths_resolve` / `cross_file_value_equals`
  read bytes + `from_utf8_lossy` instead of `read_to_string`,
  so a non-UTF-8 manifest now surfaces as a parse error rather
  than a read error (still a clear, distinct violation). The
  gap existed only within `[Unreleased]`, so no released
  version is affected.

## [0.9.23] - 2026-05-17 (GitHub Action pinning + release-pipeline hardening)

Distribution + release-reliability release. The headline is the
GitHub Action change: a pinned action ref now pins the installed
binary, so `uses: asamarts/alint@vX.Y.Z` is reproducible with zero
release-time maintenance. Bundled in: the `bump-version.sh`
`Cargo.lock` refresh that recovers the v0.9.22 release-day failure
mode, a cargo-deny supply-chain gate, the generated `/docs/rules/`
index corrected to the canonical 70-rule-kinds figure, and a
GitHub Action provenance hardening. No `.alint.yml` schema,
output-format, or rule-engine changes; safe upgrade for every
consumer except the single Action-pinning edge case called out
under **Changed** below.

### Added

- **cargo-deny license / supply-chain gate (`deny.toml`).** A
  tailored `deny.toml` plus `ci/scripts/deny.sh` enforce the
  dependency graph's license, ban, and source policy. `licenses.allow`
  is the exact permissive set present in the graph (not a blanket
  allow): an LGPL-2.1 dependency that is OR-satisfied by MIT/Apache
  is not separately allowed, and MPL-2.0 is a scoped single-crate
  exception so a future MPL dependency fails the check. Wildcard
  version requirements are banned and duplicate versions warn;
  sources are restricted to crates.io; advisories stay advisory-only
  (mirroring `audit.sh`). Wired as a `deny` job in `ci.yml` and into
  the `release.yml` preflight, so a license or supply-chain
  violation blocks a release. Build-time control only; no effect on
  the published binary or library.

### Changed

- **GitHub Action: a pinned action ref now pins the binary.** When
  `uses: asamarts/alint@vX.Y.Z` is used without a `version:` input,
  the Action now installs the alint release matching that tag
  (`vX.Y.Z`) instead of always installing the latest release. The
  Action already fetched `install.sh` from the pinned ref for
  provenance; the installed binary now follows the same ref, so
  pinning the Action is reproducible with zero release-time
  maintenance. The `version:` input default changed from `latest`
  to empty (unset). **Action required only if** you pinned
  `@vX.Y.Z` but intentionally relied on implicitly getting the
  newest release: set `version: latest` explicitly to keep that
  behaviour. Pinning to a branch/SHA (e.g. `@main`) still installs
  `latest`, and an explicit `version:` (a tag, or `latest`) behaves
  exactly as before.
- **`ci/scripts/bump-version.sh`** now refreshes `Cargo.lock`
  after bumping `[workspace.package].version` (via
  `cargo metadata --offline --format-version 1`), so the workspace
  internal-crate version entries in the lockfile track the bump
  automatically. Previously the lockfile was refreshed as a side
  effect of the next `cargo build` during pre-push preflight; if
  the maintainer didn't notice + stage the change, the release
  pushed a stale `Cargo.lock` that mismatched the bumped
  `Cargo.toml`, and CI's `cargo build --locked` in
  `release-binary.sh` failed at the cross-platform build matrix.
  Caught the hard way on v0.9.22 (`release.yml` run
  `25890555488`); recovered via tag-move after amending the bump
  commit to include `Cargo.lock`.
- **`RELEASING.md`** step 1 description updated to mention the
  `Cargo.lock` refresh; step 4 stage list now includes
  `Cargo.lock` explicitly; new "Why `Cargo.lock` is in the stage
  list" note under step 4 captures the recurrence rationale.

### Fixed

- **Generated `/docs/rules/` index now reports the canonical "70
  rule kinds".** The rules master-index headline counted only the
  60 distinct documented behaviours and excluded the 10 short-name
  aliases, contradicting `/docs/about/`, the README (corrected in
  v0.9.22), the JSON schema, and the alint.org marketing surfaces,
  all of which say 70. The generator (`xtask/src/docs_export.rs`)
  now derives the count as behaviours + aliases, so the page reads
  "70 rule kinds across 13 families (60 distinct rule behaviours
  plus 10 short-name aliases)". Documentation output only; no
  engine or schema change.

### Security

- **GitHub Action: an empty `action_ref` no longer silently falls
  back to `main`.** Previously, if `github.action_ref` resolved
  empty the Action fetched `install.sh` from `main`, silently
  defeating the supply-chain-pin guarantee for a consumer who
  pinned the Action. The fallback to `main` is now restricted to
  the `asamarts/alint` local-checkout self-test path; for any
  external consumer an empty ref is a hard error rather than an
  implicit `main` fetch. Restores the provenance guarantee that
  pinning `uses: asamarts/alint@<ref>` fetches the install script
  from that exact ref. Pairs with the binary-pin change under
  **Changed**.

## [0.9.22] - 2026-05-14 (doc-drift cleanup + prevention automation)

Doc + prevention-automation cleanup release. A 2026-05-14 audit of
the alint and alint.org repos surfaced 10 categories of drift
between them; this release lands the fixes plus the automated guards
(`check-workspace-dep-floors.sh`, extended `check-version-pins.sh`,
`coverage_audit_readme_claims`, roadmap-generator with section
markers) that prevent recurrence. No schema, output-format, or
rule-engine changes; safe upgrade for every consumer. Bundled in:
the GitHub Action manifest's display name is renamed (the bare
`alint` collides with an existing GitHub user/org account, which
Marketplace rejects) so the Action can be published to the GitHub
Marketplace on this release.

### Changed

- **GitHub Action manifest `name:`** renamed from `alint` to
  `alint - A Repo Linter` to satisfy GitHub Marketplace's
  display-name uniqueness rule (the bare `alint` name collides
  with an existing GitHub user/org account, which Marketplace
  rejects). Repo slug, install paths, and the
  `uses: asamarts/alint@vX.Y.Z` workflow syntax are unchanged;
  the rename only affects the Marketplace listing title and the
  Action step label in workflow run UIs.
- **Rule-count claim** in `README.md` corrected to 70 rule kinds
  across 13 families (was 60 / 13). The 70 figure matches the
  `all_kinds.yaml` audit fixture and `coverage_audit_readme_claims`
  (60 distinct behaviours + 10 short-name aliases such as
  `content_matches` → `file_content_matches`).
- **`Cargo.toml` internal-dep + `npm/package.json` versions** now
  track `workspace.package.version` in lockstep. The npm shim was
  six versions behind (0.9.8 → 0.9.21); the workspace internal-dep
  floors stay at 0.9.8 as the intentional API-compat floor (per
  `RELEASING.md`), and a new preflight check asserts every
  path-having floor stays `<= workspace.package.version`.
- **LSP design docs** moved from `docs/design/v0.10/` to
  `docs/design/v0.11/` to match the locked v0.11 scope ("LSP + DSL
  polish"). `docs/design/v0.10/README.md` rewritten as case-study
  coverage scope.
- **`ARCHITECTURE.md`** refreshed for v0.9 engine changes that had
  accrued: `PerFileRule` trait + dispatch flip (v0.9.3); memory
  layout with `Arc<Path>` / `Arc<str>` / `Cow<'static, str>`
  (v0.9.2); `FileIndex` lazy indexes (v0.9.5 + v0.9.8);
  `scope_filter` Scope ownership (v0.9.10); git-tracked filtered
  index (v0.9.11). v0.10 → v0.11 LSP off-by-one corrected;
  output-format example `summary` → `agent`.
- **`RELEASING.md` step 3** now calls `bash ci/scripts/preflight.sh`
  (the new 7-gate superset) instead of listing the four-command
  subset, and cross-refs the README-claims test + workspace-dep-
  floor check that live inside it. `CONTRIBUTING.md` points at the
  same preflight script.

### Added

- **`xtask gen-public-roadmap`** subcommand + roadmap-generator
  module (`xtask/src/roadmap_generator.rs`, ~180 LOC, 13 unit
  tests). Strips sections wrapped in
  `<!-- alint:internal-start -->` / `<!-- alint:internal-end -->`
  markers when generating the alint.org-bound copy of
  `docs/design/ROADMAP.md`. Byte-deterministic output; wired into
  `xtask docs-export` so the public roadmap is generated rather
  than hand-copied. Canonical ROADMAP now uses the markers around
  two engineering-process sections; the marker convention is
  documented in `ROADMAP.md` + `CONTRIBUTING.md`.
- **`ci/scripts/check-workspace-dep-floors.sh`** asserts every
  path-having `[workspace.dependencies]` pin is `<=
  workspace.package.version`. Wired into `preflight.sh`.
- **`ci/scripts/check-version-pins.sh`** extended to cover
  `npm/package.json`'s `version` field.
- **`crates/alint-e2e/tests/coverage_audit_readme_claims.rs`** —
  6 README count claims (rule kinds, families, bundled rulesets,
  auto-fix ops, output formats, subcommands) asserted against
  canonical source per claim; plus 4 unit tests on the parser
  helpers. Runs in `cargo test --workspace`, so picked up by
  preflight automatically.
- **`docs/development/README.md`** with per-file audience labels
  (public / synced via `xtask docs-export` vs internal /
  tracked-for-context). Documents the file-classification
  convention so future additions self-classify.

### Fixed

- **Canonical `docs/design/ROADMAP.md`** v0.11 section title +
  forward-refs rewritten to match the locked v0.11 scope; "Latest
  release" header line corrected v0.9.20 → v0.9.21.
- **alint.org synced surfaces** (blog draft, landing page,
  language-agnostic / compare / roadmap pages, llms-full.txt
  comment) refreshed for v0.9.21 + 70 rule kinds + 13 families
  consistency.

## [0.9.21] - 2026-05-14 (commit-range mode for git_commit_message)

Headline feature: the `git_commit_message` rule's new `since:`
option ([#26](https://github.com/asamarts/alint/issues/26)), which
makes the rule usable on `pull_request`-trigger CI for the first
time. Plus the v0.9.20 polish backlog: CLI color parity across the
non-`check`/`fix` subcommands, em-dashes scrubbed from user-facing
prose, install-snippet ordering normalised, schema + docs
drift-prevention scaffolding, and a benchmarks-trajectory pipeline
that replaces a hand-edited table.

### Changed

- **CLI color + AutoStream parity** for `list`, `explain`, `facts`,
  `suggest` — matches `check`/`fix` color discipline.
- **Em dashes** dropped from `README.md`, bundled rule messages, and
  the synced `docs/site/` tree.
- **Install snippets** lead with `curl | bash` in `README.md` and
  `docs/site/getting-started/installation.md`.
- **`--format` style** normalized to space form (`--format json`,
  not `--format=json`) across README + cookbook; clap accepts both,
  this is style consistency only.
- **Structured query** is now its own family-level H2 in
  `docs/rules.md` (was nested under Content). 13 families per the
  README claim now matches the doc structure + alint.org sidebar.
- **Docker GitHub Actions** bumped via Dependabot — `setup-qemu`,
  `setup-buildx`, `login` to v4; `build-push` to v7. `release.yml`
  and `bench-docker.yml` now on the same major versions.
- **`toml`** crate bumped 0.9 → 1.1 (Dependabot major). API stable
  on `from_str`; all tests pass.

### Added

- **`git_commit_message` range mode** ([#26](https://github.com/asamarts/alint/issues/26)):
  new `since:` option validates every commit in `<since>..HEAD`
  instead of only HEAD. Right shape for `pull_request`-trigger CI,
  where `actions/checkout` produces a synthetic merge commit at
  HEAD that the rule would otherwise always flag. `since:` accepts
  any `git rev-parse`-able ref (SHA, branch, tag, `HEAD~N`) and
  supports POSIX `${VAR}` and `${VAR:-default}` env-var
  interpolation so CI workflows can pass the PR base SHA in via
  an env var without templating the YAML. Merge commits in the
  range are skipped by default; opt in with `include_merges: true`.
  Per-commit violations carry the abbreviated SHA + a subject
  snippet so users know which commit to amend. Shallow-clone
  gotchas (the common `actions/checkout@v4` `fetch-depth: 1`
  default) hard-fail with a `fetch-depth: 0` hint rather than
  silently returning an empty range. Plus 5 new e2e scenarios
  covering range happy path, multiple failing commits, empty
  range, HEAD-only backward compat, and the merge-skip default.
- `GOVERNANCE.md` + `.github/FUNDING.yml` (pre-launch repo polish).
- `ci/scripts/preflight.sh` wrapper + git `pre-push` hook
  enforcing fmt / clippy / doc / pin-sync.
- `ci/scripts/check-version-pins.sh` + `.alint.yml` dogfood rule
  `install-snippets-match-workspace-version` for automated
  version-string drift detection.
- Usage examples for 7 under-documented rule kinds + an
  `xtask docs-coverage` audit to enforce per-kind doc presence.
- **`schemas/v1/agent-report.json`** — published JSON Schema for
  the `--format agent` output, closing the gap behind the
  `schema_version: 1` stability promise.
- **Benchmarks trajectory pipeline** — `render-history.py
  --json-out` emits `benchmarks-trajectory.json` from per-version
  `results.json` files + CHANGELOG headlines; `xtask docs-export`
  bundles it; alint.org's `/benchmarks/` page reads it at build
  time. Replaces a hand-edited HTML table that had drifted six
  releases behind. `coverage_audit_benchmarks_trajectory` test
  guards the data flow.
- **`bench-docker.yml` concurrency group** so two rapid pushes
  don't race the `:edge` image tag.

### Fixed

- Broken alint.org links to `docs/development/rule-authoring.md`
  (changelog + roadmap had uppercase `RULE-AUTHORING.md` references;
  source file renamed to match the synced lowercase slug).
- `alint init` is now suggested from the "no .alint.yml found"
  error paths (first-time-user dead-end resolved).
- `walker.rs::descendants_of` doc comment now correctly describes
  the cycle-defense rationale (was inaccurate about symlink
  exclusion; the actual guarantee comes from the `ignore` crate's
  ancestor-loop detection).
- `render-history.py` forces UTF-8 stdout so Windows CI doesn't
  trip on em dashes in the markdown render.
- `xtask docs-export` treats `python3` as a soft dependency: emits
  a warning + skips trajectory generation on hosts without it
  (the self-hosted CI runner case). `docs-bundle.yml` runs on
  ubuntu-latest which always has python3, so production bundles
  are unaffected.
- `fix(dogfood)`: exclude `alint-e2e/fixtures/**` from self-lint
  (test fixtures legitimately violate hygiene rules).
- `chore(privacy)`: replace personal email with alint.org aliases
  across CONTRIBUTING / SECURITY / README contact lines.

### Docs

- `RELEASING.md` step 1 now points at `bump-version.sh` and
  documents the two deliberate exclusions (`[workspace.dependencies]`
  intra-workspace API floor + `npm/package.json` which the release
  workflow rewrites at publish time).

### Bench

- v0.9.18 / v0.9.19 / v0.9.20 macro-bench + criterion results
  published under `docs/benchmarks/`.

## [0.9.20] - 2026-05-10 (cross-command output polish)

Extends v0.9.19's width-aware-output / `--no-docs` / message-wrap
treatment from `alint check` to every other human-renderer command
in the CLI. Surfaced while polishing the alint.org landing-page
demo: `alint fix`'s output had no width awareness, so the demo
cast had to bump its frame to 110 cols + accept one wrap to fit
the bundled-rule messages.

### Engine

- **`wrap_message` promoted to public API** under `alint_output::wrap_message`.
  Renderers outside the alint-output crate (e.g. `alint suggest`)
  can now apply the same word-aware wrap with hanging-indent
  semantics that `alint check` uses.
- **`alint fix` honours `--width`.** The fix renderer
  (`write_fix_human` in `human.rs`) wraps each violation's
  message with a 4-col continuation indent under the `· `/`✓ `
  glyph. Status-suffix prose (`(no fixer)`, `(skipped: <reason>)`)
  stays attached to the message text so it never lands on a line
  by itself looking orphaned. Same `wrap_message` helper as check.

### CLI

- **`alint suggest` honours `--width`** for `--format human` output.
  Per-proposal `summary` text wraps at 4-col indent (under the
  proposal glyph); per-proposal `evidence` text wraps at 9-col
  indent (under the `└─ ` tree marker). New `width: Option<usize>`
  field on `suggest::RunOptions` (library API).
- **`alint explain` honours `--no-docs`** by suppressing the
  `policy_url:` line. URLs remain in machine-readable formats
  regardless.
- **`alint explain` drops the `debug: {rule:?}` dump.** That line
  rendered each rule's internal `Debug` repr — useful for alint
  developers, noise for end users (24+ KB of regex automaton state
  for some rule kinds). Use `--format json` on `alint check` for
  the wire-shape, or read the YAML rule definition directly.

### Tooling

- **`ci/scripts/audit-bundled-message-lengths.sh`** — informational
  audit that walks every `crates/alint-dsl/rulesets/v1/**/*.yml`,
  extracts each rule's `message:` text, and reports any whose
  effective single-line render exceeds the budget (default 66
  chars = 80-col terminal − 14-col MSG_INDENT). Walked 94 rules
  on the v0.9.19 corpus; 66 nominally over budget but most are
  intentionally multi-paragraph and `wrap_message` handles them
  gracefully. Pass `--fail-over <N>` to make the script a CI gate
  for messages exceeding a hard limit.

### Tests

- 1 trycmd `explain.toml` snapshot regenerated for the dropped
  `debug:` line.

### Demo refresh (alint.org)

- Re-record the landing-page cast with v0.9.20: shrinks the cast
  frame back to 92 cols (vs 110 in v0.9.19's 4-command cast)
  because `alint fix` now honours `--width=88` cleanly. Same
  4-command sequence (check → cat .alint.yml → fix → check),
  same reading beats. Cleaner visual.

## [0.9.19] - 2026-05-09 (output polish)

Quality-of-life patch focused on the human formatter's behaviour
in narrow terminals, screen recordings, and CI logs — surfaced
while polishing the alint.org landing-page CLI demo (long
`docs:` URLs and 200+ char rule messages were wrapping
unpredictably inside the asciinema-player frame).

### Engine

- **Width-aware message wrapping** in the grouped human formatter.
  Long violation messages now wrap at `effective_width()` cols
  with continuation lines re-indented to `MSG_INDENT` (14 cols).
  Word-aware on whitespace; long unbreakable tokens (URLs,
  hashed identifiers) get their own line and are allowed to
  overflow rather than being broken mid-token. Embedded `\n` in
  message text honoured as paragraph breaks. Width detection
  unchanged (TTY → kernel-reported cols, non-TTY → DEFAULT_WIDTH
  = 80, both clamped [40, 120]).

### CLI

- **`--width <COLS>`** (global) — explicit override of the
  detected width. Required for reproducible captures
  (asciinema, screencasts) and useful for piping into fixed-width
  log viewers / narrow CI dashboards.
- **`--no-docs`** (global) — suppress per-violation `docs:` URL
  lines in human output. URLs remain in JSON / SARIF / GitHub /
  markdown formats. Designed for narrow terminals + screen
  recordings where long URLs disrupt visual alignment.
- **`HumanOptions::Default::show_docs = true`** (library API).
  Manual Default impl preserved current behaviour for library
  callers. The CLI's `--no-docs` flag flips it to `false`.

### Bundled rulesets — verbose-message tightening

Three of the longest-message bundled rules tightened to fit
within ~80 cols on a single wrapped line at 14-col indent. No
behavioural change; just tighter copy.

- `oss-baseline@v1::node-engine-or-nvmrc`: 196 → 95 chars.
  ("Pin the Node.js version with `.nvmrc`, `.node-version`, or
  `.tool-versions` so local and CI installs match.")
- `node@v1::node-no-tracked-dist`: 137 → 84 chars.
  ("Build-output directories shouldn't be tracked. Set `level:
  off` if this one is intentionally shipped.")
- `agent-context@v1::agent-context-recommended`: 112 → 78 chars.
  ("Add AGENTS.md / CLAUDE.md / .cursorrules so coding agents
  share versioned instructions.")

Quality bar going forward for new bundled-rule messages: aim for
≤ 80 chars on a single wrapped line. Verbose-but-useful detail
goes in the policy URL or the rule's docs page.

### Tests

- 6 new `wrap_message` unit tests covering: short text, word-
  boundary wrap, long unbreakable tokens, embedded newlines,
  empty input, tiny-width clamp.
- 9 trycmd `help-*.stdout` snapshots regenerated for the 2 new
  global flags.
- 1 `cli_flag_inventory` snapshot regenerated.

## [0.9.18] - 2026-05-08 (pre-launch fixes)

Findings from the v0.9.17 deep-analysis pass (30 case studies — see
[`docs/development/case-study-deep-analysis-log.md`](docs/development/case-study-deep-analysis-log.md))
surfaced 6 bundled-ruleset bugs + 3 case-study config issues. v0.9.18
closes them before P4 launch. The only engine change is one small
extension: `dir_absent` rules now honour `scope_filter:` (required by
A1 below), allowing dir-iterating rules to use the same ancestor-
manifest gate that scopes per-file rules.

Live-tree revalidation against the cloned case-study trees confirmed
the FP-class elimination end-to-end (see deep-analysis log v0.9.18
revalidation evidence section for the full per-repo table). Headline
numbers:

  airflow apache-2-source-has-license-header: 8,228 → 79 (-99%)
  rust-lang/rust rust-sources-snake-case:     1,091 → 10 (-99%)
  ruff python-sources-final-newline:            176 → 0  (-100%)
  ruff python-sources-no-trailing-whitespace:    59 → 0  (-100%)
  tensorflow tensorflow-bazel-files-...:         700 → 241 (-66%)
  bazel hygiene-no-js-build-outputs:              30 → 0  (-100%)
  dotnet/runtime hygiene-no-js-build-outputs:     21 → 1  (-95%)
  ruleset/per-repo total FP elimination:       ~10,000+ violations

### Engine

- **`dir_absent` honours `scope_filter:`.** Previously rejected at
  build time with "scope_filter is supported on per-file rules only".
  Required by A1 below — `hygiene-no-js-build-outputs` needs
  `scope_filter: { has_ancestor: package.json }` to scope per-JS-package.
  The fix removed the `reject_scope_filter_on_cross_file` call from
  `dir_absent::build` and switched its scope construction to
  `Scope::from_spec(spec)` (the canonical from-spec constructor).
  `Scope::matches` already worked for dir paths (`Path::parent()` is
  path-agnostic), so no other engine change beyond the build-path
  swap. JSON Schema description for `scope_filter` updated to
  document `dir_absent` support. Symmetric `dir_exists` /
  `file_absent` / `file_exists` extensions deferred — none of the
  v0.9.18 fix-list items need them.

### Fixed (bundled rulesets — A1–A6)

- **A1 — `hygiene-no-js-build-outputs` (and `node-no-tracked-dist`)
  gated on `scope_filter: { has_ancestor: package.json }`.** Without
  this gate, the rule fired on every `**/dist`, `**/build`, etc.
  directory regardless of context — false positives in 8 polyglot
  monorepos (k8s, golang/go, dotnet, bazel, vscode, nixpkgs, deno,
  angular).
- **A2 — `apache-2-source-has-license-header` pattern broadened to
  catch BOTH short SPDX form AND long ASF preamble.** Pre-fix
  pattern only matched the short SPDX form, missing the longer
  "Licensed to the Apache Software Foundation (ASF)..." preamble
  every Apache TLP uses. airflow surfaced 8,228 false positives;
  arrow + spark each carried per-repo overrides duplicating the
  alternation pattern. The bundled rule now uses
  `'Licensed (to the Apache Software Foundation|under the Apache License,?\s*Version 2)'`
  — superseding the per-repo overrides.
- **A3 — `python@v1` cosmetic rules default-exclude test-fixture
  paths.** `python-sources-final-newline` and
  `python-sources-no-trailing-whitespace` now exclude
  `tests/fixtures/**`, `**/testdata/**`, `Lib/test/**` (cpython),
  and `crates/**/resources/**` (Rust-Python projects like ruff).
  ruff alone surfaced 235 FPs across its 1,597 deliberately-malformed
  Python test fixtures.
- **A4 — `monorepo/cargo-workspace@v1` documents non-canonical
  layout override.** The bundled `select: "crates/*"` is the
  canonical layout; non-canonical workspaces (deno's
  `ext/*` + `libs/*`; clap's `clap_*`) override the rule via
  per-config (rule-id reuse). Bundled YAML now ships a SCOPE NOTE
  enumerating the override recipe with a worked example. The
  principled fix — a `toml_path_array_iter` rule kind that derives
  the iteration set from `$.workspace.members[*]` — is a v0.10
  design candidate.
- **A5 — `oss-license-exists` accepts `LICENSE.TXT` (uppercase),
  `LICENSE.rst`, `COPYING.{md,txt}`, and a few less-common
  variants.** dotnet/runtime canonical filename is `LICENSE.TXT`;
  filesystem globs are case-sensitive on most platforms. Broadened
  via brace expansion. Mirror change to `oss-license-non-empty`
  (paths must stay symmetric).
- **A6 — `rust@v1`'s `rust-sources-snake-case` excludes test-fixture
  + doc subtrees with deliberately non-snake-case filenames.**
  rust-lang/rust's actual FP class is hyphenated names in
  `src/doc/**`, Miri/clippy/rustfmt test fixtures, and `**/*.miri.rs`
  dot-suffix files (NOT `compiler/**` — that subtree is already
  snake-case-correct, contrary to the deep-analysis log description).
  rust-lang/rust violations dropped 1,091 → 10 (99% reduction).

### Fixed (case-study configs — B1–B3)

- **B1 — `examples/microsoft-typescript/.alint.yml`:** pitfall #22
  block-scalar fix on `ts-copyright-header-{src,scripts}` (`|` →
  `|-`); `ts-copyright-header-src` level lowered `warning` → `info`
  (the convention isn't actually applied to source files —
  TypeScript bundles its CopyrightNotice block via Hereby's
  `generateLibs`).
- **B2 — `examples/denoland-deno/.alint.yml`:** defensive pitfall
  #22 swap on `deno-copyright-js-ts`. Verified NOT firing today
  (regex luck), but `|-` removes the dependency on regex luck.
- **B3 — `examples/tensorflow-tensorflow/.alint.yml`:**
  `tensorflow-bazel-files-have-apache-header` premise repair
  (700 FPs pre-fix). TF declares licensing per-Bazel-package via
  `licenses(["notice"])` + `default_applicable_licenses`, NOT
  per-file headers. Pattern replaced with
  `'(licenses\(.*notice|default_applicable_licenses.*license)'`
  + scope narrowed to `BUILD` / `BUILD.bazel` only.

### Validation

- **Cross-cutting revalidation pass.** Re-ran alint v0.9.18 against
  the cloned case-study trees at `/tmp/<repo>/`. Per-repo FP
  elimination tabulated in
  `docs/development/case-study-deep-analysis-log.md` v0.9.18
  revalidation evidence section. The revalidation surfaced two
  scope-mismatches in the original A3 + A6 commits that didn't
  catch the actual FP source — corrected mid-flight in commit
  `dc7c3ed8` (A3: `crates/**/resources/test/fixtures/**` →
  `crates/**/resources/**`; A6: `compiler/**` →
  `src/doc/**` + `src/tools/{miri,clippy,rustfmt}/tests/**` +
  `**/*.miri.rs`).
- **`coverage_audit_smoke_fixtures` extended.** Three new fixtures
  exercise the A1, A3, A6 bundled-rule defaults via
  `tests/coverage_audit_smoke_fixtures.rs`. Each fixture's
  `expected.toml` documents the count drift a refactor would
  produce so the audit failure points at the regression class.
  Total smoke fixtures: 5 → 8.
- Per-README cosmetic count refresh deferred — those tables need
  re-running per-repo and become noise in the launch sprint. The
  deep-analysis log evidence table is the v0.9.18 ship-state
  authority.

## [0.9.17] - 2026-05-06

Corrective release that re-publishes v0.9.16's content + the
build.rs/main.rs lint fixes that prevented v0.9.16's Release
workflow from publishing artifacts. Same scope as v0.9.16 (Config
DX hardening + 21-pitfall catalogue + 30 OSS case studies + P2b
Wave 2 evidence + per-rule `respect_gitignore: false` knob +
operator-polish), plus the small set of corrections enumerated
below. **v0.9.16 the tag exists; v0.9.16 the release does not** —
crates.io / Homebrew / Docker / npm never received a v0.9.16
artifact (the Release workflow's preflight failed at clippy
because the workspace's `[lints.clippy]` enables `pedantic = warn`
+ `-D warnings`, which caught 7 cast lints in `crates/alint/build.rs`
and 2 lints in `crates/alint/src/main.rs` — both new files added in
v0.9.16. Local `cargo clippy --workspace --all-targets -- -D warnings`
runs against a warm cache hadn't surfaced them; running
`ci/scripts/clippy.sh` from a clean state would have).

### Fixed

- **`crates/alint/build.rs` pedantic-clippy compliance.** Casts in
  the days-to-ymd astronomical algorithm get a function-level
  `#[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss,
  clippy::cast_possible_truncation)]` with a comment explaining
  that the algorithm's invariants (`doe` ∈ `[0, 146_097)`, `yoe`
  ∈ `[0, 400)`, `m` ∈ `[1, 12]`, `d` ∈ `[1, 31]`, `y` ∈
  `[1970, ~25_000]`) make the lints fire on shapes that never
  occur. Plus `unwrap_or` → `map_or` and three doc-comment
  backtick fixes.
- **`crates/alint/src/main.rs` panic-hook lints.** `map(...)
  .unwrap_or_else(...)` → `map_or_else(...)` on `info.location()`;
  `out.push_str(&format!(...))` → `write!(out, ...)` on the
  `url_encode` percent-encoder.
- **`examples/flutter-flutter/.alint.yml` two real false-positives.**
  - BSD-header rule excludes `packages/flutter_tools/templates/**`
    (those are user-app templates created by `flutter create`,
    not flutter source — they don't carry the Flutter Authors
    header by design).
  - `resolution: workspace` rule excludes
    `packages/flutter_tools/pubspec.yaml` (flutter_tools historically
    stands outside the root pub workspace, alongside flutter_goldens
    which was already excluded — the rule was producing a FP on a
    legitimate exception).
- **`docs/launch-prep.md` cross_language_implementation_complete
  source count corrected from 4 to 5.** flutter is the 5th source
  AND a genuinely distinct topology — *platform-driven* polyglot
  variant (engine ABI surfaces ↔ per-OS native implementations
  under `engine/src/flutter/shell/platform/{android,darwin/ios,
  darwin/macos,linux,windows,fuchsia}/`) versus the data-format-
  driven variant (arrow + tensorflow + protobuf) and the within-
  language source↔golden variant (angular). Generalises to every
  cross-platform UI framework (React Native, MAUI, Qt, Tauri).
- **`docs/development/CONFIG-AUTHORING.md` pitfall #18 demand-signal
  count.** flutter ships `pubspec.lock` tracked-AND-gitignored
  (same shape as bazel's `.bazelversion`) — second independent
  demand signal for the per-rule `respect_gitignore: false` knob
  that v0.9.16 shipped. The note added at the bottom of the
  pitfall #18 source attribution.

### Added

- **Headline launch-evidence catch — Trojan-Source CVE-2021-42574.**
  alint catches **5 real CVE-2021-42574 (Trojan Source)** bidirectional-
  control-character errors in flutter/flutter's `docs/releases/archive/`,
  via the bundled `oss-baseline` ruleset's `no_bidi_control_characters`
  rule. This is the strongest single piece of "alint catches things
  other tools miss" evidence in the case-study corpus — flutter's
  existing tooling (analyzer, dart format, the per-package
  pre-commits) doesn't catch this class. Worked into the launch-
  evidence drafts under `docs/marketing/drafts/`.

### Note on v0.9.16

The v0.9.16 git tag (`bc507b17` annotated tag → commit `764c817f`)
remains on the remote pointing at the broken commit. It is not
moved: option A1 ("force-recreate the tag at the corrected
commit") was considered and rejected in favour of A3 ("cut v0.9.17
as the corrective release") for orthodox-semver reasons. Adopters
who pulled the v0.9.16 tag will see no published artifacts at any
of the usual channels (crates.io / Homebrew / Docker / npm); v0.9.17
is the first publishable tag in the 0.9.16 content lineage.

## [0.9.16] - 2026-05-06 (tag-only — never published, see v0.9.17)

Config DX hardening release. Closes the launch-prep validation pass
with seven-phase coverage of the 17 schema + runtime pitfalls
surfaced during the P2a 20-repo case-study sweep, plus the
`deny_unknown_fields` uniformity audit that makes that coverage
hold uniformly across all 60 rule kinds. A config that hits any of
the catalogued pitfalls is now caught somewhere in the toolchain
(schema at edit time, parse error at load time, runtime audit at
PR time) — for *every* rule kind, not just the half that already
had `deny_unknown_fields` declared.

(Note: there is no v0.9.15 tag. The seven-phase work landed locally
under that label on 2026-05-06 but was rolled into v0.9.16 alongside
the uniformity audit before the public release. The version sequence
is v0.9.14 → v0.9.16 by design — the `git log` entries between
them are the seven phases plus the uniformity work.)

The headline numbers:

- **21 distinct pitfalls** catalogued in `docs/development/CONFIG-AUTHORING.md`
  with canonical-correct YAML for each (17 from P2a; 2 from P2b Wave 1
  — `.gitignore` masks tracked-file presence checks; `root_only: true`
  silently no-matches multi-component literals; **2 from P2b Wave 2** —
  cross-file value-equality across structurally-different files
  needing per-file value extraction; `yaml_path_*` rules erroring on
  multi-document YAML files). Pitfalls #18 and #19 are now *fixed in
  the engine* (#18 ships the per-rule `respect_gitignore: false` knob;
  #19 ships the literal_is_nested runtime guard so misconfigurations
  no longer fire on unrelated files); pitfalls #20 and #21 stay
  documented with a workaround until the v0.10 design candidates ship.
- **7 silently-broken structured-path rules** in committed pilot +
  Wave 1 configs (6 bool-match + 1 array-semantics) caught by the
  validation pass and fixed in-flight.
- **30 production OSS repos** with working `.alint.yml` configs +
  case-study writeups under `examples/<owner>-<repo>/` — 20 from P2a
  (single-language + diverse-ecosystem) + 5 from P2b Wave 1 (curated
  pre-launch polyglot subset: NixOS/nixpkgs, bazelbuild/bazel,
  tensorflow/tensorflow, apache/spark, microsoft/vscode) + 5 from
  P2b Wave 2 (platform-driven polyglot subset: angular/angular,
  istio/istio, dotnet/runtime, protocolbuffers/protobuf,
  flutter/flutter — total 322 polyglot rules across the Wave 2 set,
  zero new rule-kind candidates beyond the v0.10/v0.11 backlogs,
  validating the saturation analysis). Five positioning narratives
  crystallised across the corpus.
- **Scale validation:** the 5-repo P2b Wave 1 wasn't expected to
  surface new pitfalls (saturation analysis from P2a's Wave 3 had
  predicted reconfirmation, not new candidates) but was commissioned
  to test unknown-unknowns: scale stress, build-system shape, per-
  language API parity, polyglot-discipline convergence, and flagship-
  visibility apples-to-apples comparison. Headline results:
  - **NixOS/nixpkgs at 39,101 files + 20,678 `pkgs/by-name/*/*/`
    package directories: alint's full 79-rule pass — including the
    headline `for_each_dir` over the by-name tree — completes in
    273 ms wall-clock.** "Any size repo" is now empirically
    defensible.
  - **microsoft/vscode `build/hygiene.ts` apples-to-apples:** alint
    covers ~75 % of the 8 distinct hygiene checks (6 of 8)
    declaratively in one config. "alint is what `build/hygiene.ts`
    would look like as a tool, not a per-repo script" — verified
    against the live tree (222 violations, zero false positives).
  - **3 Apache TLPs converge:** arrow + spark + airflow share 9 of
    12 governance artefacts; `apache/governance@v1` bundled ruleset
    promotes from "v0.10+ idea" to "v0.10 ship-target."
  - **`cross_language_implementation_complete` v0.11+ candidate now
    demand-validated AND saturated:** 4 sources after Wave 2 (arrow
    + tensorflow + protobuf + angular). protobuf is the densest with
    ~45 cross-language assertions one rule would express; angular
    contributes the within-language source↔golden variant. Promotes
    from "v0.11+ flagship" to "v0.11+ ship-target."
  - **New v0.10 rule-kind candidate: `xml_path_matches` /
    `xml_path_equals`** (spark — completes the structured-query
    family; generalises to Maven, Ant, Gradle XML, .nuspec, .csproj).
    **Promoted to v0.10 ship-target by Wave 2** — dotnet/runtime
    stress-tests at ~2,300 distinct XML manifests (1,091 .csproj +
    234 solution files + 257 Directory.Build.* + 520 .props/.targets),
    one order of magnitude bigger than spark.
  - **New bundled ruleset added to v0.10 ship-list: `dotnet@v1`** —
    surfaced by dotnet/runtime; 12 of 14 dotnet-specific rules in
    the case study consolidate into one `extends:` line. Adopter
    surface is large (every dotnet/* + every Azure SDK + every
    microsoft/* .NET project).
- **Every example config** ships with `# yaml-language-server:
  $schema=…` so opening it in any LSP-aware editor (vscode-yaml /
  JetBrains / coc-yaml) gets autocomplete + validation for free.
- **`alint validate-config <path>`** is the new entry point for
  editor LSP / pre-commit / fail-fast CI integrations.
- **`#[serde(deny_unknown_fields)]` is now uniform across all 60 rule
  kinds** — 13 rule Options structs that previously silently
  accepted unknown fields now reject them, surfacing the Phase 3
  did-you-mean enrichment for those kinds. 13 unit-style integration
  tests (one per kind) continuously verify uniformity at PR time.
- **Per-rule `respect_gitignore: false` (pitfall #18 fix).** Closes
  the bazel-style "tracked AND gitignored" pattern surfaced by Wave 1.
  `file_exists` rules can now bypass the workspace-level gitignore
  setting at single-rule granularity; the literal-path fast path also
  checks the filesystem directly so a `.bazelversion`-style file is
  found even when the walker pre-filters it out. Two regression
  scenarios (positive + negative) lock the behaviour in
  `crates/alint-e2e/scenarios/check/git/`.
- **Operator polish — `alint --version` includes commit SHA + build
  date,** baked at compile time via a new `crates/alint/build.rs`.
  Bug reports can paste the full version string and a maintainer can
  pinpoint the exact commit. Falls back gracefully to "unknown" when
  built from a published tarball without a git tree.
- **Operator polish — pre-filled GitHub-issue URL on panic.** A
  custom panic hook prints a `https://github.com/asamarts/alint/issues/new?title=…&body=…`
  link with version + OS + location + payload pre-populated. Skipped
  when `RUST_BACKTRACE` is set so the standard backtrace path stays
  unchanged for developers.
- **`SECURITY.md` adds an explicit "Telemetry-free" guarantee** —
  `alint sends nothing over the network at runtime`, with a
  verifiable `strace -e trace=network` command. Closes the
  trust-on-first-run gap for security-team adopters.

No engine changes; all bench-relevant code paths unchanged. The
release exists to make alint configs obviously-correct at write
time, parse time, and run time — the P2a pass proved that without
this layer of DX, real adopters silently drift into broken-but-
parseable rules (we found seven such drifts in our own evidence
configs before they shipped to launch readers).

### Added

- **`docs/development/CONFIG-AUTHORING.md` — 17-pitfall catalogue
  (Phase 1).** Every pitfall the P2a validation pass surfaced, with
  the canonical-correct YAML alongside. Authors writing a new
  `.alint.yml` are pointed here as the cheat sheet, with explicit
  notes for AI agents drafting configs against rule-source memory
  rather than the doc.
- **`crates/alint-e2e/tests/coverage_audit_examples_parse.rs` —
  examples-parse audit (Phase 2).** Every shipped
  `examples/*/.alint.yml` MUST load + build cleanly via
  `alint_dsl::load` + `RuleRegistry::build`. Schema drift surfaces
  at PR time. Catches the 12 schema-rename-class pitfalls
  transitively. New companion check
  (`every_example_carries_the_yaml_language_server_directive`)
  enforces the LSP-magic-comment line on every example.
- **`crates/alint-e2e/tests/coverage_audit_rules_md_drift.rs` —
  per-ruleset table drift audit.** Walks every YAML under
  `crates/alint-dsl/rulesets/v1/`, parses every `### `alint://bundled/...@v1``
  section in `docs/rules.md`, asserts identical rule-id sets per
  ruleset. Caught the same drift class on 3 more rulesets the day
  it landed (ci/github-actions, agent-hygiene, agent-context); all
  fixed in the same commit. Closes the audit gap that let the
  oss-baseline 8/9/11/15 drift slip through during P2a.
- **Did-you-mean parse errors (Phase 3).** New
  `crates/alint-core/src/did_you_mean.rs` module with two-tier
  resolution: curated overrides for the highest-drift renames
  (`argv→command`, `secondary→partner`, `style→target`,
  `pattern→prefix|suffix`, `matches↔equals` for the structured-path
  family) plus a Levenshtein fallback (distance ≤ 2) for the long
  tail. Hooked at `RuleRegistry::build` — no per-rule edits.
  18 unit tests + 9 integration tests covering 6 pitfalls
  end-to-end. Side-effect: added `#[serde(deny_unknown_fields)]` to
  `EqualsOptions` and `MatchesOptions` in `structured_path.rs` so
  pitfall #16 (`matches:` ↔ `equals:` rename) surfaces as
  unknown-field rather than missing-required.
- **Domain-specific error messages (Phase 4).** Five pitfalls now
  produce hint-bearing errors instead of generic parser output:
  - **#10 — JSONPath dashed-key access.** New
    `crates/alint-core/src/jsonpath_diagnostics.rs` inspects path
    sources for `.<dashed-ident>` patterns (top-level OR inside
    filter expressions) and suggests bracket notation.
  - **#11 — `scope_filter.has_ancestor` with path separator.** The
    error now points at `paths:` glob as the alternative for
    directory scoping.
  - **#12a — `&&`/`||`/`!` in `when:`.** Parse-error wrapper
    detects symbolic operators and suggests the
    `and`/`or`/`not` keyword.
  - **#12b — `iter.foo.bar(...)` method-call shape.** Global
    regex catches the double-dot chain and points at the
    `matches` operator.
  - **#15 — `file_starts_with.prefix: ""`.** Build-time error
    now points at `file_min_lines: 1` / `file_min_size: <bytes>`
    as the right tool for non-emptiness checks.
- **JSON Schema editor-LSP wiring (Phase 5).** New audit at
  `crates/alint-e2e/tests/coverage_audit_schema_drift.rs` walks
  the `rule_kind_dispatch` oneOf, extracts every kind name
  (handles both single-`const` and multi-name `enum`
  discriminators for legacy aliases), and verifies registry ↔
  schema parity. Five spot-check tests exercise pitfalls
  #1/#4/#9/#15/#16 against the live schema via the `jsonschema`
  crate, asserting they're rejected at edit-time. Plus the
  `# yaml-language-server: $schema=…` magic comment was added to
  all 20 example configs and a companion audit keeps it there.
- **`alint validate-config <path>` subcommand (Phase 6).**
  Parse-only command (no tree walk) that runs the full load +
  build + when-parse path and reports pass/fail. Accepts a file
  or directory. Two output formats: `human` (one-liner stdout +
  error chain on stderr) and `json` (stable
  `{valid, rule_count, config_path, error}` envelope). Exit
  codes: 0 valid / 1 invalid / 2 invocation error. Did-you-mean
  hints from Phases 3 + 4 flow through transparently — covered
  by a trycmd test that exhibits an `argv` typo and asserts the
  canonical-correct hint appears.
- **Smoke-test fixture audit (Phase 7).** New audit at
  `crates/alint-e2e/tests/coverage_audit_smoke_fixtures.rs`
  closes the runtime-semantic gap left by Phases 1-6. Walks
  `crates/alint-e2e/fixtures/smoke/<scenario>/` directories;
  each scenario is a self-contained config + file tree +
  `expected.toml` with canonical violation counts. The audit
  runs the engine over each tree and asserts actuals match —
  a refactor that silently re-introduces pitfalls #13 (regex
  anchoring), #14 (YAML `\n` in regex), #16 (`*_path_matches`
  + bool), or #17 (`*_path_equals` + `[*]`) changes the count
  and fails. Initial coverage: 4 fixtures targeting the four
  classes. Sanity-verified by deliberately removing `(?m)`
  from a fixture rule (audit failed) and restoring it (audit
  green).
- **20 P2a case studies** under `examples/<owner>-<repo>/`:
  kubernetes/kubernetes, rust-lang/rust, golang/go,
  python/cpython, nodejs/node, apache/airflow, denoland/deno,
  tokio-rs/tokio, astral-sh/uv, astral-sh/ruff, clap-rs/clap,
  microsoft/typescript, facebook/react, prettier/prettier,
  pnpm/pnpm, helm/helm, pytorch/pytorch, vercel/turbo, plus
  two polyglot wins (vercel/next.js, apache/arrow). Each is a
  working `.alint.yml` + a markdown writeup of what alint
  catches that the repo's existing tooling doesn't. The
  `examples/README.md` gallery organises them by the five
  positioning narratives surfaced.

### Fixed

- **6 bool-match rules** in committed `examples/*/.alint.yml`
  configs that were silently broken by pitfall #16
  (`*_path_matches` against bool fields produces a runtime
  "not a string" error on every match). Switched to
  `*_path_equals` with native YAML bool literals: typescript
  ×2 (`compilerOptions.strict`, `compilerOptions.checkJs`),
  vercel/turbo (replaced with `file_content_matches` for the
  either-of-many-bools case), tokio (`package.publish`),
  ruff (`package.publish`), pnpm (`engineStrict`).
- **deno's `deno-dlint-includes-camelcase` rule** was silently
  broken by pitfall #17 (`*_path_equals` against `[*]` flips
  intent from "any" to "every"). Switched to
  `file_content_matches` against the JSON text.
- **`oss-baseline@v1` ruleset count drift.** The bundled YAML
  has 15 rules; `docs/rules.md` claimed "Nine rules" but
  listed 11; the alint.org compare-page draft said 8. All
  three reconciled to 15.
- **JSONPath outer-parens filter wasn't a real pitfall.** A
  reported "pitfall #18" (the apache/arrow case study originally
  claimed `?(...)` was rejected) was investigated against
  `serde_json_path` 0.7.x and proven valid — the original
  failure was a dashed-key inside the filter (an instance of
  pitfall #10 in filter context), not the parens. Catalogue
  dropped 18 → 17; the arrow case study now points at #10.

### Changed

- **`#[serde(deny_unknown_fields)]` is now uniform across the rule
  catalogue.** Added to `EqualsOptions` + `MatchesOptions` in
  `structured_path.rs` (Phase 3 dependency for pitfall #16's
  `matches:` ↔ `equals:` rename to surface as unknown-field
  rather than missing-required), and as a v0.9.16 sweep to 13
  more rule Options structs that previously silently accepted
  unknown fields: `file_content_matches`, `file_content_forbidden`,
  `file_header`, `file_footer`, `file_max_lines`, `file_max_size`,
  `file_min_lines`, `file_min_size`, `file_shebang`, `filename_case`,
  `filename_regex`, `commented_out_code`, `markdown_paths_resolve`.
  This is the behaviour change the original v0.9.15 plan flagged
  for "v0.9.16+" — the launch-prep deliberation chose to roll it
  into the same release rather than ship a half-cooked uniformity
  story. Adopters whose configs had stray fields on these rule
  kinds will now see clean unknown-field errors with did-you-mean
  hints instead of silent acceptance.
- **`crates/alint-e2e/fixtures/smoke/content_matches_yaml_newline_canonical/`**
  — additional smoke fixture covering pitfall #14 (YAML `\n`
  in regex patterns). Phase 7 shipped 4 fixtures; v0.9.16 adds
  this 5th to bring per-pitfall regression coverage uniform.

### Marketing drafts (not yet published)

`docs/marketing/STATE.md` is the new single source of truth for
what's live, stale, and drafting across the README + alint.org
surfaces. Seven drafts under `docs/marketing/drafts/` are queued
for publish coordinated with this release's docs roll:

- `readme-hero.md` — incremental polish on the README hero adding
  the case-study social proof + 5-narrative section.
- `alint-org-hero.md` — content brief for the alint.org landing
  with version badge, speed claim in the hero bullets,
  Repolinter-archived-2026 framing, and the 20-case-study link.
- `alint-org-examples-gallery.md` — content brief for the new
  `/examples/` route + auto-synced per-case-study sub-pages.
- `alint-org-compare.md` — direct comparison table vs Repolinter,
  ls-lint, Megalinter, EditorConfig, and custom shell scripts.
- `migrate-from-repolinter.md`, `migrate-from-ls-lint.md`,
  `migrate-from-custom-bash.md` — three step-by-step migration
  guides. Repolinter is the highest-intent SEO target.

The drafts are not part of the v0.9.15 binary release; they ship
when the alint.org site repo is updated next.

## [0.9.14] - 2026-05-05

CI automation release. The `bench-record.yml` workflow that
captures publish-grade bench data on every release tag is now
fully end-to-end automated for the first time. Pre-v0.9.14 a
maintainer had to (a) manually `gh pr create` because the
workflow's gh CLI step lacked auth, (b) manually toggle a repo
policy because the GITHUB_TOKEN couldn't open PRs, (c)
manually re-render `HISTORY.md` after every bench-record
merge, and (d) manually edit `xtask/scripts/render-history.py`
to add each new version to its hardcoded list. v0.9.14 closes
all four gaps. Future tag-pushes produce a one-click-merge PR
with bench data + criterion + HISTORY refresh, no human
in the loop until the review.

No alint binary changes. CLI users see nothing different — the
release exists to seal the bench-record automation as proven
end-to-end (validated against the v0.9.13 tag in PR #14 before
this release was cut).

### Added

- **`xtask/scripts/render-history.py` auto-discovery.** Versions
  read from the filesystem (`docs/benchmarks/macro/results/<arch>/v*/`)
  instead of a hardcoded `KNOWN_VERSIONS` list. Per-release
  date + headline blurb extracted from CHANGELOG.md's
  `## [X.Y.Z] — YYYY-MM-DD` header + first sentence of the
  paragraph below. Manual fallback dict retained for
  v0.5.6/v0.5.7 (predate the CHANGELOG-tracked corpus).
  Maintainer effort to land a new release row in HISTORY drops
  from "edit two places in the script + commit" to "make sure
  the CHANGELOG entry has a punchy first sentence".
- **`bench-record.yml` HISTORY auto-refresh step.** The
  workflow now runs `render-history.py` after the bench
  capture and includes the refreshed HISTORY.md in the same
  bench-record commit. The PR is one-merge-completes-everything
  for both the bench data and the HISTORY row.
- **`bench-record.yml` standalone python3 install.** The
  self-hosted runner has no `python3` preinstalled and no
  `sudo` available; v0.9.14 installs a prebuilt standalone
  Python from indygreg/python-build-standalone into
  `$HOME/.local/python/`. Same pattern the gh CLI tarball
  install (added in v0.9.12) uses.
- **`bench-record.yml` rebase-onto-main before push.** When
  `workflow_dispatch -f ref=<old-tag>` benches an older ref,
  the working tree's workflow YAML diverges from main's; the
  resulting push triggers GitHub's "GITHUB_TOKEN cannot create
  or update workflow files" protection. v0.9.14 stages bench
  artefacts to `/tmp` and rebases onto `origin/main` before
  committing — the resulting push contains only bench data,
  no workflow-file diff. Side benefit: tag-triggered runs are
  also more robust (the bench commit always lands on top of
  current main, eliminating a class of subtle merge conflicts).
- **`bench-record.yml` working-tree reset before checkout.**
  `git reset --hard HEAD` + `git clean -fd docs/benchmarks/`
  clears the modified HISTORY.md (from the previous render
  step) and untracked bench dirs (which conflict with main's
  tracked versions of the same paths after a prior bench-record
  PR merged). Without it, the rebase checkout aborts.

### Fixed

- **`bench-record.yml` `bench-record` repo label** now exists
  (created during the v0.9.13 manual workaround). The workflow's
  `gh pr create --label bench-record` invocation no longer
  fails on label resolution.
- **Repo policy** "Allow GitHub Actions to create and approve
  pull requests" enabled. The workflow's GITHUB_TOKEN can now
  open PRs without manual fallback (previously failed with
  "GitHub Actions is not permitted to create or approve pull
  requests").

### Internal

- 4 iterations to validate the pipeline end-to-end on the
  v0.9.13 tag (failures: missing python3, workflow-file push
  protection, working-tree conflicts on rebase, then success).
  Each iteration produced a real fix; the pipeline is now
  fully proven from `gh workflow run` to merged PR.
- v0.9.13 bench data refreshed via PR #14 (the validation run).
  Slightly tighter numbers than the original capture; max CV
  0.041 across all 80 cells.

### Held for future

- **`v0.9.14`-tag-push validation** — this release is the first
  one whose bench-record run will fire from a tag-push (not
  workflow_dispatch). Expected outcome: PR auto-opens, bench
  data + HISTORY.md ready for one-click merge.
- **SHA-rotate the docker/* actions in `release.yml`** — still
  pinned at the same SHAs as v0.9.12; separate "rotate +
  verify" cycle (note from v0.9.13).

## [0.9.13] - 2026-05-04

Dependency-refresh release. Closes the 10 open Dependabot PRs
plus opportunistic bumps the version specs already accepted.
Net workspace perf neutral-to-positive across S1-S10 — most
scenarios are 4-13 % faster at 100k from upstream improvements
(jsonschema 0.46's API rework + lockfile patches).

### Cargo workspace deps bumped

- **`anstream`** 0.6 → 1.0 — stability commit, no API changes.
- **`trycmd`** 0.15 → 1.x — stability commit.
- **`rand`** 0.9 → 0.10 + **`rand_chacha`** 0.9 → 0.10 (paired
  ecosystem bump). One import in `crates/alint-bench/src/tree.rs`
  switched from `use rand::Rng` to `use rand::{Rng, RngExt, ...}`
  to pick up the new `random_range`/`random` methods (the rand
  0.10 rename of `gen_range`/`gen`).
- **`jsonschema`** 0.29 → 0.46. The 0.36 release privatised
  `ValidationError::instance_path` and exposed it via an
  accessor method; 4 call sites updated
  (`alint-rules/src/json_schema_passes.rs`,
  `alint-output/tests/{cross_formatter,fix_report_schema}.rs`,
  `alint-dsl/tests/schema.rs`).
- **`sha2`** 0.10 → 0.11 — workspace consumers (`alint-dsl/sri`,
  `alint-output/gitlab`, `alint-rules/file_hash`) all use the
  stable `Digest`/`Sha256::new()`/`update`/`finalize` API which
  is unchanged across the major. No code changes needed.
- **Lockfile refresh**: `rustls` 0.23.40, `libc` 0.2.186,
  `wasm-bindgen` 0.2.120, `winnow` 1.0.2, etc. — all in-spec
  patch-level pickups.

### GitHub Actions bumped

| Action | From | To |
|---|---|---|
| `actions/checkout` | v4 | v6 |
| `actions/setup-node` | v4 | v6 |
| `actions/cache` | v4 | v5 |
| `actions/upload-artifact` | v4 | v7 |
| `actions/download-artifact` | v4 | v8 (paired w/ upload) |
| `codecov/codecov-action` | v4 | v6 |
| `webfactory/ssh-agent` | v0.9.0 | v0.10.0 |
| `docker/build-push-action` | v6 | v7 (unpinned only) |
| `docker/login-action` | v3 | v4 (unpinned only) |
| `docker/setup-buildx-action` | v3 | v4 (unpinned only) |
| `docker/setup-qemu-action` | v3 | v4 (unpinned only) |

The SHA-pinned variants of the docker/* actions in `release.yml`
are intentionally left at their pinned SHAs — supply-chain
hardening for the publishing path requires a separate "rotate
SHA + verify" cycle, not a blanket version bump.

### Performance

100k/full deltas vs v0.9.12 baseline:

| Scenario | v0.9.12 | v0.9.13 | Δ |
|---|---:|---:|---:|
| S1 | 163ms | 151ms | **-7 %** |
| S2 | 258ms | 254ms | -2 % |
| S3 | 1301ms | 1130ms | **-13 %** |
| S4 | 156ms | 160ms | +3 % |
| S5 | 885ms | 847ms | -4 % |
| S6 | 1121ms | 1011ms | **-10 %** |
| S7 | 329ms | 316ms | -4 % |
| S8 | 1104ms | 1032ms | **-7 %** |
| S9 | 694ms | 666ms | -4 % |
| S10 | 324ms | 328ms | +1 % |

S3 / S6 / S8 wins are likely jsonschema 0.46's Validator
performance work and lockfile patch updates compounding.
1m/full all within ±5 % (most slightly faster). Full numbers in
[`docs/benchmarks/macro/results/linux-x86_64/v0.9.13/`](docs/benchmarks/macro/results/linux-x86_64/v0.9.13/)
once the bench-record workflow lands the canonical capture.

### Held for v0.9.14+

- **`toml` 0.9 → 1.1** — config-format-stability review needed
  (parser/writer rewrite, `FromStr` semantics changed,
  `serde`/`std` now opt-in default features).
- **`tower-lsp`** dormant dep — the crate appears stalled;
  re-evaluate at v0.10 LSP design time vs maintained
  alternatives (`tower-lsp-server` fork, `lsp-server`,
  `async-lsp`).
- **SHA-pinned `docker/*` actions in `release.yml`** — separate
  rotate + verify cycle.

### Internal

- All 1141 workspace tests pass.
- `cargo clippy --workspace --all-targets --all-features --
  -D warnings` clean.
- `cargo doc --no-deps --workspace` with `RUSTDOCFLAGS=-D warnings`
  clean.

## [0.9.12] - 2026-05-03

Backlog cleanup release closing the explicitly-held v0.9 items
that didn't fit cleanly into earlier cuts. Three structural
audits + one engine refactor + the bench-record CI fix.

### Removed — alint-core API

- **`Rule::wants_git_tracked()`** — deprecated in v0.9.11 with
  a v0.9.12 removal date; now gone. Override
  [`Rule::git_tracked_mode`] instead. The engine consults
  `git_tracked_mode()` (added in v0.9.11) to pick the right
  pre-filtered `FileIndex` for each opted-in rule.

### Added — alint-core API

- **`alint_core::CompiledNestedSpec`** — wraps a
  [`NestedRuleSpec`] with its `when:` source pre-compiled to a
  [`crate::when::WhenExpr`] at rule-build time. Mirrors the
  v0.9.5-era `when_iter:` pattern (parse once at build,
  evaluate per iteration with fresh iter context). The 3
  cross-file iteration rules (`for_each_dir`, `for_each_file`,
  `every_matching_has`) hold `Vec<CompiledNestedSpec>` and
  consume it via the shared `evaluate_for_each` helper.

### Fixed

- **Nested `when:` no longer re-parses per iteration.**
  Pre-v0.9.12 the nested `when:` source string was re-parsed
  inside `evaluate_for_each`'s loop on every (entry,
  nested-rule) pair — at scale (e.g. workspace bundles with
  5000+ packages × 3+ nested rules), thousands of redundant
  parses per cross-file rule eval. The new `CompiledNestedSpec`
  parses each source string exactly once at build time.
  Misconfigured nested-`when:` clauses now surface at config-
  load time instead of mid-evaluation.
- **`bench-record.yml` workflow now completes end-to-end.**
  Pre-v0.9.12 the workflow's `gh pr create` step failed with
  `gh: command not found` on the self-hosted runner — bench
  data was captured + pushed to a `bench-record/<tag>` branch
  but the PR-opening step crashed, leaving stale branches
  (`bench-record/v0.9.{9,10,11}` exist on remote with captured
  but unsurfaced data). v0.9.12 adds an `install gh CLI` step
  before the PR-opening step. Same release adds S10 to the
  workflow's scenarios list (was S1-S9 only, missing the
  v0.9.9 addition).

### Internal

- **`coverage_audit_when_wiring.rs`** (new). Asserts every
  cross-file iteration rule (`for_each_dir`, `for_each_file`,
  `every_matching_has`) wires its `when_iter:` field through
  the shared `parse_when_iter` helper at build time AND
  consults the resulting `Option<WhenExpr>` in its dispatch
  path. Catches the silent-no-op recurrence-risk shape on the
  `when_iter:` axis the same way v0.9.10's audit catches it on
  `scope_filter:`.
- **`coverage_audit_engine_when_dispatch.rs`** (new). Asserts
  every dispatch site in `engine.rs` that calls
  `rule.evaluate(...)` or `run_entry(...)` is preceded by an
  `entry.when` consultation in the surrounding 60 lines, OR
  delegates to `run_entry` (which does the consultation
  centrally). Separate sub-test asserts `run_per_file`'s
  inline `entry.when` check stays in place. Catches a future
  engine extension (e.g., a hypothetical fix-only path or LSP
  single-file re-evaluation path) that adds a new dispatch
  site and forgets the gate.
- **`compile_nested_require` helper** (`alint-rules`'s
  `for_each_dir` module) — single point all 3 cross-file
  iteration rules call to compile their `Vec<NestedRuleSpec>`
  into `Vec<CompiledNestedSpec>` at build time. New iteration
  rules thread their require list through this.

### Held for v0.9.13+ / indefinitely

- **`when:` ownership** remains explicitly out of scope.
  `when:` data (facts/vars/iter) varies at evaluate-time so it
  can't be pre-computed into a struct field; the engine
  already owns the top-level dispatch and the v0.9.12 audits
  + pre-compilation close the remaining gaps.
- **Stale `bench-record/v0.9.{9,10,11}` branches** on remote
  contain captured-but-unsurfaced bench data from before the
  workflow fix. Future bench-record runs (starting v0.9.12)
  produce a fresh PR with S1-S10 + criterion + everything;
  the stale branches can be deleted at the maintainer's
  leisure or kept as historical artefacts of the broken
  state.

## [0.9.11] - 2026-05-03

Structural fix for the `git_tracked_only:` silent-no-op
recurrence-risk shape. v0.9.10 closed the analogous
`scope_filter:` bug class via `Scope` ownership; v0.9.11
closes the `git_tracked_only` shape via engine-side
pre-filtered `FileIndex`es. Rules opted into
`git_tracked_only:` no longer carry a runtime
`if self.git_tracked_only && !ctx.is_git_tracked(...)`
check — the engine hands them a `Context` whose `index`
already iterates only the tracked subset.

### Added — alint-core API

- **`alint_core::GitTrackedMode { Off, FileOnly, DirAware }`**
  enum. Returned by `Rule::git_tracked_mode()` so the
  engine knows which pre-filtered `FileIndex` to hand the
  rule. File-mode rules (`file_exists`, `file_absent`)
  return `FileOnly`; dir-mode rules (`dir_exists`,
  `dir_absent`) return `DirAware`.
- **`Rule::git_tracked_mode(&self) -> GitTrackedMode`**
  trait method (default `Off`). Replaces the boolean
  consultation that `wants_git_tracked()` provided.

### Deprecated

- **`Rule::wants_git_tracked()`** — kept for one minor
  version as a default that delegates to
  `git_tracked_mode() != Off`. Out-of-tree rule plugins
  that override this method continue to work; the engine
  no longer consults it. **Removed in v0.9.12.** Override
  `git_tracked_mode` instead.

### Fixed

- **`git_tracked_only` silent-no-op bug class closed
  structurally.** A new rule that ships
  `git_tracked_only: bool` on its spec but forgets to
  override `git_tracked_mode()` defaults to `Off` —
  same defensive default as today. But once it overrides
  `git_tracked_mode()`, the engine's pre-filtered
  `FileIndex` automatically narrows the rule's iteration
  — no per-evaluate `is_git_tracked(...)` check to
  forget. The audit
  `coverage_audit_git_tracked_only.rs` was updated to
  flag both the missing-mode-override AND the
  re-introduction of any per-rule runtime check.
- **`file_exists` literal-path fast path now applies even
  with `git_tracked_only: true`.** Pre-v0.9.11, the rule
  forced the slow path (entry iteration) when
  `git_tracked_only` was set, because the per-entry
  `is_git_tracked` check ran inside the slow loop. The
  engine's pre-filtered index makes the literal-path
  `contains_file` lookup correct without that check —
  10-30 % S8 speedup at small/medium sizes (see Performance).

### Internal

- **Engine `build_git_tracked_indexes`**: builds at most two
  pre-filtered `FileIndex`es per run (one per `GitTrackedMode`
  in use). Build cost amortises across however many rules opt
  into each mode.
- **`pick_ctx`** extended to route opted-in rules to the
  right pre-filtered `Context`. Existence rules already
  declared `requires_full_index = true`, so the substitution
  is safe — we're swapping the full-index `Context` for a
  tracked-narrowed one.
- **Outside-git-repo silent-no-op preserved** via the
  empty-index fallback: when `git ls-files` returns nothing
  (no repo / non-zero exit), the engine builds empty
  pre-filtered indexes for opted-in modes so rules iterate
  zero entries and fire zero violations.
- 4 existence rules (`file_exists`, `file_absent`,
  `dir_exists`, `dir_absent`) cleaned up: per-evaluate
  `is_git_tracked(...)` / `dir_has_tracked_files(...)`
  checks deleted; `git_tracked_mode()` overrides added.

### Performance

S8 (git overlay) at all sizes is faster:

| Size/mode | v0.9.10 | v0.9.11 | Δ |
|---|---:|---:|---:|
| 1k/full | 24.2ms | 21.9ms | -10 % |
| 1k/changed | 29.9ms | 20.4ms | -32 % |
| 10k/full | 139.7ms | 118.0ms | -15 % |
| 10k/changed | 87.3ms | 74.7ms | -14 % |
| 100k/full | 1068.7ms | 1064.0ms | -0 % |
| 100k/changed | 623.3ms | 579.0ms | -7 % |

The 1k/changed -32 % is mostly the `file_exists`
literal-path fast path applying for the first time with
`git_tracked_only`. Larger sizes show a smaller win
because the slow-path iteration cost is dominant.

### Held for v0.9.12+

- **`Rule::wants_git_tracked()` removal.** Deprecated this
  cycle; remove next.
- **`when:` ownership** remains explicitly out of scope —
  different semantics, no shared silent-no-op shape.

## [0.9.10] - 2026-05-03

Structural fix for the `scope_filter:` silent-no-op bug class.
v0.9.6 / v0.9.7 / v0.9.9 each shipped a different group of rules
that silently dropped `scope_filter:` because rules held a
standalone `Option<ScopeFilter>` field separate from the `Scope`
that owned the path-glob match — the compiler had no way to
catch a rule that forgot to consult both. v0.9.10 collapses the
two so a single `Scope::matches(path, &FileIndex)` covers both
predicates. The bug class can no longer recur.

### Breaking — alint-core API

- **`alint_core::Scope::matches` signature changed** from
  `(&Path)` to `(&Path, &FileIndex)`. Required so the
  `ScopeFilter` ancestor walk can consult the index. CLI users
  + bundled rulesets unaffected; `alint-core`-as-a-library
  consumers (out-of-tree plugin authors) will see a compile
  error at every call site and must thread their `&FileIndex`
  through.
- **`alint_core::Rule::scope_filter()` trait method removed.**
  Any rule that overrode it should drop the override; the
  rule's `Scope` (built via `Scope::from_spec(spec)?`) now
  carries the filter and the engine consults it via the
  per-file dispatch's `path_scope().matches(path, index)`
  call.
- **New `alint_core::Scope::from_spec(spec)` constructor**
  bundles `paths:` + `scope_filter:` parsing into one call.
  Replaces the per-rule `Scope::from_paths_spec(paths)?` +
  `scope_filter: spec.parse_scope_filter()?` two-line
  pattern.
- **New `alint_core::Scope::scope_filter()` accessor**
  exposes the optional filter for dispatch sites that need
  to consult it without going through `matches` (e.g. for
  custom narrowing logic). Most callers should not need it.

### Internal

- **41 rules cleaned up** to drop their now-redundant
  `scope_filter: Option<ScopeFilter>` field, the
  `if let Some(filter) = ...` runtime check inside
  `evaluate()`, and the `impl Rule { fn scope_filter() }`
  override. -502 LOC across the rule files.
- **Engine consultation removed** —
  `crates/alint-core/src/engine.rs::run_per_file` no longer
  separately checks `entry.rule.scope_filter()` after the
  `path_scope().matches()` call; the latter covers it.
- **`for_each_dir` literal-path bypass simplified** — the
  v0.9.9 `nested_rule.scope_filter()` guard is now
  redundant (subsumed by `pf.path_scope().matches(literal,
  ctx.index)`).
- **New audit test** `coverage_audit_scope_owns_filter.rs`
  fails CI if any rule re-introduces a standalone
  `scope_filter: Option<ScopeFilter>` field or re-declares
  the deleted `Rule::scope_filter` trait method. Catches
  recurrence at PR time, not at runtime.

### Performance

- Within ±5 % of v0.9.9 across S1-S10 at all sizes — the per-
  rule iteration shape is unchanged (one `if !scope.matches(p,
  idx) { continue; }` per file), only the field layout is
  flatter. Full numbers in
  [`docs/benchmarks/macro/results/linux-x86_64/v0.9.10/`](docs/benchmarks/macro/results/linux-x86_64/v0.9.10/).

### Held for v0.9.11+

- **`git_tracked_only` ownership** parity with `scope_filter`
  was scoped out — different field semantics, separate work.
  The same recurrence risk applies (a rule could ship with
  `git_tracked_only:` silently dropped) but the field is
  rarer in practice and currently 100 % wired up. Tracked.

## [0.9.9] - 2026-05-03

Patch release for two `scope_filter:` silent-no-op gaps surfaced
by the post-v0.9.8 audit. v0.9.7 wired the filter through the
25 per-file content rules; v0.9.9 closes the residual coverage on
the 17 rules that bypass the engine's per-file dispatch path AND
the `for_each_dir` literal-path bypass introduced in v0.9.8.

### Fixed

- **17 rules now honour `scope_filter:`.** The rules whose
  `Rule::evaluate` iterates `ctx.index.files()` directly
  (rather than opting into the engine's per-file dispatch) had
  no `scope_filter` plumbing at all — the gate silently dropped
  on the way through `build()`, identical shape to the v0.9.6 →
  v0.9.7 bug for per-file content rules. Affects:
  `file_max_size`, `file_min_size`, `no_empty_files`,
  `executable_bit`, `executable_has_shebang`,
  `shebang_has_executable`, `no_symlinks`, `filename_case`,
  `filename_regex`, `no_illegal_windows_names`,
  `max_files_per_directory`, `max_directory_depth`,
  `json_schema_passes`, `command`, `git_blame_age`,
  `no_case_conflicts`. Each rule now stores
  `scope_filter: Option<ScopeFilter>`, exposes it via
  `Rule::scope_filter()`, and short-circuits its
  `ctx.index.files()` loop on out-of-scope files (preserving
  the per-file iteration shape — only the work done per file
  changes). Per-rule unit tests assert narrowing behaviour.
- **`no_submodules` rejects `scope_filter:` at build time.** The
  rule is hardcoded to inspect `.gitmodules` at the repo root —
  it does not iterate the file index, so a `scope_filter` on it
  is meaningless. Build-time rejection via the new
  `reject_scope_filter_with_reason(spec, kind, reason)` helper
  in `alint-core::scope_filter`.
- **`for_each_dir` literal-path bypass now consults `scope_filter`.**
  v0.9.8's fast path (dispatching a nested per-file rule via
  `PerFileRule::evaluate_file` against a single literal,
  bypassing the rule's own `evaluate`) executed regardless of
  whether the literal's ancestor chain satisfied the nested
  rule's `scope_filter:`. Divergent from the rule-major
  fallback, which already honoured the filter (since v0.9.7).
  The bypass guard at
  `crates/alint-rules/src/for_each_dir.rs::evaluate_for_each`
  now consults `nested_rule.scope_filter()` before taking the
  fast path. E2e regression at
  `crates/alint-e2e/scenarios/check/scope_filter/scope_filter_nested_under_for_each_dir.yml`.

### Added

- **`alint_core::reject_scope_filter_with_reason(spec, kind, reason)`**
  — symmetric to `reject_scope_filter_on_cross_file`, but for
  rules whose dispatch shape is hardcoded to a single path. The
  `reason` field surfaces *why* the rule cannot honour the
  filter (improves the error message authors get).
- **Macro bench scenario S10** at
  `xtask/src/bench/scenarios/s10_scope_filter_outside_per_file.yml`
  — 5 rules from outside the PerFileRule dispatch path
  (`file_max_size`, `no_empty_files`, `no_symlinks`,
  `filename_case`, `filename_regex`) each with
  `scope_filter: { has_ancestor: <manifest> }` over the polyglot
  tree. Catches the same bug class S9 catches for per-file
  rules, but on the rule set v0.9.8 silently dropped.
- **17 unit tests** (`scope_filter_narrows`) — one per rule —
  asserting in-scope / out-of-scope narrowing.
- **1 e2e regression** for bug #2 covering the for_each_dir
  literal-path bypass shape.

### Changed

- **`docs/design/v0.9/scope-filter.md`** gains a "Post-v0.9.6
  follow-ups" section documenting the v0.9.7, v0.9.9, and
  v0.9.10 trajectory — captures *why* this kept happening
  (separate `Scope` + `Option<ScopeFilter>` fields without
  compiler-enforced wiring) and the v0.9.10 structural fix
  that prevents recurrence.

### Performance

- 1k/10k/100k S10 numbers within ±5 % of v0.9.8 baseline (the
  per-file iteration shape is unchanged; only the per-file
  cost of an in-scope vs out-of-scope check shifts). The
  point of S10 is regression detection, not a speedup
  headline. Full numbers in
  [`docs/benchmarks/macro/results/linux-x86_64/v0.9.9/`](docs/benchmarks/macro/results/linux-x86_64/v0.9.9/).

### Internal

- All 393 alint-rules tests + 227 e2e scenarios pass.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  clean.

## [0.9.8] - 2026-05-02

Cross-file dispatch fast paths, round 2. v0.9.5 closed the
`for_each_dir × file_exists` cliff via the lazy `path_set` index;
v0.9.8 closes the residual O(D × N) shapes that v0.9.5 didn't
cover, identified by the comprehensive v0.9.5/.6/.7 macro-bench
backfill (1M S7 was stuck at ~614 s across all three releases).

### Performance

- **1M S7 full: 614 s → 15.3 s (40× speedup).**
- **1M S7 changed: 618 s → 17.9 s (34× speedup).**
- 1M S6 / S8 / S9 within ±5 % of v0.9.7 (no regression on the
  per-file dispatch fast paths).
- See [`docs/benchmarks/HISTORY.md`](docs/benchmarks/HISTORY.md)
  per-scenario tables for the full v0.9.5 → v0.9.8 trajectory
  and the bench captures under
  [`docs/benchmarks/macro/results/linux-x86_64/v0.9.8/`](docs/benchmarks/macro/results/linux-x86_64/v0.9.8/).

### Added

- **`FileIndex::children_of(dir) -> &[usize]`** — direct
  children of `dir`, by index into `entries`. Lazy `OnceLock`
  build mirrors the v0.9.5 `path_set` shape; O(N) build, O(1)
  per-dir lookup.
- **`FileIndex::file_basenames_of(dir) -> impl Iterator<Item = &str>`**
  — direct file children's basenames, borrowed from the
  underlying `Arc<Path>`. Used by `dir_contains` and equivalent.
- **`FileIndex::descendants_of(dir) -> impl Iterator<Item = &FileEntry>`**
  — recursive, depth-first, lazy. Stack-of-iterators state;
  does NOT materialise the full subtree.
- **Debug-only tracing for index builds** — emits `phase=index_build
  kind=<name> elapsed_us=N entries=M` events behind
  `#[cfg(debug_assertions)]`. Release builds compile away both
  the `Instant::now()` timer and the event emission entirely
  (zero overhead for users running release binaries; the events
  are for `xtask bench-scale` profile runs and contributor
  debugging).
- **`coverage_audit_cross_file_dispatch.rs`** — soft-fail audit
  that scans cross-file rule sources for the O(D × N)
  `entries.iter()` pattern v0.9.8 closed. Surfaces gaps for the
  next refactor; doesn't block unrelated work.

### Changed

- **`dir_only_contains`** evaluates via `children_of` instead of
  the per-dir `for file in ctx.index.files()` + `is_direct_child`
  filter. At 1M files / 5K matched dirs, drops the inner loop
  from 1M iterations to ~200 (the dir's actual children).
- **`dir_contains`** evaluates via `children_of` + inline
  basename extraction. Per-(dir, matcher) drops from O(N) to
  O(children).
- **`evaluate_for_each` in `for_each_dir.rs`** (shared by
  `for_each_dir`, `for_each_file`, `every_matching_has`) gained
  a literal-path bypass: when a nested rule's `paths:` template
  resolves to a single literal AND the rule opts into
  `as_per_file()`, dispatch via `evaluate_file` against the
  in-index entry directly instead of running the rule's full
  `evaluate(ctx)` (which would iterate `ctx.index.files()` per
  call, multiplying the 1M scan by the iteration count). Closes
  the residual cliff Phase E iter 1 surfaced (S7's
  `every-lib-has-content` was 484 s post-Phase-C; this drops it
  to milliseconds × N iterations).

### Internal

- New helpers `nested_spec_single_literal` and
  `evaluate_one_per_file_rule` in `crates/alint-rules/src/for_each_dir.rs`.
- `is_direct_child` free function deleted from
  `dir_only_contains.rs` (no longer needed once `children_of`
  is the dispatch shape).
- Memory cost of the new `parent_to_children` index: ~500 KB
  HashMap + ~8 MB usize indices at 1M files / 5K dirs.
  Negligible vs the existing 1 GB `entries` vec.
- All 376 alint-rules tests + 226 e2e scenarios + 183 alint-core
  tests + 11 new walker tests pass.

## [0.9.7] - 2026-05-02

Patch release for the v0.9.6 `scope_filter:` runtime no-op (v0.9.6
shipped the field, the type, and the engine gate but never wired the
parsed filter onto the built rule). Same release also lands the
follow-up audit cleanup (rule-count corrections, design-doc status
flips, alint.org sidebar gaps, release.yml preflight gate) and the
v0.10 LSP design pass with `tower-lsp` added to workspace deps as a
dormant dependency.

### Fixed

- **`scope_filter:` is now honoured at runtime by every per-file rule.**
  v0.9.6 shipped the field on `RuleSpec`, the `ScopeFilter`/`ScopeFilterSpec`
  runtime types, and the engine's per-file dispatch gate
  (`crates/alint-core/src/engine.rs:run_per_file`), but no per-file rule
  builder threaded the parsed filter onto the built rule — `Rule::scope_filter()`
  always returned the trait default `None`, so the gate was a silent no-op
  for every rule (including the bundled ecosystem rulesets that motivated
  the primitive). Each of the 25 per-file rule types now stores a parsed
  `Option<ScopeFilter>`, exposes it via `Rule::scope_filter()`, and gates
  its rule-major fallback path on it as well (so `alint fix` respects the
  filter the same way `alint check` does). New `RuleSpec::parse_scope_filter()`
  helper in `alint-core::config` keeps the per-rule build-site change to
  one line. Integration regression test:
  `crates/alint-rules/tests/scope_filter_integration.rs`.

### Added

- **5 e2e scenarios** under `crates/alint-e2e/scenarios/check/scope_filter/`
  covering positive/negative cases, two-name lists, `extends:` inheritance,
  combined per-file + tree-level gates, and a bundled `rust@v1` dogfood
  against a polyglot tree.
- **10 cross-file `reject_scope_filter_on_cross_file` unit tests**, one per
  cross-file rule that calls the rejection helper (only `pair.rs` had one
  before).
- **`docs/design/v0.10/`** — three-doc design pass for the v0.10 LSP server
  (`lsp_server.md`, `single_file_reevaluation.md`, `vscode_extension.md`)
  mirroring the per-cut design-pass shape used for v0.7 and v0.9.
- **`tower-lsp = "0.20"`** added to `[workspace.dependencies]` ahead of the
  `crates/alint-lsp/` crate landing in v0.10. Dormant — no current member
  depends on it, so `cargo build` doesn't pull it.
- **`development/rule-authoring.md`** now reaches alint.org via
  `xtask docs-export` (previously invisible to the sync script).

### Changed

- **`release.yml` preflight gate**: new top-level job runs fmt + clippy +
  test + `cargo doc -D warnings` on the tagged commit; every publishing
  job (build matrix, release, docker, npm, homebrew, publish-crates)
  `needs:` it. Closes the gap where release.yml could fire while ci.yml
  was failing on the same SHA.
- **`README.md`** rule-count + family-count corrections (54 → 60 across
  thirteen families; was missing `Git hygiene` and `Plugin (tier 1)`),
  ruleset count reconciled (line 28 said 19, line 589 said 17 — both now
  19), version anchor "v0.7 ships" → "v0.9.6 ships".
- **`docs/design/v0.7/{commented_out_code,markdown_paths_resolve}.md`,
  `v0.9/{parallel_walker,scope-filter}.md`** — status headers flipped
  from "Design draft" to "Implemented in v0.7.x / v0.9.x" with crate
  pointers.
- **`docs/site/**` integration pages** — 12 instances of `v0.4.7` updated
  to v0.9.6; `concepts/index.md` "Four formats" → "Eight formats";
  `quickstart.md` ruleset count "eleven" → "nineteen".
- **`action.yml`** format input description lists all 8 formats.
- **`npm/package.json`** version 0.5.10 → 0.9.7 (was four releases stale
  in the checked-in source; release.yml's publish step rewrites this from
  the tag at publish time, but a stale value misled readers).
- **`action-selftest.yml`** pinned-version test 0.3.1 → v0.9.6 (six minors
  stale).
- **`.alint.yml`** dogfood fact `is_rust` → `has_rust` to match the v0.9.6
  bundled-fact rename.

## [0.9.6] - 2026-05-02

Closes the v0.9 cut with the `scope_filter:` primitive — closest-
ancestor manifest scoping for per-file rules, designed to make the
bundled ecosystem rulesets (`rust@v1`, `node@v1`, `python@v1`,
`go@v1`, `java@v1`) correctly handle nested packages in polyglot
monorepos.

### Added

- **`scope_filter: { has_ancestor: <list> }` rule-schema field**
  (per-file rules only). The engine walks `Path::parent()` upward
  from the file and consults the v0.9.5 path-index at each
  directory; first-match-wins on the upward walk gates the rule
  per-file. The file's own directory counts as an ancestor.
  Cross-file rules (`pair`, `for_each_dir`, `file_exists`, etc.)
  reject `scope_filter:` at build time. Design:
  [`docs/design/v0.9/scope-filter.md`](docs/design/v0.9/scope-filter.md).
- **`alint_core::ScopeFilter` + `ScopeFilterSpec` runtime types,
  `Rule::scope_filter()` trait method** (default `None`). Net
  additive — rules that don't override the trait method see no
  behaviour change. New `alint_core::reject_scope_filter_on_cross_file`
  helper + 11 cross-file rule build-time guards.
- **Bench-scale S9 — nested polyglot monorepo** scenario.
  `extends: rust + node + python` over `crates/` + `packages/` +
  `apps/` ecosystem subtrees; new
  `alint_bench::tree::generate_nested_polyglot_monorepo` helper.
  100k S9 = 688 ms ± 13 ms on the published baseline machine.
- **Rule-authoring docs**: `scope_filter:` section in
  [`rule-authoring.md`](docs/development/rule-authoring.md) with
  the bundled-ruleset migration recipe.

### Changed

- **Bundled ecosystem rulesets renamed `is_*` → `has_*` for the
  ecosystem facts.** Five rulesets:
  `is_rust` → `has_rust`, `is_node` → `has_node`,
  `is_python` → `has_python`, `is_go` → `has_go`,
  `is_java` → `has_java`. The new name reads better with the
  broadened heuristic (`has_<ecosystem>` is true if any
  ecosystem manifest exists *anywhere* in the tree, not just at
  the root). No backwards-compat alias period — repos that
  override these facts in their own `facts:` blocks need to
  update the identifier; tree-level gates that referenced the
  fact name (`when: facts.is_rust` etc.) need the same update.
- **Bundled ecosystem facts broadened with `**/<manifest>`
  patterns** — `[Cargo.toml]` → `[Cargo.toml, "**/Cargo.toml"]`
  and analogously for the other four. Polyglot monorepos with
  no root manifest now correctly fire the ruleset.
- **Per-file content rules in the five ecosystem rulesets get
  `scope_filter:` added** (e.g.
  `scope_filter: { has_ancestor: Cargo.toml }` on
  `rust-sources-final-newline`). Filename-based rules
  (`filename_case`, etc.) keep their existing `paths:` globs;
  `scope_filter:` is supported on `PerFileRule`-trait rules
  only.

### Performance

`scope_filter:` adds ≤ 150 ns per (file × rule) ancestor-walk
step on the v0.9.5 path-index — sub-millisecond at K100 across
five rules. Same-machine pre/post measurement on the engine
commit (`7b080a0`) shows the null-default `Rule::scope_filter()`
trait method is unmeasurable on rules that don't override it.
See [`docs/benchmarks/investigations/2026-05-scope-filter-baseline-drift/`](docs/benchmarks/investigations/2026-05-scope-filter-baseline-drift/)
for the dispositive A/B and the lesson on machine-state drift in
cross-version bench comparisons.

## [0.9.5] - 2026-05-01

Reopens v0.9 with cross-file dispatch fast paths, the
test/coverage floor that prevents the same class of regression
from slipping by again, and a benchmark-docs reorganisation
that makes the perf story discoverable from a single entry
point. Six sub-phases, all in this release:

| Sub-phase | What it ships |
|---|---|
| .5 | Cross-file dispatch fast paths (lazy path-index on `FileIndex` + literal-path fast paths in `file_exists` / `structured_path` / `iter.has_file`) |
| .6 | Coverage audits (pass/fail symmetry, bundled-ruleset coverage, git-mode symmetry) |
| .7 | 16 new coverage scenarios filling the audit punch list |
| .8 | Bench-scale S6 / S7 / S8 + `generate_git_monorepo` helper |
| .9 | Rule-authoring workflow doc; alint already self-lints via `action-selftest.yml` |
| Reorg | Benchmark documentation reorganisation: `docs/benchmarks/{micro,macro,investigations,archive}/` layout, per-version results, top-level `README.md` / `HISTORY.md` / `RUNNING.md`, new `xtask publish-benches` subcommand |

No new rule kinds, formatters, or schema changes; output bytes
byte-identical to v0.9.4 across all 8 formatters.

Full design:
[`docs/design/v0.9/coverage-and-dogfood.md`](docs/design/v0.9/coverage-and-dogfood.md).
Workflow: [`docs/development/rule-authoring.md`](docs/development/rule-authoring.md).
Bench layout: [`docs/benchmarks/README.md`](docs/benchmarks/README.md).

### Performance (.5)

`xtask gen-monorepo --size 1m`, S3 = `oss-baseline + rust +
monorepo + cargo-workspace`, hyperfine `--warmup 1 --runs 3`:

| Cell | v0.9.4 | v0.9.5 | Speedup |
|---|---:|---:|---:|
| `1m S3 full` | 731.856 s | 11.194 s ± 0.154 | **65.4×** |
| `1m S3 changed` | 724.362 s | 6.728 s ± 0.059 | **107.7×** |
| `100k S3 engine total` | 10.7 s | 186 ms | **57×** |
| `10k S3 engine total` | 226 ms | 23 ms | **9.8×** |

Also ~50–80× faster than the published v0.5.6 baseline (the
fastest 1M S3 numbers ever shipped). Full numbers under
[`docs/benchmarks/macro/results/linux-x86_64/v0.9.5/`](docs/benchmarks/macro/results/linux-x86_64/v0.9.5/).

### Engine (.5)

- `alint-core::walker::FileIndex` gains a lazy
  `OnceLock<HashSet<Arc<Path>>>` keyed on every file (non-
  dir) entry. Built on first call to `contains_file` or
  `file_path_set`; concurrent first-call safe via OnceLock.
- New `FileIndex::contains_file(&Path) -> bool` — the
  canonical O(1) "does this exact relative path exist?"
  query. `find_file(&Path)` keeps its signature but does
  the O(1) check first; the linear `&FileEntry` scan only
  runs on a hit. New `FileIndex::from_entries(Vec<FileEntry>)`
  constructor for tests/benches.
- `Engine::run` adds `tracing::info!` per-phase + per-cross-
  file-rule wall-time emission with stable structured fields
  (`phase`, `elapsed_us`, optional `rules` / `files`).
  Drive with `ALINT_LOG=alint_core=info`. Production runs
  pay nothing — events fire only when info is enabled for
  this target.
- Per-file dispatch loop in `Engine::run_per_file` switches
  from `index.files().par_bridge()` to
  `index.entries.par_iter().filter(|e| !e.is_dir)` — the
  native Rayon `ParallelIterator` over the underlying `Vec`
  uses work-stealing slabs instead of par_bridge's Mutex-
  guarded channel. The applicable-rule inner loop carries
  `entry_idx` directly instead of re-resolving via O(L)
  `position` lookup per applicable rule per file.

### Rules (.5)

- `file_exists` — when every `paths:` entry is a literal
  relative path AND the rule is not `git_tracked_only`,
  the build path pre-extracts the literals into
  `Vec<PathBuf>`; evaluate uses `contains_file` per
  literal. Glob/exclude/git-tracked patterns keep the
  existing O(N) scope-match scan.
- `structured_path` (the shared impl behind
  `*_path_{equals,matches}` for json/yaml/toml) — same
  literal-paths fast path. Joins `ctx.root + literal`
  directly for the `std::fs::read` rather than calling
  `find_file`.
- `iter_has_file` (the `iter.has_file("…")` `when_iter:`
  builtin) — when the pattern is a literal filename,
  computes `iter.path.join(pattern)` and consults
  `contains_file`. Glob patterns (`**/*.bzl`) keep the
  scope-match scan.
- `pair::evaluate` swaps `find_file(&p).is_some()` for the
  cheaper `contains_file(&p)`.

### Coverage audits (.6)

Three new tests under `crates/alint-e2e/tests/`:

- **`coverage_audit_pass_fail.rs`** — every canonical rule
  kind has at least one scenario where it fires AND at
  least one where it stays silent. Ships with a small
  `NATIVE_FIRES_ALLOWLIST` for kinds whose firing case
  can't be expressed in YAML today (`executable_bit`,
  `executable_has_shebang`, `no_symlinks`, `git_blame_age`,
  `git_commit_message`); each entry points at the native
  Rust integration test that DOES cover the firing path.
- **`coverage_audit_bundled_rulesets.rs`** — every
  `crates/alint-dsl/rulesets/v1/**/*.yml` is referenced by
  at least one well-formed scenario AND at least one
  ill-formed scenario.
- **`coverage_audit_git_modes.rs`** — the three pure-git
  rule kinds (`git_blame_age`, `git_commit_message`,
  `git_no_denied_paths`) need both an in-repo and an
  outside-git scenario.
- **`coverage_audit_bench_listing.rs`** (soft, always
  passes) — emits an `eprintln!` listing of rule kinds
  absent from any bench scenario.

### Coverage scenarios (.7)

16 new scenarios filling the v0.9.6 audit punch list:

- `structured/{json,toml,yaml}_path_{equals,matches}_silent_when_satisfied.yml` (6)
- `bundled/{agent_context,agent_hygiene,ci_github_actions,docs_adr,hygiene_lockfiles,hygiene_no_tracked_artifacts,tooling_editorconfig}_well_formed_passes.yml` (7)
- `bundled/{agent_context_stub,agent_hygiene_scratch_doc}_flagged.yml` (2)
- `git/git_no_denied_paths_silent_outside_git.yml` (1)

E2e scenario count: 205 → 221. All four `coverage_audit_*`
tests green by default.

### Bench-scale extension (.8)

Three new perf-shape scenarios; S1–S5 unchanged:

- **S6 — Per-file content fan-out**: 13 content rules over
  `**/*.rs`.
- **S7 — Cross-file relational**: `pair`, `unique_by`,
  `for_each_dir`, `for_each_file`, `dir_only_contains`,
  `every_matching_has`.
- **S8 — Git-tracked overlay**: S3 reshape with `.git/` +
  `git_no_denied_paths` + `git_tracked_only`.

New `alint_bench::tree::generate_git_monorepo` helper for
S8 — runs `git init && git add -A && git commit` on a
materialised monorepo tree. `xtask` enum extends to
`Scenario::S1..S8`; default `--scenarios` stays `S1,S2,S3`.

### Workflow doc + dogfood scope (.9)

- New `docs/development/rule-authoring.md` — the four-step
  process every new rule / bundled ruleset / alias goes
  through; documents the two-layer enforcement (alint
  check . + Rust audits), family conventions, scenario
  shape, the native-test allowlist for testkit gaps, and
  the concrete audit failures contributors will hit.
- The aspirational declarative-coverage rules in
  `.alint.yml` (e.g. "every rule-source file has ≥1 e2e
  scenario") need a rule kind alint doesn't yet have:
  aggregate "does any file in this scope contain pattern
  X?" semantics. Today's `file_content_matches` is
  per-file; the right primitive is deferred to v0.10+.
- alint still self-lints in CI today via the existing
  `.alint.yml` and `action-selftest.yml`; v0.9.9 just
  formalises the workflow contributors should follow.

### Benchmark documentation reorganisation

The `docs/benchmarks/` tree was shaped accidentally over four
release cycles and had four different per-version layouts.
This release lands a clean micro / macro / investigations /
archive split:

- **Top-level** — `README.md` (entry point with current
  numbers), `HISTORY.md` (per-release perf changelog),
  `RUNNING.md` (how-to-run), `METHODOLOGY.md` (focused
  rewrite of the why-behind-the-split).
- **`micro/`** — criterion micro-benches with a per-bench
  catalogue and per-version `results/<arch>/<version>/`
  snapshots.
- **`macro/`** — hyperfine bench-scale with the S1–S8
  catalogue, tool matrix, and per-version
  `results/<arch>/<version>/` snapshots.
- **`investigations/`** — ad-hoc deep-dives (was
  `docs/perf/`); the v0.9.5 cliff investigation lives at
  `2026-05-cross-file-rules/`.
- **`archive/`** — superseded snapshots (v0.1 single-file
  era, v0.9 development-cycle baselines + per-phase
  outputs). Read-only by contract.

`xtask bench-scale --out` default fixes the long-standing
footgun where every run silently overwrote
`docs/benchmarks/v0.5/scale/<arch>/`; new default is
`docs/benchmarks/macro/results/<arch>/v<workspace-version>/`,
read from the workspace `Cargo.toml` so it tracks the
version as it bumps. New `xtask publish-benches` snapshots
`target/criterion/` into the per-version published
location with an optional `--trim` flag dropping criterion's
HTML reports.

### Tooling

- New `xtask gen-monorepo --size {1k|10k|100k|1m} --out PATH`
  materializes a persistent monorepo tree at a fixed path.
  Used by the perf-investigation flow to skip 5+ minutes of
  tree-gen between profile runs. Size labels match
  `bench-scale`'s internal monorepo shape, so trees are
  byte-identical to the published bench corpus.

## [0.9.4] - 2026-04-30

Mechanical follow-up to v0.9.3: migrates 16 of the
remaining ~22 per-file content rules to opt into the
file-major dispatch path. After this cut, every per-file
rule that reads file content benefits from the v0.9.3
read coalescing whenever multiple content rules share a
scope. No new rule kinds, formatters, or subcommands;
every v0.9.3 config runs unchanged. Output bytes from
all 8 formatters are byte-identical to v0.9.3.

### Performance

Each migrated rule grows a `PerFileRule` impl alongside
its existing `Rule` impl; the existing `Rule::evaluate`
body becomes a thin wrapper that walks the index, reads
each file, and delegates to `evaluate_file` (used by
`alint fix` and test harnesses that bypass the engine).

**Migrated (16 rules):**

Content-pattern (5): `file_content_matches`,
`file_content_forbidden`, `file_header`, `file_footer`,
`file_shebang`.

File-property (5): `file_max_lines`, `file_min_lines`,
`file_hash`, `file_is_ascii`, `file_is_text` (declares
`max_bytes_needed: TEXT_INSPECT_LEN`).

Unicode-safety (4): `no_bom` (declares
`max_bytes_needed: 4`; rule-major path uses the bounded
`read_prefix_n` helper), `no_bidi_controls`,
`no_zero_width_chars`, `no_merge_conflict_markers`.

Complex-content (2): `markdown_paths_resolve` (uses
`ctx.index` in `evaluate_file` to look up backticked
paths), `structured_path` (which backs `json_path_*`,
`yaml_path_*`, `toml_path_*`).

### Deliberately not migrated

- `file_max_size` / `file_min_size` — metadata-only
  checks, no `fs::read` to coalesce.
- `json_schema_passes` — emits one repository-level
  violation when the schema fails to compile; the per-
  file dispatch path would multiply that 1 schema-error
  into N file-errors.

### Build / test

- Workspace test suite green (1000+ tests across 7
  crates).
- Pre-v0.9.4 baseline frozen at
  `docs/benchmarks/archive/v0.9-development-baselines/baseline-v0.9.3/criterion/`.
  v0.9.4 numbers at
  `docs/benchmarks/micro/results/linux-x86_64/v0.9.4/criterion/`.

## [0.9.3] - 2026-04-30

Third phase of the v0.9 engine-optimization cut: the
per-file dispatch flip plus the per-rule scanning
conversions deferred from v0.9.2. No new user-visible rule
kinds, formatters, or subcommands; every v0.9.2 config
runs unchanged. Output bytes from all 8 formatters are
byte-identical to v0.9.2.

### Performance

- **Per-file dispatch flip.** New `PerFileRule` trait next
  to `Rule`; rules opt in via `Rule::as_per_file(&self) ->
  Option<&dyn PerFileRule> { None }`. The engine partitions
  entries: per-file rules run under a file-major loop that
  reads each matched file once and dispatches to every
  applicable rule against the same byte buffer. Cross-file
  rules (`pair`, `for_each_dir`, `every_matching_has`,
  `unique_by`, `dir_*`, `file_*` existence rules,
  `git_no_denied_paths`, `git_commit_message`) keep the
  rule-major `par_iter` path. Coalesces N reads of one file
  across N rules sharing it.
- **Line-oriented rules migrated to `PerFileRule` + byte-
  slice scanning** (deferred from v0.9.2):
  `no_trailing_whitespace` (skips the redundant UTF-8
  validation pass via `bytes.split(|&b| b == b'\n')` +
  `last()` byte check), `final_newline` (bounded `last()`
  byte check; `max_bytes_needed: Some(1)`), `line_endings`
  (existing byte-level walker preserved through
  `evaluate_file`), `max_consecutive_blank_lines`,
  `indent_style`, `line_max_width` (the latter three keep
  per-line UTF-8 validation where character counts /
  whitespace classifiers need `&str`).
- **Bounded prefix/suffix reads on byte-pattern rules**
  (deferred from v0.9.2): `file_starts_with` reads only the
  first `prefix.len()` bytes; `file_ends_with` reads only
  the last `suffix.len()` bytes via a new `read_suffix_n`
  helper that seeks from EOF. Both opt into `PerFileRule`
  and declare `max_bytes_needed`.
  `executable_has_shebang` and `shebang_has_executable`
  switch their rule-major paths to bounded 2-byte reads
  (`#!`); they deliberately skip the dispatch-flip path
  because they need `metadata().permissions()` to
  short-circuit on non-`+x` files before any read happens.

### Internal

- New `crates/alint-rules/src/io.rs` helpers:
  `read_prefix_n(path, n)` and `read_suffix_n(path, n)`.
- `Rule::evaluate` for migrated rules becomes a thin
  wrapper that walks the index, reads each file, and
  delegates to `evaluate_file` — used by `alint fix`
  (sequential filesystem mutation rules out coalesced
  reads there) and by test harnesses that bypass the
  engine.
- `Engine::run` aggregates per-file violations back into
  per-rule `RuleResult`s preserving each rule's metadata
  (level / policy_url / is_fixable). Final assembly walks
  `self.entries` in order so output remains deterministic.
- `when:` evaluates once per per-file rule before the file
  loop (not per file) — verdict is independent of the file
  since `iter` is `None` at the engine level.

### Tests

- Four new engine tests exercising the dispatch flip:
  `dispatch_flip_routes_per_file_rule_through_file_major_loop`,
  `dispatch_flip_aggregates_multiple_per_file_rules`
  (verifies aggregation buckets violations per rule across
  N rules sharing one scope),
  `dispatch_flip_passes_when_no_violations` (passing rules
  omitted), `dispatch_flip_preserves_cross_file_rules_unchanged`.

### Migration scope

8 of ~30 per-file content rules migrated to `PerFileRule`
in v0.9.3 (the line-oriented + bounded-read families
deferred from v0.9.2). The remaining ~22 content rules
(`file_content_matches`, `file_content_forbidden`,
`file_header`, `file_footer`, `file_max_lines`,
`file_min_lines`, `file_max_size`, `file_min_size`,
`file_hash`, `file_is_ascii`, `file_is_text`,
`file_shebang`, `json_path_*` / `yaml_path_*` /
`toml_path_*`, `json_schema_passes`, `no_bom`,
`no_bidi_controls`, `no_zero_width_chars`,
`no_merge_conflict_markers`, `markdown_paths_resolve`,
`commented_out_code`) keep the rule-major path. They'll
migrate incrementally in v0.9.4 as a mechanical follow-up;
v0.9.3 ships the engine restructure + 8-rule reference
implementation that proves the shape works.

### Out of scope (deferred)

- Engine-side honouring of the `max_bytes_needed()` hint —
  v0.9.3 reads whole files; a future pass could bound the
  read when every applicable rule's hint sums to less than
  the file size. Hint is captured on the trait so rules
  can declare it now.

### Build / test

- Workspace test suite green (1000+ tests across 7
  crates).
- Pre-v0.9.3 baseline frozen at
  `docs/benchmarks/archive/v0.9-development-baselines/baseline-v0.9.2/criterion/`.
  v0.9.3 numbers at
  `docs/benchmarks/archive/v0.9-development-phases/v0.9.3-dispatch-flip/criterion/`.

## [0.9.2] - 2026-04-30

Second phase of the v0.9 engine-optimization cut: the
type-level memory pass. No new user-visible rule kinds,
formatters, or subcommands; every v0.9.1 config runs
unchanged. Output bytes from all 8 formatters are
byte-identical to v0.9.1.

### Performance

- **`Arc<Path>` on `FileEntry::path` and
  `Violation::path`.** The walker builds one `Arc::from`
  per file; every `Violation` referencing that file now
  shares the same allocation via `Arc::clone` (atomic
  refcount bump) instead of copying the path bytes. At
  100k violations that's ~100k saved path allocations.
  Canonical caller pattern is now
  `.with_path(entry.path.clone())`.
- **`Arc<str>` on `RuleResult::rule_id` and
  `policy_url`.** Set once per rule by the engine, shared
  across N violations of that rule (one alloc per rule
  instead of per violation).
- **`Cow<'static, str>` on `Violation::message`.** Per-
  match templated messages (`format!("line {n}:…")`) live
  as `Cow::Owned(String)` (no change in cost); fixed
  messages can live as `Cow::Borrowed("…")` for a future
  audit pass. Public API preserves `&v.message: &str`
  via `Cow::as_ref`.

### Internal

- `Violation::with_path(impl Into<Arc<Path>>)` accepts
  `PathBuf`, `&Path`, `Box<Path>`, and `Arc<Path>`. Tests
  using a string literal migrate to
  `with_path(Path::new("…"))`.
- `Violation::new(impl Into<Cow<'static, str>>)` accepts
  `&'static str`, `String`, and `Cow<'static, str>`.
  Borrowed `&str` from non-static lifetimes pass via
  `.to_string()`.
- ~70 rule call sites and 8 output formatter structs
  migrated. Output formatters that previously held
  `Option<PathBuf>` / `String` internally now borrow
  `Option<&'a Path>` / `&'a str` from the source where
  possible (json, agent, markdown), or convert at the
  serialise boundary via `.to_string()` / `.as_deref()`
  where the format requires owned.
- `human` formatter: `BTreeMap` keyed on
  `Option<Arc<Path>>` instead of `Option<PathBuf>` — sort
  order unchanged (Arc<Path> sorts via Path's Ord impl).
- `markdown` formatter: bucket map switches to
  `BTreeMap<&'a Path, …>` — eliminates the per-violation
  path clone the bucketing did before.
- `unique_by` and `no_case_conflicts` group on
  `Arc<Path>` internally instead of `PathBuf`.

### Scope deferred to v0.9.3

The original v0.9.2 design scope included per-rule
byte-slice scanning conversions (the 6 line-oriented rules:
`no_trailing_whitespace`, `final_newline`, `line_endings`,
`max_consecutive_blank_lines`, `indent_style`,
`line_max_width`) and bounded prefix/suffix reads (the 4
first-/last-bytes-only rules: `file_starts_with`,
`file_ends_with`, `executable_has_shebang`,
`shebang_has_executable`). Both moved to v0.9.3 because
the per-file dispatch flip hands each rule a pre-loaded
`&[u8]` slice — bundling the conversions there avoids
touching the same rule bodies twice. See
`docs/design/v0.9/memory_pass.md` and
`docs/design/v0.9/dispatch_flip.md` for the rationale.

### Build / test

- Workspace test suite green (1000+ tests across 7
  crates).
- `target-baseline*/` added to `.gitignore` for the
  baseline-bench scratch dirs the v0.9.x flow uses.
- Pre-v0.9.2 baseline frozen at
  `docs/benchmarks/archive/v0.9-development-baselines/baseline-v0.9.1/criterion/` for
  same-day per-phase delta comparisons. v0.9.2 numbers
  at `docs/benchmarks/archive/v0.9-development-phases/v0.9.2-memory-pass/criterion/`.

## [0.9.1] - 2026-04-30

First phase of the v0.9 engine-optimization cut. Parallel
walker. No user-visible rule kinds, formatters, or
subcommands change; every v0.8 config runs unchanged.

### Performance

- **Walker is now parallel.** `crates/alint-core/src/walker.rs`
  switches from `WalkBuilder::build()` (single-threaded
  iterator) to `WalkBuilder::build_parallel()` driving a
  per-thread `ParallelVisitor` that accumulates `FileEntry`s
  in a thread-local `Vec` and merges via `Drop`. A
  deterministic post-sort by relative path
  (`entries.sort_unstable_by(|a, b| a.path.cmp(&b.path))`)
  restores the byte-identical output that snapshot tests +
  formatters depend on. Walker output is the same as v0.8.2
  for any input tree; only the intermediate execution is
  parallel.

  Numbers vs the captured pre-v0.9 baseline at
  `docs/benchmarks/archive/v0.9-development-baselines/baseline-pre/criterion`:

  | bench | before | after | delta |
  |---|---:|---:|---:|
  | `walker/10000` | 52.25 ms | 18.67 ms | **-64.26%** |
  | `walker/1000` | 8.85 ms | 5.25 ms | **-40.62%** |
  | `walker/100` | 1.62 ms | 2.62 ms | +61.18% |

  The walker/100 regression is the small-N thread-spawn-
  overhead trade the design doc anticipated — 1ms of
  overhead at the 100-file size, dwarfed by the 33.6ms saved
  at 10k files. v0.7.0 gate stays green; max delta -4.77% on
  `rule_engine/1000`.

### Added — design + benchmark snapshots

- **`docs/design/v0.9/`** — design pass for the v0.9 cut.
  README + per-sub-theme drafts for the parallel walker
  (this release), memory-footprint pass (v0.9.2), and
  per-file-rule dispatch flip (v0.9.3). Same shape as
  `docs/design/v0.7/` (Problem → Surface → Semantics →
  False-positive surface → Implementation → Tests → Open
  questions).
- **`docs/benchmarks/archive/v0.9-development-baselines/baseline-pre/`** — frozen
  criterion snapshot captured on `bec0cf4` (the v0.9 starting
  point) so subsequent v0.9 phases have a same-day, same-
  hardware "before" reference to measure deltas against.
- **`docs/benchmarks/archive/v0.9-development-phases/v0.9.1-parallel-walker/`** —
  post-merge bench snapshot for this release, with a README
  documenting the deltas vs both the v0.7.0 floor (the gate)
  and the pre-v0.9 baseline (the per-phase delta).

### Tests

- Three new walker unit tests:
  `walk_output_is_deterministic_across_runs` (two runs over
  the same tree produce equal `FileIndex`s — guard against a
  forgotten post-sort), `walk_output_is_alphabetically_sorted`
  (catches a sort that silently breaks), and
  `walk_handles_thousand_files` (concurrency stress: exactly
  1k entries, sorted, no duplicates / drops).

### Internal

- `walk()` body factored into `build_walk_builder` and
  `result_to_entry` private helpers so the visitor closure
  stays small. No public API change — `walk(root, opts) ->
  Result<FileIndex>` is unchanged.

### Out of scope (v0.9.2 / v0.9.3 sub-themes)

- Memory-footprint pass (Cow audit on `Violation` /
  `RuleResult` / `Report`; line-slice scanning on the line-
  oriented rules; dhat workflow). Designed in
  `docs/design/v0.9/memory_pass.md`; ships as v0.9.2.
- Per-file-rule dispatch flip (new `PerFileRule` sub-trait;
  file-major engine loop; coalesce reads when N rules share
  one file). Designed in `docs/design/v0.9/dispatch_flip.md`;
  ships as v0.9.3.

## [0.8.2] - 2026-04-29

Second hotfix on v0.8.0; supersedes v0.8.1. Same code as
v0.8.0 / v0.8.1; manifest + script changes only.

v0.8.1 reverted `publish = false` on the three internal
crates (the v0.8.0 audit-residue bug) but the next publish
attempt failed at `alint-dsl@0.8.1` with "no matching
package named `alint-rules` found". Cargo publish validates
**dev-dependencies** too, and `alint-dsl` carries
`alint-rules` as a `[dev-dependencies]` entry (added by
v0.8.5's fixture-completeness test). With the original
publish order (`alint-core → alint-dsl → alint-rules →
alint-output → alint`), `alint-dsl` packages before
`alint-rules` is on crates.io.

v0.8.1 thus shipped to GitHub Releases, npm, Homebrew,
Docker, and `alint-core@0.8.1` on crates.io — but
`alint-dsl@0.8.1` and downstream (alint-rules, alint-output,
alint) all failed.

### Fixed

- **`ci/scripts/publish-crates.sh`** — re-ordered the
  `CRATES` list so `alint-rules` and `alint-output` publish
  before `alint-dsl`: `alint-core → alint-rules →
  alint-output → alint-dsl → alint`. The new order
  satisfies every dep direction including dev-deps.
- `cargo install alint` against v0.8.2 resolves cleanly.
  v0.8.0 and v0.8.1 remain partially published on
  crates.io; consumers should pin to v0.8.2 or `latest`.

## [0.8.1] - 2026-04-29 (partially published — superseded by v0.8.2)

Reverted `publish = false` on alint-dsl/rules/output to fix
v0.8.0's crates.io publish failure. Pipeline progressed
further (alint-core@0.8.1 published) but failed on
alint-dsl@0.8.1 because of an unrelated dep-order issue —
see v0.8.2 above for the resolution. Use v0.8.2 instead.

### Changed (carried into v0.8.2)

- **`alint-dsl` / `alint-rules` / `alint-output`** —
  reverted `publish = false`. Their manifests now publish to
  crates.io alongside `alint-core` and `alint`. The crates'
  descriptions still read "Internal: Not a stable public
  API" — the published status is a load-bearing accident of
  cargo's resolver, not an invitation to use them. The
  comment block on each manifest documents the constraint
  so a future audit doesn't repeat the trap.

## [0.8.0] - 2026-04-29

> **Note**: this tag shipped to GitHub Releases, npm, Homebrew,
> and Docker, plus `alint-core` on crates.io — but the `alint`
> binary's crates.io publish failed (see v0.8.1 above for the
> root cause + fix). For `cargo install alint` use v0.8.1+; all
> other distribution channels for v0.8.0 are usable.

The v0.8 cut — five sub-phases (v0.8.1 → v0.8.5) building
the comprehensive test/bench/rot-prevention foundation that
v0.9's engine optimization needs to land safely. No new
user-facing rule kinds, formatters, or subcommands; entirely
internal. Schema-compatible: every v0.7 config runs unchanged.

### Added — test infrastructure

- **Cross-platform CI** — `.github/workflows/cross-platform.yml`
  runs `cargo test --workspace --locked` on macOS-arm64 and
  Windows-x86_64 GitHub-hosted runners on every PR/push.
  Caught two real production bugs the Linux lane missed
  (glob mixed-separator + bench-compare key separator,
  both fixed before merge).
- **Coverage workflow** — `.github/workflows/coverage.yml`
  runs `cargo llvm-cov` over the workspace (xtask excluded
  as dev tooling). Enforces 85% line-coverage floor;
  workspace currently at 90.57%. LCOV + HTML uploaded as
  artifacts; opt-in Codecov upload via `CODECOV_TOKEN`.
- **Mutants nightly** — `.github/workflows/mutants.yml`
  rotates one workspace crate per night through cargo-mutants.
  Surfaces unkilled mutants as test-coverage gaps via
  uploaded artifacts.
- **Comprehensive bench suite** —
  `single_file_rules.rs`, `cross_file_rules.rs`,
  `structured_query.rs`, `output_formats.rs`,
  `fix_throughput.rs`, `blame_cache.rs`, `dsl_extends.rs`
  under `crates/alint-bench/benches/`. Plus hyperfine
  scenarios S4 (agent-hygiene) + S5 (fix-pass) and a walker
  parallelism baseline captured for v0.9.

### Added — rot prevention

- **`xtask bench-compare`** — diffs two `target/criterion/`
  trees and gates on regressions past `--threshold`
  (default 10%). PR-time perf-regression gate.
- **JSON report schemas** —
  `schemas/v1/check-report.json` and
  `schemas/v1/fix-report.json` lock the public contract
  for `alint check --format json` and
  `alint fix --format json`. Cross-formatter and
  fix-report tests validate output against the schemas.
- **Fixture-completeness test** in alint-dsl asserts the
  canonical `all_kinds.yaml` fixture exercises every
  registered rule kind (now 70, up from 18).
- **Scenario-coverage audit** in alint-e2e asserts every
  registered rule kind has at least one e2e scenario.
- **Default-option snapshot** in alint-dsl captures every
  rule's resolved Debug output into a checked-in snapshot.
  Catches silent shifts to `#[serde(default = ...)]`
  values (e.g. `commented_out_code::min_lines`).
- **CLI flag inventory snapshot** captures the full
  per-subcommand flag list separately from `--help` text
  to catch flag-name drift independently of help-text edits.
- **Cross-formatter snapshot test** —
  `crates/alint-output/tests/cross_formatter.rs`. Same
  fixed Report rendered through all 8 output formatters
  with 13 invariant tests (rule_id presence, JSON parses,
  schema_version key, SARIF driver completeness, etc.).
- **Pty integration test** —
  `crates/alint/tests/pty_color.rs` (Unix-only) covers
  the `--color=auto` resolution branch trycmd's pipe-only
  spawn can't reach.
- **`.gitattributes`** pins LF for byte-stable test
  artifacts (snapshots, trycmd `.stdout`/`.stderr`/`.toml`,
  YAML fixtures) so Windows checkouts don't introduce
  CRLF drift.

### Added — coverage breadth

- **~155 new rule-kind unit tests** across 34 previously
  under-covered rule kinds (build / options / evaluate
  fires / evaluate silent / edge cases — the standard
  quintet).
- **Fail-variant e2e scenarios** for the 3 pass-only
  unix-metadata rules (`no_symlinks`, `executable_bit`,
  `executable_has_shebang`).
- **E2E scenarios** for the 4 zero-e2e rules
  (`json_schema_passes`, `git_no_denied_paths`,
  `git_commit_message`, `command`).
- **Integration tests** under `crates/alint-rules/tests/`
  for the shell-out rules: `git_no_denied_paths`,
  `git_commit_message`, `command` (mirrors v0.7.3's
  `git_blame_age` integration-test pattern).
- **alint-core internal tests** — `engine.rs`, `walker.rs`,
  `registry.rs`, `report.rs`, `error.rs`, `level.rs`,
  `scope.rs`, `config.rs`, `rule.rs` all gained unit
  tests (had 0 pre-v0.8.2).
- **alint-dsl edges** — extends-chain cycle detection,
  diamond inheritance, `extends:` filter validation,
  nested-config path-prefix rewriting, `.alint.d/` merge
  order determinism, HTTPS timeout / size-cap
  enforcement, SRI algorithm-mismatch errors,
  template-instantiation edge cases.

### Added — CLI surface

- **trycmd 33 → 56 cases** — stderr snapshots for every
  error path, per-subcommand `--help` snapshots,
  `--color × NO_COLOR × CLICOLOR_FORCE` matrix,
  `--progress × TTY/non-TTY` matrix.

### Fixed — production bugs surfaced by the new lanes

- **DSL nested-config glob mixed separators** — on
  Windows, `discover_nested` joined `rel_dir.to_string_lossy()`
  (native separators) with the user's pattern (`/`), producing
  globs like `packages\foo/README.md` that globset couldn't
  match. Nested-config rules silently no-op'd on Windows. Now
  the prefix is normalised to `/` before joining.
- **xtask bench-compare key separators** — comparison keys
  carried native separators, breaking cross-OS tree
  comparison (`target/criterion-main` from a Linux runner
  vs. `target/criterion` from a Windows checkout). Now
  normalised to `/`.
- **CLICOLOR_FORCE on `--color=auto`** — the `auto`
  resolution path didn't honor `CLICOLOR_FORCE` before
  passing the choice to anstream. Fixed via a
  `ColorChoice::resolve()` pre-pass.

### Internal — release safety

- `alint-dsl`, `alint-rules`, `alint-output` now carry
  `publish = false`. Their descriptions have always read
  "Internal: Not a stable public API"; the manifest now
  matches. Historical v0.5.x → v0.7.0 versions remain on
  crates.io for compatibility; new versions are gated off.
- `ci/scripts/publish-crates.sh` reduced to publishing
  only `alint-core` (public library) and `alint`
  (binary). The other three are internal implementation
  detail.

## [0.7.0] - 2026-04-28

Closes the v0.7 cut. Three new rule kinds and two new
subcommands targeting agent-driven development workflows
specifically. Where v0.6 was config-only — bundled rulesets
composed from existing primitives — v0.7 extends the engine
itself: `markdown_paths_resolve` / `commented_out_code` /
`git_blame_age` are new rule kinds with their own heuristic
surfaces, and `alint suggest` / `alint export-agents-md`
add the first new top-level subcommands since `alint init`
in v0.5.4.

Schema-compatible: every v0.6 config runs unchanged. The new
rule kinds parse new YAML shapes that older configs simply
don't use; `version: 1` continues to cover them.

The two subcommands close the cold-start adoption gap for
agent-heavy repos. `alint suggest` scans for known
antipatterns and proposes rules to catch them — its
stale-TODO suggester eats `git_blame_age`'s own dogfood.
`alint export-agents-md` makes alint the single source of
truth for `AGENTS.md` / `CLAUDE.md` / `.cursorrules`
directive blocks: the agent reads what alint enforces, no
duplicate config to maintain.

### Added

- **`alint export-agents-md` subcommand** — generate (or
  maintain a section of) `AGENTS.md` from the active rule
  set, so the agent's pre-prompt directives stay in sync with
  the lint config. Closes the "67% of teams maintain
  duplicate configs between AGENTS.md and CI lint" gap by
  making alint the single source of truth.

  Two output formats:

  - `markdown` (default) — section-per-severity bullet
    list shaped to drop into an `AGENTS.md` /
    `CLAUDE.md` / `.cursorrules` directive block.
  - `json` — stable shape behind `schema_version: 1`,
    parallel to `suggest`'s envelope, suitable for agent
    consumption.

  Three output destinations:

  - **stdout** (default) — pipe / paste by hand.
  - **`--output PATH`** — overwrite-create the named
    file.
  - **`--inline --output PATH`** — splice the generated
    section between `<!-- alint:start -->` and
    `<!-- alint:end -->` markers in the target file. The
    canonical workflow: humans own the prose outside the
    markers; alint owns the directive block between
    them. Re-runs are idempotent — when the existing
    between-markers content already matches what we'd
    generate, the file isn't rewritten.

  Inline mode auto-initialises markers when the target
  file lacks them: the section is appended to the end
  with a stderr warning, and subsequent runs splice in
  place. Multiple-pair / orphan-marker shapes hard-error
  rather than silently overwrite — splicing is destructive
  and ambiguity surfaces explicitly.

  Severity grouping: `Errors (commit will fail)`,
  `Warnings (review before merge)`, optional
  `Info (informational nudges)` (gated by
  `--include-info` — info-level rules are nudges, not
  directives, and clutter the agent's context window
  unless you really want them). Rules without an explicit
  `message:` fall back to a synthesised "<kind> rule"
  line so no directive is silently dropped.

  Stable byte-for-byte output across runs: line endings
  always `\n`, sort by severity desc + rule_id asc within
  each section. Re-running `--inline` produces identical
  bytes; round-trip identity short-circuits the write.

  ```bash
  alint export-agents-md                          # stdout
  alint export-agents-md --output AGENTS.md       # write a file
  alint export-agents-md --inline --output AGENTS.md  # splice in place
  alint export-agents-md --format=json            # stable JSON for agents
  alint export-agents-md --include-info           # include info-level rules
  alint export-agents-md --section-title "Lint policy"  # custom heading
  ```

  Design doc: `docs/design/v0.7/alint_export_agents_md.md`.

- **`alint suggest` subcommand** — scan the repo for known
  antipatterns and propose rules that would catch them. Acts
  as a smart `alint init` for retrofitting alint onto a long-
  running, agent-heavy codebase. Three output formats:

  - **`--format=human`** (default): colourised proposal table
    with optional `--explain` evidence block.
  - **`--format=yaml`**: paste-ready config snippet (`extends:`
    + `rules:`).
  - **`--format=json`**: stable shape behind
    `schema_version: 1` for agent consumption.

  Three suggester families ship in v0.7.4:

  1. **Bundled-ruleset** — high-confidence proposals for
     `oss-baseline@v1` (always) plus per-language
     (`rust@v1` / `node@v1` / `python@v1` / `go@v1` /
     `java@v1`) and workspace-flavour overlays based on the
     same ecosystem detection `alint init` uses.
  2. **Antipattern** — medium-confidence proposal of
     `extends: agent-hygiene@v1` when the repo contains
     backup-suffix files, scratch / planning docs at root
     (`PLAN.md`, `NOTES.md`, …), or `console.log`-style debug
     residue in non-test JS / TS source. `tests/`,
     `__tests__/`, `fixtures/`, and `snapshots/` paths are
     skipped automatically.
  3. **Stale-TODO** — medium-confidence `git_blame_age` rule
     proposal when ≥ 3 `TODO` / `FIXME` / `XXX` / `HACK`
     markers are older than 180 days. Eats our own
     v0.7.3 dogfood.

  `--include-bundled` overrides the already-covered filter
  (which would otherwise skip a `rust@v1` proposal when the
  user's existing `.alint.yml` already extends it).
  `--confidence={low,medium,high}` raises the floor on what
  surfaces. The command always exits 0 unless the scan
  itself fails — `suggest` is exploration, not a CI gate.

- **`--progress={auto,always,never}` + `-q` / `--quiet`
  global flags** — controls stderr-side progress for slow
  commands. Strict stream split: structured stdout
  (`--format=json` / `yaml` / `human`) is byte-for-byte
  clean regardless of progress activity; spinners and
  status lines live exclusively on stderr.

  - `auto` (default): animated bars when stderr is a TTY;
    one-line milestones to plain stderr when captured
    (CI logs).
  - `always`: same as `auto` plus a stderr summary line.
    Bars still require a TTY — non-TTY `always` falls back
    to one-line milestones.
  - `never`: zero stderr noise. `--quiet` is the alias.

  `indicatif` powers the animated bars; the
  `crates/alint/src/progress.rs` module wraps it behind a
  null-handle pattern so suggester code passes
  `&Progress` without branching on visibility.

- **`git_blame_age` rule kind** — fire on lines matching a regex
  whose `git blame` author-time is older than `max_age_days`.
  Closes the gap between `level: warning` on every TODO (too
  noisy) and `level: off` (accepts unbounded debt accumulation).
  Same regex match shape as `file_content_forbidden`, plus a
  per-line age gate. New `{{ctx.match}}` message placeholder
  substitutes capture group 1 (or the full match when no
  capture is present), so messages can be specific about which
  marker was caught. Outside a git repo, on untracked files, or
  when blame fails for any other reason, the rule silently
  no-ops per file — same advisory posture as
  `git_no_denied_paths` / `git_commit_message`. Check-only.

  ```yaml
  - id: stale-todos
    kind: git_blame_age
    paths: "**/*.{rs,ts,tsx,js,jsx,py,go,java}"
    pattern: '\b(TODO|FIXME|XXX|HACK)\b'
    max_age_days: 180
    level: warning
    message: "`{{ctx.match}}` is >180 days old — resolve or remove."
  ```

  Engine plumbing: a new shared `BlameCache` is built once per
  run when any rule reports `wants_git_blame()`, so multiple
  blame-aware rules over overlapping `paths:` re-use the parsed
  output. Cache memoises both successes and failures so a
  large rule fan-out doesn't re-shell-out to git per file.
  Heuristic notes (formatting passes reset blame age unless
  `.git-blame-ignore-revs` is honoured; vendored / imported
  code carries the import commit's timestamp; squash-merged
  PRs collapse to a single date) are documented in
  `docs/rules.md` and `docs/design/v0.7/git_blame_age.md`.

  Pairs naturally with `alint check --changed` so blame only
  runs over modified files in CI.

  Design doc: `docs/design/v0.7/git_blame_age.md`.

- **`commented_out_code` rule kind** — heuristic detector for
  blocks of commented-out source code (as opposed to prose
  comments, license headers, doc comments, or ASCII banners).
  Counts the fraction of non-whitespace characters that are
  structural punctuation strongly biased toward code
  (`( ) { } [ ] ; = < > & | ^`); scores ≥ `threshold` (default
  0.5 after normalisation; midpoint between obvious-prose 0.0
  and obvious-code 1.0) mark the block as code-shaped.
  Supports rust / typescript / javascript / python / go / java
  / c / cpp / ruby / shell. Doc-comment blocks (`///`, `//!`,
  `/** */`) and the file's first `skip_leading_lines` lines
  (default 30 — license headers) are excluded by construction.
  Runs of 5+ identical characters (`============`, `----`,
  `####`) are dropped before scoring so ASCII-art separators
  don't flag as code. Severity floor is `warning`, never
  `error` by default — heuristics have non-zero FP rate.
  Check-only — auto-removing commented-out code is
  destructive. Field-tested against 5 repos (alint, alint.org,
  Aider, Cline, OpenHands) before commit: zero false positives
  across 16 hits.

  ```yaml
  - id: no-commented-code
    kind: commented_out_code
    paths: "src/**/*.{ts,tsx,js,jsx,rs,py}"
    min_lines: 3
    threshold: 0.5
    level: warning
  ```

  Design doc: `docs/design/v0.7/commented_out_code.md`.

- **`markdown_paths_resolve` rule kind** — validates that
  backticked workspace paths in markdown files resolve to
  real files or directories. Targets the AGENTS.md /
  CLAUDE.md / `.cursorrules` staleness problem more
  precisely than v0.6's regex-heuristic
  `agent-context-no-stale-paths` rule. Required `prefixes`
  field declares which path-shapes to validate (eliminating
  the "is this a path or a word" question by construction).
  Skips fenced and 4-space-indented code blocks. Strips
  trailing punctuation, trailing slashes, `:line` /
  `#L<n>` location suffixes before lookup. Glob characters
  in the path resolve via the file index. Check-only.
  Design doc: `docs/design/v0.7/markdown_paths_resolve.md`.

  ```yaml
  - id: agents-md-paths-resolve
    kind: markdown_paths_resolve
    paths: ["AGENTS.md", "CLAUDE.md", ".cursorrules"]
    prefixes: ["src/", "crates/", "docs/"]
    level: warning
  ```

## [0.6.0] - 2026-04-27

Two bundled rulesets and a new output format aimed at the
agent-driven-development era. Schema-compatible: every v0.5.12
config runs unchanged, and the new bundled rulesets compose
from rule kinds that have shipped since v0.1.

The framing: alint's structural / repo-shape niche
(filesystem shape and contents of a repository, not the
semantics of code inside it) fits agent-driven development
naturally. Coding agents leave characteristic structural
debris — backup-suffix files, scratch / planning docs,
debug-print residue, stale model-attributed TODOs — that
existing rule kinds catch cleanly when packaged into the
right bundled ruleset. v0.6 does that packaging, plus a
new output format optimised for agents consuming alint
inside their own self-correction loops. No new rule
kinds, no engine changes, no architectural shift.

### Added

- **`alint://bundled/agent-hygiene@v1`** — six-rule bundled
  ruleset targeting the leftover patterns that show up
  disproportionately in commits authored or co-authored by
  Claude Code, Cursor, Copilot agent, Aider, Codex, and
  similar tools. Composes with the existing `hygiene/*`
  rulesets — extend all three on agent-heavy projects
  without overlap:

  ```yaml
  extends:
    - alint://bundled/hygiene/no-tracked-artifacts@v1
    - alint://bundled/hygiene/lockfiles@v1
    - alint://bundled/agent-hygiene@v1
  ```

  Rules:
  - `agent-no-versioned-duplicates` — bans filenames matching
    `*_old.*` / `*_new.*` / `*_final.*` / `*_FINAL.*` /
    `*_copy.*` / `*_backup.*` / `*.copy.*` (warning). The
    `*_v[0-9]*` / `*-v[0-9]*` patterns were considered and
    deliberately omitted — too many real codebases use those
    for legitimate API versioning, schema migrations, release
    notes, and versioned tests (`gitlab_v1_*.py`,
    `076_add_v1_tables.py`, `release-notes-v1.md`,
    `test_v1_api.py`).
  - `agent-no-scratch-docs-at-root` — bans `PLAN.md` /
    `NOTES.md` / `ANALYSIS.md` / `SUMMARY.md` / `FIX.md` /
    `DECISION.md` / `TODO.md` / `SCRATCH.md` / `DEBUG.md` /
    `TEMP.md` / `WIP.md` at the repo root (warning,
    `root_only: true`).
  - `agent-no-affirmation-prose` — flags AI-style stock
    phrases in source / markdown (`"You're absolutely
    right"`, `"Excellent question"`, `"Happy to help"`,
    etc.) (info).
  - `agent-no-console-log` — bans `console.log` / `.debug` /
    `.trace` in non-test JS / TS source (warning). Excludes
    test directories (`**/*test*/**` — broader than
    `test*/**`, catches `cross-sdk-tests/`, `e2e-tests/`,
    etc.), build / dev tooling configs, `**/scripts/**`,
    `**/website/**` / `**/public/**` / `**/demo/**`,
    `**/vendor/**` and `**/.claude/**` (agent-worktree scratch
    space).
  - `agent-no-debugger-statements` — bans `debugger;` /
    `breakpoint()` in non-test source (error). The regex
    requires `;` immediately after `debugger` so the rule
    doesn't trip on the WORD "debugger" appearing in prose
    comments. Same exclusion list as the console-log rule.
  - `agent-no-model-todos` — bans `TODO(claude:)` /
    `FIXME(cursor:)` / `XXX(gpt:)` and similar
    model-attributed markers (warning). Excludes CHANGELOG /
    ROADMAP / cookbook / test directories — projects that
    document these patterns trip on their own examples
    otherwise.

- **`alint://bundled/agent-context@v1`** — four-rule bundled
  ruleset for the agent-instruction files coding agents read
  on every session: `AGENTS.md` (the cross-tool standard
  backed by agents.md / OpenAI Codex), `CLAUDE.md`,
  `.cursorrules`, `GEMINI.md`, and
  `.github/copilot-instructions.md`. Gated by
  `facts.has_agent_context` so it's a safe no-op in repos
  without any of these files; extend it unconditionally even
  from polyglot configs.

  Rules:
  - `agent-context-recommended` — `file_exists` info-level
    nudge.
  - `agent-context-non-stub` — `file_min_lines: 10`
    (warning).
  - `agent-context-not-bloated` — `file_max_lines: 300`
    (info). Threshold from Augment Code's 2026-03 research
    on AGENTS.md effectiveness.
  - `agent-context-no-stale-paths` — regex-heuristic
    info-level reminder that backticked workspace paths
    drift. The precise check ships in v0.7 as the
    `markdown_paths_resolve` rule kind.

- **`--format=agent` JSON output** — eighth output format,
  alongside `human` / `json` / `sarif` / `github` /
  `markdown` / `junit` / `gitlab`. Shaped for AI coding
  agents that consume alint inside their own self-correction
  loops. Differences vs. `--format=json`: violations are a
  flat list (no per-rule nesting); each violation carries an
  `agent_instruction` field with templated remediation
  phrasing (severity + human message + location + fix
  availability + policy URL); `severity` is the lowercase
  string (`"error"` / `"warning"` / `"info"`). Aliases:
  `--format=agent` / `--format=agentic` / `--format=ai`.
  Stable behind `schema_version: 1`. The fix-report falls
  back to the human formatter (an agent confirming a fix
  landed re-runs `alint check --format=agent`).

### Internal

- Two new tests in `crates/alint-dsl/src/bundled.rs` continue
  to enforce that every shipped ruleset declares its
  canonical `# alint://bundled/<name>@v<rev>` URI tag and
  parses as a valid config; the new rulesets are exercised
  by the existing test loop.
- Five unit tests for the agent formatter cover empty
  reports, path-bound violations, fixable violations
  (suggesting `alint fix --only <id>` in
  `agent_instruction`), cross-file violations
  (repository-level phrasing), and severity-count
  aggregation.
- Help-text snapshot (`crates/alint/tests/cli/help-top-level.stdout`)
  refreshed to mention `agent` in the `--color` flag's
  documented list of plain-bytes formats.

## [0.5.12] - 2026-04-27

Maintenance release. Verifies the npm auto-publish CI wiring
end-to-end after v0.5.11's `publish-npm` job failed (the
`NPM_TOKEN` secret hadn't been provisioned yet, and a detour
through Trusted Publishing was blocked by a broken 2FA
configuration UI on npmjs.com).

No code changes. Every v0.5.11 config runs unchanged.

### Changed

- The npm scope is now `@asamarts/alint` (matches the
  `asamarts/alint` GitHub repo, `asamarts/homebrew-alint`
  tap, and `ghcr.io/asamarts/alint` Docker image). The
  v0.5.11 entry referenced `@alint/alint`, then `@a-lint/alint`
  during the org-name dance; both were placeholders. The
  install snippet now matches what's actually published.

```bash
npm install --save-dev @asamarts/alint
npx alint check
```

## [0.5.11] - 2026-04-27

npm install channel. Closes the v0.5 milestone — every
deferred item from the v0.5 roadmap is now shipped.

### Added

- **`@asamarts/alint` npm package** — fifth install channel
  alongside `cargo install alint`, the Homebrew tap, the
  Docker image, and `install.sh`. The npm package is a thin
  shim that downloads the matching pre-built binary at
  install time, verifies its SHA-256 against the same
  `.sha256` companions the other paths consume, and stages
  it under `bin-platform/` for the npm-exposed
  `bin/alint.js` shim to spawn at runtime.

  ```bash
  # project-local
  npm install --save-dev @asamarts/alint
  npx alint check

  # global
  npm install -g @asamarts/alint
  alint check
  ```

  - The package itself ships zero JS runtime behaviour.
  - Single runtime dep (`tar` for archive extraction).
  - Skip the postinstall network hop with
    `ALINT_SKIP_INSTALL=1` (for CI systems that snapshot
    `node_modules`).
  - Supported platforms: linux x64/arm64 (musl), macOS
    x64/arm64, Windows x64.
  - Auto-published from `release.yml` on tag push,
    alongside crates.io / Docker / Homebrew. The publish
    job stamps `package.json`'s version to match the tag
    immediately before `npm publish --access public`.

### Internal

- New `npm/` directory at the repo root holds
  `package.json`, `install.js` (postinstall), `bin/alint.js`
  (runtime shim), `README.md`, and `.npmignore`.
- `release.yml` gains a `publish-npm` job: `needs: release`
  (the GH Release must be live before any user's
  postinstall can fetch the binary tarballs); reads
  `NPM_TOKEN` secret from repo settings.

## [0.5.10] - 2026-04-27

DSL ergonomics: three composition primitives that close
common monorepo / ops pain points. Schema-compatible: every
v0.5.9 config runs unchanged.

### Added

- **`content_from: <path>` on fix ops** —
  `file_create` / `file_prepend` / `file_append` accept a
  path-relative-to-lint-root as an alternative to inline
  `content:`. The two are mutually exclusive (XOR
  enforced at config-load time). Read at fix-apply time
  via the new `ContentSourceSpec` enum on the fixer
  struct; missing source produces a `Skipped` outcome
  with a clear message rather than aborting the run.
  Use case: LICENSE / NOTICE / CONTRIBUTING /
  SPDX-header boilerplate that's awkward to inline lives
  in `.alint/templates/` and gets referenced by short
  relative path.

- **`.alint.d/*.yml` drop-ins** — auto-discovered next to
  the top-level `.alint.yml` and merged in alphabetical
  order. The last drop-in wins on field-level conflict,
  mirroring the `/etc/*.d/` convention. Stage `00-base.yml`
  for ops defaults, `50-team.yml` for team policies,
  `99-local.yml` for developer-local tweaks. Trust-
  equivalent to the main config (same workspace) — drop-
  ins CAN declare `custom:` facts and `kind: command`
  rules without the trust-gate that protects HTTPS /
  bundled extends. Non-yaml files in the dir are
  silently skipped. Sub-extended configs don't get their
  own `.alint.d/`; only the top-level config does.

- **Rule templates / parameterized rules** — top-level
  `templates:` block defines reusable rule bodies; rules
  instantiate them via `extends_template: <id>` and a
  `vars:` map for the `{{vars.<name>}}` substitution.
  Recursive substitution walks lists and nested mappings,
  so `paths:` / `fix.file_create.content` / etc all get
  vars-expanded. Unknown placeholders preserve literally
  so typos surface. Leaf-only (a template can't itself
  `extends_template:` another, mirroring the bundled
  rulesets restriction). Templates merge through
  `extends:` chains by id.

  ```yaml
  templates:
    - id: dir-has-readme
      kind: file_exists
      paths: ["{{vars.dir}}/README.md"]
      level: warning
      message: "{{vars.dir}} is missing a README"
  rules:
    - extends_template: dir-has-readme
      id: packages-have-readme
      vars: { dir: packages }
    - extends_template: dir-has-readme
      id: services-have-readme
      vars: { dir: services }
  ```

### Internal

- New `ContentSourceSpec` (`Inline(String)` /
  `File(PathBuf)`) and `resolve_content_source` helper in
  `alint-core::config`, exported through the crate root.
  `From<String>` / `From<&str>` impls keep inline-string
  construction terse for tests.
- `RawConfig` gains `templates: Vec<Mapping>`; `merge()`
  merges templates by id; `finalize()` runs the
  `expand_template` pass before each rule deserializes
  into `RuleSpec`.
- New `expand_template` + `substitute_template_vars` /
  `_value` helpers in `alint-dsl` reuse the existing
  `alint_core::template::render_message` engine for the
  `{{namespace.key}}` substitution layer.
- JSON Schema (`schemas/v1/config.json` + the in-crate
  copy) defines top-level `templates: []`, the new
  `rule_template_instance` shape (`oneOf`-branch with
  the kind-driven shape), and the `oneOf` between
  `content` / `content_from` on each affected fix op.
  Drift test passes.
- 12 new unit tests across the three features (3 fixer
  tests for content_from, 3 lib tests for drop-in
  collection + merge, 6 lib tests for template
  expansion).

## [0.5.9] - 2026-04-27

`json_schema_passes` (last unshipped structured-query
primitive), two new git-aware rule kinds, and four
OpenSSF Scorecard-overlap additions to `oss-baseline@v1`.
Schema-compatible: every v0.5.8 config runs unchanged.

### Added

- **`json_schema_passes`** — validate JSON / YAML / TOML
  files against a JSON Schema. Targets coerce through
  serde into the same `serde_json::Value` tree the schema
  sees, so YAML configs (Kubernetes manifests, GitHub
  Actions workflows, Helm `values.schema.json`) and TOML
  manifests (Cargo, pyproject) all work against a JSON
  schema document. Schema is loaded + compiled lazily on
  the first `evaluate()` call and cached on the rule via
  `OnceLock`. Each schema-validation error becomes one
  violation with the failing instance path; a target that
  fails to parse produces a single parse-error violation,
  not a flood. Format is detected from extension; pass
  `format:` to override.

- **`git_no_denied_paths`** — fire when any tracked file
  matches a configured glob denylist. The absence-axis
  companion of `git_tracked_only` (v0.4.8). Catches
  secrets (`*.env`, `id_rsa`, `*.pem`), bulky generated
  artefacts (`dist/**`, `*.log`), and "do not commit"
  sentinels in one rule rather than one `file_absent`
  per pattern. Reports every matching denylist entry per
  offending path. Outside a git repo, silently no-ops.

- **`git_commit_message`** — validate HEAD's commit
  message shape via regex (`pattern:`), max subject
  length (`subject_max_length:`), and body-required
  (`requires_body:`). At least one of the three must be
  set. Subject length counts characters, not bytes.
  Outside a git repo, with no commits, or when `git`
  isn't on PATH, silently no-ops. Pairs naturally with
  `alint check --changed` for per-PR enforcement.

- **`alint-core::git::head_commit_message(root)`** —
  new helper alongside `collect_tracked_paths` /
  `collect_changed_paths`, with the same advisory
  `Option<String>` return shape.

- **Four Scorecard-overlap rules in `oss-baseline@v1`**
  (info-level, no new rule kinds — composes from
  existing `file_exists` + `file_min_size`):
  - `oss-security-policy-non-empty` — 200B floor on
    SECURITY.md (catches the empty stub that satisfies
    Scorecard's existence check while providing no
    reporting guidance).
  - `oss-dependency-update-tool` — `file_exists`
    against every blessed Dependabot / Renovate config
    location.
  - `oss-codeowners-exists` — CODEOWNERS at root,
    `.github/`, or `docs/`.
  - `oss-codeowners-non-empty` — 10B floor on
    CODEOWNERS.

### Internal

- `alint-rules` gains `jsonschema = "0.29"` as a
  regular dep (already a workspace dep used by
  `alint-dsl`'s drift tests).
- JSON Schema (`schemas/v1/config.json` + the in-crate
  copy) defines `rule_json_schema_passes`,
  `rule_git_no_denied_paths`, and
  `rule_git_commit_message`. Drift test passes.
- 21 new unit tests across the three rule files (9 for
  `json_schema_passes`, 5 for `git_no_denied_paths`, 7
  for `git_commit_message`); 1 new e2e fixture
  (`oss_baseline_complete_repo_pass` updated for the
  four new rules); two existing override scenarios
  updated to account for the new bundled rules.

## [0.5.8] - 2026-04-26

Three new output formats. Brings the count to seven and
closes the v0.5 output-format roadmap item. Schema-compatible:
every v0.5.7 config runs unchanged.

### Added

- **`--format markdown`** (alias `md`) — GitHub-Flavored
  Markdown suited to PR comments, mkdocs report pages, and
  Slack via webhook bridges. H1 banner + one-line summary
  (`**N violations across M files** (E errors, W warnings)`)
  + one H2 section per file with bulleted violations + a
  trailing "Cross-file" section for path-less / cross-file
  violations. Output is byte-deterministic so PR-comment
  workflows can diff alint output across runs without
  spurious churn. `alint fix --format markdown` gets a
  dedicated renderer too — lists each rule's items with
  `applied` / `skipped` / `unfixable` status.

- **`--format junit`** (alias `junit-xml`) — the de-facto-
  standard CI test-report XML consumed by Jenkins, Azure
  DevOps, GitHub's `dorny/test-reporter`, and GitLab CI's
  JUnit integration. Common-denominator schema: a single
  `<testsuites>` wrapping a single `<testsuite name="alint">`,
  with one `<testcase>` per (rule, file/path-less-bucket).
  Passing rules contribute self-closed testcases; each
  violation becomes a testcase with a `<failure>` whose
  `type` attribute carries the alint level (`error` /
  `warning` / `info`) so consumers can filter
  level-specifically. XML 1.0-illegal control characters
  are stripped on the way out.

- **`--format gitlab`** (aliases `gitlab-codequality`,
  `code-quality`) — GitLab CI's native Code Quality JSON,
  which is the upstream Code Climate "Issue" specification.
  One issue object per violation:
  `{ description, check_name, fingerprint, severity,
  location: { path, lines: { begin } } }`. Severity
  mapping: `Error → major`, `Warning → minor`,
  `Info → info`. Fingerprint is the SHA-256 hex of
  `rule_id|path|message` — the line number is intentionally
  omitted so a violation that drifts up or down by a few
  lines stays the same issue across runs. Path-less /
  cross-file violations emit `location.path = "."`
  (repository root) so the report still validates against
  the GitLab schema.

### Internal

- `alint-output` gains a `sha2` dep for the GitLab
  fingerprint (already a workspace dep used by
  `alint-rules` + `alint-dsl`).
- 38 new unit tests across the three formatters cover empty
  reports, level-mapping, cross-file edge cases,
  determinism, special-character escaping, and (for
  GitLab) fingerprint stability across line-drift +
  sensitivity to message changes.

## [0.5.7] - 2026-04-26

Competitive bench publication. The v0.5.6 harness becomes a
multi-tool driver: alint, ls-lint, Repolinter, and `find` +
`ripgrep` pipelines all run against the same synthetic
trees on the same hardware, producing wall-time numbers
that are directly comparable. Reproducibility via a new
`ghcr.io/asamarts/alint-bench` Docker image (every
competitor pinned by version) and an `xtask bench-scale
--docker` flag that re-execs the bench inside the image.
Schema-compatible; every v0.5.6 config runs unchanged.

### Added

- **`xtask bench-scale --tools <list>`** — the bench
  harness now runs an arbitrary set of tools across the
  same `(size × scenario × mode)` matrix v0.5.6
  introduced. Default `alint` (preserves the v0.5.6
  publication shape), `all` expands to every known tool,
  comma lists pick a subset (`alint,grep`,
  `alint,ls-lint`). Tools missing on `PATH` are
  log-and-dropped at resolve time so a partial
  installation still produces alint-only rows without
  aborting.
  - **alint** — full matrix (every scenario × mode).
  - **ls-lint** — gated to (S1, full); ls-lint is
    extension/case-class only and has no
    `--changed`-equivalent.
  - **Repolinter** — gated to (S2, full); pinned to
    0.11.2 (last pre-archive release, repo archived
    2026-02-06). The size-bound check (no files >10 MiB)
    is dropped: Repolinter has no built-in primitive
    and emulating via `script` rules would distort
    timings beyond recognition.
  - **find + ripgrep pipeline** (`grep`) — gated to
    (S1, full) and (S2, full); shell-pipeline baseline
    representing the small-team "we just chain `find`
    and `rg`" status quo. S3's cross-file rules have no
    sane shell expression and are out of scope.

- **`bench/Dockerfile` + `ghcr.io/asamarts/alint-bench`
  image** — the canonical competitive-bench environment.
  Built and pushed by `.github/workflows/bench-docker.yml`
  on tag pushes (`:<ver>` + `:latest`), main pushes
  (`:edge`), and manual dispatch. Pinned versions of
  hyperfine, ripgrep, repolinter, ls-lint, Node 20, and
  the Rust toolchain so a given image tag IS the bench
  environment for that release.

- **`xtask bench-scale --docker`** — re-execs the bench
  inside the published image. Bind-mounts the workspace
  at `/work`, uses a named volume for the cargo target
  dir so target/ artefacts persist across runs without
  shadowing the host. Image override via
  `ALINT_BENCH_IMAGE=...`.

- **First competitive numbers** under
  `docs/benchmarks/macro/results/linux-x86_64/v0.5.7/`. Same
  fingerprint as v0.5.6's alint-only publication; rows
  for ls-lint / repolinter / grep added at the
  scenarios + sizes each tool supports. Headline
  ratios (linux-x86_64, 100k files):
  - **S1 (filename hygiene)**: alint vs ls-lint vs
    `find | grep` pipelines.
  - **S2 (existence + content)**: alint vs Repolinter
    vs `find` + `rg` pipelines.
  - **S3 (workspace bundle)**: alint only — no
    competitor models cross-file rules.

  Per-row markdown plus a `results.json` with the full
  matrix; the new `Tool` column makes pivoting trivial.

- **Published 1M-file numbers** under
  `docs/benchmarks/macro/results/linux-x86_64/v0.5.6/1m/results.md`.
  Six rows (`1m × {S1,S2,S3} × {full,changed}`) on the
  same hardware as v0.5.6's 1k/10k/100k publication.
  Headlines: 1m / S1 / full ≈ 3.5s, 1m / S2 / full ≈ 10s,
  1m / S3 / full ≈ 9.5min — the cross-file rules in the
  workspace bundle scale superlinearly with N, exactly as
  the methodology predicted. `--changed` saves ~34% on
  S2's content rules at 1m but barely helps S3 (cross-file
  rules can't be filtered).

- **Auto-reduced sampling at 1m**. The harness caps
  `1m`-row warmup at 1 and measured runs at 3 regardless
  of `--warmup` / `--runs`. A single `1m / S3` invocation
  runs for several minutes; thirteen of them per row would
  push the matrix to many hours. The trade-off is wider
  stddev at 1m — `methodology.md` is updated to flag this
  so readers don't compare 1m's stddev to the smaller-size
  rows like-for-like. Stddev is reported as `0.0` when
  hyperfine emits `null` (single-run rows) instead of
  failing the whole bench.

### Fixed

- **`--include-1m` now actually adds the 1m size** to the
  matrix when `--sizes` is at its default (1k,10k,100k).
  Previously the flag only filtered 1m out unless you
  also retyped the size list — the opposite of what the
  help text implied.

- **Tool-version fingerprint stays one line**. Multi-line
  `--version` banners (notably ripgrep's) were being
  stored verbatim in the fingerprint's `tool_versions`
  map, blowing up the rendered "**Tools:** ..." line in
  committed reports. Capture now keeps just the first
  line of each tool's `--version` output.

## [0.5.6] - 2026-04-26

Scale-ceiling bench publication + a latent walker bug fix
that the bench surfaced. New `xtask bench-scale` subcommand
runs alint across a (size × scenario × mode) matrix with
hardware-fingerprint capture and JSON + Markdown
publication; v0.5.7 layers competitive comparisons
(ls-lint, Repolinter, find/grep) on top of the same
infrastructure. Schema-compatible; every v0.5.5 config
runs unchanged.

### Added

- **`xtask bench-scale`** — scale-ceiling benchmark
  driver. Runs alint across:
  - **Sizes**: `1k` / `10k` / `100k` (default), opt into
    `1m` via `--include-1m`. Synthetic monorepo trees
    generated deterministically from the seed
    (default `0xa11e47`).
  - **Scenarios**: `S1` (filename hygiene, 8 rules) /
    `S2` (existence + content, 8 rules) / `S3` (workspace
    bundle: `oss-baseline` + `rust` + `monorepo` +
    `monorepo/cargo-workspace`).
  - **Modes**: `full` (every file evaluated) and
    `changed` (`alint check --changed` against a
    deterministic 10% diff — measures the v0.5.0
    incremental path).

  Per-row hyperfine measurement (3 warmup + 10 measured
  runs by default); JSON + Markdown output under
  `docs/benchmarks/macro/results/<os>-<arch>/<version>/`. Hardware
  fingerprint (CPU model + cores, RAM, FS type, kernel,
  rustc, alint version + git SHA, hyperfine version)
  embedded in every report so cross-machine comparisons
  stay honest.

- **`alint_bench::tree::generate_monorepo`** — new
  Cargo-workspace-shaped synthetic-tree generator with
  real workspace `[workspace]` + per-package
  `[package].name` Cargo.toml content (so the
  `monorepo/cargo-workspace@v1` ruleset's structured-query
  rules see well-formed manifests). Full determinism for
  byte-identical trees across platforms.

- **`alint_bench::tree::select_subset`** — deterministic
  Fisher-Yates partial shuffle for picking a fraction of
  files to "touch" in `--changed`-mode benches.

- **First published numbers**:
  `docs/benchmarks/macro/results/linux-x86_64/v0.5.7/` with 18 rows
  (3 sizes × 3 scenarios × 2 modes) on AMD Ryzen 9 3900X /
  62 GB / ext4 / Linux 6.1. Companion
  `docs/benchmarks/{README.md,RUNNING.md,macro/README.md} (post-reorg layout)`
  documents the harness + scenario definitions + how to
  reproduce.

- **CLI flags**: `--sizes`, `--scenarios`, `--modes`,
  `--warmup`, `--runs`, `--seed`, `--diff-pct`, `--out`,
  `--include-1m`, `--quick`, `--json-only`. The default
  (`cargo xtask bench-scale` with no args) produces the
  full publication-grade matrix.

### Fixed

- **Walker no longer descends into `.git/`.** `alint
  check` against a tree containing a `.git/` directory
  used to walk into git's internal storage — wasted work
  for every alint rule (none of them target
  `.git/objects/*`) and a TOCTOU hazard during git's
  auto-gc / packfile rewrites. The walker now adds
  `.git` to its exclusion overrides unconditionally.
  No user-visible behaviour change for repos whose
  `.gitignore` already covers `.git/`-shaped paths;
  benchmark and large-monorepo runs become both faster
  and reliable.

### Internal

- New `xtask/src/bench/` module: `mod.rs` (orchestration
  + types), `fingerprint.rs` (hardware capture per OS),
  `scenarios/*.yml` (S1/S2/S3 alint configs embedded via
  `include_str!`).
- `xtask` gains `serde` (with `derive`) and `serde_json`
  dev-deps for hyperfine `--export-json` parsing and the
  results.json schema.
- 11 new unit tests on `alint_bench::tree` covering the
  monorepo shape, file-count exactness, deterministic
  output for the same seed, and `select_subset`'s
  fraction / clamping / determinism semantics.

### Compatibility

- Schema version remains `1`. No rule-config changes.
- Public API additions are non-breaking. Walker
  `.git/`-exclusion is a behaviour fix, not a config
  change.

## [0.5.5] - 2026-04-26

Two license-compliance bundled rulesets — the v0.5 cycle's
expansion beyond the workspace-tier monorepo audience to
OSS maintainers and corporate-policy teams.
Schema-compatible; every v0.5.4 config runs unchanged.

### Added

- **`alint://bundled/compliance/reuse@v1`** — FSFE
  [REUSE Specification](https://reuse.software/)
  compliance. Three rules covering the spec's two
  load-bearing requirements:
  - `reuse-licenses-dir-exists` — top-level `LICENSES/`
    directory present (per
    [§ License files](https://reuse.software/spec/#license-files)).
  - `reuse-source-has-spdx-identifier` — every source
    file carries an `SPDX-License-Identifier:` header in
    its first ~10 lines.
  - `reuse-source-has-copyright-text` — every source
    file carries an `SPDX-FileCopyrightText:` header.

  Source-file rules cover the common code extensions
  (`*.{rs,py,js,jsx,ts,tsx,go,java,kt,c,cc,cpp,h,hpp,
  hh,sh,rb,swift}`) and exclude vendored / build /
  dist directories. Projects that license files via
  `.license` companions or `REUSE.toml` mappings can
  narrow `paths:` on the source rules.

  ```yaml
  extends:
    - alint://bundled/compliance/reuse@v1
  ```

- **`alint://bundled/compliance/apache-2@v1`** —
  compliance for projects distributed under the Apache
  License, Version 2.0. Three rules verifying the
  artefacts the license text itself requires of
  redistributors:
  - `apache-2-license-text-present` — LICENSE (or
    LICENSE.md / LICENSE.txt / COPYING) contains the
    canonical "Apache License, Version 2.0" text.
  - `apache-2-notice-file-exists` — root NOTICE file
    present (per Apache-2.0 §4(d)).
  - `apache-2-source-has-license-header` — every source
    file carries the canonical "Licensed under the
    Apache License, Version 2.0" header in its first
    ~25 lines.

  Substring-matches the canonical license title rather
  than doing full bit-for-bit comparison, so SPDX
  templates, apache.org's template, and GitHub's
  auto-init all parse as compliant. Dual-licensed
  projects (e.g. Apache-2.0 OR MIT) can extend this
  ruleset and use `level: off` on rules they don't want
  firing strictly.

  Bundled catalog: 15 → 17.

### Internal

- New ruleset directory
  `crates/alint-dsl/rulesets/v1/compliance/` with two
  `.yml` files; registered in
  `alint_dsl::bundled::REGISTRY`. Neither ruleset uses a
  fact gate — adopting a compliance ruleset is the
  user's signal that the project intends to be
  compliant with the named scheme.
- 6 new e2e scenarios under
  `crates/alint-e2e/scenarios/check/bundled-compliance/`:
  per ruleset, happy-path + missing-core-artefact +
  missing-source-header.

### Compatibility

- Schema version remains `1`. Pure config — no new rule
  kinds, no new core APIs.
- JSON / SARIF / GitHub outputs byte-equivalent for
  configs that don't extend the new rulesets.

## [0.5.4] - 2026-04-26

`alint init` — the missing one-line adoption story.
Detects the repo's ecosystem (Rust / Node / Python / Go /
Java) and optionally its workspace shape (Cargo / pnpm /
Yarn-or-npm), then writes a `.alint.yml` extending the
right bundled rulesets. Closes the v0.5 monorepo theme on
the adoption side: every primitive shipped in v0.5.0–v0.5.3
now has a one-line on-ramp. Schema-compatible; every v0.5.3
config runs unchanged.

### Added

- **`alint init [PATH]`** — new subcommand. Detects the
  repo's ecosystem from root manifests and writes a
  `.alint.yml` with `extends:` lines for
  `oss-baseline@v1` plus each detected language ruleset.
  Detection is deliberately a presence check (file
  exists, no parsing) so it stays fast and predictable:
  - Rust: `Cargo.toml`
  - Node: `package.json`
  - Python: `pyproject.toml` / `setup.py` / `setup.cfg`
  - Go: `go.mod`
  - Java: `pom.xml` / `build.gradle` / `build.gradle.kts`

  Refuses to overwrite an existing config (any of
  `.alint.yml` / `.alint.yaml` / `alint.yml` /
  `alint.yaml`) — exits non-zero with a clear message
  pointing the user at deletion.

- **`alint init --monorepo`** — adds workspace detection
  on top of the language scan. Recognises:
  - **Cargo workspaces** — root `Cargo.toml` contains a
    `[workspace]` table (line-prefix check, no TOML
    parsing).
  - **pnpm workspaces** — root `pnpm-workspace.yaml` /
    `.yml` exists.
  - **Yarn / npm workspaces** — root `package.json`
    contains `"workspaces"`.

  When a workspace is detected, the generated config also
  extends `monorepo@v1` and the matching
  `monorepo/<flavor>-workspace@v1` overlay, plus sets
  `nested_configs: true` so each subdirectory can layer
  its own `.alint.yml` on top.

  ```yaml
  # `alint init --monorepo` in a Cargo workspace produces:
  version: 1
  nested_configs: true

  extends:
    - alint://bundled/oss-baseline@v1
    - alint://bundled/rust@v1
    - alint://bundled/monorepo@v1
    - alint://bundled/monorepo/cargo-workspace@v1
  ```

  Bazel / Lerna / Nx / Turbo detection deferred — the
  three flavours that have bundled overlays cover the
  workspace-tier sweet spot.

### Internal

- New `crates/alint/src/init.rs` module with `Detection` /
  `Language` / `WorkspaceFlavor` types, a pure `detect()`
  function and a deterministic `render()` emitter. Output
  is hand-formatted YAML (not serialized via serde) so the
  generated file can carry header comments documenting
  what was detected and how to use it.
- 17 new unit tests covering the detector + emitter
  (per-language detection, polyglot repos, workspace
  precedence, header summary).
- 3 new trycmd CLI tests under `tests/cli/init-*.toml`:
  empty-repo init, monorepo-cargo init, refuses-overwrite.
  The trycmd `fs.sandbox = true` mode lets us assert on
  the post-run sandbox state (the generated `.alint.yml`)
  alongside stdout / stderr / exit.
- `help-top-level` snapshot regenerated to include the
  `init` subcommand line.

### Compatibility

- Schema version remains `1`. Pure additive — no rule,
  config, or output changes.
- Public API unchanged (the `init` module is private to
  the binary crate).
- New `tempfile` dev-dependency on `alint` (already a
  workspace dep elsewhere; the binary needs it for the
  init unit tests).

## [0.5.3] - 2026-04-26

Three workspace-aware bundled rulesets layered on top of
`monorepo@v1`. Each is gated by a workspace-flavor fact and
uses the v0.5.2 `when_iter:` filter to scope per-member
checks to actual package directories — `crates/notes/`
(no `Cargo.toml`) or `packages/drafts/` (no `package.json`)
are filtered out without firing false positives.
Schema-compatible; every v0.5.2 config runs unchanged.

### Added

- **`alint://bundled/monorepo/cargo-workspace@v1`** —
  Cargo workspaces. Gated by `facts.is_cargo_workspace`
  (root `Cargo.toml` declares `[workspace]`). Three rules:
  `members = [...]` declared at the workspace root
  (`toml_path_matches`); every `crates/*` directory with
  its own `Cargo.toml` has a README; every member's
  `Cargo.toml` declares `[package].name`.

  ```yaml
  extends:
    - alint://bundled/monorepo@v1
    - alint://bundled/rust@v1
    - alint://bundled/monorepo/cargo-workspace@v1
  ```

- **`alint://bundled/monorepo/pnpm-workspace@v1`** —
  pnpm workspaces. Gated by `facts.is_pnpm_workspace`
  (root `pnpm-workspace.yaml` / `.yml` exists). Three
  rules: `packages: [...]` declared in
  `pnpm-workspace.yaml` (`yaml_path_matches`); every
  `packages/*` with a `package.json` has a README; every
  member's `package.json` declares `name`.

- **`alint://bundled/monorepo/yarn-workspace@v1`** —
  Yarn / npm workspaces (the workspace declaration lives
  in the root `package.json` for both). Gated by
  `facts.is_yarn_workspace` (root `package.json` contains
  `"workspaces"`). Three rules: `workspaces: [...]` is
  non-empty (`json_path_matches` against
  `$.workspaces[*]`); every `{packages,apps}/*` with a
  `package.json` has a README; every member's
  `package.json` declares `name`. Validates the array
  form; the rarer object form
  (`"workspaces": {"packages": [...]}`) is gated by the
  fact but not field-validated here.

  Bundled catalog: 12 → 15 rulesets.

### Internal

- New ruleset directory
  `crates/alint-dsl/rulesets/v1/monorepo/` with three
  `.yml` files; registered in `alint_dsl::bundled::REGISTRY`.
  Each ruleset declares its own `is_*_workspace` fact
  inline rather than promoting them to the core `facts:`
  catalogue — the facts are workspace-specific and not
  meant to be referenced from user configs.
- 7 new e2e scenarios under
  `crates/alint-e2e/scenarios/check/bundled-monorepo/`:
  per-flavor "filters non-member dirs" + "silent outside
  workspace" pair, plus `cargo-workspace`'s "fires on
  missing members" case.

### Compatibility

- Schema version remains `1`. All three rulesets are
  pure config — no new rule kinds, no new core APIs.
- JSON / SARIF / GitHub outputs byte-equivalent for
  configs that don't extend the new rulesets.

## [0.5.2] - 2026-04-26

Per-iteration `when:` filter on iterating rules — closes the
second monorepo-scale gap from the v0.5 roadmap. Combined
with `--changed` (v0.5.0) and `command` plugin (v0.5.1),
this is the third leg of the v0.5 monorepo theme.
Schema-compatible; every v0.5.1 config runs unchanged.

### Added

- **`when_iter:`** field on `for_each_dir`, `for_each_file`,
  and `every_matching_has`. Optional expression evaluated
  against each iterated entry's `iter` context; iterations
  whose verdict is false are skipped before any nested rule
  is built. Closes the Bazel/Cargo/pnpm-workspace gap where
  users previously had to widen `select:` and rely on inner
  rules to short-circuit.

  ```yaml
  - id: workspace-member-has-readme
    kind: for_each_dir
    select: "crates/*"
    when_iter: 'iter.has_file("Cargo.toml")'
    require:
      - kind: file_exists
        paths: "{path}/README.md"
    level: error
  ```

  Without `when_iter:`, `crates/notes/` (no `Cargo.toml`)
  would have fired the missing-README rule. With it, only
  workspace members are evaluated.

- **`iter.*` namespace in `when:` expressions** — exposes
  the iterated entry's metadata to the existing `when:`
  grammar. Same expression compiles in `when_iter:` (outer
  iteration filter) and in any nested rule's `when:`
  (per-iteration nested gate). Outside an iteration
  context, `iter.X` resolves to `null` and
  `iter.has_file(_)` to `false`, matching the
  "missing fact is falsy" convention.

  | Reference | Type | Notes |
  |---|---|---|
  | `iter.path` | string | Relative path of the iterated entry. |
  | `iter.basename` | string | Basename. |
  | `iter.parent_name` | string | Parent dir name. |
  | `iter.stem` | string | Basename minus final extension. |
  | `iter.ext` | string | Final extension without the dot. |
  | `iter.is_dir` | bool | `true` for `for_each_dir`, `false` for `for_each_file`. |
  | `iter.has_file(pattern)` | bool | Glob match relative to the iterated directory. Always `false` on file iteration. |

- **Function-call syntax in the `when:` grammar.** Limited
  to a fixed allow-list of methods on `iter` (currently just
  `has_file`); typos in user configs surface as
  "unknown iter method" parse errors instead of silently
  coercing to `false`. Calls on non-iter namespaces are a
  parse error.

### Internal

- New public types `IterEnv` and `WhenEnv::with_iter()` in
  `alint-core::when`. New `WhenExpr::Call` AST variant.
  `WhenEnv::new()` constructor for callers without
  iteration context.
- Shared parser helper `for_each_dir::parse_when_iter`
  reused by `for_each_file` and `every_matching_has`.
- 9 new unit tests in `alint-core::when` covering the iter
  namespace + function-call grammar + outside-iter
  fallback. 4 new e2e scenarios under
  `crates/alint-e2e/scenarios/check/when_iter/`: marker-file
  filter, basename predicate, recursive-glob predicate,
  composition with `facts.*`.

### Compatibility

- Schema version remains `1`. `when_iter:` is opt-in; rules
  that don't use it behave identically to v0.5.1.
- Public API additions are non-breaking. `WhenEnv` gains an
  `iter: Option<IterEnv>` field; the new `WhenEnv::new()`
  constructor and existing struct-literal syntax both work.
  Out-of-tree code constructing `WhenEnv { facts, vars }`
  (without explicit `iter`) needs to add `iter: None` (or
  switch to `WhenEnv::new(facts, vars)`).
- `evaluate_for_each` (an `alint-rules` crate-private
  helper) gained a `when_iter` parameter — only matters if
  you've forked the crate.

## [0.5.1] - 2026-04-26

Plugin tier 1: `command` rule kind. Wraps any CLI on `PATH`
into alint's report. Continues the v0.5 monorepo-scale theme
— pairs naturally with `--changed` so per-file external
checks (actionlint, shellcheck, kubeconform, …) become
incremental in CI. Schema-compatible; every v0.5.0 config
runs unchanged.

### Added

- **`kind: command`** — per-file rule that spawns argv with
  path-template substitution (`{path}`, `{dir}`, `{stem}`,
  `{ext}`, `{basename}`, `{parent_name}`). Exit `0` is a
  pass; non-zero produces a violation whose message is the
  truncated stdout+stderr. Working dir is the repo root;
  stdin is closed (`/dev/null`). Output is capped at 16 KiB
  per stream to keep reports legible.

  ```yaml
  - id: workflows-clean
    kind: command
    paths: ".github/workflows/*.{yml,yaml}"
    command: ["actionlint", "{path}"]
    level: error
  ```

  Environment threaded into each invocation: `ALINT_PATH`
  (relative to root), `ALINT_ROOT` (absolute), `ALINT_RULE_ID`,
  `ALINT_LEVEL`, plus `ALINT_VAR_<NAME>` per top-level
  `vars:` entry and `ALINT_FACT_<NAME>` per resolved fact.

- **`timeout: <seconds>`** option on `command` rules. Default
  30s. Past the limit, the child process is killed and a
  violation reports the timeout. Bounds runaway tools so a
  hung child never stalls the whole run.

- **`--changed` interaction.** `command` is per-file (no
  `requires_full_index` override), so it inherits the v0.5
  filtered-index iteration: `alint check --changed` spawns
  the wrapped tool only for files in the diff. A
  `shellcheck` rule on a 200-script repo invokes
  `shellcheck` zero times when the diff doesn't touch any
  `.sh`. Largest practical multiplier on CI cost for
  external-linter wrappers.

### Security

- **Trust gate.** `command` rules are only permitted in the
  user's own top-level `.alint.yml`. A `kind: command` rule
  introduced via `extends:` — local file, HTTPS URL, or
  `alint://bundled/<name>@<rev>` — is rejected at load time
  with a clear error pointing at the offending source.
  Adopting a published ruleset must never gain it arbitrary
  process execution. New public function
  `alint_dsl::reject_command_rules_in` mirrors the existing
  `alint_core::facts::reject_custom_facts_in` gate.

### Internal

- New `alint_rules::command` module (~330 LOC including 9
  unit tests). Polling-based wait loop with 10ms granularity
  for the timeout path; output capping via
  `Read::take(OUTPUT_CAP_BYTES)`. JSON Schema gains a
  `rule_command` branch; root + in-crate copies kept
  byte-identical by the existing drift-guard test.

- 3 new e2e integration tests under
  `crates/alint-e2e/tests/command_plugin.rs` (`#[cfg(unix)]`
  — relies on `/bin/sh`): full-engine pass case, full-engine
  fail case (one violation per failing file), and the
  `--changed` interaction (only invoked for files in the
  diff). 2 new unit tests in `alint-dsl` covering the trust
  gate (rejected from `extends:`, allowed in top-level).

### Compatibility

- Schema version remains `1`. JSON / SARIF / GitHub outputs
  byte-equivalent for configs that don't use `command`.
- Public API additions are non-breaking. `Rule` trait
  unchanged; the new `reject_command_rules_in` is a new
  public function in `alint-dsl`.

## [0.5.0] - 2026-04-26

First v0.5 cut. Headline: incremental `alint check --changed`
mode for pre-commit and PR-check paths. Schema-compatible;
every v0.4.10 config runs unchanged. JSON / SARIF / GitHub
outputs byte-equivalent for full-tree runs.

### Added

- **`alint check --changed [--base=<ref>]`** and the same
  flags on `alint fix`. With `--base`, the changed-set is
  derived from `git diff --name-only --relative
  <base>...HEAD` (three-dot — merge-base diff, the right
  shape for PR checks). Without `--base`, it's
  `git ls-files --modified --others --exclude-standard`
  (working-tree diff, the right shape for pre-commit). The
  engine evaluates per-file rules against a [`FileIndex`]
  filtered to the changed-set, so a Java license-header rule
  scoped to `**/*.java` skips entirely when no `.java` file
  is in the diff. Cross-file rules (`pair`, `for_each_dir`,
  `every_matching_has`, `unique_by`, `dir_contains`,
  `dir_only_contains`) and existence rules (`file_exists`,
  `file_absent`, `dir_exists`, `dir_absent`) keep full-tree
  semantics for iteration; existence rules additionally skip
  when their `paths:` scope doesn't intersect the diff so an
  unchanged-but-missing LICENSE doesn't fire on every PR.
  Empty diffs short-circuit to an empty report (the
  no-op-commit case in pre-commit). Outside a git repo or
  when `git` isn't on PATH, `--changed` exits non-zero with
  a clear message rather than silently fall back to a full
  check.

  ```bash
  # Pre-commit: lint the working-tree diff.
  alint check --changed

  # PR check: lint everything that diverged from main.
  alint check --changed --base=main --format=sarif
  ```

- **`Rule::requires_full_index() -> bool`** and
  **`Rule::path_scope() -> Option<&Scope>`** on the public
  `alint-core::Rule` trait. Both default to "no opt-in", so
  out-of-tree rule implementations compile unchanged.
  Internal rules override on the eleven cases that need
  full-tree semantics: the six cross-file kinds plus the
  four existence kinds. Per-file rules need no override —
  the engine hands them the filtered index and their
  existing `Scope::matches` loops do the right thing.

- **`alint_core::git::collect_changed_paths(root, base)`**
  helper, parallel to the existing
  `collect_tracked_paths`. Returns the changed-set as a
  `HashSet<PathBuf>` of paths relative to `root`, or `None`
  outside a git repo / when `git` exits non-zero.

- **`Engine::with_changed_paths(set)`** builder method.
  Threads the changed-set through `Engine::run` and
  `Engine::fix`. Every call costs one walk over the
  index entries to build a filtered subset; absent the
  builder call, the engine behaves exactly as before.

- **`Step::CheckChanged`** in `alint-testkit`'s scenario
  harness. Five new e2e scenarios under
  `crates/alint-e2e/scenarios/check/changed/` cover:
  per-file rule skipped when scope misses the diff,
  per-file rule fires only on changed files, cross-file
  `pair` keeps full-tree semantics, existence rule skips
  when scope doesn't intersect, and the empty-diff
  short-circuit.

### Compatibility

- Schema version remains `1`. Every v0.4 config runs
  unchanged.
- Public API additions are non-breaking: `Rule` trait
  methods have defaults, `Engine::with_changed_paths` is
  additive. Embedders that hand-construct an `Engine` keep
  compiling.
- `alint-testkit::Step` gained a variant
  (`Step::CheckChanged`); embedders that exhaustively
  matched on `Step` need to add an arm for it.

## [0.4.10] - 2026-04-25

Three new content-family rule kinds rounding out the family.
Schema-compatible; every v0.4.9 config runs unchanged. JSON /
SARIF / GitHub outputs byte-equivalent.

### Added

- **`file_max_lines`** (alias `max_lines`). Mirror of
  `file_min_lines`: files in scope must have AT MOST
  `max_lines` lines. Same `wc -l` accounting. Catches the
  everything-module anti-pattern.
- **`file_footer`** (alias `footer`). Mirror of `file_header`
  anchored at the END of the file: the last `lines:` lines
  must match a regex. Use cases: license footers, signed-off-by
  trailers, generated-file sentinels. Fix op: `file_append`.
- **`file_shebang`** (alias `shebang`). First line of each
  file must match a regex. Pairs with `executable_has_shebang`
  (which checks shebang *presence*) — `file_shebang` checks
  shebang *shape*, e.g. `^#!/usr/bin/env bash$` to enforce a
  specific interpreter. Defaults to `^#!` (presence only).

  Brings the rule catalogue to ~55 kinds.

- 6 new e2e scenarios covering pass/fail paths for the three
  new kinds.

## [0.4.9] - 2026-04-25

Java bundled ruleset. Schema-compatible; every v0.4.8 config
runs unchanged. JSON / SARIF / GitHub outputs byte-equivalent.

### Added

- **`alint://bundled/java@v1`** (10 rules). Maven + Gradle
  hygiene, gated `when: facts.is_java`:
  - `java-manifest-exists` — `pom.xml`, `build.gradle`, or
    `build.gradle.kts` at the root (error).
  - `java-build-wrapper-committed` — `mvnw` / `gradlew` checked
    in for reproducible builds (info).
  - `java-no-tracked-target` / `java-no-tracked-build` —
    Maven's `target/` and Gradle's `build/` not committed.
    Both use `git_tracked_only: true` (the v0.4.8 primitive)
    so a developer's locally-built directories stay silent;
    only directories whose contents made it into git's index
    fire (error).
  - `java-no-class-files` — `*.class` files not committed
    (`git_tracked_only: true`, error).
  - `java-sources-pascal-case` — PascalCase filenames for
    `*.java`, with `package-info.java` / `module-info.java`
    excluded (warning).
  - `java-sources-final-newline` /
    `java-sources-no-trailing-whitespace` — text hygiene,
    auto-fixable (info).
  - `java-sources-no-bidi` / `java-sources-no-zero-width` —
    Trojan Source defenses (error).

  Brings the bundled catalogue to 12 rulesets. The
  `git_tracked_only` rules in this ruleset are the first
  bundled use of v0.4.8's git-aware primitive — the
  `silent_on_locally_built_target` e2e scenario proves the
  wiring end-to-end.

## [0.4.8] - 2026-04-25

First git-aware primitive lands. Schema-compatible; every v0.4.7
config runs unchanged. JSON / SARIF / GitHub outputs gain no new
keys.

### Added

- **`git_tracked_only: bool`** option on `RuleSpec`, currently
  honoured by `file_exists`, `file_absent`, `dir_exists`, and
  `dir_absent`. When `true`, the rule's `paths`-matched entries
  are intersected with `git ls-files`'s output so only files /
  directories actually in git's index participate. Closes the
  approximation gap documented on the
  [walker-and-gitignore concept page](https://alint.org/docs/concepts/walker-and-gitignore/):
  a `dir_absent` rule on `**/target` with `git_tracked_only: true`
  fires only when `target/` was actually committed, never on a
  developer's locally-built `target/` (gitignored or not). Outside
  a git repo, or when `git` isn't on PATH, the tracked-set is
  empty and rules with the flag set become silent no-ops — the
  right default for "don't let X be committed" semantics.

  ```yaml
  - id: target-not-tracked
    kind: dir_absent
    paths: "**/target"
    git_tracked_only: true
    level: error
  ```

  Other rule kinds currently ignore the field; we'll extend
  coverage as concrete use cases come up. The roadmap'd
  `git_no_denied_paths` and `git_commit_message` primitives are
  still pending.

### Changed

- `alint-core::Context` gains a `git_tracked: Option<&HashSet<PathBuf>>`
  field, plus `is_git_tracked` / `dir_has_tracked_files` helpers.
  External embedders constructing a `Context` by hand need to add
  `git_tracked: None`. The engine collects the set at most once
  per `run` / `fix`, only when at least one rule's
  `wants_git_tracked()` is true — zero cost when no rule opts in.
- `alint-testkit`'s `Given` block accepts an optional `git: { init,
  add, commit }` block so e2e scenarios can stand up a real git
  repo in their tempdir before alint runs.

## [0.4.7] - 2026-04-24

Distribution breadth. Schema-compatible; every v0.4.6 config
runs unchanged. JSON/SARIF/GitHub outputs byte-equivalent. No
Rust code changes — this release ships new install paths only.

### Added

#### Docker image

- **`ghcr.io/asamarts/alint`** — distroless multi-arch
  (`linux/amd64`, `linux/arm64`) image based on
  `gcr.io/distroless/static-debian12:nonroot`. Built by the
  release workflow from the same statically-linked musl
  binaries shipped in the GitHub Release tarballs, so the
  in-image binary matches the tarballed one byte-for-byte.
  Tags published per release: the exact git tag (`:v0.4.7`),
  the bare semver (`:0.4.7`), the `<major>.<minor>` channel
  (`:0.4`), and `:latest`.

  ```bash
  docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:latest
  ```

  Runs as the distroless `nonroot` user (UID 65532). For
  `alint fix` workflows that need to write with host
  ownership, pass `-u $(id -u):$(id -g)`.

#### Homebrew tap

- **`asamarts/alint`** — dedicated Homebrew tap at
  [asamarts/homebrew-alint](https://github.com/asamarts/homebrew-alint)
  shipping a `Formula/alint.rb` that resolves the right
  pre-built tarball for each platform (macOS arm64 + x86_64,
  Linuxbrew arm64 + x86_64) and verifies its SHA-256.

  ```bash
  brew tap asamarts/alint
  brew install alint
  ```

  The formula is regenerated on every tagged release by a new
  `homebrew` job in `.github/workflows/release.yml` driving
  `ci/scripts/update-homebrew-formula.sh`. The script takes
  SHAs directly from the release's `SHA256SUMS` artifact —
  no re-download, no re-build — and pushes via a per-repo
  ed25519 deploy key scoped to the tap.

### Infrastructure

- New release-workflow jobs: `docker` (builds + pushes the
  multi-arch image to ghcr.io) and `homebrew` (regenerates
  the formula and pushes to the tap). Both run after the
  existing `build` / `release` jobs; failures there skip the
  distribution jobs cleanly.
- New script `ci/scripts/update-homebrew-formula.sh` emits a
  complete `Formula/alint.rb` given `VERSION` + a
  `SHA256SUMS` path. Handles both `sha256sum`-style and
  Windows-mode (`*file`) sum lines, errors clearly on
  missing platforms, validates required env up front.
- New test harness `ci/scripts/test-update-homebrew-formula.sh`
  (14 assertions — happy path, asterisk-prefix handling,
  missing-platform + missing-env error paths, version /
  license / test-block shape). Wired into `ci/scripts/test.sh`
  so it runs on every CI pass.

## [0.4.6] - 2026-04-23

Ecosystem coverage + debugging ergonomics. Schema-compatible;
every v0.4.5 config runs unchanged. JSON output unchanged for
existing commands; SARIF and GitHub outputs byte-equivalent.

### Added

#### Two new bundled rulesets

- **`alint://bundled/python@v1`** (7 rules). Canonical
  Python-project hygiene:
  - `python-manifest-exists` — pyproject.toml / setup.py /
    setup.cfg at the root (error).
  - `python-has-lockfile` — uv.lock / poetry.lock / Pipfile.lock
    / pdm.lock (warning).
  - `python-pyproject-declares-name` — PEP 621 `$.project.name`
    via `toml_path_matches` (warning).
  - `python-pyproject-declares-requires-python` — a
    `requires-python` floor via `toml_path_matches` on
    `$.project['requires-python']` (info).
  - `python-module-snake-case` — PEP 8 snake_case filenames
    for top-level and `src/**/*.py` (info).
  - `python-sources-final-newline` + `python-sources-no-trailing-whitespace`
    (info, auto-fixable).
  - `python-sources-no-bidi` — Trojan Source defense (error).

  Every rule gated with `when: facts.is_python`, so the ruleset
  silently no-ops on non-Python repos.

- **`alint://bundled/go@v1`** (7 rules). Go-module hygiene:
  - `go-mod-exists` — go.mod at the root (error).
  - `go-sum-exists` — go.sum at the root (warning).
  - `go-mod-declares-module-path` — `module <path>` directive
    (error, `file_content_matches`).
  - `go-mod-declares-go-version` — `go <major>.<minor>` directive
    (warning, `file_content_matches`).
  - `go-sources-no-bidi` / `go-sources-no-zero-width` —
    Trojan Source defenses (error).
  - `go-sources-final-newline` (info, auto-fixable).

  Every rule gated with `when: facts.is_go`.

  Brings the bundled catalog to eleven rulesets.

#### `alint facts` subcommand

- New top-level subcommand that evaluates every `facts:` entry
  in the effective config and prints the resolved value.
  Debugging aid for `when:` clauses — quickly answers "did my
  `facts.is_python` actually match?" without running the full
  check pass. Supports `--format human` (columnar) and
  `--format json` (`{facts: [{id, kind, value}, ...]}`).

### Changed

- `alint-core::FactKind::name()` added — returns the YAML
  discriminator string (`any_file_exists`, `count_files`, etc.).
  Used by the `facts` subcommand's renderers; available to
  external embedders.

## [0.4.5] - 2026-04-23

Supply-chain hardening ruleset + composition ergonomics.
Schema-compatible; every v0.4.4 config runs unchanged. JSON
output gains no new keys; SARIF and GitHub outputs are
byte-equivalent.

### Added

#### New bundled ruleset

- **`alint://bundled/ci/github-actions@v1`** (3 rules). GitHub
  Actions hardening guided by the two OpenSSF Scorecard checks
  with the strongest supply-chain signal:
  - `gha-workflow-contents-read` — every workflow declares
    `permissions.contents: read` at the workflow level
    (`yaml_path_equals`, warning).
  - `gha-pin-actions-to-sha` — every `uses:` across every job
    is pinned to a 40-char commit SHA, not a mutable tag
    (`yaml_path_matches` with `if_present: true`, warning).
  - `gha-workflow-has-name` — every workflow declares a
    `name:` so the Actions UI shows something friendlier than
    the filename (`yaml_path_matches`, info).

  Scoped to `.github/workflows/*.y{,a}ml`, so it no-ops in
  repos that don't use GitHub Actions. Brings the bundled
  catalogue to nine rulesets.

#### Structured-query `if_present`

- **`if_present: true`** option on every structured-query rule
  kind (`{json,yaml,toml}_path_{equals,matches}`). When
  enabled, a JSONPath query that returns zero matches is
  silently OK — only actual matches that fail the op produce
  violations. Preserves the default "missing = violation"
  semantics (`if_present: false`, the existing behaviour).
  Required for conditional predicates like "every `uses:` is
  SHA-pinned" where a workflow with only `run:` steps
  shouldn't be flagged.

#### Selective bundled adoption

- **`only:` / `except:` on `extends:` entries.** An entry can
  now be a mapping that filters the inherited rule set by id
  before merging:

  ```yaml
  extends:
    - url: alint://bundled/oss-baseline@v1
      except: [oss-code-of-conduct-exists]      # drop one rule

    - url: alint://bundled/ci/github-actions@v1
      only: [gha-pin-actions-to-sha]            # keep one rule
  ```

  Filters resolve against the fully-resolved rule set of the
  entry (i.e. anything it transitively extends). `only:` and
  `except:` are mutually exclusive on a single entry. Listing
  an unknown id is a load-time error so typos don't silently
  drop anything.

  Closes the "all-or-nothing" limitation on bundled-ruleset
  adoption that forced users to extend + then restate overrides
  with `level: off` for every rule they wanted to skip.

### Changed

- `Config.extends` type changed from `Vec<String>` to
  `Vec<ExtendsEntry>`. `ExtendsEntry` is an untagged enum that
  accepts either a bare string (classic form) or a mapping
  `{url, only?, except?}`. YAML ergonomics unchanged for the
  string form; existing configs continue to parse as before.

## [0.4.4] - 2026-04-23

Rule-catalogue expansion + README rewrite. Schema-compatible;
every v0.4.3 config runs unchanged. JSON output gains no new
keys; SARIF and GitHub outputs are byte-equivalent.

### Added

#### Content-family additions

- **`file_min_size`** — files in scope must be at least
  `min_bytes` bytes. Complements `file_max_size`. Picks up the
  "zero-byte LICENSE" case that passes `file_exists` but carries
  no information.
- **`file_min_lines`** — files in scope must have at least
  `min_lines` lines (`wc -l` semantics: every `\n` terminates a
  line, plus one more when the file has trailing unterminated
  content). Catches the classic "README is a title plus
  `TODO`" stub. Both kinds register short aliases (`min_size`,
  `min_lines`) alongside the prefixed names.

#### Structured-query family (six new rule kinds)

JSONPath (RFC 9535) queries over JSON / YAML / TOML documents,
powered by `serde_json_path`. YAML and TOML files are
deserialized through serde into a `serde_json::Value` so the
same path-expression engine applies to all three. Missing
JSONPath matches are treated as violations (conservative — scope
narrowly or relax the path for optional keys); when a query
returns multiple matches, every match must satisfy the rule.
Unparseable files surface a single per-file violation rather
than being silently skipped.

- **`json_path_equals`** / **`json_path_matches`** — `equals`
  compares by value (string / number / bool / null); `matches`
  runs a regex against the string form of the matched value.
  Canonical use: enforce a `package.json` license, require a
  semver `version`, lock a `private: true` flag.
- **`yaml_path_equals`** / **`yaml_path_matches`** — same
  engine over YAML. Canonical use: lock GitHub Actions
  workflows to `permissions.contents: read`, require every
  `uses:` across every job to be pinned to a 40-char commit
  SHA.
- **`toml_path_equals`** / **`toml_path_matches`** — same
  engine over TOML. Canonical use: require `edition = "2024"`
  across every `Cargo.toml` in a workspace, enforce
  `$.project.version` semver in `pyproject.toml`.

#### `oss-baseline` ruleset extensions

- `oss-license-non-empty` — `file_min_size` at 200 bytes on the
  LICENSE, catching zero-byte placeholders.
- `oss-readme-non-stub` — `file_min_lines` at 3 on the README,
  gentle enough to pass for early-stage repos.

### Changed

- **README rewrite.** Replaced the single monolithic `.alint.yml`
  example with a 12-pattern cookbook covering the real-world use
  cases v0.4 now spans: bundled-ruleset adoption, composition
  overrides, structured queries against `package.json` / GitHub
  workflows / `Cargo.toml`, monorepo per-package rules via
  `for_each_dir`, nested-config subtree scoping, auto-fix
  hygiene, fact-gated conditionals, cross-file `pair` / `unique_by`,
  and the security-family bans. Bumped the family count from
  ten to eleven and the rule-kind count from ~42 to ~50.

## [0.4.3] - 2026-04-23

Composition ergonomics + monorepo support + four new bundled
rulesets. Schema-compatible; every v0.4.2 config runs unchanged.
JSON output gains no new keys; SARIF and GitHub outputs are
byte-equivalent.

### Added

#### Composition

- **Field-level rule override.** Children in the `extends:`
  chain can specify only the fields that change. A common
  override shrinks from four lines to two:

  ```yaml
  # before — had to restate kind + paths to tweak level
  rules:
    - id: no-bak
      kind: file_absent
      paths: "**/*.bak"
      level: warning
  ```

  ```yaml
  # after — id + changed fields are enough; rest inherits
  rules:
    - id: no-bak
      level: warning
  ```

  The loader keeps rules as raw `serde_yaml_ng::Mapping`s
  through the `extends:` chain and field-merges by id. After
  all extends resolve, each merged mapping is deserialized
  once into a `RuleSpec` — a rule that never receives a
  `kind` anywhere in its chain surfaces as a clean error
  referencing the offending id. Facts still replace wholesale
  by id (their kind is a discriminated union).

- **Nested `.alint.yml` discovery for monorepos.** Opt in with
  `nested_configs: true` on the root config. The loader walks
  the tree (respecting `.gitignore` + `ignore:`), picks up
  every nested `.alint.yml` / `.alint.yaml`, and prefixes each
  nested rule's path-like fields (`paths`, `select`, `primary`)
  with the nested config's relative directory. A rule declared
  in `packages/frontend/.alint.yml` with `paths: "**/*.ts"`
  evaluates as if it read `paths: "packages/frontend/**/*.ts"`
  at the root.

  MVP guardrails: nested configs can only declare `version:`
  and `rules:`; every nested rule must have at least one
  scope field; absolute paths and `..`-prefixed globs are
  rejected; rule-id collisions across configs error with a
  clear message (per-subtree overrides are a follow-up).

#### Bundled rulesets

Four new rulesets pulled from a research pass across
Turborepo/Nx/Bazel/Cargo/pnpm docs, OpenSSF Scorecard, and
Repolinter's archived corpus. Buildable on the existing
primitive set — no new rule kinds required.

- **`alint://bundled/hygiene/no-tracked-artifacts@v1`** — 11
  rules. `dir_absent` on `node_modules`, `target`, `dist`,
  `build`, `out`, `.next`, `.nuxt`, `.svelte-kit`, `.turbo`,
  `coverage`, `__pycache__`, `.venv`, `.mypy_cache`,
  `.pytest_cache`, `.ruff_cache`, `.bundle`, `vendor/bundle`,
  `.go-build`. `file_absent` on `.DS_Store`, `._*`, `Thumbs.db`,
  `desktop.ini`, `*~`, `*.swp`, `*.swo`, `*.bak`, `*.orig`,
  `.env`, `.env.local`, `.env.*.local`, `.env.development` /
  `production` / `staging` (`.env.example` is exempt). 10 MiB
  size gate. Several rules auto-fixable via `file_remove`.

- **`alint://bundled/hygiene/lockfiles@v1`** — 7 rules, one per
  package manager (npm / pnpm / yarn / bun / Cargo / Poetry /
  uv). Each uses an `include/exclude` path pair so the root
  lockfile is exempted while nested copies are flagged as a
  workspace-misconfiguration smell.

- **`alint://bundled/tooling/editorconfig@v1`** — 3 info-level
  rules: root `.editorconfig` + `.gitattributes` exist, and
  `.gitattributes` contains a `text=` normalization directive.

- **`alint://bundled/docs/adr@v1`** — 4 rules. Files under
  `docs/adr/` match `NNNN-kebab-case-title.md`; each ADR has
  `## Status`, `## Context`, `## Decision` sections. Gap-free
  ADR numbering deferred to a future `numeric_sequence`
  primitive.

Bundled catalog now: 8 rulesets (4 ecosystem + 4 namespaced).
Slash-namespaced names (`hygiene/*`, `tooling/*`, `docs/*`)
route through the existing `alint://bundled/<name>@<rev>` URI
scheme — the `@` separator parses cleanly around slashes in
the name.

#### Config

- **`nested_configs: true`** field on the root `Config` to
  opt in to nested-config discovery.

### Changed

- **`extends:` schema description** refreshed to cover SRI
  syntax, `alint://bundled/` URLs, merge semantics, and the
  `level: off` disable idiom. Old description claimed HTTPS
  was "reserved for a future version" (shipped in v0.2.1).

### Tests

Workspace: 422 → 437 tests (+15). Includes 6 new unit tests
on nested-discovery, 3 e2e on field-level override, 2 e2e on
nested discovery, 4 e2e on Phase A bundled rulesets.

## [0.4.2] - 2026-04-22

Pretty-output overhaul of the `human` formatter. Schema-compatible;
every v0.4.1 config still runs, every JSON/SARIF/GitHub output is
byte-equivalent. Only the human-mode rendering and three new global
CLI flags (`--color`, `--ascii`, `--compact`) are new.

### Added

- **Grouped-by-file layout** — violations now render under a
  dim section header per file (`─── src/foo.rs ─────…`), with
  a leading "Repository-level" bucket for path-less findings.
  Each violation shows `<sigil>  <level>  <rule-id>  [fixable]`
  on one line and the message (optionally prefixed with
  `line:col`) on the next. Policy URLs render as `docs: <url>`
  immediately under the relevant violation.
- **Per-severity summary** — `Summary (N violations): ✗ 2 errors
  ⚠ 1 warning  ℹ 5 info` + `X passing · Y failing · Z
  auto-fixable` + a call-to-action line `→ run `alint fix` to
  resolve N fixable violation(s).` when anything's auto-fixable.
  All-passed gets a concise green `✓ All N rule(s) passed.`.
- **`--color <auto|always|never>` global flag.** Defaults to
  `auto` — honors `NO_COLOR`, `CLICOLOR_FORCE`, and TTY status
  via `anstream::AutoStream`. JSON / SARIF / GitHub formats are
  unaffected.
- **`--ascii` global flag** forces ASCII glyphs (e.g. `x`/`!`/`i`
  instead of `✗`/`⚠`/`ℹ`, `---` instead of `───`). Auto-enabled
  when `TERM=dumb`.
- **`--compact` global flag** switches to one-line-per-violation
  output suitable for editors, `grep`, `wc -l`. Format:
  `path:line:col: level: rule-id: message  [fixable]`, with
  `<repo>` as the pseudo-path for path-less findings.
- **OSC 8 hyperlinks on policy URLs** when the terminal supports
  them (detected via `supports-hyperlinks`). Modern terminals
  (iTerm2, Kitty, WezTerm, Alacritty, VSCode, GNOME Terminal,
  Windows Terminal) make the `docs:` URL clickable without any
  visible change. Older terminals see the plain URL they always
  saw.
- **`RuleResult.is_fixable`** exposed on the JSON output as
  `fixable: bool`, letting tooling decide whether to prompt
  users toward `alint fix` without cross-referencing rule
  metadata.

### Changed

- **Terminal-width-aware section separators.** Auto-detected via
  `terminal_size` on TTY, clamped to `[40, 120]` cols so wide
  terminals don't produce unreadably long rules and narrow ones
  still get some visual fill. Falls back to 80 cols off-TTY.
- **Tightened vertical density** — no blank lines between
  violations within a bucket. Visual separation still reads
  clearly because the colored sigil anchors at column 2 while
  continuation lines indent to column 14. A typical 8-violation
  run drops from ~36 to ~27 lines of output.

### Internal

- New `alint-output::style` module centralizing role-based
  `anstyle::Style` constants, `GlyphSet` (Unicode / ASCII), and
  `HumanOptions` (plumbing for glyphs / hyperlinks / width /
  compact). Swapping the palette is a one-file edit.
- `Format::write_with_options` / `Format::write_fix_with_options`
  added; existing `write` / `write_fix` remain as default-opts
  shims so external embedders compile unchanged.

### Dependencies

- `anstyle` + `anstream` (ANSI styling with built-in `NO_COLOR` /
  TTY handling).
- `supports-hyperlinks` (OSC 8 detection).
- `terminal_size` (column-width detection).

## [0.4.1] - 2026-04-21

Packaging fix. v0.4.0 is functionally identical but failed to
publish beyond `alint-core` on crates.io — the bundled-rulesets
`include_str!` paths crossed the crate boundary, so
`cargo publish` for `alint-dsl` couldn't find
`rulesets/v1/*.yml` when packaging the tarball.

### Fixed

- **Move `rulesets/` → `crates/alint-dsl/rulesets/`**. The
  rulesets now live inside the crate that embeds them, so
  `cargo publish` picks them up automatically. Compile-time
  `include_str!` paths in `bundled.rs` change from
  `"../../../rulesets/…"` to `"../rulesets/…"`. No user-visible
  behaviour change — the `alint://bundled/<name>@<rev>` URI
  scheme and all four rulesets work identically.

### Known leftover

- `alint-core@0.4.0` is live on crates.io (it published
  successfully before the packaging error stopped the chain).
  It's functionally identical to `alint-core@0.4.1` and nothing
  transitively depends on it — safe to ignore or yank later.

## [0.4.0] - 2026-04-21

Headline: **bundled rulesets**. The single biggest adoption
lever identified during pre-launch review — reduces onboarding
from "write a ruleset" to "add one `extends:` line." Also lands
pre-commit framework integration so any pre-commit user can
adopt alint with 4 lines of YAML.

### Added

#### Bundled rulesets

- **`alint://bundled/<name>@<rev>` URI scheme** for offline
  resolution of built-in rulesets. Rulesets live under
  `rulesets/<rev>/<name>.yml` and are embedded in the binary via
  `include_str!` at compile time. Cycle-safe, leaf-only — a
  bundled ruleset cannot itself declare `extends:` and cannot
  introduce `custom:` facts, inheriting the same safety guards
  as HTTPS extends.
- **`alint://bundled/oss-baseline@v1`** — 9 rules. Community
  docs (README, LICENSE, SECURITY.md, CODE_OF_CONDUCT.md,
  .gitignore) + merge-marker + bidi-control bans +
  trailing-whitespace / final-newline hygiene (auto-fixable).
- **`alint://bundled/rust@v1`** — 10 rules. Cargo.toml /
  Cargo.lock / rust-toolchain existence, no tracked `target/`,
  snake_case source filenames, Trojan-Source defenses. Every
  rule gated `when: facts.is_rust` so extending it from a
  polyglot repo is a safe no-op outside Rust trees.
- **`alint://bundled/node@v1`** — 8 rules. package.json +
  lockfile (npm / pnpm / yarn / bun), no tracked `node_modules/`
  or common build outputs, Node version pinned via `.nvmrc` /
  `.node-version` / `.tool-versions`, JS/TS source hygiene.
  Gated `when: facts.is_node`.
- **`alint://bundled/monorepo@v1`** — 4 rules. Every directory
  under `{packages,crates,apps,services}/*` has a README +
  ecosystem manifest; unique basenames. Pair with rust@v1 /
  node@v1 for per-package ecosystem checks.

#### Distribution

- **`.pre-commit-hooks.yaml`** at the repo root exposes two
  hooks for [pre-commit](https://pre-commit.com/) users:
  - `alint` — runs `alint check`; non-mutating.
  - `alint-fix` — runs `alint fix`; `stages: [manual]` by
    default so it only runs when explicitly invoked via
    `pre-commit run alint-fix`.

  Both use `language: rust`, so pre-commit builds alint on
  first run — zero install step.

### Changed

- **README quickstart** gains a "Bundled rulesets (one-line
  baseline)" section and a "pre-commit" subsection under "Use
  in CI".
- **`docs/rules.md`** gains a Bundled rulesets section with a
  per-ruleset table (rule id / kind / default level / fix op)
  for every shipped ruleset, plus the override pattern.
- **ROADMAP**: the original v0.4 scope (structured-query
  primitives, git-aware primitives, Homebrew / Docker / npm,
  markdown/junit/gitlab outputs, `command` plugin, nested
  config discovery) rolls forward to v0.5. Bundled rulesets
  moved from v0.5 → v0.4 because they're the largest
  single-step adoption lever.

### Compatibility

- Schema version remains `1`. Every v0.3 config runs unchanged.
- No changes to the `Rule`, `Fixer`, or `Engine` APIs.
  `alint-dsl` gains a new public `bundled` module; the existing
  `load` / `load_with` entry points are unchanged.

## [0.3.2] - 2026-04-21

Patch release fixing a broken `action.yml` that affects every
`asamarts/alint@v0.2.1` / `v0.3.0` / `v0.3.1` consumer. No code
changes to the CLI; no schema changes.

### Fixed

- **`action.yml`** — the `outputs.sarif-file.description` value
  was an unquoted YAML scalar containing `` `format: sarif` ``.
  GitHub's Actions YAML parser recently tightened and now
  rejects the embedded `:` as an ambiguous nested mapping,
  causing `uses: asamarts/alint@<any-released-tag>` to fail at
  job-setup time with `Mapping values are not allowed in this
  context.`. The description is now double-quoted.
- **docs build** — rustdoc's `redundant_explicit_links` became
  warn-by-default in a recent stable; combined with the
  workspace's `RUSTDOCFLAGS="-D warnings"` it was breaking the
  `cargo doc` CI job. Fixed a redundant link target in
  `alint-testkit::runner`. Affects contributors / anyone
  building from source with the current stable; no effect on
  the release binary.

### Changed

- **Action self-test workflow** now pins `uses:` refs to
  `@main` so it exercises the current action code instead of
  an immutable (and, as of this week, unparsable) `@v0.2.1`.
  The explicit-version test still passes a stable release tag
  as the `version:` input — bumped from `v0.2.1` to `v0.3.1`
  so the downloaded CLI understands the dogfood config's v0.3
  rule kinds.

### Upgrade note

Consumers pinning `asamarts/alint@v0.2.1` / `v0.3.0` / `v0.3.1`
should bump to `@v0.3.2`. There are no API / CLI / config
changes — configs that worked under `@v0.3.1` continue to work
verbatim.

## [0.3.1] - 2026-04-21

Documentation-only patch release following v0.3.0. No code
changes; no schema changes; v0.3.0 configs run unchanged.

### Added

- **`docs/rules.md`** — per-rule user reference organised by the
  ten families (Existence, Content, Naming, Text hygiene,
  Security / Unicode, Encoding, Structure, Portable metadata,
  Unix metadata, Git hygiene, Cross-file). Each entry has a
  purpose, a small YAML example, and a pointer to its fix op if
  one exists.

### Changed

- **`docs/design/ARCHITECTURE.md`** — rule-catalogue section
  expanded with new family tables (Text hygiene, Security /
  Unicode sanity, Structure, Portable metadata, Unix metadata,
  Git hygiene) and a new Fix operations subsection listing
  every op with its rule-kind cross-reference.
- **`README.md`** — status line bumped from "v0.2 / 18 rules /
  4 families" to "v0.3 / ~42 rules / 10 families / 12 fix ops";
  GitHub Action references updated from `asamarts/alint@v0.2.1`
  to `@v0.3.0`.
- **`.alint.yml`** (dogfood) — expanded from 17 to 32 rules to
  exercise the v0.3 catalogue against alint's own tree. All
  rules pass.

## [0.3.0] - 2026-04-21

Rule-catalogue expansion. Adds ~25 new rule kinds across seven
phase commits plus one new fix op, covering categories other
repo-linters don't reach: Windows-name reserved words, bidi /
zero-width Unicode scanners, Unix-metadata checks, and
byte-level prefix/suffix. Also introduces the `fix_size_limit`
config knob and short-name aliases for the rules that don't
have a `dir_*` sibling.

### Added

#### Rule kinds (text hygiene — Phase 1)

- **`no_trailing_whitespace`** — flag trailing space/tab on any
  line. Fixable via `file_trim_trailing_whitespace` (preserves
  LF vs CRLF endings).
- **`final_newline`** — file must end with `\n`. Fixable via
  `file_append_final_newline`.
- **`line_endings`** — `target: lf | crlf`; every line must use
  the configured ending. Fixable via
  `file_normalize_line_endings`.
- **`line_max_width`** — cap line length in characters (not
  bytes); optional `tab_width` for tab expansion.

#### Rule kinds (security / Unicode — Phase 2)

- **`no_merge_conflict_markers`** — flag `<<<<<<< `, `=======`,
  `>>>>>>> ` markers at the start of a line.
- **`no_bidi_controls`** — flag Trojan-Source bidi overrides
  (U+202A–202E, U+2066–2069). Fixable via `file_strip_bidi`.
- **`no_zero_width_chars`** — flag body-internal zero-width
  characters (U+200B/C/D plus non-leading U+FEFF). Leading BOM
  is `no_bom`'s concern. Fixable via `file_strip_zero_width`.

#### Rule kinds (encoding + content fingerprint — Phase 3)

- **`file_is_ascii`** — every byte must be < 0x80.
- **`no_bom`** — flag UTF-8 / UTF-16 LE/BE / UTF-32 LE/BE
  byte-order marks. Fixable via `file_strip_bom`.
- **`file_hash`** — assert a SHA-256 digest for specific files
  (rules-as-tripwire for generated artefacts).

#### Rule kinds (structure — Phase 4)

- **`max_directory_depth`** — cap how deep the tree may go.
- **`max_files_per_directory`** — cap per-directory fanout.
- **`no_empty_files`** — flag zero-byte files. Fixable via
  `file_remove`.

#### Rule kinds (portable metadata — Phase 5)

- **`no_case_conflicts`** — flag paths that collide under a
  case-insensitive filesystem (macOS HFS+/APFS, Windows NTFS
  defaults).
- **`no_illegal_windows_names`** — reject CON/PRN/AUX/NUL,
  COM1-9, LPT1-9 (case-insensitive, regardless of extension),
  trailing dots/spaces, and the reserved chars `<>:"|?*`.

#### Rule kinds (Unix metadata + git — Phase 6)

- **`no_symlinks`** — flag tracked paths that are symbolic
  links. Fixable via `file_remove`.
- **`executable_bit`** — `require: true|false`; enforce or
  forbid the `+x` bit. Unix-only; no-op on Windows.
- **`executable_has_shebang`** — `+x` files must begin with
  `#!`. Unix-only.
- **`shebang_has_executable`** — files starting with `#!` must
  have `+x` set. Unix-only.
- **`no_submodules`** — flag `.gitmodules` at the repo root.
  Always targets `.gitmodules` (no `paths` override). Fixable
  via `file_remove`.

#### Rule kinds (hygiene + fingerprint — Phase 7)

- **`indent_style`** — `style: tabs|spaces`, optional `width`
  for spaces; every non-blank line must indent with the
  configured style.
- **`max_consecutive_blank_lines`** — `max: N`; cap runs of
  blank lines. Fixable via new op `file_collapse_blank_lines`.
- **`file_starts_with`** — byte-level prefix check. Works on
  binary files, unlike `file_header` which is UTF-8 text.
- **`file_ends_with`** — byte-level suffix check.

#### Fix ops

- **`file_trim_trailing_whitespace`** — strip trailing space/tab
  on every line (preserves line endings).
- **`file_append_final_newline`** — add `\n` when missing.
- **`file_normalize_line_endings`** — rewrite to the parent
  rule's `lf` / `crlf` target.
- **`file_strip_bidi`** — remove U+202A–202E, U+2066–2069.
- **`file_strip_zero_width`** — remove U+200B/C/D and
  body-internal U+FEFF.
- **`file_strip_bom`** — strip a leading UTF-8/16/32 BOM.
- **`file_collapse_blank_lines`** — collapse blank-line runs to
  the parent rule's `max`.

#### Config + ergonomics

- **`fix_size_limit`** (top-level config field) — maximum bytes
  a content-editing fix will touch. Default 1 MiB; explicit
  `null` disables the cap; path-only fixes (`file_create`,
  `file_remove`, `file_rename`) ignore it. Over-limit files
  report `Skipped` with a stderr warning.
- **Short-name rule aliases** — rules without a `dir_*` sibling
  also resolve under their unprefixed name:
  `content_matches`, `content_forbidden`, `header`, `max_size`,
  `is_text`. `file_exists` / `file_absent` keep the prefix
  because they mirror `dir_exists` / `dir_absent`.

### Changed

- JSON Schema (`schemas/v1/config.json`) gains every new rule
  kind, fix op, and the `fix_size_limit` field. Root and
  in-crate copies stay byte-identical via the drift-guard test.

### Compatibility

- Schema version remains `1`. Every v0.2 config runs unchanged
  under v0.3. The `Rule` and `Fixer` traits gained no new
  required methods; out-of-tree implementations compile
  unmodified. `Engine::with_fix_size_limit` is additive.

## [0.2.1] - 2026-04-20

Patch release. Finishes the v0.2 roadmap item that didn't make it
into v0.2.0: `extends:` composition.

### Added

- **`extends:` for local files.** A config can inherit rules,
  facts, vars, and ignore globs from another YAML file on disk
  (relative or absolute path). Resolution is recursive with cycle
  detection; merge is id-based with child-overrides-parent
  semantics for rules + facts, dict-merge for vars, and
  concatenation for ignore globs.
- **`extends:` for HTTPS URLs with SHA-256 SRI.** Remote entries
  take the form `https://.../foo.yml#sha256-<64 hex chars>`. The
  SRI is non-negotiable: URLs without it are rejected. Responses
  are verified against the declared hash before use and cached
  atomically on disk at `<user-cache-dir>/alint/rulesets/<sri>.yml`.
  `ureq` is the underlying HTTPS client (rustls for TLS — no
  OS-native crypto linking).
- **`alint_dsl::load_with(path, &LoadOptions)`** for embedders and
  tests that need to pin the cache path or override the fetcher.
- **Action self-test workflow** (`.github/workflows/action-selftest.yml`)
  that dogfoods `asamarts/alint@<tag>` across `ubuntu-latest` on
  four configurations (default, `format: sarif` + JSON-parse
  assertion, `format: json`, explicit `version:` input). Catches
  regressions in the release-tarball → install.sh → binary
  distribution chain that in-process tests don't exercise.

### Security

- HTTPS `extends:` requires SRI on every entry; no
  trust-on-first-use. `http://` schemes are rejected outright.
  Cache entries are re-verified against SRI on read, so a
  tampered on-disk cache fails loudly rather than serving bad
  content. Body size is capped at 16 MiB to bound memory against
  a hostile server.

### Known limitations

- Remote configs cannot themselves contain `extends:` (nested
  remote extends deferred — a relative path inside a fetched
  config has no principled base for resolution).
- The config-wide `respect_gitignore` field cannot distinguish
  "unset" from the `true` default during merge; the child's
  value wins unconditionally.

### Added dependencies

- `ureq` (rustls TLS), `sha2`, `directories`. Release-binary size
  impact: ~+1.5–2 MiB, mostly from rustls' embedded root certs.

## [0.2.0] - 2026-04-19

Second release. The theme is composition and remediation: cross-file rules,
conditional gating, auto-fix, and two new output formats. Every v0.1 config
continues to validate and run under v0.2 without changes.

### Added

#### Rule kinds

- **Cross-file primitives (7 kinds)** — `pair`, `for_each_dir`, `for_each_file`,
  `dir_contains`, `dir_only_contains`, `unique_by`, `every_matching_has`. These
  support path-template substitution (`{dir}`, `{stem}`, `{ext}`, `{basename}`,
  `{path}`, `{parent_name}`) and nested rule instantiation for per-iteration
  semantics.

#### Facts + conditional rules

- **Facts system** — repository properties evaluated once per run. Three kinds
  shipped: `any_file_exists`, `all_files_exist`, `count_files`. Referenced
  from `when:` clauses and rule messages.
- **`when:` expression language** — bounded recursive-descent parser with
  boolean logic (`and`, `or`, `not`), comparison operators (`==`, `!=`, `<`,
  `<=`, `>`, `>=`), `in` (list or substring), `matches` (regex), literal
  types (bool/int/string/list/null), and `facts.*` / `vars.*` identifiers.
  Parsed at rule-build time; gates both top-level rules and nested rules
  inside `for_each_*`.

#### `fix` subcommand

- **`alint fix [path]`** — applies mechanical corrections for violations whose
  rule declares a fix strategy. Five ops:
  - `file_create` (paired with `file_exists`) — writes declared content.
  - `file_remove` (paired with `file_absent`) — deletes the violating file.
  - `file_prepend` (paired with `file_header`) — injects content at the top,
    preserving UTF-8 BOM.
  - `file_append` (paired with `file_content_matches`) — appends content.
  - `file_rename` (paired with `filename_case`) — converts the stem to the
    rule's target case, preserving extension and parent directory.
- **`--dry-run`** previews the outcome without touching disk.
- **Safety** — `file_create` refuses to overwrite existing files;
  `file_rename` refuses to overwrite a collision; all ops skip cleanly with
  a diagnostic when preconditions aren't met.

#### Output formats

- **`sarif`** — SARIF 2.1.0 JSON, targeting GitHub Code Scanning's upload
  action. Each violation becomes a `result` with a `physicalLocation`
  anchored on the violating path.
- **`github`** — GitHub Actions workflow-command annotations
  (`::error title=...::`). Renders inline on PR file-changed view.

#### Distribution

- **Official GitHub Action** at `asamarts/alint@v0.2.0` — composite action
  wrapping `install.sh`. Inputs: `version`, `path`, `config` (multi-line),
  `format` (default `github`), `fail-on-warning`, `args`, `working-directory`.
  Output: `sarif-file` for feeding the Code Scanning uploader.

#### Testing infrastructure

- **`alint-testkit` + `alint-e2e`** (internal crates) — scenario-driven
  end-to-end tests. 51 YAML scenarios auto-generated into `#[test]`s via
  `dir-test`, 20 CLI snapshot tests via `trycmd`, and 4 property-based
  invariants via `proptest`. See the [ARCHITECTURE / testing
  sections](docs/design/ARCHITECTURE.md) for the rationale.

### Changed

- **Internal workspace crates published** — `alint-dsl`, `alint-rules`, and
  `alint-output` are now published on crates.io. They carry
  `description: "Internal: ... Not a stable public API."`; only `alint` and
  `alint-core` are semver-stable. This change is required so that
  `cargo install alint` resolves its transitive path-or-crates-io
  dependencies.
- **JSON Schema** (`schemas/v1/config.json`) gains the `fix:` block, every
  new rule kind, and the `facts:` + `when:` fields. Root + in-crate copies
  are kept byte-identical by a drift-guard test.

### Fixed

- **`install.sh`** no longer aborts on SIGPIPE when resolving the latest
  release tag. Previously, `curl | awk '{...; exit}'` under
  `set -o pipefail` caused curl to error out with exit 23 after awk's early
  exit; the fetch is now decoupled from the parse.

### Compatibility

- Config schema version remains `1`. Existing v0.1 configs run unchanged.
- The public API of `alint-core` has not been broken. The `Rule` trait
  gained a `fixer()` method with a `None` default, so out-of-tree rule
  implementations compile without modification.

## [0.1.0] - 2026-04-19

Initial release. MVP.

### Added

- 11 rule primitives — `file_exists`, `file_absent`, `dir_exists`,
  `dir_absent`, `file_content_matches`, `file_content_forbidden`,
  `file_header`, `filename_case`, `filename_regex`, `file_max_size`,
  `file_is_text`.
- Walker that honors `.gitignore`; globset-based scopes with Git-style
  `**` semantics (separator-aware).
- CLI subcommands: `check`, `list`, `explain`.
- Output formats: `human`, `json`.
- JSON Schema at `schemas/v1/config.json` for editor autocomplete.
- Benchmarks (criterion micros + hyperfine macros).
- Static binaries on GitHub Releases for Linux
  (x86_64/aarch64 musl), macOS (x86_64/aarch64), and Windows (x86_64).
- Install script (`install.sh`) with platform detection + SHA-256
  verification.
- Dogfood `.alint.yml` exercising the tool against its own repo.

[Unreleased]: https://github.com/asamarts/alint/compare/v0.15.2...HEAD
[0.15.2]: https://github.com/asamarts/alint/compare/v0.15.1...v0.15.2
[0.15.1]: https://github.com/asamarts/alint/compare/v0.15.0...v0.15.1
[0.15.0]: https://github.com/asamarts/alint/compare/v0.14.2...v0.15.0
[0.14.2]: https://github.com/asamarts/alint/compare/v0.14.1...v0.14.2
[0.14.1]: https://github.com/asamarts/alint/compare/v0.14.0...v0.14.1
[0.14.0]: https://github.com/asamarts/alint/compare/v0.13.0...v0.14.0
[0.13.0]: https://github.com/asamarts/alint/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/asamarts/alint/compare/v0.11.1...v0.12.0
[0.11.1]: https://github.com/asamarts/alint/compare/v0.11.0...v0.11.1
[0.11.0]: https://github.com/asamarts/alint/compare/v0.10.2...v0.11.0
[0.10.2]: https://github.com/asamarts/alint/compare/v0.10.1...v0.10.2
[0.10.1]: https://github.com/asamarts/alint/compare/v0.10.0...v0.10.1
[0.10.0]: https://github.com/asamarts/alint/compare/v0.9.23...v0.10.0
[0.9.23]: https://github.com/asamarts/alint/compare/v0.9.22...v0.9.23
[0.9.22]: https://github.com/asamarts/alint/compare/v0.9.21...v0.9.22
[0.9.21]: https://github.com/asamarts/alint/compare/v0.9.20...v0.9.21
[0.9.20]: https://github.com/asamarts/alint/compare/v0.9.19...v0.9.20
[0.9.19]: https://github.com/asamarts/alint/compare/v0.9.18...v0.9.19
[0.9.18]: https://github.com/asamarts/alint/compare/v0.9.17...v0.9.18
[0.9.17]: https://github.com/asamarts/alint/compare/v0.9.16...v0.9.17
[0.9.16]: https://github.com/asamarts/alint/compare/v0.9.14...v0.9.16
[0.9.14]: https://github.com/asamarts/alint/compare/v0.9.13...v0.9.14
[0.9.13]: https://github.com/asamarts/alint/compare/v0.9.12...v0.9.13
[0.9.12]: https://github.com/asamarts/alint/compare/v0.9.11...v0.9.12
[0.9.11]: https://github.com/asamarts/alint/compare/v0.9.10...v0.9.11
[0.9.10]: https://github.com/asamarts/alint/compare/v0.9.9...v0.9.10
[0.9.9]: https://github.com/asamarts/alint/compare/v0.9.8...v0.9.9
[0.9.8]: https://github.com/asamarts/alint/compare/v0.9.7...v0.9.8
[0.9.7]: https://github.com/asamarts/alint/compare/v0.9.6...v0.9.7
[0.9.6]: https://github.com/asamarts/alint/compare/v0.9.5...v0.9.6
[0.9.5]: https://github.com/asamarts/alint/compare/v0.9.4...v0.9.5
[0.9.4]: https://github.com/asamarts/alint/compare/v0.9.3...v0.9.4
[0.9.3]: https://github.com/asamarts/alint/compare/v0.9.2...v0.9.3
[0.9.2]: https://github.com/asamarts/alint/compare/v0.9.1...v0.9.2
[0.9.1]: https://github.com/asamarts/alint/compare/v0.8.2...v0.9.1
[0.8.2]: https://github.com/asamarts/alint/compare/v0.8.1...v0.8.2
[0.8.1]: https://github.com/asamarts/alint/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/asamarts/alint/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/asamarts/alint/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/asamarts/alint/compare/v0.5.12...v0.6.0
[0.5.12]: https://github.com/asamarts/alint/compare/v0.5.11...v0.5.12
[0.5.11]: https://github.com/asamarts/alint/compare/v0.5.10...v0.5.11
[0.5.10]: https://github.com/asamarts/alint/compare/v0.5.9...v0.5.10
[0.5.9]: https://github.com/asamarts/alint/compare/v0.5.8...v0.5.9
[0.5.8]: https://github.com/asamarts/alint/compare/v0.4.10...v0.5.8
[0.5.7]: https://github.com/asamarts/alint/compare/v0.5.6...v0.5.7
[0.5.6]: https://github.com/asamarts/alint/compare/v0.5.5...v0.5.6
[0.5.5]: https://github.com/asamarts/alint/compare/v0.5.4...v0.5.5
[0.5.4]: https://github.com/asamarts/alint/compare/v0.5.3...v0.5.4
[0.5.3]: https://github.com/asamarts/alint/compare/v0.5.2...v0.5.3
[0.5.2]: https://github.com/asamarts/alint/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/asamarts/alint/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/asamarts/alint/compare/v0.4.10...v0.5.0
[0.4.10]: https://github.com/asamarts/alint/compare/v0.4.9...v0.4.10
[0.4.9]: https://github.com/asamarts/alint/compare/v0.4.8...v0.4.9
[0.4.8]: https://github.com/asamarts/alint/compare/v0.4.7...v0.4.8
[0.4.7]: https://github.com/asamarts/alint/compare/v0.4.6...v0.4.7
[0.4.6]: https://github.com/asamarts/alint/compare/v0.4.5...v0.4.6
[0.4.5]: https://github.com/asamarts/alint/compare/v0.4.4...v0.4.5
[0.4.4]: https://github.com/asamarts/alint/compare/v0.4.3...v0.4.4
[0.4.3]: https://github.com/asamarts/alint/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/asamarts/alint/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/asamarts/alint/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/asamarts/alint/compare/v0.3.2...v0.4.0
[0.3.2]: https://github.com/asamarts/alint/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/asamarts/alint/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/asamarts/alint/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/asamarts/alint/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/asamarts/alint/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/asamarts/alint/releases/tag/v0.1.0
