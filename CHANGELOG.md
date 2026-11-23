# Changelog

All notable changes to alint are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Upgrading from 0.23

- Windows users with baselines containing `for_each_file`, `for_each_dir`, `every_matching_has`, or
  `unique_by` rules that reference `{path}` or `{dir}` must regenerate their baseline once with
  `alint baseline --accept-new`: `{path}` and `{dir}` now always render with `/` separators on
  every platform so messages, rendered globs, rendered `unique_by` keys, and baseline fingerprints
  agree across Linux, macOS, and Windows (#579).
- `alint://bundled/java@v1`'s `java-no-tracked-build` rule is now gated on both `facts.has_java`
  and Gradle presence (`facts.has_gradle`: `build.gradle`, `build.gradle.kts`, `settings.gradle`,
  `settings.gradle.kts`, plus their `**/` monorepo companions) and excludes `**/src/**/build`, so
  pure-Maven repositories and Java/Groovy source packages named `build` are no longer reported
  (#546).
- Library users (`alint-core`): `baseline::BaselineEntry` gains `pub paths: Vec<String>` and
  `baseline::FingerprintedViolation` gains `pub path_free_identity: bool` (ADR-0049; prefer
  `FingerprintedViolation::from_violation` when constructing violations, #601).
- `key_parity` and `constraint` now cap emitted findings at `10,000` per rule by default
  (`limits.findings_per_rule`), emitting one non-baselinable analysis failure naming the cap when
  reached instead of allocating unbounded findings. Raise `limits.findings_per_rule` in your
  top-level `.alint.yml` (up to `1,000,000`) if a repository legitimately exceeds the default
  (#596).

### Added

- Top-level `limits:` configuration section (`limits.findings_per_rule`, default `10,000`, range
  `1..=1,000,000`; `limits.key_path_bytes_per_decode`, default 32 MiB, range `1..=16 GiB`)
  supported in top-level configs and `.alint.d/` drop-ins and refused in `extends:` and nested
  configs (#596, #569).
- Baseline entries for non-hidden repeated fingerprints (such as `constraint` rules) now record an
  optional sorted `paths` field (keeping `schema_version: 1`, ADR-0049): `baseline::apply`
  suppresses matching visible paths first before filling any remaining count in deterministic
  order, keeping unchanged baselined findings and their SARIF occurrence fingerprints stable when
  a new repeat sorts earlier; older baselines keep count-order matching and hidden-path entries
  store no `paths` (#601).
- `alint check --baseline` prints a one-line regeneration hint on stderr when a loaded baseline's
  `alint_version` header is below `0.22` and at least one hidden-path entry matches nothing (#547).
- JetBrains IDE plugin (`editors/jetbrains`): forwards IntelliJ Trusted Projects state as
  `initializationOptions.trusted` (defaulting to untrusted), automatically restarts `alint lsp`
  and reconnects open editor tabs when project trust is granted or revoked without starting stopped
  servers or re-enabling disabled ones, and covers both trust states in headless integration tests
  (#545).

### Changed

- Remote `extends:` cache directory resolution (`alint-dsl`) now uses an internal platform helper
  (`XDG_CACHE_HOME` / `HOME` on Unix, `Library/Caches/org.asamarts.alint` on macOS,
  `LOCALAPPDATA` / `USERPROFILE` on Windows), removing `directories`, `dirs-sys`, `option-ext`,
  and the workspace's sole MPL-2.0 copyleft license exception from `deny.toml` and `about.toml`
  (#611).
- Binary/text classification (`alint-rules`) now uses an internal SIMD NUL-byte, BOM, magic-header,
  and 64-bit SWAR control-byte density heuristic, removing unmaintained `content_inspector 0.2.4`
  from default release builds (it remains transitive dev-only via `trycmd`/`snapbox`, #612).
- VS Code extension (`editors/vscode`): replaced `node-tar` (`tar`) with Node's built-in
  `node:zlib` (`gunzipSync` with a 256 MiB output cap) and an in-memory USTAR/PAX/GNU-LongName
  single-file extractor (`src/tar.ts`) that rejects symlinks, hardlinks, traversal/absolute paths,
  corrupt checksums, and duplicate entries, leaving `vscode-languageclient` as the extension's sole
  runtime dependency (#613).
- Adopted `adotenv 1.6.0` across `alint-core`: delegated dotenv byte/physical-line limits to
  `ParseOptions::with_max_lines`, multi-BOM stripping to `BomPolicy::StripAll`, bounded verified
  edits to `Document::edit_with_options` / `EditOptions`, read-only bare-preserving queries to
  `parse_entries_compact_with_options`, and batch removal lookups to `LookupIndex::unique`,
  dropping local budget pre-scans and BOM offset translation (#615).
- Replaced `java-properties 2.0.0` and `encoding_rs` with an internal dependency-free, span-native
  Java `.properties` parser (`alint-core`), supporting UTF-16 surrogate pairs (`\uXXXX\uYYYY`) and
  retaining entries ending with a trailing backslash at EOF, unifying entry-row extraction and
  unlocking safe structured fixes for multiline continuations, escaped keys/values, and
  whitespace-separated `.properties` lines (#610).
- `roxmltree` upgraded from `0.20` to `0.21.2` (CVE-2026-92987); XML depth scanning (`xml_scan.rs`)
  now also rejects duplicate default `xmlns` attributes on the same element to match `roxmltree`
  0.21.2, and XML attribute value spans are computed directly from attribute byte ranges (#609).
- Path tokens `{path}` and `{dir}` always render with `/` separators on every platform (matching
  Linux and macOS), and the cross-platform baseline fingerprint golden gate now runs on Windows and
  macOS in CI (#579).
- `alint://bundled/java@v1`'s `java-no-tracked-build` now gates on `facts.has_gradle` and excludes
  `**/src/**/build` (#546).
- Unknown `--format` values now list the valid output formats for the active subcommand (and
  validate `--format` up front before loading config, walking, or running tests/snapshot updates,
  with case-insensitive `--format JSON validate-config` fixed), and `key_parity` with
  `strip_root: true` names null and array roots explicitly (`but it is null` / `but it is an
  array`), rejects top-level non-object files (`null`, arrays, scalars), and treats empty-object
  roots (`fr: {}`, `{"fr": {}}`, TOML `[fr]` / `fr = {}`) as empty locales (#597).
- Documented the finite IEEE-754 `f64` number-range limit (`1e400` fails parsing for the whole
  JSON file) across all JSON-reading rule kinds and version 2 JSON readers (#564).
- Interned `PROVENANCE_MARKER` source strings in `alint-dsl` into the load's `FieldSourceNames`
  table (resolved by `Provenance::read`), matching `FIELD_SOURCES_MARKER` so marker memory no
  longer scales with the length of extended source paths or URLs (#581).
- Per-rule finding cap (`findings_per_rule`, default `10,000`) enforced across `key_parity` and all
  `constraint` assertion families (`equal`, `unique`, `set`), and per-decode key-path memory cap
  (`key_path_bytes_per_decode`, default 32 MiB) enforced across JSON, YAML, TOML, dotenv, INI,
  properties, Android strings XML, `.resx`, and version 2 `entries:` (`leaves: true`) (#596, #569).
- Nested rule token substitution (`for_each_dir`, `for_each_file`, `every_matching_has`) now
  escapes `{path}`, `{dir}`, `{basename}`, `{stem}`, `{ext}`, and `{parent_name}` according to each
  option's syntax: Rust regexes (including whitespace for `(?x)` patterns), RFC 9535 `JSONPath`
  queries (including RFC 9485 I-Regexp arguments to `match()` and `search()`), `when_iter:` /
  nested `when:` expressions (including leading `!` in `iter.has_file`), `key_parity.ignore_keys`,
  keyword-bearing paths (`registry_paths_resolve.base`), `scope_filter` manifest extractors at all
  nesting depths, and disables legacy `${VAR}` expansion on rendered nested
  `git_commit_message.since` (#583).

### Security

- `--format junit` and `--format agent` now escape terminal controls, bidi overrides/isolates,
  zero-width characters, and astral tag characters in JUnit `<testcase>` / `<failure>` text and
  attributes (normalizing bare CR to LF) and in `--format agent` `human_message`,
  `agent_instruction`, `policy_url`, and `analysis_failures[].human_message`, leaving raw machine
  `file` paths intact (#600).
- Environment-value redaction outside the loader now uses a set-backed Aho-Corasick matcher with
  overlapping-match coalescing (preventing shorter earlier matches from hiding longer overlapping
  secrets and removing quadratic `O(N^2)` recording/scrubbing loops) and structurally masks short
  (`<4`-character) environment-derived paths across `extends:`, `baseline:`, rule filters,
  `alint fix` refusals, and `alint lsp` errors (#585).

## [0.23.0] - 2026-11-22

v0.23.0 launches Phase 3 of the general-linter program, hardens alint's trust boundaries end to
end, switches YAML parsing to pure-Rust `ayaml` 0.2.2, and ships the last planned library API
break before 1.0. Opt-in experimental `version: 2` configurations add named `selectors:`,
`relations:` (`entries:`, `read:`, `union:`, `join:`, `transform:`, `where:`), aliased local
`extends:`, and the `constraint` rule kind (`equal`, `unique`, `set`), accompanied by a draft
`schemas/v2/config.json` and 14 conformance policy tests covering locale parity, dependency
version consistency and toolchain pins. Security hardening (advisory GHSA-78jj-xfpm-g6px) adds
global `--untrusted` and `--no-spawn` modes, makes `alint lsp` wait for workspace trust, stops
config discovery at the repository top, blocks Git-directory reads/writes and extended-config
writes, bounds ignore/config/baseline/overlay/YAML parser work, and hardens the `alint test`
sandbox. `ayaml` 0.2.2 replaces `yaml_serde`/libyaml at 0.52x to 0.97x peak heap across all nine
measured shapes and cuts peak RSS by 22% to 25% on multi-megabyte OpenAPI specs. Library users:
YAML values and errors move to `alint_core::yaml`, key config/option structs and `alint_core::Error`
are now `#[non_exhaustive]` with constructors and `with_*` setters, and `baseline::Fingerprints` is
removed (re-run `alint baseline --accept-new` if a baseline was last written by v0.21 or earlier).

### Upgrading from 0.22

These changes can stop a setup that works with 0.22 from working as it did. Each is described in
full below.

- Config discovery stops at the repository's top: a repository without a `.alint.yml` of its own
  reports `no .alint.yml found` where a config above it governed it. Move the config into the
  repository or pass `--config` (Changed, Security).
- A rule that reads a file in a Git directory by name fails to load, and no fix writes there, so a
  `file_create` of `.git/hooks/...` is reported skipped. Keep hooks in a tracked directory under
  `core.hooksPath` (Changed, Security).
- `allow_out_of_root` covers your own rules only, not a ruleset's from `extends:` (Security).
- The language server runs no command rule, `custom:` fact or Git rule, fetches no remote
  `extends:`, refuses `allow_out_of_root:`, and reads no environment value until the editor says
  the workspace is trusted; editors other than VS Code need the `trusted` initialization option
  (Changed, Security).
- The language server lints each workspace folder with the folder's own config, as `alint check`
  run there; a package's nested `.alint.yml` joins only through `nested_configs: true`. `alint lsp
  --config` and `--no-gitignore` now take effect, and the `--config` file must exist when the
  server starts (Security, Fixed).
- YAML is read by ayaml as YAML 1.2. An enum value written as a one-entry mapping where its name
  goes (a fix op's `applicability: {safe: ~}`, `cross_file`'s `normalize: [{trim: ~}]`,
  `file_graph`'s `require:`) is an error, and a configuration, `extends:` source or nested
  configuration over 32 MiB is refused (Changed, Security).
- `git_tracked_only: true` sees tracked files an ignore file excludes, so a `file_absent` rule can
  fire on a force-added file (Changed).
- The human and Markdown fix reports count a fix that ran without resolving its violation as
  `unresolved` (Changed).
- `alint test` asks for the spawn capability for a spawning rule nested in another rule, and a
  test's `when:` and `${VAR}` read only the test's `given.env`, not the host's environment or the
  runner's sandbox paths (Security).
- An ignore file over 32 MiB, or one that is not a regular file, stops the walk; a baseline the
  `baseline:` key names must be a regular file inside the repository; and an absolute local
  `extends:` needs `allow_out_of_root: true` (Security).
- An `extends:` list's entries and the `.alint.d/` drop-ins may load and merge at most 4,194,304
  entries in all, and the entries at most 64 MiB of sources, each counted for every entry that
  merges it, as the [configuration limits](docs/site/configuration/index.md#limits) say. A list
  merges into one config in place (#580), so an entry is charged what it loads, what it brings in,
  and once more each rule and template it merges onto an earlier one of its id, in its own source
  too; one that an earlier entry naming the same source wrote last is not merged again, though each
  consecutive repeat of the entry still counts as one entry (#567). A flat list of distinct rulesets that override none of each other's rules or
  templates, and repeat no id within one, stays within the first bound while they hold at most
  2,097,152 entries in all; the second is reached at 64 entries of a 1 MiB source (Fixed).
- On Unix, a `.git` another user owns does not mark a repository's top. Run from a subdirectory
  of a checkout another user owns (as root in a container, say), alint skips that checkout's
  config with a warning; run it from the top or pass `--config` (Security).
- A baseline last written by v0.21 or earlier no longer matches findings whose path the
  environment hides; regenerate it once with `alint baseline --accept-new` (Removed).
- Under `--changed` and `--base`, a renamed file changes its old path too, so a rule whose
  `paths:` match only the old path now runs: after `git mv LICENSE LICENSE.txt`, a `file_exists`
  rule for `LICENSE` fires, and `alint fix --changed` recreates `LICENSE` through its
  `file_create` fix. `pair_changed_together` and the `when_changed:` of
  `changeset_requires_path` count a file renamed out of their globs as changed, so they can fire
  where 0.22 stayed silent (Fixed).
- A SARIF upload may open new alerts for version 1 findings that repeated a fingerprint within one
  run, such as a rule's findings in files the environment chose (ADR-0030): each repeat now carries
  a fingerprint of its own, where GitHub code scanning may have merged them into one alert
  ([ADR-0047](docs/adr/0047-sarif-tells-apart-a-fingerprint-repeated-within-a-run.md)) (Fixed).
- GitLab output may give a version 1 finding that shares its fingerprint with another in the report
  a different fingerprint than 0.22 did: it numbers such findings by path, line and column, not in
  the order it writes them, so the one that keeps the shared fingerprint may change
  ([ADR-0047](docs/adr/0047-sarif-tells-apart-a-fingerprint-repeated-within-a-run.md)) (Fixed).
- Library: `WhenEnv::env` is an `Option<&BTreeMap<String, String>>`; `LspOptions` gains `config`,
  `no_gitignore`, `no_spawn` and `untrusted`, and `LoadOptions` gains `global_git_excludes` and
  `out_of_root_policy` (`OutOfRootPolicy`), which
  breaks a struct literal; YAML values and errors are alint's own types (`alint_core::yaml`) in
  place of `yaml_serde`'s; `Violation`, `Context`, `RuleSpec`, `NestedRuleSpec`, `WalkOptions`,
  `LspOptions` and `LoadOptions` are `#[non_exhaustive]`, as is `alint_core::Error`, so build them
  with their constructors and `with_*` setters; `baseline::Fingerprints`, the six
  `alint_dsl::reject_*_in` functions and `alint_core::template::render_mapping`, `render_value`
  and `render_nested_options` are no longer public; and the crates pin ayaml exactly (`=0.2.2`)
  (Changed, Added, Removed).

### Security

GitHub security advisory GHSA-78jj-xfpm-g6px, published with this release, describes the holes
below in the policy-test sandbox and in the trust alint gives `extends:` rulesets, nested configs
and editor workspaces ([ADR-0040](docs/adr/0040-the-v023-advisory-and-the-allowlist-approval.md),
[ADR-0043](docs/adr/0043-trust-and-disclosure-decisions-from-the-post-v0221-audit.md),
[ADR-0044](docs/adr/0044-untrusted-runs-git-as-a-process-and-a-wider-memory-gate.md),
[ADR-0046](docs/adr/0046-a-linear-extends-merge-sealed-options-and-the-rounds-open-questions.md)). No v0.22.x
patch release follows: upgrade to v0.23.0. Before upgrading, run `alint test` only on policy-test
packages whose configurations and `given:` blocks you trust, `alint check` and `alint lsp` only on
repositories whose configs you trust (or with your own `-c` config), and `alint fix` only with
`extends:` rulesets you trust and, with a content fix, only on repositories you trust.

- **The language server runs nothing a workspace's config could start until the editor trusts the
  workspace.** Since v0.11.0, opening a folder in an editor with alint's language server ran that
  repository's `.alint.yml` as `alint check` does: its command rules (`kind: command`,
  `generated_file_fresh`, `command_idempotent`) and `custom:` facts started processes, a remote
  `extends:` was fetched, `allow_out_of_root:` was honored, and `{{env.*}}` and `env.*` in a
  `when:` read the editor's environment, so
  opening a cloned repository ran its commands. Until the client sends the initialization option
  `{"trusted": true}`, the server now skips every rule of a spawning kind (at any `require:` depth),
  every rule that runs Git (the `git_*` kinds, `changeset_requires_path`, `pair_changed_together`,
  `git_tracked_only`, `scope_filter.changed_since`: Git runs the commands a repository's
  `.git/config` names, such as `core.fsmonitor`) and every
  `custom:` fact, lints a rule whose fix alone spawns (such as `file_absent` with `fix:
  git_untrack`) without offering that fix, refuses a remote `extends:` and `allow_out_of_root:`
  with a diagnostic on the
  config, and reads no environment value. Every other rule lints as usual, and the server says what
  it turned off once, in an information message. The VS Code extension sends VS Code's Workspace
  Trust state and restarts the server when trust is granted, so a trusted VS Code workspace behaves
  as before; other editors opt in through their server settings
  ([editors](docs/site/integrations/editors.md#workspace-trust)). The JetBrains plugin does not send
  the option yet, so command rules do not run there. `alint lsp --untrusted` takes every workspace
  as untrusted, whatever the editor sends and even when `--config` is passed.
- **A nested or vendored `.alint.yml` no longer governs files in the language server.** Since
  v0.18.0, the server linted each open file with the nearest `.alint.yml` above it as a
  top-level config. A package under `node_modules/`, which the workspace's `.gitignore` excludes
  and `alint check` never reads, could ship one whose command rule ran when one of its files was
  opened, although a nested config may not declare command rules. Each workspace folder is now
  linted by its own config, rooted at that config's directory, as `alint check` there. A folder
  without a config of its own (a directory holding several repositories) lints each file with
  the config at the top of the nearest repository above it inside the folder (the nearest
  directory holding a `.git` directory or file), rooted there, as `alint check` in that
  repository does; a directory without `.git`, such as a package under `node_modules/`, is never
  such a top. A deeper config joins only under the governing config's `nested_configs: true`,
  and then as a nested config.
- **Reading `nested_configs` no longer resolves a config's `extends:`.** Since v0.18.0,
  `alint_dsl::nested_configs_enabled` (and `nested_configs_enabled_with` since v0.19.0, and the
  language server, for every config between an open file and its workspace folder, nested ones
  included) loaded the config in full to read that one key: a remote `extends:` was fetched and
  `{{env.*}}` values were interpolated, so a vendored config could send an environment variable
  to a host it named. Both now read the key from the file and its `.alint.d/` drop-ins only (an
  extended config's `nested_configs` never applied anyway), so a config whose `extends:` would
  fail to load still answers.
- **`alint lsp` honours `--config` and `--no-gitignore`.** Since v0.11.0 the server accepted and
  silently ignored both, so `alint lsp -c my-policy.yml` linted with the repository's own config
  instead. `--config` now lints every workspace folder with that file, rooted at the folder,
  and counts as trust as `alint check -c` does; the file must exist when the server starts.
  `--no-gitignore` lints the files `.gitignore` excludes.
- **Hover text no longer renders a finding as Markdown.** Since v0.11.0, the hover showed a
  finding's message, rule id and policy URL as Markdown, so text from the repository (a file
  name or matched line in the message) could render an image the editor fetched
  (`![x](http://...)`), a link, or HTML. The message is now escaped, the rule id is a code span,
  and a policy URL is a link only when it is `http` or `https` (any other shows as text).
- **The language server's notes hide a path alint hides.** Since v0.20.0, a note of a rule whose
  files a value from the environment picks showed `<redacted>` for its path in `alint check
  --show-notes`, but the language server's notes, which it writes to its log with `--show-notes`,
  showed the file's path. They now hide it as `alint check` does, `<redacted>`, and name a file
  once, escaped as alint's messages escape text (`\u{1b}`, where `--show-notes` writes the
  terminal's `\x1b`), with no control, bidi or zero-width character left raw.
- **Configuration discovery stops at the repository's top.** alint looked for `.alint.yml` from the
  current directory up to the file-system root, so in a repository without a config of its own, or
  from a directory below one, a `.alint.yml` that someone else wrote in a directory above it
  (`/tmp/.alint.yml` over a checkout in `/tmp`, a shared home) governed the run, command rules
  included. Discovery now checks the repository's top (the nearest directory holding a `.git`
  directory or file) and stops there. A `.git` marks the top only when the current user owns it and
  the directory that holds it (on Unix), so an empty `.git` that another user put above a directory
  without a `.git` of its own (an unpacked archive in `/tmp`) does not end the climb before the
  owner check below. Outside every repository it
  climbs as before but skips a config file another user owns in a directory above the one it starts
  from, with `alint: warning: skipping /tmp/.alint.yml: owned by another user; to use it if you
  trust it, run alint from /tmp or pass --config /tmp/.alint.yml` on stderr (on Unix, a
  file whose owner is not the effective user id; Windows does not check). A config in the starting
  directory itself is used whoever owns it, so `alint check` as root in a container, on a mounted
  directory the host's user owns, still finds that directory's config. This affects `alint check`,
  `fix`, `list`, `explain`, `validate-config`, `export-agents-md` and every other command that
  discovers a config, and the language server, in v0.1.0 to v0.22.1.
- **A repository's configuration can no longer make loading it crash, hang or exhaust memory.**
  A configuration is untrusted for what it costs to load, and a small one could:
  - overflow the stack, since v0.20.0: the `requires:` check walked the whole first document
    recursively, so a value nested 100,000 levels deep, as the check's parser read it, crashed
    the process. The check now reads the events alint decodes, without recursion, and refuses
    nesting past 128 levels;
  - double `ignore:` at every level of an `extends:` diamond, since v0.2.1: 30 configurations
    that each extend the next one twice held 2^30 copies of one pattern and aborted at 5.8 GB.
    A pattern listed more than once is now kept once (an `ignore:` pattern only excludes, so no
    result changes), and the merged configuration holds at most 1,048,576 rules, templates,
    facts, vars and `ignore:` patterns;
  - expand templates without bound, since v0.5.10: every `extends_template:` instance copied its
    template and rendered `{{vars.<name>}}` at every placeholder, so a 1.2 MB configuration (a
    1 MiB `message:` and 4,000 instances) needed more than 8 GB, and a message of 100,000
    placeholders for a 20 KB var rendered 2 GB. All the instances of a configuration now expand
    to at most 64 MiB, and one rendered string to at most 32 MiB; the error names the budget,
    the rule and the template;
  - hang, since v0.2.1: a configuration, a local `extends:` target or a nested configuration
    was read whatever it was, so a FIFO blocked the load forever, and a symlink to `/dev/zero`
    grew until memory ran out. The `.alint.yml` that `alint suggest` reads (since v0.7.0) was
    read the same way. Each is now read only when it is a regular file, and only up to 32 MiB,
    and so is a config the language server reads again to place a YAML error.

  These affect every command that loads a configuration (`alint check`, `alint fix`, `alint
  test`, the language server and the others), from the version each item names to v0.22.1.
  Until v0.23.0, run alint on a repository you do not trust under a time and a memory limit.
- **`pinned_references` no longer hangs on a workflow whose merge keys refer to themselves.** Since
  v0.19.0, the rule resolved a `<<` merge key in a GitHub Actions workflow or a GitLab CI file by
  following every merge source again at each level, with no memo. A 16-byte workflow (an anchor `&r`
  on its top-level mapping, over `<<: [*r, *r]`) made `alint check` with `ci/github-actions@v1` spin
  until it was killed. The YAML alias guard did not stop it: it measures with the parser alint
  decodes with, and took that parser's refusal of the text (here of an alias to a collection that
  contains it, or of anything before an alias bomb, such as an empty `!<>` tag) for "within budget",
  while the rule reads the text with another parser, which accepts it. The guard's verdict now says
  when the text could not be read, and the rule refuses such a file with an analysis failure, as
  does `key_parity`'s YAML entry extraction. The rule also refuses an alias to a collection that
  contains it, and holds merge-key lookups to a work budget, so a file the guard passes cannot spin
  either. This affects every command that runs the rule, from v0.19.0 to v0.22.1.
- **An `extends:` URL's scheme is read in any case, any other scheme is refused, and its
  credentials stay out of errors.** `HTTPS://`, `Http://` and `ALINT://bundled/` were not
  recognized as remote, plain-HTTP and bundled entries, and `ftp://`, `file://`, `ssh://` or
  `git+https://` were not recognized at all: each was read as a path to a local file, with no
  integrity hash required, and the error that named it printed the URL whole, a
  `user:password@` and a `?token=` included. Schemes now match in any case: such an entry is
  fetched and pinned like any `https://` one, and a plain `http://` one is refused. An entry
  with any other `<scheme>://` (two or more characters, so a Windows drive stays a path) is a
  load error, ``unsupported `extends:` scheme `ftp` (entry "ftp://host/x.yml"); use https://,
  alint://bundled/ or a local path``. Errors show such an entry without its userinfo, query and
  fragment. This affects configs with a non-lower-case or other `extends:` scheme in v0.2.1 to
  v0.22.1.
- **A fix no longer writes or reads a Git directory.** A `file_create` could write `.git/HEAD`,
  `.git/config` with a `core.fsmonitor`, and `.git/objects/` and `.git/refs/` entries, a `.git`
  file naming another directory (`gitdir: gitdata`), or a `.git/commondir` pointing at one, and
  the next Git command to read the repository (a later step's, in a policy test, or the user's)
  ran that command. In `alint test` it needed no `spawn` capability; in `alint fix` a ruleset
  reached through `extends:`, which the trust gate keeps from spawning, could run one, and its
  `file_create: { content_from: .git/config }` copied the repository's configuration, credential
  sections included, into a worktree file. Every fixer path a
  configuration chooses (a `file_create` target and `content_from`, a `dir_create` target,
  `sync_from` and cross-file sources and targets, `create_and_register` members, `file_rename`
  and `relocate` targets) now refuses a component that names a Git directory in a spelling Git's
  own checks name (`.GIT`, `.git.`, `git~1`, `git~2` and other `GIT~N` short names; a hashed
  Windows short name past `~4` still passes), or that an in-repository symlink leads
  into one; no rule reads such a path either (below), including out-of-root paths resolved under
  `allow_out_of_root:`. `alint baseline` also refuses to write into a Git directory directly or
  through a symlink. This affects `alint fix` with a ruleset
  from `extends:` in v0.2.1 to v0.22.1 (`content_from` since v0.5.10), and `alint test` since
  v0.20.0.
- **No rule reads a file in a Git directory.** A rule could name one, and print what it read:
  a `cross_file` rule with `source: { file: .git/config, extract: { regex: 'extraheader =
  (.*)' } }` and `relation: equals` against any file put the `AUTHORIZATION: basic ...` header
  that `actions/checkout` persists into its mismatch message, from any configuration, a remote
  ruleset pinned by its hash and not in `trusted_extends` included. A path a rule reads by name
  that names a Git directory, in a spelling Git's own checks name, is now a load
  error: `cross_file` `source.file` and `targets[].file`, `pair_hash` `target`,
  `json_schema_passes` `schema_path`, `registry_paths_resolve` `source`, `key_parity`
  `reference` and a literal `peers`, `generated_file_fresh` `file`, a `scope_filter` manifest
  `source`, and every `content_from`
  (``rule "x": `source.file` .git/config is in a Git directory; rules never read there``). A path
  that reaches one through a symlink is refused when it is read, as a `resource_read` analysis
  failure in the same terms, and the walk no longer follows such a link (below). Checks that a
  path exists (`file_exists`, the `*_resolve` kinds) are unchanged, and a gitignored file such
  as `.env` is still read by name. This affects `alint check` and `alint fix` in v0.10.0
  (`cross_file_value_equals`, which v0.12.0 folded into `cross_file`) to v0.22.1.
- **A content fix no longer writes through a symlink into the Git directory.** A repository
  could commit `src/evil.rs -> ../.git/config`, which the walk followed, so the user's own
  `file_header` rule on `src/**/*.rs` prepended its header to `.git/config`. With
  `content_from: HEADER.txt`, a file the repository supplies, that header could set
  `core.fsmonitor`, and the next Git command to read the repository ran it: the one a
  `git_tracked_only` rule runs in the same `alint fix`, or the user's. A directory link
  `gitdata -> .git` indexed the repository's own files the same way. The walk now leaves out a
  symlink whose target is in a Git directory, in the same sense as above (`no_symlinks` still
  reports it), and every fix that writes, removes, renames or changes the mode of a file refuses
  one in a Git directory, through symlinks too, and reports it skipped: `src/evil.rs is in a Git
  directory; a fix never writes or reads there`. A symlink to an ordinary file is still fixed
  through. This affects `alint fix` on a repository you do not trust in v0.2.0 to v0.22.1
  (writing the configuration's own text into `.git/config`), and running a command since
  v0.5.10 (`content_from`).
- **A fix from an extended ruleset no longer writes alint's configuration.** A ruleset reached
  through `extends:` may not declare a `kind: command` rule, but its fixes could write one: a
  `file_create` of `.alint.d/housekeeping.yml`, or a `file_append` to `.alint.yml`, put a `command`
  rule into the user's own configuration, and the next `alint check` ran it. A fix of a rule an
  extended ruleset or a nested config shaped (it declared the rule, contributed a field to it, or
  defined its template) now never creates, edits, removes, renames, moves onto or changes the mode
  of a configuration file: `.alint.yml`, `.alint.yaml`, `alint.yml` or `alint.yaml` at any depth, in
  any case and with the trailing dots, spaces or stream suffix Windows drops, anything under
  `.alint.d/` or `alint-tests/` (where `alint test` finds policy tests, which may run commands), a
  Windows 8.3 short name of any of these (`ALINT~1.YML`), or a file the configuration was loaded
  from (a `--config` file and local `extends:` targets included, along with their alternate Windows
  and HFS+ spellings such as trailing dots, NTFS stream suffixes, short names and ignorable code
  points). It reports `.alint.yml is an alint
  configuration file; a fix from an extended ruleset never writes one`. The user's own rules still
  write configuration, and `trusted_extends` does not lift this (it never let a ruleset spawn). This
  affects `alint fix` with a local ruleset from `extends:` in v0.5.8 to v0.22.1 (a drop-in since
  v0.5.10), with any `https://` ruleset before v0.17.0, and since then with one listed in
  `trusted_extends`.
- **`allow_out_of_root` no longer lets an extended ruleset's fix write outside the repository.**
  The top-level `allow_out_of_root: true` lifted the root confinement of a fix's write targets
  too, for every rule, so a `file_create` in a ruleset reached through `extends:` could write
  `../outside/planted.sh`. The human report then called it skipped, because no re-check of the
  root sees the file. A fix of a rule an extended ruleset or a nested config shaped now never
  writes outside the root (`dir_create`, `sync_from`, cross-file and `create_and_register`
  targets included), and reports `../outside/planted.sh escapes the repo root; a fix from an
  extended ruleset never writes outside it, whatever `allow_out_of_root` says`. The user's own
  rules keep the option. This affects `alint fix` with `allow_out_of_root` and an `extends:`
  ruleset in v0.14.0 to v0.22.1 (before v0.14.0 no fix write was confined to the root).
- **`allow_out_of_root` no longer lets an extended ruleset's fix read outside the repository.**
  The same option lifted the confinement of a fix's sources, so a `file_create` in a ruleset
  reached through `extends:` with `content_from: ../secret` copied a file from outside the
  repository into it, and a `sync_from` or cross-file `source` read one. A fix of a rule an
  extended ruleset or a nested config shaped now never reads a source outside the root (the
  `content_from` of `file_create`, `file_prepend`, `file_append`, `insert_header` and
  `create_and_register`, the `sync_from` source, and a cross-file fix's source file), and
  reports `../secret escapes the repo root; a fix from an extended ruleset never reads outside
  it, whatever `allow_out_of_root` says`; the `alint fix --dry-run` preview reads none either.
  The user's own rules keep the option. This affects `alint fix` with `allow_out_of_root` and an
  `extends:` ruleset in v0.14.0 to v0.22.1.
- **`allow_out_of_root` no longer lets an extended ruleset's rule read outside the repository
  when it is checked.** The top-level grant covered every rule it named, so under `true`, a
  `kinds:` list or a `rules:` list naming its id, a `json_schema_passes` (`schema_path`),
  `pair_hash` (`target`) or `registry_paths_resolve` (`source`) rule from a ruleset reached
  through `extends:` read a file outside the repository, and its findings told it apart
  (`registry_paths_resolve` printed the entries it listed). The grant now covers the user's own
  top-level rules only: a rule an extended ruleset or a nested config shaped (it declared the
  rule, contributed a field to it, or defined its template, so a rule the user's config
  overrides by id counts) reads only inside the root and reports `registry source
  ../outside.json escapes the repo root`, in `alint check`, `alint fix` and the language server.
  This affects configs with `allow_out_of_root` and an `extends:` ruleset in v0.12.0 to v0.22.1.
- **A structured YAML fix no longer overflows the stack.** Since v0.17.0, `set_value`,
  `remove_value` and `replace` on a YAML file found the value's bytes with saphyr's loader, which
  recurses once per level of nesting. saphyr does not read every document as the check side does:
  a file that starts `&ak:---` (saphyr's anchor names run through a `:`), or that has a
  byte-order mark at the start of a later line, can be one plain scalar to the check side and a
  sequence 100,000 levels deep to saphyr. `alint fix` aborted on such a 60 KB file, as did
  `check --format sarif`, and the language server's code action at 3,000 levels. The resolver
  now walks saphyr's parser events without recursing, declines a document nested past 128
  levels (block or flow, the check side's limit) and stops at a second document.
- **A structured YAML fix no longer exhausts memory on an alias bomb.** Since v0.17.0, saphyr's
  loader copied an anchored node for every alias. Its alias budget does not count what a mapping
  key holds, so a 600-byte file of twelve nested `{? *a}` levels ran `fix` for 17 s and aborted
  at 3.35 GB, where `check` took 0.02 s. The resolver keeps an alias as one node and never copies
  it.
- **An extended or nested config can no longer promote a fix by writing its tier as a mapping.**
  Since v0.17.0, a ruleset reached through `extends:` or a nested config could write `fix: {
  file_remove: { applicability: {safe: ~} } }`: a fix op's options were read through a buffered
  enum that took a one-entry mapping as a unit variant, and the trust check
  compared the value as a string only, so a bare `alint fix` applied the fix (deleting files, for
  `file_remove`) without asking. A fix op's options now decode by the op's name, a tier is taken
  only as a string (any other spelling is an error that lists the tiers), and the trust gates
  refuse, or strip from a rule an extended source shaped, an `applicability` that is not a string,
  and a `{{` placeholder anywhere inside one. `file_graph`'s `require:` and `cross_file`'s
  `normalize:` take their values only as strings too
  ([ADR-0044](docs/adr/0044-untrusted-runs-git-as-a-process-and-a-wider-memory-gate.md)). This
  affects `alint fix` from v0.17.0 to v0.22.1.
- **The walk no longer reads an ignore file without bound.** A committed `.gitignore` or `.ignore`
  that is a symlink to `/dev/zero` or a FIFO, or an oversized `info/exclude`, worktree `.git` file
  or `commondir`, made `alint check`, `alint fix` and the language server exhaust memory or hang.
  Every ignore file the walk reads (in the tree, above it, and the global excludes file) is now read
  only when it is a regular file of at most 32 MiB; otherwise the walk stops with an error that
  names it, except that a global excludes file that is not a regular file is skipped unread. A
  regular file that gives its size as zero, as a `/proc` file does, is counted by reading it, so a
  symlink to `/proc/self/pagemap` (gigabytes) is refused too. The check costs three `stat` calls
  for each directory the walk enters and each directory above the root, and one read of each empty
  ignore file. This affects every released version.
- **The `git_branch` fact reads `.git/HEAD` only when it is a regular file of at most 64 KiB.** A
  `HEAD` that was a symlink to `/dev/zero` made the run exhaust memory, and a FIFO hung it, also
  under `--no-spawn`. A larger or irregular `HEAD` names no branch (`""`), as an unreadable one
  did. This affects every released version.
- **A baseline is read only when it is a regular file of at most 256 MiB, and one the `baseline:`
  key names only inside the repository and outside `.git`.** Since v0.14.0, `check`, `fix` and the
  language server read the baseline whole, wherever it was, so a repository's `.alint.yml` could
  name `/dev/zero`, a FIFO or a file outside the repository. `--baseline` on the command line may
  still name a file anywhere.
- **A local `extends:` written as an absolute, rooted, drive-letter or network path is refused
  before alint touches it,** unless `allow_out_of_root: true`. From v0.14.0, which first confined a
  local `extends:`, to v0.22.1, the path was resolved before it was confined to the config's
  directory, so on Windows `extends: //host/share/x.yml`
  opened a connection to that host before the confinement refused it. An absolute path inside the
  root now needs `allow_out_of_root: true` or a relative path.

The fixes below are in the policy-test runner. Except where a bullet says otherwise, they affect
`alint test` and the `alint_testkit::Runner` library in v0.20.0 to v0.22.1, and only for a
policy-test package the user does not trust.

- **`given.env` in a policy test no longer reaches Git.** Since v0.20.0, `alint test` passed a
  test's `given.env` to the Git commands it runs itself: the `given.git` fixture setup, and
  alint's Git reads for rules such as `git_tracked_only`. A variable such as `GIT_CONFIG_*`
  (naming an fsmonitor or filter command), `GIT_TRACE` or `LD_PRELOAD` could then run a command,
  load a library or write a file without `capabilities: { spawn: true }` or `--allow-spawn`, and
  the fixture setup ran before the capability check, so even a refused test could do it. Those
  Git commands now run with a fixed environment (a sandboxed home, no system or global Git
  configuration, `PATH` and the C locale), and `setup_git` drops any other variable it is given.
  `given.env` reaches configuration interpolation, `when:` conditions and `${VAR}` expansions
  (below), and granted commands, never Git, as the
  [policy-test design](docs/design/v0.20/policy-tests.md) section 2.4 specifies. `alint check`
  and `alint fix` were not affected: they read the invoking user's own environment.
- **`alint test` now asks for the spawn capability for a spawning rule nested in another
  rule's `require:`.** Since v0.20.0, a policy test could run a `command` rule nested in
  `for_each_dir`, `for_each_file` or `every_matching_has` without `capabilities: { spawn: true }`
  or `--allow-spawn`: the preflight read a nested rule only when it also had an `id` and a
  `level`, which nested rules do not have. It now reads each `require:` entry by its `kind`, at
  any depth, as the configuration trust gate does, and names it in the error
  (`rule "per-dir" require[0] (command)`). Host-keyring kinds are checked the same way. A
  spawning `fix:` operation on a nested rule counts too, although loading already refuses a
  nested `fix:`. `alint check` and `alint fix` were not affected: a configuration's own rules may
  spawn by design, and the trust gate already refuses spawning rules, nested ones included, from
  `extends:`.
- **A policy-test fixture shaped like a bare repository no longer runs a command through Git.**
  Without `given.git`, a test's tree has no `.git`, and the Git reads alint makes for
  `git_tracked_only` and the `git_*` rules discovered a repository from the fixture's root. A
  tree with `HEAD`, `objects/`, `refs/` and `config` at its top was read as a bare repository, and
  a `core.fsmonitor` in that `config` ran a command without `capabilities: { spawn: true }` or
  `--allow-spawn`. alint now finds the repository itself (the nearest directory holding a `.git`
  directory or file, never a directory taken as a bare repository) and names it to every Git
  command with `--git-dir` and `--work-tree`, so Git discovers none, whatever its version:
  `safe.bareRepository=explicit`, which alint also passes, does nothing before Git 2.38. With no
  repository found, alint runs no Git command, and the runner runs Git only on the repository
  `given.git` made. A repository the current user does not own is named only when Git, asked to
  discover exactly it, accepts it, so Git's ownership check and `safe.directory` still apply.
  That also covers `alint check` and `alint fix` in a directory without `.git` that holds such
  files, which ran the command with a Git rule or `git_tracked_only` since v0.4.8, as `git status`
  run there would. In a repository, alint still runs what its `.git/config` names, as Git does:
  run alint in an unpacked archive's repository only as you would run `git status` there. The
  runner also refuses fixture entries that
  Windows or macOS open as `.git` (`.git.`, `.git `, `.git::$INDEX_ALLOCATION`, `GIT~1`, and
  `.git` with a code point HFS+ ignores), as Git refuses them in a checkout.
- **A policy test's `when:` conditions and `${VAR}` expansions no longer read the host's
  environment.** `env.X` in a `when:` (top-level or nested in `require:`) and the legacy `${VAR}`
  form of `git_commit_message`'s `since:` read the runner's own process environment, so a test
  could tell a CI secret's characters apart through which of its rules fired, and could not see
  its own `given.env`. Both now read `given.env` only: neither host variables nor runner sandbox
  paths (`PATH`, `HOME`, `GNUPGHOME`, `XDG_CONFIG_HOME`, `GIT_CEILING_DIRECTORIES`, Windows system
  variables) reach rule evaluation, so
  `when: 'env.PATH matches "^/home/"'` is never true. `alint check` and `alint fix` still read
  the invoking user's environment.
- **An inline `given.tree` symlink can no longer lead out of a policy test's sandbox.** Since
  v0.20.0 the runner checked where a tree's symlink leads by reading `..` in its target as text,
  but the file system follows a symlink before it steps up from it. With `a/c/t -> ../../b`, a
  link `s -> a/c/t/../../../x` passed as `x` and led two levels above the repository root, and a
  tree key under it (`s/f`) wrote there while the tree was written, before any capability check
  and during preflight too. An executable named `git` written that way into a directory on `PATH`
  ran when the runner ran Git, and `expect_tree` read host files through such a link. The runner
  now resolves a link's target through the symlinks it reaches, refuses a tree entry under a
  symlink, writes symlinks after every other entry, and checks where each written link leads.
  That covers `given.fixture` too: a fixture link was checked where it resolves in the fixture,
  but its text is copied as it is and resolves from the copy's place, so `../../../secret`, led
  back into a deep fixture through the package, read a host file above the temporary directory
  in `expect_tree` (section 2.3 of the design said the copy was checked; nothing checked it).
- **A fix step can no longer move a symlink out of a policy test's repository.** Since v0.20.0,
  a `relocate` fix (v0.17.0) could move an in-tree symlink to the root, where its relative text
  led above the repository, and `expect_tree` then read a host file through it. The runner checks
  where every link leads after each fix step and refuses the test when one leads outside.
- **A policy-test fixture copy is capped at 64 MiB.** `fixture:` copied a package's directory
  into the sandbox file by file, with no limit on the total, and a hard link was copied once per
  name, so a package of many hard links to one large file filled the disk. The runner now stops
  a copy past 64 MiB with `test "<name>": its fixture copies more than 67108864 bytes into the
  sandbox (at <path>); a test copies at most 67108864 bytes, each hard link counted as a copy`.
  Each name is still copied as a file of its own, never linked into the sandbox. The largest
  policy-test package in alint's own repository is about 150 KiB. This affects `alint test` in
  v0.20.0 to v0.22.1.
- **A policy test's Git reads no longer find a repository above the sandbox.** Since v0.20.0,
  without `given.git` the test repository has no `.git`, and Git's discovery climbed to any
  repository the temporary directory was in: with `TMPDIR` inside a checkout, `git_commit_message`
  read the host repository's HEAD message. The runner now sets `GIT_CEILING_DIRECTORIES` to the
  sandbox's parent, which alint keeps for its own Git commands (it still drops the invoking
  user's), so such a test reports that the root is not a usable Git repository.
- **With the host keyring granted, the host's global excludes no longer hide files from a policy
  test.** Since v0.20.0, a test granted `host_keyring` ran the runner's Git with the host's home,
  and `GIT_CONFIG_GLOBAL` does not keep Git from its default global excludes and attributes files
  there (`~/.config/git/ignore`), so `check_changed` missed an untracked file the host's excludes
  listed. The runner's Git commands now get `XDG_CONFIG_HOME` in the sandbox.
- **Additional pre-v0.23 hardening across trust boundaries, bounded reads, outputs and redaction:**
  - `nested_configs` confines nested globs and path templates strictly to the nested config's
    subtree, rejecting leading `/` or `\`, Windows drive prefixes, `..` traversal segments anywhere
    (including inside `{...}` brace groups or after `{path}`/`{dir}`), non-subtree-anchored tokens
    (`{**}`, `{basename}`, `{dir}sibling`), and non-string entries in `paths:`.
  - `pinned_references` memoizes GitLab CI `extends:` DAG lookups under a work budget (preventing
    exponential blowup on diamond `extends:` graphs) and strips leading UTF-8 BOMs.
  - `--untrusted` and `--no-spawn` refuse `--allow-host-keyring` on `alint test`, matching
    `--allow-spawn`.
  - `policy_url` links in LSP `CodeDescription`, Markdown reports, human OSC 8 hyperlinks and
    `export-agents-md` accept `http://` and `https://` URLs only; `export-agents-md` uses safe
    code spans, collapses line breaks and escapes `<!--` in directives, refuses symlinks and
    special files through atomic safe writes, and caps `--inline` reads; binary diff summaries
    sanitize terminal control characters; human output omits `related_locations` on hidden-path
    findings; and all output formats suppress `line`, `column` and ranges when a finding's path is
    `<redacted>`.
  - Disk reads in LSP buffer overlays, `alint test` discovery/parsing, `alint init` workspace
    detection (with root confinement), `alint suggest` scanners, `alint fix` command snapshots and
    `alint-testkit` artifacts/tree verification are bounded to regular files.
  - A version 1 config or template extended into a version 2 config cannot supply a `constraint`
    `assert:` block, preserving v2's literal-name invariant against v1 `{{env.*}}` interpolation.
  - Nested rules and version 2 selectors with secret-bearing manifest `scope_filter:` redact paths
    and markers; `constraint` `assert.set` hides end-column spans when either side is sensitive;
    and `close_sensitivity` propagates explicit `sensitive: true` across sibling fields of
    non-join relations and across transitive multi-pair joins (`on: { a: x, b: x }`).

### Added

- **`related_locations` in the JSON report**: each violation, note and baselined finding of
  `check --format json` carries the related locations its rule found, as SARIF's
  `relatedLocations` and the language server's `relatedInformation` do: an array of `path`,
  `label` and, when the range is known, `line`, `column`, `end_line` and `end_column` (1-based,
  Unicode code points, end exclusive), at most 20 in the finding's order, and
  `related_locations_omitted`, the count of the rest. Both are left out when there are none, and
  for a finding whose path is `<redacted>`, as SARIF leaves its related locations out. The report
  stays `schema_version: 1`; `schemas/v1/check-report.json` describes both fields. A test over a
  `constraint` fixture with a finding of each family checks that JSON, SARIF and the language
  server carry every related location the engine produced, in its order, with its path, range
  and label, and the count of the rest.
- **`alint_dsl::discover_config` and `alint_dsl::Discovery`** (library API, see Security):
  discovery's result with the config files it skipped because another user owns them, for a caller
  to warn about. **`alint_core::git::repository_top`**, the nearest directory at or above a path
  that holds a `.git` directory or file the current user owns, and
  **`alint_core::git::owned_by_current_user`**. **`RuleEntry::fix_may_leave_root`**: whether a
  rule's fix may read or write outside the root (its `allow_out_of_root`, never for a rule from an
  `extends:`'d ruleset), and **`AllowOutOfRoot::allows_rule`**: whether the grant covers an
  effective rule (never one an extended ruleset or a nested config shaped; `allows` still answers by
  id and kind alone). `alint-core` depends on `rustix` on Unix, already in the dependency tree, for
  the effective user id.
- **`--no-spawn`** (global flag): refuse a config whose rules, fixes or facts start a process or run
  Git, to run a repository's own config without letting it start anything. An enabled rule of a
  spawning kind (at any `require:` depth), a spawning fix such as `git_untrack` or `command`, a
  `custom:` fact, or a rule that runs Git (the `git_*` kinds, `changeset_requires_path`,
  `pair_changed_together`, `git_tracked_only`, `scope_filter.changed_since`; Git runs the commands a
  repository's `.git/config` names) fails the load before anything runs, naming it: `rule "x" (kind:
  command) spawns a process; --no-spawn refuses it`. A rule at `level: off` runs nothing and is not
  refused. `--changed` and `--base` are refused, and the run starts no Git command. It applies to
  `check`, `fix`, `baseline`, `list`, `explain`, `facts`, `validate-config`, `export-agents-md`,
  `suggest` (which then runs no Git and skips its stale-TODO suggestion) and `lsp`; `test` rejects
  it beside `--allow-spawn`, and `init` and `rules`, which load no config and run nothing, reject
  it. It does not refuse
  a remote `extends:` or environment values: for a repository you do not trust, use `--untrusted`.
- **`--untrusted`** (global flag): lint a repository you do not trust with its own config. It
  refuses what `--no-spawn` refuses, naming `--untrusted`, refuses a remote `extends:` before
  connecting and a top-level `allow_out_of_root:` before its `extends:` resolves (it would let the
  config read and write outside the repository; the language server's untrusted mode and `alint
  test` refuse it too, through `LoadOptions::with_out_of_root_policy`), and gives the config an
  empty environment, so `{{env.NAME}}`, `env.*` in a `when:` and a legacy `${VAR}` read nothing.
  `alint lsp --untrusted` takes every workspace as untrusted, whatever the editor sends
  ([ADR-0044](docs/adr/0044-untrusted-runs-git-as-a-process-and-a-wider-memory-gate.md)).
  SECURITY.md recommends it for a checkout you do not trust.
- **The language server's `trusted` initialization option** (`{"trusted": true}`): until the
  editor sends it, `alint lsp` runs no command rule, Git rule or custom fact, fetches no remote
  `extends:` and reads no environment value (see Security). The VS Code extension sends VS
  Code's Workspace Trust state.
- **Workspace trust in each editor integration.** The VS Code extension declares limited support for
  untrusted workspaces, and in Restricted Mode ignores the workspace's own `alint.path` and
  `alint.serverArgs` settings and turns off **alint: Show effective rules**. The bundled `alint.el`
  gains `alint-trusted` (nil by default); Sublime's `LSP.sublime-settings` sends `"trusted": false`,
  to set to true; Helix's `languages.toml` and Neovim's `lsp/alint.lua` show the setting in a
  comment; and the Eclipse, JetBrains and Zed READMEs say how a workspace is trusted there
  ([editors](docs/site/integrations/editors.md#workspace-trust)).
- **`alint_dsl::rule_spawn_use`, `rule_git_use`, `fact_spawns` and `refuse_spawning`** (library
  API): what in a rule spawns a process (its kind, its fix, or a nested `require:` kind), what in a
  rule runs Git, whether a fact spawns, and the check `--no-spawn` runs.
- Library API for this release's bounds and scans: **`alint_core::git::deny_for_process`** (run no
  Git command in this process from now on, as `--no-spawn` does), **`alint_core::bounded_file`**
  (read a regular file up to a cap), **`Baseline::load_file`** (a baseline read with the bounds
  above), **`alint_core::POLICY_TEST_DIRECTORY`**, **`Scope::candidates`, `candidate_files` and
  `candidate_dirs`** (the index entries a scope can match, narrowed to the literal directories its
  patterns start with), **`LspOptions::config`, `no_gitignore`, `no_spawn` and `untrusted`**,
  **`LoadOptions::global_git_excludes`** and
  **`alint_testkit::Runner::experimental_config_version`**. `LspOptions` and `LoadOptions` are now
  sealed (see Changed), so a caller builds them with the setters below.
- **Language-server configuration errors are placed where they are.** A YAML syntax error in
  `.alint.yml` is shown on its line instead of on the first line of the file.
- **`alint_core::yaml_depth::expansion`, `GuardScan::expansion` and `Expansion`** (library
  API): the alias guard's verdict on a text, as within the budget, over it, or not read, when
  ayaml refuses the text before the budget decides. A caller that reads the text with another
  parser refuses the last (see Security); `expansion_within_limit` keeps its meaning for a caller
  that decodes with ayaml. `mapping_subtree_is_literal` now reads the text as ayaml does.
- **`alint_core::names_git_directory`, `path_names_git_directory` and `reaches_git_directory`**
  (library API): whether a name, a path or a path below a root (through its symlinks) names a
  Git directory in a spelling Git's own checks name. Fixers refuse such paths (see
  Security).
- **Where a fix may write** (library API, see Security): `FixContext::write_refusal`, the reason
  a fix may not write a path, with the `extended_source`, `config_files` and `index` fields and
  their setters `with_extended_source`, `with_config_files` and `with_index` (the walk, which
  spares a file it reached without a symlink any file system call for these checks);
  `alint_core::read_source_for_fix`, for a fix's read of a file it does not write;
  `alint_core::path_names_alint_config`, `reaches_alint_config`, `CONFIG_FILE_NAMES` and
  `CONFIG_DROP_IN_DIRECTORY`; `Config::config_files` and `Config::with_config_files`, the local
  files a configuration was loaded from, which `Engine::with_config_files` hands the fixes; and
  `FixReport::unresolved`.
- **`Engine::with_condition_environment` and `Context::condition_environment`** (library API,
  with `Context::with_condition_environment` and `Context::environment_for_conditions`): the
  environment `env.*` in a `when:` and a legacy `${VAR}` read, when it is not the process
  environment. `alint test` leaves the host variables it passes its granted commands out of it.
- **Constructors and setters for the structs v0.23 seals** (library API, #517, ADR-0032
  decision 3, ADR-0038). Code outside `alint-core` builds each of these without a struct literal,
  which `#[non_exhaustive]` no longer allows (see Changed):
  - `Context::new(root, index)`, with `with_registry`, `with_facts`, `with_vars`,
    `with_git_tracked`, `with_git_blame`, `with_analysis_time` and `with_process_environment`.
    `Context` is now `Clone`, so a copy with one part changed is `ctx.clone().with_vars(None)`.
  - `RuleSpec::new(id, kind, level)` and `NestedRuleSpec::new(kind)`, with a `with_*` setter
    for each other field.
  - `WalkOptions::default()`, as before, with `with_respect_gitignore`, `with_extra_ignores`
    and `with_global_git_excludes` (below).
  - `alint_lsp::LspOptions::default()`, with a `with_*` setter for each field (`with_show_notes`,
    `with_analysis_time`, `with_registry`, `with_config`, `with_no_gitignore`, `with_no_spawn`
    and `with_untrusted`).
  - `alint_dsl::LoadOptions::default()`, `with_catalog(catalog)` and `with_cache(cache)`, as
    before, with `cache` and a `with_*` setter for each other field (`with_kind_catalog`,
    `with_fetcher`, `with_environment`, `with_config_source`, `with_remote_policy`,
    `with_out_of_root_policy` (with `OutOfRootPolicy`), `with_repository_root` and
    `with_global_git_excludes`).
  - `Violation::new` and its existing setters are unchanged; its other fields are assigned.
- **The options structs of 14 fix operations are exported from `alint_core`** (library API,
  ADR-0038): `ChmodFixSpec`, `CommandFixSpec`, `CreateAndRegisterFixSpec`, `DirCreateFixSpec`,
  `GitUntrackFixSpec`, `IndentStyleFixSpec`, `InsertHeaderFixSpec`, `InsertLineFixSpec`,
  `RelocateFixSpec`, `RemoveValueFixSpec`, `ReplaceFixSpec`, `SetValueFixSpec`, `SortFixSpec` and
  `SyncFromFixSpec`. `FixSpec`'s variants already held them, but they could not be named and had
  no documentation. `alint-core`, `alint-dsl` and `alint-testkit` now deny the
  `unnameable_types` lint, so every public type they reach has a name and a page.
- **`WalkOptions::global_git_excludes`** (library API). With `false`
  (`with_global_git_excludes(false)`), a walk leaves out the user's global excludes file (Git's
  `core.excludesFile`, by default `$XDG_CONFIG_HOME/git/ignore`), which it finds through the
  process's environment. `alint test` sets it, so what a policy test sees no longer depends on the
  host's Git setup; the default keeps it, as before.
- **`alint_core::git::collect_changed_set`, `collect_changed_set_checked` and `ChangedSet`**
  (library API). They return a `ChangedSet`: the changed paths, with those Git lists as a
  directory (a submodule, an untracked repository) marked. `Engine::with_changed_set` takes one;
  `with_changed_paths` still takes the paths alone and marks none, dropping any marks an earlier
  `with_changed_set` gave. A `constraint` rule runs on a marked path when its selectors can match
  below it (see the kind's entry).
- **A YAML parser facade with test-only reference parsers** (ADR-0041). `alint_core::yaml` decodes
  through a backend. alint-core's `yaml-reference` feature, which no release enables and which is
  not a stable API, adds libyaml (through `yaml_serde` 0.10.7, the parser v0.22 used) and a compare
  mode, chosen with `ALINT_YAML_PARSER=libyaml` or `compare` in a build with the feature. The
  feature is for alint's own tests, and a crate that depends on alint-core must not enable it:
  the variable then chooses the parser of every decode in the process. In compare mode every
  decode returns ayaml's result, and a difference from libyaml that neither saphyr's reading nor
  the differential harness's allowlist explains panics. Neither mode lets libyaml read an input
  past the limits ayaml decodes under, which libyaml lacks: compare mode leaves such an input
  uncompared, and logs it, and libyaml mode refuses it; libyaml would expand an alias bomb in
  full, even one that ayaml refuses for its syntax. CI and the release preflight run the
  workspace's tests that way (`ci/scripts/yaml-compare.sh`), and a gate fails if a build without
  the feature depends on `yaml_serde`.
- **`version: 2` configurations (experimental)**
  ([ADR-0034](docs/adr/0034-v2-version-boundary-and-phase-3-design.md),
  [ADR-0037](docs/adr/0037-phase-3-design-refinements.md)). A config whose top-level `version:`
  is the plain integer `2` loads as version 2, which is experimental: its syntax may change in
  any release until one declares it stable, v0.24 at the earliest, and this file lists each
  change. Loading one prints a warning once per run, naming the file
  (`` alint: warning: `version: 2` is experimental; ... ``); `validate-config` names the version on
  its success line and puts the warning in its JSON `warnings`. The language server loads a v2
  config without the warning.
  - alint reads a config's `version:` before anything else in it and decodes the rest as that
    version. A 2 written any other way than as the plain integer `2` (quoted, tagged, through a
    YAML alias, or as `+2`, `0x2`, `0o2` or `2.0`) is an error that says to write `version: 2`;
    a version read from the environment is version 1 and must equal 1, as before. An `.alint.d/`
    fragment without `version:` is read as its root's version.
  - Version 2 accepts every version 1 key and rule kind with its version 1 meaning, plus
    top-level `selectors:` and `relations:` and an `as:` alias on a local `extends:` entry. Each
    of those in a version 1 config is an error that names the version it needs
    (`` `selectors:` needs `version: 2`; this config is version 1 ``), and an unknown key in a
    version 2 config is reported with the version 2 keys.
  - Selectors (`paths:`, `scope_filter:` without `changed_since`) and relations (`from:` with
    `entries:` or `read:`, `union:`, `join:`, `fields:`, `transform:` and `where:`) are checked
    when the config loads: every key, value type, field source, transform and predicate. Names,
    aliases and references are literal: `{{...}}` in one is an error. A name is declared once
    per config file, across selectors, relations and aliases. Selectors and relations count
    toward the entries a merged config may hold.
  - Only local files may be version 2: the top-level config, a local `extends:` target and an
    `.alint.d/` fragment. A remote `extends:` source, a bundled ruleset, a nested config and the
    string `alint_dsl::parse` reads must be version 1, and a version 1 config cannot extend a
    version 2 one; each is an error that says so.
  - Each config file has its own namespace. A bare name (`from:`, a join's `left:` and
    `right:`, a `where:` filter's `{ relation, field }`) names an object the same file declares;
    `alias.name` names an object the file an `extends:` entry gives `as: alias` declares itself,
    with no re-export and no `a.b.c` chain. Nothing shadows, and an `.alint.d/` fragment's
    namespace is its own too. A name or alias that does not resolve is an error naming the file
    and key, with the names that file can reach or the nearest one
    (`` `relations.titles.from`: no selector named `doc` in this config; the selectors it can
    reach are `docs` ``).
  - The relation plan is checked when the config loads, before any file is read: every field a
    join's `on:`, a `left.F` or `right.F`, a `transform:` or a `where:` names exists; a relation's
    rows bind each of its fields, so a `bound:` field needs `entries:` or a `read:` of JSON, YAML
    or TOML; every part of a `union:` binds every field; `strip-root` applies first to an
    `entry.path` field, or to a join's copy of one no transform has changed; `semver-major` and
    `semver-minor` do not apply to a path, a path token (`dir`, `basename`, `stem`, `ext`,
    `parent_name`) or an `entry.path`, nor to a join's copy of one; a regex compiles and a `read:`
    regex has a capture group; a JSONPath parses; an `in` or `not_in` literal is not null or a
    number, nor a boolean for a field that holds strings; and relations that read each other are
    refused, naming the cycle
    (`` relation `a` reads itself: `a` -> `b` -> `a` ``). A selector compiles as a rule's scope
    does, so a manifest `source:` stays inside the repository and out of `.git`.
  - Each selector and relation passes the trust gates of the file that declares it, with what it
    references, as an extended file's rules do.
  - A selector's or relation's error, when it decodes or compiles and in the checks across
    objects, stands only when the same object with each value it reads from the environment
    masked fails the same way (with the relations it joins masked too); otherwise it is redacted,
    naming the object (`` `relations.deps`: <redacted> (the error could quote a value read from
    the environment) ``), as a version 1 rule's is. So no error shows part of such a value: a
    JSONPath's hint, which quotes the path rewritten, a `has_ancestor:` path's last segment, or a
    value too short for the whole-value scrub.
  - `alint list` lists a version 2 config's named selectors and relations after its rules, each
    with the file that declares it and the aliases that reach that file; `list --format json`
    adds `selectors` and `relations` for a version 2 config only, and `--category` lists rules
    only. A file an `extends:` entry read from the environment loads is shown as `<redacted>`,
    here, in `export-agents-md` and in errors found in it, and so is every file loaded below it
    and an `extends:` URL there that leads to one, however short its name.
  - `alint export-agents-md` lists a version 2 config's named selectors and relations the same
    way, after its rules, under `### Named selectors and relations`; `--format json` adds
    `selectors` and `relations`, shaped as in `list --format json`, for a version 2 config only.
    A version 1 config's output is unchanged. In both, each version 2 file's `extends:` aliases
    are listed once per file, and `--format json` adds a `sources` list that `selectors` and
    `relations` entries refer to by index. An unsupported `version:` names version 2 as the
    experiment, and `alint test` warns once a run when a policy test's config is version 2.
    `init` still writes `version: 1`, and `suggest --format yaml` still writes no `version:`, so
    its snippet can be added to a config of either version.
  - Only the `constraint` rule kind (below) reads a relation: every other rule runs as in version
    1, and a relation no `constraint` rule reads is never evaluated.
  - alint can now evaluate a relation over a repository: an internal evaluator, which the
    `constraint` rule kind will call, reads the files a selector picks (as a rule's `paths:` and
    `scope_filter:` pick them, inside the repository, never in `.git` and only regular files, as
    `cross_file` reads them), emits one row per entry or extracted value with each field's file, key
    path and byte span, and applies `union:`, `join:`, `transform:` and `where:`. Values are scalars
    compared by type and text (a number by its lexeme, so `1.10` is not `1.1`). What a relation
    cannot hold, such as an `entry.value` that is an object, a TOML date, a `bound:` path that does
    not match exactly once, a repeated key on a join's `unique:` side or a comparison across types,
    is an analysis failure naming the relation, the file and the key path, never a dropped row; so
    is reaching a row, cell, byte, join or distinct-value limit, which is never truncation (a run's
    relations hold at most 5,000,000 cells, a cell per field of each row, and 1 GiB besides: a
    value's text is held once however many fields, parts and joins copy it, and each decode, key
    path and transformed value takes its bytes once). Every `where:` predicate tests every row, so
    their order does not matter: a row that any predicate is false for is dropped, even when another
    cannot test it. A value substituted from `{{env.X}}` in `selectors:` or `relations:` stays text,
    except in a field that takes a boolean (`optional:`, `leaves:`, a field's `sensitive:`). A field
    the environment writes to (`{{env.X}}` in its `where:` or `transform:`) is sensitive, as is
    every field that shows the same value (an entry's key, path and value, and fields bound to the
    same value). So is a join's copy of a sensitive field, and of the field an `on:` pair joins
    with one, and a field (with its family) that an `in:` or `not_in:` list of a sensitive field
    filters, in every relation that reads them. A rule that reads a relation which takes a value
    from the environment anywhere in its definition, a field's `where:` or `transform:` included,
    shows its findings' paths as `<redacted>`, as an environment-written selector's rule does: the
    rows such a filter keeps say which files hold the value. A relation whose selection reads the
    environment shows no detail in its failures, nor does a relation that reads one, as a join side
    or through a membership reference.
    The engine evaluates, in plan order, only the relations an enabled `constraint` rule reads,
    before any rule runs, so a run whose rules read none reads nothing for them.
  - `facts.json` (format 7) gains `compatibility.experimental_config_versions: [2]`;
    `config_versions` stays `[1]`. It also gains `compatibility.kind_config_versions`
    (`{"constraint": 2}`, the kinds that need more than version 1) and
    `compatibility.experimental_rule_kinds` (`["constraint"]`). `validate-config --format json`
    gains `config_version`, the loaded config's version, for every config that loads (a failed
    validation has no version to report).
  - A draft JSON Schema for version 2, `schemas/v2/config.json`, with the `$id`
    `https://raw.githubusercontent.com/asamarts/alint/v0.23.0/schemas/v2/config.json`. It is the
    version 1 schema with `version: 2`, `selectors:`, `relations:` and `as:` added, and checks
    what the loader checks of those when it decodes a file: every key, value type, name,
    reference, field source, transform, `entries:` format, `read:` form and count. It leaves to
    the loader what needs every source loaded (names that resolve, cycles, the relation plan),
    regexes and JSONPaths, and a name declared twice. An editor uses it through a
    `# yaml-language-server: $schema=` modeline in a version 2 file; the version 1 schema accepts
    the same configs and keeps its `$id` (only its descriptions change, under Changed).
  - A docs page, `docs/site/configuration/version-2.md`, describes version 2: the version
    boundary, named selectors, relations (`entries:`, `read:`, `union:`, `join:`, fields,
    `transform:` and `where:`), names, aliases and per-file namespaces, what the loader and the
    draft schema check, trust, the limits of this release and the three conformance policies
    (dependency version consistency, locale key parity and toolchain-pin consistency, with the
    shorter version 1 `key_parity` and `cross_file` spellings shown alongside their version 2
    forms). A test loads and validates each of its examples, and the three conformance policies
    ship firing and silent `alint test` cases.
- **The `constraint` rule kind (version 2 only, experimental)**
  ([design](docs/design/v0.23/selectors-and-constraints.md), sections 6, 8, 10 and 11). A
  `constraint` rule reads one relation, its `input:` (a name, bare or qualified by an `extends:`
  alias, or an inline relation, whose values may not read the environment), and makes one
  assertion over its rows. It exists only in a `version: 2` config and is experimental as version
  2 is: its syntax may change before a release declares version 2 stable. A version 1 config that
  uses it, or extends a file that does, fails to load with an error that names version 2, and so
  does a `constraint` rule nested under a `for_each_*` kind's `require:`. `alint rules list` and
  `rules show` mark it `version 2 only, experimental`, and their JSON gains `config_version` and
  `experimental` for every kind. The kind takes no `paths:`, `scope_filter:` or fix.
  - `equal`: rows agree on a field, within each `group_by:` group, with one source row
    (`to: { row: { source: .nvmrc } }`), or with another field of the same row
    (`to: { field: G }`). `unique`: no two rows share a key; with `determines:`, rows that share
    the key agree on the determined fields. `set`: each group's set of a field's values is equal
    to, a subset of or disjoint from a reference group's (`{ group: en }`), or the values of
    another relation's field (`{ relation: R, field: F }`, a name, bare or qualified, never an
    inline relation), with `plurals: cldr` merging CLDR plural groups of an `entry.path` field
    as `key_parity` does (against a group only).
  - A `set` grouped by `path` or a path token gives every resource its relation reads a group, rows
    or not, except one `where:` rejects by its path alone, and `groups:` names groups that must
    exist (with `equal_to:` only; under the other two a group without rows passes, so they refuse it
    at load): a group with no rows lacks every value of the other set, at its first resource, or
    without a path when only `groups:` names it. A missing value is related to the row that holds
    it; an extra one sits at its row; a shared one sits at its row, related to a row of the other
    set. The other relation is read before the rule, its failures withhold the rule's findings, its
    notes are reported once, and its set is bounded by the 100,000 distinct-value limit. A sensitive
    field on either side hides the values of both, and a sensitive field of the other relation makes
    the compared field sensitive at load, with every field of the input that can spell the same
    values (for an `entry.*` field, every `entry.*` field, `entry.value` included).
  - Under `strip-root`, a file whose top-level key holds nothing (`fr:`, `fr: {}`, `fr: []`) reads
    as a file with no keys, so it lacks every key of the reference group, and its key still counts
    as the file's one top-level key; a top-level key holding a scalar or a non-empty array is an
    analysis failure. (A stub locale withheld every locale's verdict before audit round 4: 71 of
    Mastodon's 530 locale files are `xx:`.)
  - Values compare by type and text (a number by its lexeme); null and a missing value match
    nothing, and messages and baseline keys call a value a `read:` matched nothing `missing`, and a
    null `null`, in every family. With a `union:` input, a missing value's message names the part
    that read nothing, by its place and query (`relations.node-pins.union[2]`, `read.json:
    $.volta.node`), or by its place alone when that part's relation reads the environment or the
    field is sensitive. A key, group or set comparison across types, a null group, a missing or
    repeated source row and an empty reference group are analysis failures, never a pass, and so is
    a relation that fails to evaluate. Any analysis failure withholds the rule's findings, the
    findings for values a `read:` skipped included.
  - Each family generates its message, quoting the keys and values (each cut at 64 characters; a
    key path at 64 per key and 256 in all, then its count of segments); `message:` may use
    `{{row.FIELD}}`, the reported row's value, cut the same way, which a `sensitive` field shows as
    `<redacted>`. Key paths, `{{row.FIELD}}` and the files a message names (a row's, a note's, the
    file that declares a relation) escape a control, bidi or zero-width character (`\n`,
    `\u{202e}`), as a quoted value does, and so does the rest of the text of a note or an analysis
    failure. A `strip-root` failure cuts the top-level keys it names, and a note on a skipped
    non-literal value cuts the value, at 64 characters, as other messages do. A field that spells
    an entry's key (`entry.key`, `entry.path`, `entry.parent`, `entry.root`) is sensitive when any
    of them is. Findings carry
    related locations at the other rows involved: at most 20, picked with a row of each other value
    first and listed by path, then a count of the rest, and only those 20 are placed; rows at one
    place (rows of one file without a span) are listed once and the rest counted, so the locations
    and the count add up to the rows. A finding about a field a join copies from its `unique:`
    side, or a `bound:` field, sits at the row's own entry. A finding sits at its cell's line and
    column: an entry's key (its value for `entry.value`), a `lines:` value's line, a `regex:`
    capture, or a JSON, YAML or TOML `read:` value an object member holds; a `whole_file` value, a
    `read:` value in an array, a path token and a value of a file that is not UTF-8 have none. A
    finding or related location at a sensitive cell, or at any cell while key paths are hidden,
    shows only where it starts, since its end would give away the value's length. Placing the
    findings reads each file they sit in again, one at a time. Each file must fit in what the run's
    relations left of `bytes_per_run` (the limit less what their decodes and their rows took,
    counted once they are evaluated, before any rule runs): alone, before its `read:` values'
    places are looked for, and then with its offsets and what finding those places
    held (the document parsed again, with what its parser held beside the tree, such as a JSONC
    file's copy without its comments or the TOML lexer's tokens, and what was read from the
    parse). Placing takes none of it, so whether a rule's findings fit does not depend on the
    rules placing beside it. `alint fix` evaluates the relations once a pass, before its first
    rule as `alint check` does, and checks every rule of the pass against what they took; the
    rules after a fix that removes or moves a file, or runs a command, read their relations
    again, and `fix --dry-run`, `fix --diff` and a pass that changes nothing place findings as
    `alint check` does. When a file does not fit, the rule reports an analysis failure, not
    findings without places. A `read:` value's
    place is found only in the files a finding or related location sits in. A cell is placed only
    where the file read again still holds what was read there (its value, or its key for an
    entry's key), so a file changed during the run leaves a finding or related location without a
    place rather than at other text. A typo in the kind's options gets a "did you mean" for the
    nearest field, not for the typo itself.
  - When a relation the rule reads takes a value from the environment anywhere in its definition, a
    field's `where:` and `transform:` included, or reads one that does (a join side, an `in:` list's
    relation, a `set` target), every finding's path and message are redacted, as for a version 1
    rule whose `paths:` the environment writes: the rows a filter keeps say which files hold the
    value it was given. A value from the environment anywhere in `assert:` redacts every message of
    the rule the same way.
  - A finding's baseline fingerprint is its rule and its key, never its path, so a row that sorts
    first or a renamed first file keeps a baselined finding baselined. The key holds the values
    the finding is about (a value past 128 bytes as its SHA-256 digest, a sensitive one as the
    word `sensitive`); the rows that repeat a `unique` key share one key and are told apart by
    their count, as are findings that differ only in a sensitive value. Other kinds'
    fingerprints are unchanged. SARIF and GitLab output tell findings that share a fingerprint
    apart by occurrence (see Fixed).
  - Every field, literal and `{{row.FIELD}}` is checked against the relation when the rule
    builds, naming the rule, the field and the relation; names may not hold `{{...}}`. A
    `groups:` literal must be of the `group_by:` field's type, once, and not the reference
    group; `plurals: cldr` needs an `entry.path` field with no transform but `strip-root`.
  - Under `--changed` and `--base`, a rule runs when a changed path (added, modified or deleted) is
    in its trigger scope: the selectors of every relation it reads (its input, both sides of each
    join, the relations its `in:` and `not_in:` lists probe and the relation its `set` compares
    against), narrowed by their `scope_filter:`, with the manifests that `scope_filter:` reads, and
    the `has_ancestor` markers and `.gitignore` and `.ignore` files in the directories where those
    selectors' `paths:` can match, and a submodule added, moved or removed or an untracked
    repository below which those selectors' `paths:` can match, which Git lists as one changed
    directory (an untracked repository on every run without `--base`, until it is committed or
    ignored). It then judges the whole snapshot and reports every finding, those in unchanged files
    included. Outside its trigger scope it does not run and reports nothing, and a change to a
    configuration file alone runs no rule. An edit to a symlink's target, a file Git ignores under
    `respect_gitignore: false`, an ignore file Git does not report, a staged or committed change
    to a submodule Git is told to ignore, `git submodule deinit` or `update --init`, and an edit in
    a submodule's work tree on the same commit run no rule; `docs/rules.md` lists them. A pass decides
    once which rules the changed paths run, and a selector tests only the changed paths below the
    literal directories its `paths:` start with, each once: with 200 rules and 10,000 changed files
    outside every selector, `check --changed` runs 0.79 billion instructions, against 1.56 billion
    for a full check. `facts.json` lists the kind's dispatch as `full_index` with
    `path_scope: true`.
  - `alint explain <rule>` shows the rule's resolved plan, after its options: the assertion in
    words; every relation the rule reads, in the order a run evaluates them, named as the config
    writes each reference that reaches it (with the file that declares it when two relations would
    read alike, so each reference names one) and marked as the input, an inline input or the `set`
    target; each one's source (a selector with its `paths:`, its `scope_filter:` and the manifests
    that resolves to, a join with its `left`, `right`, `on` and `unique`, or a union's parts, each
    named by its place from 0, `union[0]`, as messages and config paths count), its reader
    (`entries:` with its format and `at:`, or `read:` with its query), its fields with their
    sources, transforms and sensitivity, its `where:`, and the config file that declares it; the
    selectors that run the rule under `--changed`; and its cost class. `--format json` adds the same
    as a `plan` object, which the experimental trace schema's rule document describes key by key
    (`schemas/experimental/rule-trace.json`), where each relation has an `id` (its place in the
    list) by which a join's sides, an `in:` list, the trigger, `input` and `set_target` name it; a
    rule of any other kind has no `plan`, so its output is unchanged. The plan is configuration: it
    shows no value read from a file, except the paths a manifest-derived `scope_filter:` resolves
    to, as for any rule (ADR-0010). What the environment wrote stays redacted as `explain` redacts
    it elsewhere: a selector or relation definition that reads the environment shows `<redacted>`
    for each value it holds, an `extends:` entry the environment chooses names its files
    `<redacted>`, and an environment-written assertion is `<redacted>` whole. A sensitive field's
    `where:` literals and patterns, its transforms, and the literals the assertion compares with it
    (`to: { row: }`, `{ group: }`, `groups:`) show as `<redacted>`, in the plan and in the rule's
    options; a rule marked `sensitive: true` shows its plan as `<redacted>`.
  - The version 2 draft schema describes the kind and refuses it nested; the version 1 schema
    leaves it out. The draft checks what the kind's builder checks of an `assert:` alone: one
    family, one `set` target of one shape, the names of fields and relations, counts and repeats,
    and the keys that need or exclude others (`group_by:` with `to:`, a `{ group: }` target
    without `group_by:`, `groups:` outside `equal_to:`, `plurals: cldr` against a relation). A key
    written as null is refused, as the builder refuses it. What needs the relation (a field it
    has, a literal of the field's type) or compares two values (`determines:` naming a key field)
    is the builder's alone.
  - Library API: **`alint_rules::constraint`**, the kind's builder, its `META`, its
    `options_schema` and `MIN_CONFIG_VERSION`; **`RuleRegistry::register_for_config_version`**,
    which registers a kind only a config of a given `version:` or later may use, with
    **`RuleRegistry::min_config_version`** and **`KindCatalog::min_config_version`**, the lowest
    `version:` a kind needs; **`alint_core::relation::plan::is_name`**, the name rule the loader
    and `NamedRelation::is_inline` share; **`relation::eval::Span`**, where a cell's value is
    written (`Span::Bytes`, or `Span::Read` for a `read:` value, found only for a value a finding
    places), and **`relation::eval::find_spans`**, which finds a resource's `read:` values in its
    text; and `relation::eval::Provenance::empty`, whether an entry's value holds nothing.

### Changed

- **Config discovery stops at the repository's top** (see Security). Run from a repository, or
  from a directory below its top, alint uses only a config at or below the top (the nearest
  directory holding a `.git` directory or file); a submodule or linked worktree, whose `.git` is
  a file, is a repository of its own. A repository without a config of its own now reports
  `no .alint.yml found` where a config above it used to govern it: move that config into the
  repository, or pass it with `--config`. Outside every repository, a config file another user
  owns in a directory above the starting one is skipped with a warning; one in the starting
  directory is used. Library users of `alint_dsl::discover` get the same stop and
  skip; `alint_dsl::discover_config` also returns the skipped files.
- **Language-server code actions keep `alint fix`'s write limits.** A code action from a rule an
  `extends:`'d ruleset or a nested config shaped is no longer offered when it would edit alint's
  configuration (a config file at any depth, anything under `.alint.d/`, or a file the
  configuration was loaded from), and no rule's code action edits a path in a Git directory.
- **The language server treats a workspace as untrusted unless the editor says otherwise.**
  Editors other than VS Code now need the `trusted` initialization option for command rules,
  custom facts, remote `extends:` and environment values to take effect
  ([editors](docs/site/integrations/editors.md#workspace-trust)); see Security.
- The language server drops a re-evaluation that a newer edit superseded before it starts, and
  computes code actions off the request thread.
- **`alint fix` no longer writes into `.git` for the user's own rules either.** The Git-directory
  refusal (see Security) applies to every configuration, the top-level one included, so a
  user's own `file_create` of `.git/hooks/pre-commit`, which v0.22.0 created, is now reported
  skipped. Keep hooks in a tracked directory and point `git config core.hooksPath` at it, or use
  a hook manager. [`alint fix`](docs/site/cli/fix.md) and
  [fixing](docs/site/concepts/adoption/fixing.md) say so.
- **The human and Markdown fix reports call a fix that ran without resolving its violation
  `unresolved`, not `skipped`.** Such a fix may have written a file (a `file_create` into a path
  the walk does not index, or outside the root under `allow_out_of_root`), so its line reads
  `(unresolved: fix ran (created ...) ...)` and the summary counts it apart:
  `0 applied, 0 skipped, 0 unfixable, 1 unresolved.`, with `unresolved` shown only when nonzero.
  The JSON report is unchanged (`status: "skipped"`, `skip_kind: "unresolved"`, and the
  summary's `skipped` still includes it), as is the exit code.
- **Breaking (library API):** the public field `WhenEnv::env` holds an `Option<&BTreeMap<String,
  String>>`, and `WhenEnv::with_env` takes a `&BTreeMap<String, String>`, the type of the engine's
  process environment, where both used a `HashMap`. The engine now passes its condition environment
  there whenever it has one (by default its process environment), so `env.*` in a condition reads
  the environment the embedder chose, not the host's (see Security).
- **`git_tracked_only: true` sees tracked files that an ignore file excludes** (#503, ADR-0033
  decision 3). Under the default `respect_gitignore: true` the walk leaves out what `.gitignore`,
  `.ignore`, `.git/info/exclude` and the global excludes file list, and the tracked view was
  built from the walk. A file forced in with `git add -f`, or committed before its `.gitignore`
  entry, therefore never reached `file_exists`, `file_absent`, `dir_exists` or `dir_absent`. The
  tracked view now adds back each tracked path that only an ignore file hid, and each directory
  above a tracked path, when it exists inside the root. This is the documented behavior. A
  `file_absent` or `dir_absent` rule with `git_tracked_only: true` can now fire on a force-added
  path, as the bundled `java@v1` rules do on a force-added `target/`, `build/` or `.class` file,
  and a `file_exists` or `dir_exists` rule can now pass. The config's `ignore:` and the walker's
  other exclusions still apply. A `.ignore` file, which git does not read, no longer hides a
  tracked path from these rules either; list the path in `ignore:` to keep it out. `fix
  --dry-run` and the language server agree with `check` on such paths.
- `java@v1`'s `java-no-tracked-target` and `java-no-tracked-build` say to untrack a directory
  that holds build output with `git rm -r --cached` as well as list it in `.gitignore`, since the
  directory they report can now already be listed there, and `java-no-class-files` says to
  untrack the files with `git rm --cached`.
- `registry_paths_resolve` and `file_graph`'s `no_dangling` no longer say that a path is missing "on
  disk", and `markdown_links_resolve` no longer says that "no file or directory exists". They
  resolve paths against the walked tree, so a path that an ignore file or `ignore:` excludes does
  not resolve either, and the messages now say "missing or ignored". A `registry_paths_resolve`
  entry of the other kind now says so ("resolves to a file, not a directory"), and `php@v1`'s
  Composer rules say an entry must exist and not be ignored. Their baseline keys are unchanged.
- **alint parses YAML with ayaml 0.2.2, a pure-Rust parser, in place of `yaml_serde` and
  `libyaml-rs`** (#510, ADR-0035). Configurations, `extends:` sources, the `yaml_path_*` rules,
  `json_schema_passes` on YAML and `alint test` packages all read through it. The
  depth limit is that of 0.22 (128 levels), and the 32 MiB size limit 0.22 set on data files now
  holds for configurations too: a configuration, `extends:` source or nested configuration past
  33,554,432 bytes fails with "input exceeds the maximum supported size for a structured document
  (33554432 bytes)" and its path, where 0.22 read it. ayaml reads YAML 1.2, where libyaml read
  YAML 1.1. A differential harness (#482) compared the two on more than three
  million generated and mutated documents (3,848,472 inputs across seeds 1 to 12) and on every YAML
  file in this repository (986 files): valid documents decode to the same values, except in the
  cases below.
  - **Read differently:**
    - a plain scalar may contain U+0085 (next line), U+2028 (line separator) or U+2029
      (paragraph separator), which YAML 1.2 reads as content; libyaml read them as line breaks.
      Between tokens they are now an error;
    - a node with a global tag (`!<tag:...>`, a `%TAG` handle, or `!!` with a name outside the
      core schema, such as `!!binary`, `!!timestamp`, `!!merge` or `!!value`) keeps the tag in a
      value, and where alint drops tags (`yaml_path_*`, `json_schema_passes`) its
      scalar resolves as if untagged, so `!<tag:x,2000:y> 1` is the number 1 where 0.22 read the
      string `"1"`. A core tag on another kind of node, as in `!!map 1`, is ignored the same
      way, and an empty node tagged `!!null` is null, where 0.22 rejected it;
    - a node tagged with the non-specific `!` is a string, as YAML 1.2 says, where 0.22 resolved
      it as if untagged;
    - an alias to an anchor defined more than once refers to the latest definition before it,
      and is an error inside that node; 0.22 could resolve it to the wrong node, or fail with
      "recursion limit exceeded";
    - an integer beyond the 64-bit range decodes as a 128-bit integer, where 0.22 rejected the
      document (the JSON form the structured-format rules query still rejects it). In a rule or
      template of a configuration it is still an error, which names the file and the field
      (`rules[0].message`);
    - an anchor or alias name may hold any character but white space and the flow indicators,
      where 0.22 took only letters, digits, `_` and `-` (`&a.b x` was an error). As in 0.22, a
      name ends at a `?` before a character other than a flow indicator and, outside a flow
      collection, at a `:` before a character other than white space, so `&a:b x` is still the
      string `:b x`; but `&a: x` anchors `x` under the name `a:`, where 0.22 read a mapping, and
      `[*a:b]` holds an alias to `a:b`, where 0.22 read `{*a: b}`;
    - in a flow sequence, a plain scalar that starts with `:` after properties is that scalar,
      so `[!!str :a]` holds the string `:a`, where 0.22 read the mapping `{"": a}`;
    - a byte-order mark that starts a configuration, an `extends:` source, a policy test or a
      data file is dropped before the text is read, so one may come before `---` or `%YAML`,
      where 0.22 read `---` as the start of a scalar, and it no longer counts as a column of the
      first line. One that starts another line still counts as one.
  - **Now accepted:** a tab as separating white space outside indentation, including on a blank
    line after `...`, and as content after the indentation of a block scalar's first line; an
    empty implicit key (`: value`); a stream that starts with `...`, with or without a document
    after it; reserved directives, which are ignored; `%YAML 1.x` with any minor version other
    than 1 and 2, earlier or later, read as 1.2; top-level block scalar content in column 1; YAML
    1.2's flow forms, such as `[:a]`, `{:}`, `[a:]`, `[? ]` and a flow mapping key that spans
    lines; an implicit key of more than 1,024 characters in a flow mapping; an implicit key of
    1,024 characters or fewer that takes more than 1,024 bytes, such as 600 accented letters,
    which 0.22 measured in bytes; `#` inside a tag or a `%TAG` prefix, as in `!t#`; U+007F,
    U+0080 to U+009F other than U+0085, U+FFFE and U+FFFF inside a single- or double-quoted
    scalar, where YAML 1.2 allows any JSON character; aliases that fan out four levels deep, as
    ten aliases to a node of ten aliases to a node of ten aliases to a node of ten aliases to
    `[x]`, within ayaml's alias-expansion allowance, where 0.22 stopped after 100 alias uses per
    event ("repetition limit exceeded"); and, in a configuration, `-0` where an unsigned integer
    is expected, as in `fix_size_limit: -0`, which 0.22 refused (``invalid type: integer `0`,
    expected u64``), and a quoted scalar tagged `!!int` where a number is expected, as in
    `version: !!int '1'`.
  - **Now rejected:** a `#` directly after a token, as in `'x'#c` or `%YAML 1.2#`, which is not a
    comment; a quoted scalar's continuation line that is not indented past its parent; a block
    scalar whose header or content is not indented past its parent, as in `-` followed by `|2` on
    the next line at the same column; a lone `-` in a flow collection, as in `[-]`; a flow `?`
    not followed by white space, as in `{?}`; a `:` directly before a flow collection after an
    empty key with properties, as in `[&a :[b]]`; a tag handle holding characters other than
    letters, digits and `-`, as in `!e:a!z`; the verbatim tag `!<!>`, which names no tag; a flow
    collection's continuation line in a block collection that only tabs indent, as a tab and
    `b]` on the line after `a: [x,`; a mapping key whose core tag does not fit it, as
    `!!int x: 1`, which 0.22 read as the key `x` where alint drops tags; and, in a
    configuration, a core tag that does not fit its field, as in `version: !!str 1`, `!!float 1`
    where an integer is expected or `!!str true` where a boolean is, which 0.22 read by their
    text. Some tags 0.22 dropped are now refused: a global tag (`!<tag:...>`, a `%TAG` handle,
    or `!!` with a name outside the core schema, such as `!!binary`) in a rule, a template, a
    fact, an `extends:` entry or a policy test, and a `!!` tag outside the core schema where a
    configuration takes a number or a boolean, as in `version: !!timestamp 1`. The error names
    the field and the tag (`` `version` has the YAML tag `!!timestamp` ``). `vars:` and
    `ignore:` still read their strings through any tag.
  - **Errors.** Several parse errors are worded differently, in a `resource_parse` analysis failure
    and in a configuration error. A stream of more than one document reports "more than one document
    in stream at line 2 column 1" for `version: 1`, `---` and `version: 1` on three lines, where
    0.22 said "deserializing from YAML containing more than one document is not supported", and an
    unclosed quoted scalar reports "quoted scalar continuation must be indented more than its
    parent" at the first line after it, where 0.22 said "found unexpected end of stream ... while
    scanning a quoted scalar". Errors in what a file holds say more than 0.22's did. An option of
    the wrong type names its field as well as what it found, as "pattern: invalid type: integer `1`,
    expected a string"; a value that is not one of an enum's, as a rule's `level`, a policy test's
    step or an option such as `direction`, lists them, as "level: expected one of `error`,
    `warning`, `info`, `off`, found a mapping", where 0.22 said "invalid type: map, expected a
    Value::Tagged enum"; a key or entry indented one space past a parent mapping or sequence reports
    "this line is indented differently from the mapping/sequence at line X column Y", a line tabbed
    as indentation notes that YAML does not count tabs as indentation, and remaining "unexpected
    content after document" mistakes (such as `-id: a` with no space after the `-`, a key two or
    more spaces off its siblings, or an indented top-level key before a column-1 key) end with
    "(check this line's indentation against the lines above)". A configuration that is a scalar,
    not a mapping, quotes the scalar when it holds no `{{` (as 0.22 did:
    `invalid type: string "...", expected struct RawConfig`), and reports
    "invalid type: string, expected struct RawConfig" without quoting it when it holds `{{`, where
    0.22 gave only `<redacted>`. An error can also be reported at another place: ayaml scans the
    whole stream's tokens before it parses or builds values, so a scanner error later in a file is
    reported before an earlier parse or value error; parser and document-building errors (duplicate
    keys, invalid core tags, undefined or cyclic aliases) report whichever comes first, before
    typed field errors; and an error about an empty value is reported where what follows starts (the next
    line's content or the end of the file), where 0.22 reported the place after the key:
    `respect_gitignore:` with nothing after it on line 2 is reported at line 3, column 1, where
    0.22 said line 2, column 19. A character YAML does not allow, such as a C0 control, is reported
    at its line and column, as "non-printable character U+0001 is not allowed in YAML at line 3
    column 4", where 0.22 gave a byte offset ("control characters are not allowed at position 24").

  Source spans and structured YAML fixes still use saphyr. The checks that refuse deep flow
  nesting and alias bombs before a parse now read YAML as ayaml does, and ayaml's own depth,
  input and alias limits are set from the same constants (#483).
- **Breaking (library API): YAML values and errors are alint's own types** (#484, #516,
  ADR-0038). No public item of `alint-core`, `alint-dsl` or `alint-testkit` names a YAML parser
  crate any more, so a later change of parser is not a breaking change. The published crates
  pin ayaml exactly (`=0.2.2`), since the differential harness qualifies one version: a project
  that depends on them cannot use another ayaml 0.2.x beside them, and each ayaml release
  reaches alint's library users only in an alint release. `alint_core::yaml` holds
  `Value`, `Mapping`, `Sequence`, their iterators (`SequenceIter`, `SequenceIterMut`,
  `SequenceIntoIter`, `MappingIter`, `MappingIterMut`, `MappingIntoIter`, `MappingKeys`,
  `MappingValues` and `MappingValuesMut`), `Error` and `Location`. Its functions are `from_str`,
  which decodes under alint's parser limits, and `from_value`, `to_value` and `to_string`. A CI
  gate keeps the parser out: it documents the three crates with every feature and with hidden
  items shown, fails on a page that links to a parser crate's documentation or to its source
  (which a re-exported parser item has), and fails on what the documentation cannot show: code
  under a `cfg` other than one on features or tests (such as code for another platform or a
  release build) unless the gate's allowlist names it with a reason, and then any item or macro
  call under it that the entry does not except, an attribute a macro builds, a public type in a
  private module, code included from outside the crate, an exported macro that names a parser, or
  a `pub use` of a parser crate. In place of the `yaml_serde` types of 0.22:
  - `RuleSpec::extra`, `NestedRuleSpec::extra` and `RuleEntry::extra()` hold an
    `alint_core::yaml::Mapping`. Read an option with `get("name")` and the `Value` accessors
    (`as_str`, `as_bool`, `as_u64`, `as_mapping`, `as_sequence` and the rest) in place of a
    `match` on the parser's `Value` variants; `insert` takes anything that converts into a
    `Value`, such as a `&str` or a `bool`.
  - `alint_core::Error::Yaml`, `alint_testkit::Error::Yaml` and
    `alint_testkit::PolicyTestParseError::Yaml` hold an `alint_core::yaml::Error`, which
    `TreeSpec::from_yaml` and `TreeSpec::to_yaml` return as well.
  - `alint_core::Error` is `#[non_exhaustive]`, so a `match` on it needs a wildcard arm.
  - `Value` has `is_null`, `is_bool`, `is_number`, `is_string`, `is_sequence`, `is_mapping` and
    `tag`, the `as_*` accessors, `get` and `get_mut` by string key, and indexing by string key
    (`&str` or `&String`) or by integer, which can also assign; assigning a key on a null, tagged
    or not, makes it a mapping. It converts from `bool`, every integer and float type, `String`,
    `&str`, `Vec<Value>`, `Sequence` and `Mapping`, and compares with `==` against strings,
    `bool` and the integer and float types up to 64 bits. `Sequence` and `Mapping` iterate by
    reference, mutably and by value, and implement `FromIterator` and `Extend`. `Mapping` also
    has `keys`, `values` and `values_mut`, and index assignment, which adds a missing key last.
    Every iterator knows its length, runs from both ends and is fused, and those over shared
    references or owned values are `Clone`.
  - What the `yaml_serde` types did that these do not:
    - `Value` is not an enum: read it with the accessors in place of matching its variants, and
      build one with `From`. There is no `Number`, `Tag` or `TaggedValue`: a number reads
      through `as_i64`, `as_u64` or `as_f64`, and a tag as a string through `tag()`; a tagged
      value comes only from a document, and the accessors look through its tag, as
      `yaml_serde`'s did. There is no `as_null`, `is_i64`, `is_u64`, `is_f64` or `apply_merge`.
    - `Value::get`, `Value::get_mut`, and `Mapping`'s `get`, `get_mut`, `contains_key`, `remove`
      and indexing take a string key, which they find untagged only, as `yaml_serde`'s did:
      `value.get(0)` is `value[0]` or `Sequence::get`, and a key of another kind, or a tagged
      one (`!t k: v`), is found by iterating. Indexing takes `&str`, `&String` or an integer,
      not a `Value` or an owned `String`.
    - `mapping["missing"]` is null, where `yaml_serde` panicked, and `mapping[key] = value` adds
      a missing key, where it panicked. Assigning a key on a tagged null makes a mapping with
      that tag, where `yaml_serde` panicked (it made a mapping of an untagged null only).
      `Mapping::remove` keeps the order of the other keys, where `yaml_serde`'s moved the last
      key into the removed one's place.
    - `Mapping` has no `entry`, `retain`, `clear`, `with_capacity`, `reserve`, `capacity`,
      `shrink_to_fit`, `swap_remove`, `shift_remove` (its `remove` is one), `remove_entry` or
      the other `*_entry` removals, `into_keys` or `into_values`. Of the public types of
      `yaml_serde::mapping`, `Iter`, `IterMut`, `IntoIter`, `Keys`, `Values` and `ValuesMut`
      are `MappingIter`, `MappingIterMut`, `MappingIntoIter`, `MappingKeys`, `MappingValues`
      and `MappingValuesMut`; `Entry`, `OccupiedEntry`, `VacantEntry`, `IntoKeys` and
      `IntoValues` have no counterpart.
    - `Sequence` is a type of its own, not a `Vec<Value>`. It has `len`, `is_empty`, `get`,
      `get_mut`, `push`, `pop`, iteration and indexing by position, and converts from a
      `Vec<Value>`, but has no other `Vec` or slice method, such as `first`, `insert`, `sort` or
      `&sequence[0..1]`; collect its values into a `Vec` for those.
    - `&value == 1` does not compile, where `yaml_serde` compared a reference with a number
      (write `*value == 1`), and values and mappings have no order (`<`). `Value::from` takes
      a `Vec<Value>`, not a `Vec` or slice of another type or a `Cow<str>`, and a `Sequence`,
      not a `Value`, is collected from an iterator.
    - Neither `Value` nor `&Value` is a serde `Deserializer`, and `Value` is not
      `IntoDeserializer`, so decode one with `from_value`. `{:?}` prints ayaml's form, such as
      `Number(Number(PosInt(1)))` where `yaml_serde` printed `Number(1)`.
    - `Error` has a message and a line and column, without the byte offset.
    - `to_string` writes ayaml's layout: a string that needs quoting is double-quoted where
      `yaml_serde` single-quoted it (`"1"`, `""`, `"x: y"`), and a tagged struct variant puts
      its tag on a line of its own under the key (`e:`, then `!C` and `x: 2` indented), where
      `yaml_serde` wrote `e: !C` with the fields under it.
  - `alint_core::template::render_mapping`, `render_value` and `render_nested_options` are no
    longer public, and neither are `alint_dsl::reject_command_rules_in`,
    `reject_fix_promotion_in`, `reject_fix_promotion_templates_in`,
    `reject_spawning_fix_op_templates_in`, `reject_spawning_fix_ops_in` and
    `reject_spawning_templates_in`. The loader still runs them on every configuration.
- **Breaking (library API): `Violation`, `Context`, `RuleSpec`, `NestedRuleSpec` and
  `WalkOptions` are `#[non_exhaustive]`** (#518, ADR-0032 decision 3, ADR-0038), **and so are
  `alint_lsp::LspOptions` and `alint_dsl::LoadOptions`** (ADR-0046 decision 6). Adding a field
  to them is no longer a breaking change, and code outside their crate can no longer build them
  with a struct literal or a functional record update (`..other`). Build them with the
  constructors and setters listed under Added; the fields stay public to read and to assign, and
  a pattern that destructures one needs `..`.
- **`explain` and `alint rules show` label a kind's selector fields `selector fields:`** in human
  output (it was `selectors:`), so the line is not read as a version 2 config's named selectors.
  The JSON key `selectors` is unchanged.
- The version 1 JSON Schema's descriptions follow the changes above: a path resolves in the walked
  tree, `git_tracked_only` sees a tracked file only an ignore file hides, `allow_out_of_root`
  describes its top-level-only reads, fixes and local `extends:` scope, and `require:` and
  `normalize:` describe their value forms only. No key or type changed.
- Dependencies updated: `adotenv` 1.5.4, `saphyr-parser` 0.3.4, `jsonschema` 0.61.2, and `rustix`
  1.1.6 (RUSTSEC-2026-0486 did not affect alint, which uses only `getuid`/`geteuid`).

### Performance

- A `key_parity` whose `peers:` glob starts with literal directories (`locales/*.json`) tests only
  the files under them, where it tested every file in the tree.
- **A rule nested in `for_each_dir` or `for_each_file` whose `paths:` is a glob below the iterated
  directory (`{path}/*.md`) reads only that directory's entries,** where it tested every entry in
  the tree on every iteration, so its time grew with the square of the number of directories (0.49 s
  at 2,000 directories, 4.50 s at 8,000). About 40 kinds, `key_parity`'s peers and
  the per-file rules narrow this way, and so does the search for the directories a
  `registry_paths_resolve` glob entry matches (on Windows too, where the entry is joined to its
  registry's directory with `\`).
- **YAML files over 1 MiB share a budget of 32 MiB of input parsing at once** (ADR-0042). At the
  32 MiB input cap, one YAML parse peaks at 0.75 to 1.54 GiB of RSS with ayaml 0.2.2 (down from
  4.5 to 6.2 GB with ayaml 0.1.2), and every worker thread could run one. A parse of a larger file
  now waits until its size is free in the budget (or, when its aliases expand, a 32nd of the
  expanded size, if that is larger: sixteen 500 KB files of aliases peaked at 3.1 GB before the
  budget), so at most one file at the cap parses at a time; a file of 1 MiB or less waits only when
  its aliases expand past 32 MiB, and findings are unchanged. The budget bounds the parses in
  flight, not the trees they leave: a tree that copies what aliases name lives outside it while a
  rule holds it, up to the alias guard's 256 MiB each (#582). The `yaml_path_*` rules,
  `json_schema_passes`, the cross-file rules' `extract:`, `pinned_references` and the structured
  fixes all take from it, and so does `alint_core::Format::parse`, which may now wait for another
  thread's YAML parse.
- **A JSONC file (a `.json` file with comments or trailing commas) is read from one copy of its
  text without them,** where the copy was made in two passes that held the text four times over as
  characters in between. Checking an 8 MB JSONC file with a `json_path_equals` rule peaks at 37 MB
  RSS, against 70 MB, and runs half the instructions; 3,000 small `tsconfig.json` files run 13%
  fewer. What the copy holds is unchanged, so every rule reads the same values.
- **YAML parsing uses less memory than in 0.22 across every measured document shape** (ADR-0042,
  ADR-0044). Decoding into the structured rules' JSON tree, ayaml 0.2.2 peaks at 0.517x to 0.965x
  of libyaml's peak heap across all nine measured shapes (21.8 to 21.9 bytes of heap per input byte
  on 37 KB to 4.4 MB OpenAPI documents, against 37.3 to 37.8 for libyaml), meeting ADR-0042 and
  ADR-0044's 1.2x release bound on every shape. End-to-end release checks on OpenAPI documents
  (50 x 37 KB, 1.1 MB and 4.4 MB) run 0.6% to 8.1% faster at 0.74x to 0.81x peak RSS; 1 MB
  block-word sequences run 8.8% faster at 0.91x RSS, while dense 1 MB `kN: 1` block and flow
  mappings run 10.8% and 26.1% slower at 0.76x RSS.

### Fixed

- **A rename under `--changed` changes both of its paths.** Git's rename detection listed a
  renamed file under its new path alone, so neither `--changed` (staged) nor `--base`
  (committed) saw the old path change: after `git mv LICENSE LICENSE.txt`, a `file_exists` rule
  for `LICENSE` was skipped, and so were `file_absent`, `key_parity`, `max_files_per_directory`
  and `no_case_conflicts` rules whose `paths:` matched only the old path. The changed set now lists
  both paths, as it lists a deleted file and a new one. `pair_changed_together` and the
  `when_changed:` of `changeset_requires_path` read the same set, so a file renamed out of their
  globs now counts as changed there too. `changeset_requires_path`'s `add_glob:` detects renames
  itself, whatever `diff.renames` says, so a moved file does not count as an added one and a copied
  file does; with `diff.renames: false` a move counted as an addition, and with `copies` a copy did
  not. After `git mv LICENSE LICENSE.txt`, `fix --changed` recreates `LICENSE` when its
  `file_exists` rule has a `file_create` fix, and `pair_changed_together` counts a `then_changed`
  file renamed away as changed, as it counts a deleted one.
- **SARIF tells apart findings that share a fingerprint**
  ([ADR-0047](docs/adr/0047-sarif-tells-apart-a-fingerprint-repeated-within-a-run.md)). A
  `constraint` finding's fingerprint leaves its path out, as does a version 1 finding whose path
  the environment chose, so several findings of one rule could carry one `partialFingerprints`
  value, which GitHub code scanning may merge into one alert. Findings that share a fingerprint in
  a run are now numbered by the path, line and column they show, then rule and message, baselined
  ones before live ones. The first keeps the canonical fingerprint the baseline file uses, and
  each other carries one derived from it and its number. A repeat keeps its fingerprint while the
  repeats that sort before it stay; one that sorts before it, new or gone, shifts it, baselined or
  not, since the baseline suppresses a fingerprint's first findings in that order, up to its
  count, so a new one can take a baselined one's place. GitLab output numbers its findings the
  same way (before, it numbered them in the order it wrote them), so without a baseline the two
  formats agree finding by finding. A run in which no fingerprint repeats is written as before.
- **An extended config is named by its path under the repository on every platform.** Under a
  root reached through a symlink, from a relative `--config`, or on macOS (where `/var` is
  `/private/var`), a config file an `extends:` entry reads was named by its file name alone
  (`base.yml`) in messages; it is now named as a plain run names it (`rules/base.yml`).
- **An unknown rule field's "did you mean" suggestion costs time and memory in proportion to the
  key's length.** A long key holding backticks made the suggestion build a full edit-distance
  table against itself (an 8,000-character key took half a second and 498 MB); the distance now
  stops past two edits, and suggests the same fields.
- **A `key_parity` rule nested in the `require:` of `for_each_file`, `for_each_dir` or
  `every_matching_has` compares its files.** It used to stop on every iteration with
  "internal error ... key_parity requires the relation read cache", unless another rule in the
  configuration happened to enable the run's relation caches. `--only <parent>` and
  `explain --trace <parent>` failed even then (#502). Each iteration's relation rules
  (`key_parity`, `cross_file`) now read through caches of their own, which nothing outlives. With
  a top-level relation rule configured as well, a nested one used the run's caches instead: every
  iteration's files and decoded rows stayed in memory for the whole run and counted against its
  row limit, so a long iteration could fail with "relation limit `rows_per_run` (5000000)
  reached: ... rows or values".
- **A nested `key_parity` whose `reference:` or literal `peers:` takes its key format from a path
  token loads.** A configuration with `reference: "{path}"` or `peers: "{dir}/fr.{ext}"` in a
  `require:` block was rejected with "cannot infer the key format", because nested rules are
  checked at load with a placeholder path that has no extension. The format, and the
  `strip_root` and `ignore_keys` checks that depend on it, are now made in each iteration, and an
  iteration whose file has no key format is a `nested_rule_build` analysis failure whose message
  lists the supported names. What holds for every iteration is still a load error: a literal
  extension such as `reference: "{dir}/strings-{stem}.xml"`, a name no path token renders (even
  one spelled like the load-time placeholder, `reference: "data/_stem_"`), a literal peer whose
  key model does not allow `strip_root` or an `ignore_keys` pattern, and
  `peers: "{dir}/{basename}"` against `reference: "{path}"` (#504).
- **On Windows, a nested `key_parity` tells its templated `reference:` apart from its peers.**
  `{path}` and `{dir}` render with the OS separator, so `reference: "{path}"` became
  `apps\web\en.json`, which `key_parity` read as one file name where the walk lists
  `apps/web/en.json`. A `peers:` glob that selected only the reference compared it with itself
  and passed, where Linux and macOS report that the glob matched no other file;
  `peers: "{dir}/en.json"` against `reference: "{path}"` passed, where they report that the peer
  is the reference; and messages quoted the reference with backslashes. On Windows, `key_parity`
  now reads a `\` in `reference:` and in a literal `peers:` path as a separator, as globs already
  do.
- **Nested rules of every kind are checked at load with a placeholder for each path token**
  (`{stem}` as `_stem_`), where 0.22 rendered `{dir}`, `{ext}` and `{parent_name}` empty. A
  template whose empty rendering was invalid now loads, such as `reference:
  "i18n/{parent_name}.json"`, `command_idempotent`'s `workdir: "{dir}/.."` or `json_path_equals`'s
  `path: "$.{dir}"`, and a load error quotes the template as written (`{dir}/[{stem}`) (#504).
- **`fix --dry-run` agrees with `fix` on a create the file system spells differently.** Where
  the file system ignores case, creating `Docs/x.txt` beside an existing `docs/` writes
  `docs/x.txt`, which the walk reports under that spelling, so the rule that asked for
  `Docs/x.txt` still fails and `fix` says so. The dry run reported the create as applied; it now
  says the path exists under another spelling.
- `alint test` redacts a runner error's paths correctly when the test package and the temporary
  directory overlap. It replaced the temporary directory first, so a package under `TMPDIR`
  reported `<temporary-directory>` with the rest of its path instead of `<policy-test-package>`,
  and a temporary directory such as `/tmp` that also occurs inside the package path, as in
  `/home/me/tmp/policies`, left the part of the package path before it visible. The longer path
  is now replaced first (#494).
- **A structured YAML fix no longer edits through an alias.** Since v0.17.0, a `set_value` or
  `remove_value` whose path ran through an alias, as `$.job.image` does with `job: *base`,
  edited the anchored node's definition: every other alias of it changed too, and the re-verify
  checked only the path fixed. Such a path now declines, as one that ends at an alias did.
- **`set_value` on an anchored YAML scalar keeps the anchor.** Since v0.17.0, the replaced bytes
  began at the scalar's anchor, so `a: &x 1` set to `5` became `a: 5`, and a later `*x` no longer
  parsed; a quoted scalar after an anchor also lost its trailing comment. The value is now replaced
  after the anchor (`a: &x 5`) when no alias names it; when one does, the fix declines, since the
  edit would change every alias too. A core-schema tag is still replaced with the value, which
  carries its own type (`!!str 5` set to `6` becomes `6`), and a core tag written before an anchor
  declines.
- **`set_value`, `remove_value` and `replace` on YAML, JSON and TOML change only what they aim at.**
  A fix now reads the whole file again after its edits and applies only when the result is the
  original tree with the intended change. In YAML, removing a line could add a blank line to a `|+`
  block scalar, attach an explicit `?` key or a key-less `:` entry to another value, or leave a bare
  `?`; in TOML, removing the only table under a parent known only from a dotted header dropped the
  parent. When two rules' fixes together would change more than their targets, the later rule waits
  for the next fix pass, also beside another rule's free-form `replace` in the same file. A YAML set
  or removal at a node an alias or merge key names, or on a path through one, declines. Removing a
  YAML block mapping's last entries writes `{}`, where it left the key with no value, which reads as
  null ([ADR-0044](docs/adr/0044-untrusted-runs-git-as-a-process-and-a-wider-memory-gate.md)).
- **In a nested rule, a token's value is substituted once.** In a rule nested two or more levels
  deep, the inner iterations rendered the outer one's values again, and a nested `pair`'s
  `partner`, `unique_by`'s `key`, `cross_file`'s `register_as` and `command`'s arguments, which the
  iteration had rendered, were rendered again by the rule for each entry it checked. A file or
  directory named like a token (`x{stem}`) was expanded twice: under `packages/x{stem}`, a nested
  `pair` looked for `packages/x{stem}/api.h` as `packages/xa/api.h` and passed when that file
  existed, `unique_by` read one shared key as several, `key_parity` reported a missing peer, and
  messages showed the wrong path. The outermost iteration now substitutes every token, and a rule
  it built takes these options as text. In the same way a nested `file_graph` takes a `$` in a
  value as text in `derive_target.to` (a directory `a$1` is not a capture reference), and a
  nested rule that fills `{{ctx.*}}` placeholders in its message (`pair`, `unique_by`,
  `dir_contains`, `dir_only_contains`, `key_parity`, `git_blame_age`) shows a name spelled like
  one (`x{{ctx.primary}}`) as it is.
- **A nested `command`, `command_idempotent` or `generated_file_fresh` passes a name that starts
  with `-` as a path.** The iteration rendered `{path}` in their `command:` arguments without the
  guard `command` gives the files it matches (L13, 0.14.0), so a directory named `--write`
  reached the tool as the option `--write`; it is now `./--write`.
- **`key_parity` compares a YAML key written as a block scalar (`? |`) or left empty,** at the place
  its text starts, where it dropped such a key.
- **`key_parity` holds each key path of a deep, wide JSON or YAML file once, in the file's decode,
  and charges the key paths of every file a run decodes to one byte budget** (`bytes_per_run`,
  1 GiB). The extraction copied a key's path into every key below it; the comparison copied each
  path again for its key table, its leaves, the reference's keys, each comparison's rows and the
  analyzer's input, none of them charged; and each decode charged its key paths to a budget of its
  own, so every peer added its own. A 320 KB reference 120 levels deep with 30,000 keys and one
  identical peer, which peaked at 2,300 MiB and took 12 s in a release build once the extraction
  stopped copying, now peaks at 437 MiB and takes 2.6 s. The key paths every decode builds (a
  relation's too) and the tables `key_parity` keeps of its files take their bytes from the run's
  decode cache, so a run past the budget ends with an analysis failure rather than exhausting
  memory. Each decode still holds every key's whole path, about 300 MB for a 1 MB file 120 levels
  deep with 100,000 keys (#569).
- **An `extends:` list that names one source many times loads it once,** and the work of a load's
  `extends:` entries is bounded: 2,000 entries naming one 2,000-rule ruleset took 152 s to load. An
  entry is charged the source it loads, before `only:` or `except:` narrows it and at least one
  entry for an empty one, and each merge, and each `.alint.d/` drop-in's, what it brings in
  (#580), up to 4,194,304 entries in all; each entry is also charged its source's
  size, up to 64 MiB in all (see the
  [configuration limits](docs/site/configuration/index.md#limits)). A remote source named by other
  URLs with one hash, and a bundled ruleset named with other `only:` or `except:` lists, are parsed
  once per load. Before, 40 entries of one cached 15 MiB remote source with other queries took
  157 s, 60 entries naming a one-rule file whose rule had 50,000 keys took 41 s, the budget
  admitted about 230,000 entries of a bundled ruleset with a different `only:` list each (about
  27 minutes), and 200 empty drop-ins on 50,000 `ignore:` patterns took 10 s; each is now
  refused, or loads in a few seconds. A config the environment chose that names one
  file in 3,000 spellings took more than 100 s, and 2,000 entries checked against 20,000
  `trusted_extends:` URLs took 23 s; both now load in under a second. A version 1 configuration
  that fails to decode is no longer parsed a second time to tell its version.
- **`alint test` no longer lets the host's global Git excludes file hide a test's nested configs.**
  The walk that finds `nested_configs:` files read `$XDG_CONFIG_HOME/git/ignore` and a
  `core.excludesFile` that `~/.gitconfig` names, where the test's own walk does not, so a host whose
  excludes listed `.alint.yml` or `*.yml` dropped those nested rules from the test. `alint check`
  still honours the file.
- **With `allow_out_of_root: true`, a config outside the root is named by its path from the root
  (`../b/rules.yml`, `..\b\rules.yml` on Windows) in listings and in errors found in it,** where
  they showed its file name alone; a refusal of an `extends:` entry still names it as the entry
  spells it.
- **Additional pre-v0.23 audit fixes:**
  - `baseline::order_key` and SARIF/GitLab `OccurrenceKey` share canonical slash-separated path
    ordering so repeated fingerprints on paths like `a.json` vs `a/b.json` never swap under
    `--baseline`.
  - `constraint` `visit_sensitive_paths` visits matched trigger paths (and sensitive-by-default
    formats) rather than over-suppressing unrelated files or missing sensitive relation inputs
    during `fix --diff` and `path_contains_sensitive_evidence`; `TomlRowsAdapter` enforces
    `MAX_STRUCTURED_BYTES` before parsing; and `unique_table` selects the first key column whose
    provenance differs when reporting collisions.
  - Consecutive duplicate `extends:` entries charge the entry budget, reuse cached resolved local
    targets per URL, and avoid duplicating `config_files`.
  - `structured_fix::exact::intended` deduplicates repeated sequence removals; TOML `sync_from`
    verifies edits structurally; `SpanIndex::read` enforces `MAX_STRUCTURED_BYTES`;
    `yaml::root_is_a_block_collection` scans events lazily past `%YAML`/`%TAG` directives;
    `yaml_depth` allows column-0 top-level block scalars and ends them at document markers; and
    `extract::yaml_rows` preserves block-scalar start offsets on non-ASCII UTF-8 files.
  - `git.rs` strips trailing slashes from untracked nested repository paths, and `git_repository`
    keys ownership verdicts by `(work_tree, git_dir)` and bypasses the cache when custom
    environment overrides are supplied.
  - `xtask docs-export` strips unreleased `alint:since` blocks on concept pages and unwraps
    released markers consistently; `docs-bundle.yml` drops `installation.md` from the overlay;
    `release.yml` validates the VS Code extension (`npm ci`, `npm-audit.sh`, `check-types`,
    `build`) in preflight; and `editors/nvim/lsp/alint.lua` prefers `.git` over nested `.alint.yml`
    in `root_markers`.
- **`remove_value` and `replace` on many YAML matches take linear time.** Since v0.17.0, each
  matched node parsed the whole file again to find its bytes, the engine parsed the result again
  to verify each edit, and each edit's splice moved the rest of the file: 2,000 matches took
  3.8 s, 5,000 took 25 s, and 10,000 did not finish in two minutes, in `fix` and in
  `check --format sarif`. One parse now finds every match's bytes, each verification pass parses
  the result once (for every structured format), and the edits are spliced in one pass; 10,000
  matches take under two seconds.
- **A configuration with `requires:` loads exactly when it loads without it.** Since v0.20.0,
  the `requires:` check read the configuration with a second YAML parser, so text alint decodes
  but that parser refuses, such as a tab after a key's colon (`a:\tx`), failed with that
  parser's message. The check now reads the events alint decodes. A configuration that fails to
  decode is parsed again, to say why a `requires:` is not a literal, only when it spells
  `requires`, so a large one that fails is no longer parsed three times.

### Removed

- **Breaking: a baseline no longer matches the fingerprint v0.21 wrote for a hidden-path finding**
  (#500, ADR-0030 decision 1). A finding's path is hidden when a field that chooses files was read
  from the environment. v0.22 gave such findings a fingerprint without the path and, for one minor
  release, still matched the v0.21 form, which hashed the path. If your baseline file was last
  written by v0.21 or earlier, `check --baseline` and `fix --baseline` now treat those findings as
  new and their entries as stale, and the language server shows those findings again. Regenerate
  the file once with `alint baseline --accept-new`; the flag is needed because the findings count
  as new. A file written by v0.22 is unaffected.
- **Breaking (library API):** `alint-core` drops `baseline::Fingerprints` and
  `baseline::fingerprints`, which existed only to match the v0.21 form. `baseline::apply` takes a
  fingerprinter that returns a `String` again, such as `baseline::fingerprint`.

## [0.22.1] - 2026-11-11

v0.22.1 changes how alint installs, not what it checks. The npm package no longer downloads the
binary when it is installed: `@asamarts/alint` runs it from a per-platform package that npm, pnpm,
Yarn and Bun install as an optional dependency, so it now installs with install scripts disabled,
under Bun and Yarn Plug'n'Play, offline and from a registry mirror. One behavior to note:
installing with optional dependencies omitted (`--omit=optional`) now leaves an `alint` that exits
1 with a reinstall hint. Windows on ARM gets a native binary on every channel. Findings and fixes
are unchanged.

### Added

- **Windows on ARM.** Each release builds a native `aarch64-pc-windows-msvc` binary: a release
  tarball, a `win_arm64` wheel on PyPI, the `@asamarts/alint-win32-arm64` npm package, and the
  binary the VS Code, JetBrains and Zed extensions download. Windows on ARM previously ran the x64
  binary under emulation through npm and had no other native install.

### Changed

- **The npm package no longer downloads anything at install time** (ADR-0031). `@asamarts/alint`
  now runs the binary from a per-platform package (`@asamarts/alint-linux-x64`,
  `@asamarts/alint-darwin-arm64`, and so on) that it lists as an exact-version optional
  dependency, so your package manager installs only the one for your OS and CPU. No package has an
  install script, so the npm package now installs under `--ignore-scripts`, with Bun and with
  pnpm's default script blocking (no `pnpm approve-builds` needed), with Yarn Plug'n'Play, offline,
  and from a registry mirror. It also keeps working if a GitHub Release is deleted. Each platform
  package carries the binary from the release tarball, and npm provenance covers it
  (`npm audit signatures` verifies it).
- Installing the npm package with optional dependencies omitted (`--omit=optional`,
  `--no-optional`) now leaves an `alint` command that exits 1 and says how to reinstall, where it
  used to download the binary. Set `ALINT_BINARY` to run a binary of your choosing instead, on any
  platform. `ALINT_SKIP_INSTALL` no longer does anything.
- `.env` files are parsed with adotenv 1.4.0, up from 1.3.0. Its parser and edits are unchanged; 1.4.0
  adds a reference-rename edit that alint does not use, so findings and fixes are the same.

## [0.22.0] - 2026-11-07

v0.22 completes general-linter phase 2. `cross_file` reads the values it compares through the
relation layer that `key_parity` brought in v0.21, and the old reader is gone. Findings are
unchanged: a replay of the 110-repository case-study corpus against v0.21.2 shows no
differences. Two changes need attention. Library users: `FixContext`, `Config` and
`CompiledNestedSpec` in `alint-core` are now `#[non_exhaustive]`, so build them with their
constructors and `with_*` setters. Baseline users: a finding whose path is hidden gets a
fingerprint without the path; v0.22 still accepts the form v0.21 wrote, but v0.23 will not, so
re-run `alint baseline` before upgrading to v0.23. `fix --dry-run` also predicts a create
through a dangling symlink, and predicting many creates costs far less.

### Changed

- `cross_file` reads the values it compares through the relation layer: the run's read and
  decode caches and the extract adapter `key_parity` already uses (ADR-0025 decision 3, gate 1).
  Findings are unchanged. A file the configuration names (`source.file`, a `targets` entry) is
  read and decoded once per run for every rule that names it; a file a glob matches is still
  read once per rule and not kept, so memory does not grow with the matches. A glob-matched file
  whose name is not UTF-8 reads through the same path.
- `alint fix` clears the relation caches after a rule's fix lands, so a rule evaluated later in
  the same pass reads a file that fix created, removed or renamed as it now is.
- **Breaking (library API):** `alint-core`'s `FixContext`, `Config` and `CompiledNestedSpec`
  are `#[non_exhaustive]` (ADR-0030 decision 2), so adding a field to them is no longer a
  breaking change, and code outside `alint-core` can no longer build them with a struct
  literal. Build a `FixContext` with `FixContext::new(root, dry_run)` and a `Config` with
  `Config::new(version, rules)`, then the `with_*` setters for the other fields; a
  `CompiledNestedSpec` comes from `CompiledNestedSpec::compile`, as before. The fields stay
  public to read.
- **Baseline fingerprints of hidden-path findings (ADR-0030 decision 1).** A finding whose path
  is `<redacted>`, because a field that chooses files was read from the environment, gets a
  fingerprint that leaves the path out, in the baseline file, SARIF `partialFingerprints` and
  GitLab `fingerprint`. Holding one of those files no longer lets anyone confirm a guessed path.
  A rule's hidden-path findings share the fingerprint, and a baseline grandfathers them by count.
  `check --baseline`, `fix --baseline`, the language server and `alint baseline`'s regeneration
  guard still accept the fingerprint v0.21 wrote for such a finding, so existing baseline files
  keep working; `alint baseline` rewrites them in the new form without `--accept-new`. v0.23
  will drop the old form, so re-run `alint baseline` before upgrading to it. A baseline file
  v0.22 writes does not grandfather these findings in v0.21. In `alint-core`,
  `baseline::apply` takes a fingerprinter that returns `baseline::Fingerprints` (from
  `baseline::fingerprints`) to match both forms; one that returns a `String`, as before, matches
  only the new form.
- The `.env` parser behind the `dotenv_path_*` rules, `key_parity` and dotenv fixes (`adotenv`)
  moves from 0.10.4 to 1.3.0. Findings and fixes are unchanged: alint built with either version
  gives identical `check` and `fix` output on 50,000 generated `.env` files, of which about
  18,000 parse and 10,000 take a fix.

### Performance

- `fix --dry-run` and `fix --diff` read the walk's ignore files once per pass, not once per
  predicted create (ADR-0030 decision 4): the overrides, the repository boundary, the ignore
  files above the root and the global excludes file once, and each directory's ignore files once.
  In three 100-file repositories, a dry run of 20 creates spent 85% to 91% fewer instructions
  predicting them (at most 6.8M to 0.6M). The whole run saved 9% to 40%, depending on how much
  of it prediction took. A single create costs the same.

### Removed

- The test-only `shadow-ir` feature of `alint-rules`, and the shadow implementation of
  `cross_file` `equals` it enabled, now that the relation layer is the implementation.

### Fixed

- `fix --dry-run` and `fix --diff` predict where a create lands when a symlink on its path leads
  nowhere until the create's own directories exist (ADR-0030 decision 4). A path written as
  given, which `allow_out_of_root` permits, such as `../repo/made/../pointer/p.txt` in a
  repository `repo` with `pointer -> made`, makes `made` and lands in it; the dry run reported
  the create unresolved. This was the one difference v0.21.2's OS differential test found; the
  test now covers it, directly, through another symlink and through `..`.

## [0.21.2] - 2026-11-06

A fix release: `fix --dry-run` and `fix --diff` agree with `fix` on creates in more layouts, and
values read from the environment stay out of more of what alint prints. A create into a missing
directory, under a file or through a symlink is predicted as `fix` runs it, and a create written
outside the root no longer overwrites a file or writes through a symlink when a `..` follows a
directory it makes. More paths read from the environment show as `<redacted>`, hidden paths stay
out of redaction markers and log warnings, `fix --baseline` skips a grandfathered finding with
redacted evidence, and nested rules run 1.3% to 2% fewer instructions than in v0.21.1.

### Changed

- `ci/scripts/check-patch-compat.sh` deletes each previous crate's
  `Cargo.lock` before building it, checks with all features, fails unless
  every sibling it built against is this workspace's, and refuses a version
  that is not an earlier release on the workspace's caret line (the same
  `0.y`, or after 1.0 the same major version). The lock pinned the siblings at
  the previous version, so once the workspace version was bumped cargo ignored
  the `[patch]` and the check passed without testing anything. It also takes
  every earlier release in one run, finds the workspace from its own location
  rather than the current directory, and refuses a prerelease workspace
  version. It keeps each extracted crate out of any workspace around the
  scratch directory, builds in `target/patch-compat` unless `CARGO_TARGET_DIR`
  is set rather than in a scratch directory under `$TMPDIR`, and exits 2
  rather than 1 when cargo cannot resolve or download the previous crates.
  `ci/scripts/test-check-patch-compat.sh` exercises each exit code with
  `curl` and `cargo` stubbed.

- The YAML parser behind source spans, structured fixes and duplicate-aware key
  extraction (`saphyr` and `saphyr-parser`) moves from 0.2.2 to 0.3.3, the two
  still pinned together.
  - An empty block scalar at the end of a file now reads as an empty string
    instead of a newline.
  - A compact pair in a flow sequence whose value is a flow mapping
    (`[k: {a: 1, b: 2}]`) now reads as one mapping. The parser ended the
    mapping at its first comma and started a new one for the keys after it.
  - The loader stops after 100,000 nodes produced by aliases. alint loads a
    document that way only to place a `set_value`, `remove_value` or
    `yaml_path_matches` `replace` edit, after its own alias guard, which
    allows more, so such a fix now declines on a document whose aliases
    expand past that limit.

### Performance

- Nested rules run 1.3% to 2% fewer instructions than in v0.21.1, where each
  iterated entry's rule specification passed through one more call that copied
  it. In the deterministic micro-benchmark gate (Callgrind), `for_each_dir` is
  back within 0.5% of v0.20.1 and `every_matching_has` within 0.7%, with Rust
  1.99.1 and with 1.97.0, the bench record's toolchain. With 1.99.1, v0.21.1's
  `for_each_dir` was 2.2% to 2.4% slower than v0.20.1. The specification is now
  built in place, and a successful nested build returns straight from the
  builder.

### Fixed

- `fix --dry-run` agrees with `fix` again for a `dir_create` or
  `create_and_register` through a symlink. The rules these fix (`dir_exists`
  and `cross_file`) look for the path as written, but since 0.21.1 the dry run
  also accepted where the file lands, so a create through an ignored symlinked
  directory was predicted applied. A `create_and_register` member written as
  an absolute path, or with a `..` that leaves the root (`../repo/...`), is
  predicted unresolved, as the rule never matches it. A create written outside
  the root (`allow_out_of_root`) is predicted where it lands: through a
  directory it makes (`../repo/new/../x.txt` with `create_parents`), through a
  symlink after one (`../repo/new/../link/x.txt`), and, for an absolute path,
  through a symlink before a `..`, which the dry run used to cancel against it.
  An absolute path whose `..` cancels a real directory keeps the name written
  after it, so `<root>/sub/../alias/x.txt` is matched as `alias/x.txt`, as the
  walk lists it.
- `create_and_register` declines a member written with a backslash on Linux
  and macOS, where the backslash is part of a name, and says to write it with
  `/`. The rule looked for that name, but the fix created the member with `/`
  in its place, so the finding stood after every run while `fix --dry-run`
  predicted it applied.
- `file_create` with `create_parents: false` declines a file whose directory
  does not exist, and says to set `create_parents: true`. `fix` failed on the
  write instead, and `fix --dry-run` and `fix --diff` reported the create as
  applied. When another fix in the same run makes that directory (a
  `dir_create`, or a create with `create_parents`), all three report the create
  applied, and `fix` makes the file in the same pass or the next. The editor's
  quick fix no longer offers a create into a missing directory.
- `file_create`, `dir_create` and `create_and_register` decline in every mode
  when a file or a symlink that leads nowhere stands where a directory is
  needed (`README.md/x.txt`), saying the path is under a file. `fix` failed on
  the write, while `fix --dry-run` and `fix --diff` reported the create as
  applied.
- A create written outside the root that passes through a directory it makes
  before a `..` (`../repo/new/../dl`) checks for an existing file or a symlink
  where it lands. It checked the path as written, which does not exist yet, so
  it could overwrite a file or write through a symlink out of the root.
- A create written outside the root with a trailing slash (`../repo/y.txt/`)
  creates `y.txt`, as `fix --dry-run` predicted. `fix` failed with "Is a
  directory".
- `check --format sarif`, `agent` and `json --include-fixes` word their warning
  that a whole-file fix pushed a located edit out of the preview as "the fix
  preview", not a `--diff` preview nobody asked for. They build their proposed
  fixes with the same single pass as `fix --diff`.
- `fix --diff` no longer shows a create outside the root (`allow_out_of_root`)
  from a rule with redacted findings. It printed the file's path and content,
  which can hold a value read from the environment or set in a
  `sensitive: true` rule.
- A rule that reads the environment in a selector with a dotted name
  (`targets.files`, `source.file`, `require.forbidden_edges[].to`) now shows
  its paths as `<redacted>` in reports, `fix` and the baseline, as one reading
  `paths:` or `select:` does. The check compared the dotted name with the
  top-level field the environment marks, so it never matched.
- A nested rule whose `when_iter:` reads the environment shows its paths as
  `<redacted>`, as one whose `require:` reads it does, whether the value is
  interpolated (`{{env.X}}`), read when the expression runs (`env.X`) or
  carried in by a variable. The filter picks the entries the rule reports, so
  their names can spell the value.
- A redaction marker no longer hashes a path alint hides, so two findings that
  differ only in such a path get the same marker. The marker covered the
  finding's path before the path was hidden. This includes the markers a rule
  writes itself, such as a `dotenv_path_equals` mismatch, when a manifest
  scope hides the path, and the analysis-failure markers `fix` writes for such
  a rule.
- `fix --baseline` skips a grandfathered finding of a `sensitive: true` rule
  or one that reads a field from the environment, as `check --baseline` does.
  It fingerprinted the finding before redacting it, so the fingerprint never
  matched the baseline's and `fix` rewrote the file.
- `alint baseline`, `check --baseline`, `fix` and the language server no longer
  warn that they cannot read each directory a nested rule flags. The baseline
  fingerprint reads a file only for a finding on a line whose fingerprint
  uses the line's content, so it no longer reads a directory, the file of a
  finding with redacted evidence or a whole-file finding, and
  `check --baseline --format gitlab` reads each file once for both
  fingerprints. A file over the 256 MiB
  analysis cap, or one that grows past it while alint reads it, is reported
  through its finding, which hides such a path, without a warning that named
  it.
- Warnings and other log lines show `<redacted>` for a value the environment
  gave the configuration, such as the path of an unreadable file a fact read
  through `{{env.X}}`, and escape control characters, so a file name cannot
  start a forged log line or drive the terminal.
- A JSONPath, or a `when_iter:` or `when:` in a nested rule at any depth, read
  from the environment no longer hides an unrelated configuration error after
  it: the rerun that decides whether an error could quote the value masks it
  with one that still parses.
- `alint baseline`'s confirmation, and its error for a regeneration that would
  grandfather new findings, show `<redacted>` for a `baseline:` path read from
  the environment. `alint facts` does the same for a fact value that holds one,
  as a `custom:` command given it as an argument can print it.
- `alint explain` no longer prints a value read from the environment in a
  rule's `fix:` line. A fixer describes itself from the rule's fields, so a
  `file_create` or `dir_create` named the `paths:` entry, a `replace` quoted the
  `pattern:` and a `set_value` the `path:`, although `explain` showed those
  fields as `<redacted>`. The `fix:` line is now `<redacted>` whenever the
  rule's evidence is, in the human and JSON formats.

## [0.21.1] - 2026-11-05

A fix release: ignore files stop at the repository boundary as git's do, `fix --dry-run` agrees with
`fix` in many more layouts, and configuration errors are readable again without showing values read
from the environment. A `.gitignore` in a directory above the repository (a home directory kept in
git) can no longer hide the tree and make every rule pass, and a linked worktree honors the
repository's shared `info/exclude`. A configuration error is shown unless it could quote a value
read from the environment or set in a `sensitive: true` rule, and such values stay out of the
errors, warnings and listings that printed them.

### Security

- The locked `pulldown-cmark` is now 0.13.6, fixing RUSTSEC-2026-0433
  (quadratic parsing of nested image wikilinks with `ENABLE_WIKILINKS`). alint
  does not enable wikilinks, so it was not exposed.

### Changed

- Ignore files stop at the repository boundary, as git's do; a directory holding
  `.git` or `.jj` is a repository. A `.gitignore` or
  `info/exclude` in a directory above the repository, such as a home directory
  kept in git, no longer applies, so a `*` there can no longer hide the whole
  tree and make every rule pass. A repository's `.gitignore` also stops at a
  nested repository or submodule below it, as in git: files there that only
  the outer file excluded are now linted, so give the nested repository its
  own patterns or list them under the config's `ignore:`. A root linted from a
  subdirectory still reads its repository's ignore files above it. Outside any
  repository, no ignore file above the linted root applies, and a repository
  nested below the root is not a boundary. `.ignore` files above the
  repository still apply, as they do for ripgrep. `fix --dry-run` predicts the
  same walk. See ADR-0028 and ADR-0029, which lists where the walk still
  differs from git: a submodule's `info/exclude`, anchored patterns in the
  global excludes file, a linted root inside an ignored directory, and the
  excludes file `~/.gitconfig` names, which the walk still reads when
  `GIT_CONFIG_GLOBAL` names a file that sets no `core.excludesFile`.
- `dir_create` requires the directory in `paths:` in normal form, such as
  `docs/adr`. A configuration with `docs/`, `./docs`, `docs//adr` or `/docs`
  now fails to load. The rule never matched those forms, so `check` failed
  even with the directory present and `fix` could not resolve it.
- The walker uses `ignore` 0.4.34. In a linked git worktree
  (`git worktree add`), alint now honors the repository's shared
  `.git/info/exclude`, as it does in the main checkout; before, files excluded
  only there were linted. The same applies to a linked worktree nested inside
  the linted tree. A worktree made with `--relative-paths` is honored only when
  alint runs from the worktree's root. A checkout whose `.git` file names no
  common directory, such as a submodule or one made with
  `git init --separate-git-dir`, still has its exclude file ignored.

### Performance

- Nested rules (`for_each_dir`, `for_each_file`, `every_matching_has`) look up
  which of a nested rule's options are globs with a `match` instead of scanning
  a table for every iterated entry. Adding `key_parity` to that table in v0.21.0
  stopped the compiler unrolling the scan, which then cost about 300
  instructions per entry and made `for_each_dir` 3% slower than v0.20.1 in the
  release's deterministic micro-benchmark gate. That change alone brought it
  back within 0.5% of v0.20.1, but later fixes in this release added about 2%
  again, so v0.21.1's `for_each_dir` is 2.2% to 2.6% slower than v0.20.1 in
  the same gate.

### Fixed

- `alint fix --dry-run` agrees with `alint fix` on whether a file a fix would
  create is walked in more layouts. It reported such a fix as unresolved when
  the file's directory has an ignore file that excludes all but a few entries
  (`*`, `!.gitkeep`), because it applied that file to the directory itself,
  which the walk does not. It also missed exclusions the walk applies: ignore
  files in directories above the linted root (a monorepo package linted from
  its own directory) and the `info/exclude` of a repository or linked worktree
  nested inside the tree. It now also predicts that the walk leaves out a
  directory `dir_create` makes in an ignored path, a created ignore file that
  excludes itself (a `cache/.gitignore` of `*`) or a file another fix in the
  same run creates, and a file created below a symlink the walk does not
  follow (a loop, or one that leads out of the root). A `file_create` path
  with `..` is checked where the file is written (`out/../README.md` is
  `README.md`), not through each component. A create the walk leaves out
  still counts as resolving a `file_exists` rule with `respect_gitignore:
  false`, which looks for its literal path on disk (the `.bazelversion`
  pattern); the dry run used to report it unresolved. It now predicts that a
  created file does not resolve a `git_tracked_only` rule unless git tracks
  the path, and that a file `file_create` or `create_and_register` writes
  outside the root, which `allow_out_of_root` permits, is never linted. A
  create is judged by where the file lands as well as by its path as written,
  so one through an in-tree symlink (`sub/up/x.txt` with `sub/up -> ..`), or by
  an absolute or `..` path that leads back into the root, resolves the rule
  as the real run finds. An ignore file created through a symlinked directory
  applies where it lands, its content is read under `allow_out_of_root` as
  the real fix reads it (a `content_from` outside the root), and of two fixes
  that create the same ignore file the first one's content counts. For a
  sensitive rule (`sensitive: true`, or any field other than `policy_url` read
  from the environment) the reason a create would not resolve the violation
  says "the target" instead of printing the path, which its redacted summary
  already leaves out. `fix --diff` makes none of these predictions, and its
  help no longer says its exit code matches a real `fix`.
- A line that cannot be parsed in an ignore file above the linted root (`a\`
  in a home directory's `.gitignore`), or a `.git` file there that alint
  cannot read, no longer stops `alint check` and `alint fix` with a walk error
  (exit 2). The line is skipped, as git skips a pattern it cannot parse (and
  above the repository git reads no ignore file at all), and as the same line
  in the linted root's own `.gitignore` already was.
- A configuration error is no longer replaced with `<redacted>` just because the
  rule reads a value from the environment or sets `sensitive: true`. alint now
  repeats the failed step with those values masked and shows the error when the
  masked run fails the same way, so the message cannot quote one of them: an
  unsupported field, an unknown option or a misplaced `scope_filter` predicate
  is readable again, in `alint check`, `validate-config`, the language server
  and `alint test`, including inside a nested rule (`require:`). Numbers and
  booleans are masked as well as strings, because interpolation turns a value
  that reads as one (a 12-digit account id) into a number or a boolean. A
  number is masked by moving it one step, up and then down, so a value at one
  end of its range (a `threshold` of `1.0`) still passes its range check and
  leaves an unrelated error readable; numbers at opposite ends of their ranges
  in one rule leave it redacted, as does a value for an option with a fixed
  set of choices (`style: tabs`) placed before the error. An error that quotes a possibly secret
  value, such as an invalid regex read from the environment, stays redacted
  and now says why. The same applies, with the same masks, to a whole
  configuration file that reads the environment. A rule whose `fix:` block or manifest-path `scope_filter`
  may hold such a value cannot be masked, so its errors stay redacted. An
  `extends_template:` read from the environment no longer appears in the
  unknown-template error, and an error about a field the template supplies
  names "a template" instead of its id.
- A value read from the environment into a top-level field (`extends:`,
  `ignore:`, `facts:`, `baseline:`, `version:`) is no longer printed when the
  field causes an error, such as an invalid `ignore:` glob or a missing
  `extends:` target. alint and the language server now show `<redacted>` in its
  place, also where the error prints the value trimmed, normalized as a path,
  as a remote `extends:` URL without its credentials, query and fragment, or
  one line at a time, and in the warnings that name it (an empty manifest
  set). A fact whose pattern holds one raises no pattern hint. An `only:` or
  `except:` read from the environment leaves its entry's URL readable. `alint facts` shows a fact id read from the
  environment as `<redacted>`, and the message for a file over
  `fix_size_limit` no longer prints the limit. A value inside a longer word
  takes the word with it. The same holds for a nested config's `version:` and
  for a nested config's rule whose `paths:` from the environment is rejected
  for leaving its directory. A boolean, or a value that an error prints in
  fewer than four characters, is still printed: removing every occurrence
  would garble the message, and it is too short to be much of a secret.
- A git ref read from the environment into `scope_filter.changed_since`,
  including a nested rule's (`require:`), or into `git_commit_message`'s
  `since:` with the deprecated `${VAR}` syntax, is no longer printed when git
  cannot resolve it. The error shows `<redacted>` (or the `${VAR}` reference
  as written) and leaves out git's message, which can quote part of the ref
  (`a:b` is "invalid object name 'a'"). A nested rule's manifest path read
  from the environment is left out of the empty-manifest warning too, and a
  `policy_url` read from the environment is `<redacted>` in `alint list`.
- A rule whose `paths:`, `scope_filter:`, selector or `require:` is read from
  the environment shows `<redacted>` for the paths in its findings, and now
  `alint fix` does the same in the warnings it prints for those files: a file
  over `fix_size_limit`, a `replace` match left unfixed because the
  replacement still matches, a write that fails, and the `--diff` notice of
  edits left to a later pass. A `pattern:` read from the environment, or in a
  `sensitive: true` rule, raises no warning about its `^` or `$` anchors,
  which would tell how the pattern starts or ends. The deprecation warning for
  a `${VAR}` in `git_commit_message`'s `since:` leaves out a value read from
  the environment or set in a `sensitive: true` rule, and the error for an
  untrusted remote `extends:` that substitutes an environment-derived var
  names "a template" when the rule's `extends_template:` is read from the
  environment.
- A `baseline:` file whose name holds a glob character (`base[1].json`) is
  kept out of the walk. Its name was read as a pattern, so alint linted the
  baseline and could leave out another file (`base1.json`) instead.

## [0.21.0] - 2026-11-01

General-linter phase 2 adds `key_parity` on a new internal relation layer. It keeps locale and
environment files in step with a reference across eight formats (JSON, YAML, TOML, dotenv, INI,
Java properties, Android string resources and .NET `.resx`), and later cross-file rules will
share the relation layer; `cross_file`'s `relation: equals` already has a differential shadow
implementation on it.
The first draft of the plugin contract (`alint:plugin@0.1.0`) ships as internal Rust traits and a
WIT file that CI checks against witforge's generated code, though alint does not load plugins yet.
The release also moves to `jsonschema` 0.60, makes `import_gate` cheaper on import-free Rust,
fixes benchmark workloads that measured findings instead of linting, and stops read errors from
naming absolute paths.

### Security

- Upgraded `pulldown-cmark` to 0.13.5, fixing RUSTSEC-2026-0414
  (exponential event growth with `ENABLE_WIKILINKS`). alint does not enable
  wikilinks, so it was not exposed.
- The VS Code extension bundles `minimatch` 11.0.3 through an npm override,
  because `vscode-languageclient` 11 still asks for 10. This fixes
  GHSA-g3hv-75wg-m249, a pattern-expansion denial of service that only occurs
  at `optimizationLevel` 2 or higher. The language client uses the default
  level, so the extension was not exposed. Its development tools move to
  `glob` 14.0.3 (GHSA-3g3m-2fjr-6pw4).

### Added

- **`key_parity`** keeps a reference locale or environment file and a glob of peers in step. JSON, YAML, TOML, dotenv, INI, Java properties, Android string resources, and .NET `.resx` keys are compared as leaf paths, by identity (a dotted key such as `app.title` is one key, never a nested path); `direction`, `strip_root`, `plurals: cldr`, and `ignore_keys` handle the common locale layouts. Findings show a dotenv, properties, or `.resx` key as written (`app.title`) and any other key as its path (`nav.home`, `nav['a.b']`), and `ignore_keys` matches that form. A `peers` value without glob metacharacters is read even when gitignored, so `reference: .env.example` with `peers: .env` checks a local `.env`; `allow_missing_peers: true` turns its absence (in CI, say) into an informational note. Missing references and literal peers, unparsable files, and a peer glob that matches nothing otherwise fail loudly, and findings quote keys only. Key-insertion fixes remain reserved for the generic edit plan.
- The plugin contract `0.x`, internal for now: std-only adapter and analyzer traits and a bridge to the relation layer, in doc-hidden `alint-core` modules (`plugin_api`, `relation`) with no compatibility promise, and a committed `alint:plugin@0.1.0` WIT draft at `wit/alint-plugin/v0.1/`, which CI checks against the code generated from it with a pinned witforge release. alint cannot load plugins yet. The public additions are `alint-testkit`'s `Runner::with_registry`, which runs policy tests against a custom rule registry, and the test-only `shadow-ir` feature below. Contract 0.1 stays a draft until the first release that loads plugins freezes it (ADR-0027).
- `cross_file`'s `relation: equals` has a differential shadow implementation under the test-only `shadow-ir` feature, exercised by e2e properties and the corpus replay.

### Changed

- Updated the dotenv engine from `adotenv` 0.2.1 to 0.10.4. The lossless parser,
  diagnostics, spans and verified-edit APIs used by alint remain compatible;
  no source changes were required.
- `json_schema_passes` uses `jsonschema` 0.60. A regex in a schema that
  exceeds the backtracking limit no longer counts as a non-match, which could
  let `not` or `patternProperties` accept an invalid document; the verdict now
  holds only if it is the same either way, and otherwise the document gets a
  backtracking-limit finding.
- The VS Code extension uses `vscode-languageclient` 11 (still VS Code 1.91 or
  newer).
- Release binaries are built with Rust 1.99.1 (was 1.98.1).

### Performance

- `import_gate`'s Rust preset now checks for the literal `use` before running
  its per-line regex, avoiding a full regex scan of import-free source.

### Fixed

- The benchmark tree generator writes clean sources at every size. Generated
  Rust sources (S1-S3 and SFIX) and S4's Rust, JavaScript, and Python sources
  end with a newline, and generated text lines never start or end with a
  space. Before, S2's `final_newline` rule reported one finding per generated
  source and its whitespace rules tens of thousands more (about 86,000 at
  100k), so S2's timings and its 1M RSS budget measured findings rather than
  ordinary linter work. S1-S3 and SFIX carry workload revision 2, and S4
  revision 4; compare timings only within a revision.
- A development build's `alint --version` now reports the checked-out commit.
  The build script watched a fixed `.git/HEAD` path, so the SHA lagged commits
  on the current branch. It now asks git for the checkout's real files, watches
  only the ones that exist, and ignores repository variables such as `GIT_DIR`
  inherited from another checkout's git hook. As a result, building in a linked
  git worktree no longer recompiles the `alint` crate and relinks its
  integration tests on every build. `ci/scripts/test-build-script-freshness.sh`
  drives real cargo builds through each case. Release binaries were not
  affected.
- A read error for a literal path that is a directory or another non-regular
  file (a `key_parity` reference or peer, a `cross_file` source or target, and
  the other direct reads) no longer names the file's absolute path, which made
  findings and fingerprints depend on where the repository was checked out.
  The finding carries the repo-relative path, as for every other read error.

## [0.20.1] - 2026-10-23

A performance and diagnostics patch: rule path checks, nested-rule kinds and several other hot
paths are cheaper, and alint now warns when a content pattern's `^` or `$` anchors to the whole
file where a line was almost certainly meant (#343). Structured-query and cross-file checks run
3-5% faster end to end, the docs explain what each kind matches a regex against, and releases
gain a deterministic micro-benchmark gate and a long-horizon performance anchor.

### Added

- `xtask micro-det` counts the instructions of every criterion micro
  benchmark under Callgrind, across git refs. Pull requests and pushes to
  `main` run a curated low-noise subset (advisory); `bench-record` runs the
  full suite at release and fails when a benchmark grows more than 3% over
  the previous release.
- A long-horizon performance anchor (`docs/benchmarks/ANCHOR`):
  `xtask bench-gate --anchor` and `xtask micro-det --anchor` fail a release
  that exceeds the anchor release by more than 10%, so creep that passes
  every release-to-release check still surfaces. `bench-record` now runs
  `bench-gate` itself and adds a same-session criterion A/B against the
  previous release to the results PR.
- **Warnings for mis-anchored content patterns.** In `file_content_forbidden`,
  `file_content_matches`, `file_header`, `file_footer` and the
  `file_content_matches` fact, `^` and `$` anchor to the whole file (or block),
  so `^TODO` only ever checks line 1 and `TODO$` never matches a file that ends
  in a newline (#343). alint now warns when a config loads with a `$` that
  cannot match a newline-terminated file, or a forbidden pattern that starts
  with `^`: on `check`, `fix`, `baseline`, `validate-config` (its JSON output
  gains `warnings`) and as diagnostics on the config file in the editor. The
  warnings never change findings or exit codes, and `--quiet` silences them.

### Changed

- The deterministic perf gate also runs on pushes to `main`, comparing the
  pushed range.

### Performance

- **Every rule's path check is cheaper.** Matching a file against a rule's
  `paths:` include and exclude globs prepared the path for matching twice, once
  per glob set; it is now prepared once. The rule-dispatch benchmark drops 23%
  in instructions and raw glob matching 12%.
- **Nested-rule kinds are about three times cheaper.** `every_matching_has`,
  `for_each_dir` and `for_each_file` build each nested rule once per matched
  entry, and compiling its literal path (`packages/p0/README.md`) into a glob
  set was most of that cost. A scope whose only pattern is a literal path now
  matches by comparing the path instead. Nested rules also no longer re-run
  build-time validation per entry, skip glob escaping for values with nothing
  to escape, and derive only the path tokens their templates use. Instruction
  counts drop 67-70% (`every_matching_has`, `for_each_dir`), below v0.17.0.
- **`pair` and `unique_by` are cheaper.** Both now derive only the path tokens
  their templates use; `pair` skips its partner-path normalization when the
  rendered path is already normal, and `unique_by` groups keys in a hash map
  (output order unchanged). Instruction counts drop 26-27% (`pair`) and 32-47%
  (`unique_by`).
- **Terminal output of many findings is cheaper.** The human, github and
  markdown formats sanitize every path and message; printable ASCII now skips
  the per-character check. Instruction counts drop 16-28%, below v0.17.0.
- **YAML is scanned once, not twice, before parsing.** The denial-of-service
  guards (flow-collection depth and alias expansion) share one pass and skip it
  when the document has none of the bytes that could trip them. YAML structured
  queries drop 16% (back to v0.17.0), `pinned_references` 22%.
- **Loading an `extends:` chain is cheaper.** A config reached through
  `extends:` is memoized only when it is loaded a second time (diamond chains
  stay linear), and provenance markers are updated in place. A 16-level chain
  loads with 21% fewer instructions.
- **End to end**, against v0.20.0 on the bench host in two separate sessions,
  structured-query and cross-file checks (S2, S4) run 3-5% faster from 10k to
  1m files (best run of each cell). Walk- and content-scan-bound runs (S1, S3)
  take the same time, although S1 now executes 6% fewer instructions.

### Fixed

- **What a content pattern matches against is documented.** A new "Regex
  patterns and anchors" configuration page lists what each kind matches a
  regex against and what `^` and `$` mean there, and the content rules'
  reference pages and schema descriptions say how they anchor. Seven
  mis-anchored patterns in the docs and case-study examples are corrected:
  the monorepo page's `.bazelversion` shape check, for one, failed every
  file that ends in a newline (#343).
- The 1k `changed`-mode benchmark cells no longer swing between about 11
  and 20 ms from one release to the next: the harness committed the
  generated tree within one second, so git re-hashed every "racily clean"
  file on each timed run. The index is now settled before timing.
- The deterministic `det_engine` benchmark now counts the rayon worker that
  evaluates rules; it had been counting about 0.06% of the engine's work.
- Correct the crates.io Trusted Publishing instructions for a brand-new crate:
  bootstrap its first tagged version with a scoped maintainer token from the
  exact release commit, configure the crate's OIDC publisher, then rerun the
  idempotent failed release job.

## [0.20.0] - 2026-10-21

General-linter phase 1b closes the policy-authoring loop with a public, hermetic policy-test
runner, selected-rule production traces with bounded redaction-safe previews, and per-source
configuration compatibility requirements. The release also hardens sensitive diagnostics and YAML
handling, moves exact-path facts onto the lazy file index, and makes release, benchmark, and
ruleset ownership contracts executable.

### Added

- **`alint-testkit` now ships the public, hermetic policy-test v1 runner.** `PolicyTest` documents
  parse through bounded YAML guards and aggregate validation, while `Runner::run` separates the
  read-only source package from the temporary repository, denies remote config resolution, injects
  deterministic environment/time/Git state, reloads config for every step, and enforces two-key
  grants for process and host-keyring capabilities. Structured observations support counted exact
  matching, notes, analysis failures, related locations, canonical diffs, redacted diagnostics,
  final-tree checks, and atomic JSON snapshots. All 578 declarative tests use the public contract;
  bundled ruleset tests now live beside their owning rulesets under `alint-dsl/ruleset-tests/v1`,
  and the obsolete internal scenario model and unchecked runner are removed. The crate is
  publishable with an opt-in `strategies` feature and stable policy-test/report JSON Schemas.

- **`alint test` now provides the public policy-authoring loop.** It discovers `.yml` and `.yaml`
  documents under `alint-tests/` or explicit paths, applies repeatable AND tag filters and a
  path/name substring filter, preflights every selected definition and capability before execution,
  and runs tests in deterministic source/name order. Human and stable JSON reports distinguish
  passed, failed, invalid, and platform-skipped tests; exits 0/1/2/3 map to pass, mismatch, invalid
  input, and operational failure. Snapshot updates are confined and deferred until every selected
  test's inline expectations pass. Native process and host-keyring access each require both a
  document declaration and an explicit CLI grant.

- **The v0.20 selected-rule trace engine now executes production dispatch without observing
  ordinary checks.** `Engine::trace_rule` evaluates only the requested effective rule through the
  existing common gates, rule-major/file-major paths, nested-rule builder, result partitioning, and
  redaction. Typed deterministic stages carry checked aggregate counts, bounded producer-redacted
  preview rows, and exactly one terminal outcome; runtime completion rejects undeclared, missing,
  malformed, or result-inconsistent stages against mandatory metadata on all 102 canonical kinds.
  Preview producers cannot inject engine-owned stages or bypass typed value classes, sensitivity,
  repository-relative path normalization, source-range validation, or non-UTF-8-safe serialization.
  Ordinary checks construct no trace recorder. `facts.json` format v6 and `alint rules show` expose
  each kind's trace inventory.
- **`alint explain <rule-id> --trace` now diagnoses one effective rule end to end.** It walks the
  configured repository, runs no unrelated rule, and renders typed pass, violation, skip, and
  analysis-failure stages in human or explicitly experimental JSON. The production fact context is
  preserved, so configured custom facts may execute subprocesses just as they do under `check`.
  `--changed`/`--base` mirror check scope; opt-in `--preview` retains the deterministic first 1–100
  structured-extraction or nested-child rows with exact omission/redaction/truncation accounting.
  Environment-derived and explicitly sensitive option, message, policy-URL, condition, preview,
  result, and configuration-error evidence is redacted before retention, including through
  template expansion and config merges. The generated rule-trace JSON Schema is committed at
  `schemas/experimental/rule-trace.json` and packaged with the CLI.

### Changed

- **Exact-path repository facts now use the lazy `FileIndex` path-to-entry index.**
  `any_file_exists`, `all_files_exist`, and `file_content_matches` retain glob, exclusion, ignore,
  and validation semantics without rescanning every walked file for each literal fact;
  `count_files` now takes the same indexed path for an exact zero-or-one count. On the controlled
  100k-file, 20-fact existence/content workload this removes 90.4% of the incremental instruction
  cost; a Criterion scaling benchmark and a deterministic Callgrind cell guard the linear shape.
  See the [investigation](docs/benchmarks/investigations/2026-10-s4-literal-facts/).

- **Release hot paths no longer build avoidable intermediate state.** Static `explain --format
  json` serializes its typed, borrowed rule document directly through a buffered writer instead of
  constructing and then copying an outer JSON value tree. Git subprocess setup also skips the
  scoped-environment registry lookup and map clone for ordinary repositories while continuing to
  strip every repository redirector from policy-test environments.

- **The S4 benchmark workload is now revision 3.** Its Git fixture commits realistic
  version and toolchain authorities, activating all 35 rules added in revision 2;
  the contract pins 23/24 true facts and 82/85 active rules. Published history and
  trajectory data now carry explicit workload revisions and mark reset boundaries
  instead of presenting unlike timings as a continuous series.

- **Oversized Rust implementation files are now split by responsibility.** CLI command execution
  lives in per-command modules; engine execution, fixing, gates, and tests use phase-focused
  modules; DSL tests are grouped by concern; and docs export separates rule, ruleset, and CLI page
  generation. All Rust sources remain below the repository's 2,000-line cap without public API or
  behavior changes.

- **YAML parsing now uses the YAML organization's maintained fork.** `yaml_serde` 0.10.7 and
  `libyaml-rs` 0.3.0 replace `serde_yaml_ng` and the archived `unsafe-libyaml`, removing
  `RUSTSEC-2024-0449` from the dependency graph without changing accepted YAML, value semantics,
  diagnostics, the 128-level recursion boundary, or alint's stricter flow/alias resource guards.
  The migration includes a parser-independent compatibility contract and a measured
  [buy-versus-build decision](docs/design/yaml-parser.md).

- **Malformed YAML can no longer produce a partial extraction or an ambiguous fix.**
  Raw NUL bytes fail direct event-stream extraction instead of terminating it early, and
  structured fixes decline duplicate mapping keys rather than selecting one value. The
  aligned `saphyr` and `saphyr-parser` 0.2.2 update is regression-tested and kept in one
  Dependabot group so their pre-1.0 event contracts cannot drift.

- **Every configuration source now proves language and binary compatibility before composition.**
  Root, local-extended, bundled, HTTPS, nested, and drop-in sources validate their own `version`
  before trust gates or merge. Complete sources must declare it; a drop-in may omit it and inherit
  the root's version. A source may add the closed literal
  `requires: { alint: "MAJOR.MINOR.PATCH" }` mapping; malformed, interpolated, or unsatisfied
  requirements fail with source and running-version diagnostics before that source's child
  resolution. Requirements are retained as audit metadata and never merge as policy. The config
  JSON Schema and embedded copy include the new field; `facts.json` format v5 publishes the
  supported config and policy-test version lists from runtime constants.

- **Bundled ruleset tests now have executable ownership gates.** The runtime catalog and
  `alint-dsl/ruleset-tests/<revision>/` directories must match exactly; all 24 shipped v1 bundles
  own explicit, unfiltered pass and fail scenarios. Catalog filtering, composition, and declared
  compatibility metadata run through production loading. The complete 578-document corpus is
  count-pinned, parsed by `PolicyTest::from_yaml`, and executed by the public runner; supplementary
  audits no longer deserialize the public test type through an unchecked YAML path.

- **Post-publish distribution smoke now exercises policy-test behavior, not only version
  metadata.** Every installed CLI channel—release tarball, crates.io source and binary installs,
  npm, Docker, Homebrew, and PyPI—runs a fresh downstream `alint test` package and validates its
  stable JSON report, catching package-only runtime omissions that `alint --version` cannot expose.

### Fixed

- **HTTPS configuration loading no longer trusts certificates through compromised emSign
  intermediates.** The locked `rustls-webpki` and Mozilla root store now contain the coordinated
  fixes for `RUSTSEC-2026-0382` and `RUSTSEC-2026-0381`.

- **Policy-test corpus audits now follow public discovery semantics.** The CLI, 578-document
  migration gate, and bundle-ownership gate share one bounded top-level `test_version`
  discriminator. Package-local YAML support files are no longer misclassified as owned tests,
  recursive audit walks do not follow directory symlinks, and `.yaml` scenario documents execute
  through the public runner instead of falling outside the repository harness.

- **Environment-derived selector paths no longer leak through diagnostics.** Static and traced
  `explain`, bounded previews, check/fix formats, baselines, `export-agents-md`, SARIF annotations,
  agent output, policy-test observations, and LSP diagnostics now project protected `paths:`,
  kind-specific selectors, manifest scopes, and nested selectors as `<redacted>` while preserving
  the original path internally for baseline identity and remediation.

- **The published `alint-testkit` crate now includes the repository's Apache 2.0 license, MIT
  license, and NOTICE files.** Release packaging no longer omits the attribution files required of
  every published workspace crate.

- **Policy-test discovery and diagnostics now remain safe around real fixture packages.** Recursive
  `alint test` discovery ignores YAML without a top-level `test_version`, so config chains,
  workflows, malformed parser fixtures, and other repository-owned YAML are not misclassified as
  policy tests; explicitly named files still fail loudly. Policy-test observations, diffs,
  mismatch details, baseline keys, fingerprints, and pre-execution capability denials now redact
  every declared `given.env` value before reports or snapshots retain them.

- **Platform-specific policy tests are skipped before fixture materialization again.** The public
  runner now honors `requires-case-sensitive-fs` on default macOS as well as Windows filesystems,
  preserving the migrated corpus harness contract instead of overwriting case-variant fixtures.
  The canonical pass/fail trace audit remains on Linux, where every platform-tagged fixture can
  execute; non-capturing docs validation accepts explicit platform skips; and the integration suite
  compiles warning-free while its CLI help snapshots normalize the executable suffix on Windows.

- **Release installation now retries connection-level asset download failures.** The installer uses
  a bounded portable retry loop for the archive and checksum, covering TLS resets and similar curl
  failures that `--retry` alone does not classify as transient.

- **Counted policy-test matchers no longer allocate from an impossible declared cardinality or
  recurse through an unbounded augmenting path.** Exact matching first checks the overflow-safe
  expected total against the observed result count, so a tiny test document with a platform-sized
  `count` produces one deterministic mismatch instead of panicking on `Vec` capacity. Matching
  reuses its traversal storage, flips augmenting paths iteratively to prevent process-stack
  exhaustion on wide valid documents, and compiles each message regex only once per step.

- **Selected-rule traces can no longer accept successful results that omit required evaluation
  stages.** Trace completion now checks the declared stage inventory whenever a producer returns a
  result, while retaining the intentional exemption for engine-owned terminal gate results.

- **The published GitHub Actions guide now follows the post-release immutable pin.** The docs bundle
  overlays that guide from `main` after a tag is published and rejects generated output whose
  Action examples do not match `ci/action-doc-pin.env`, instead of leaving alint.org's copy-paste
  snippets one release behind.

- **Release-bump shell validation now tests the actual candidate tree.** The version-pin harness
  overlays and commits every tracked working-tree change in its disposable clone, so local
  pre-commit validation neither fails on the expected uncommitted version bump nor tests stale
  release metadata from `HEAD`; clean CI checkouts also skip the absent overlay instead of asking
  `git apply` to consume an empty patch.

- **Nested configs now reject every execution setting they cannot honor.** Explicit
  `respect_gitignore`, `fix_size_limit`, and `nested_configs` declarations now fail, including an
  intentional `fix_size_limit: null` or `nested_configs: false`, instead of being silently dropped
  while only `version`, `requires`, and `rules` take effect. File loading also rejects `null` for
  the two boolean settings rather than silently applying their defaults.

- **Benchmark regressions now compare like workloads.** Macro reports record a semantic revision
  for every scenario, `bench-gate` skips cross-version timing comparisons across an intentional
  revision change while retaining the new run's quality gate, and a contract test pins each
  revision's resolved rule count. The v0.19.0 S4 expansion from 50 to 85 rules is now explicit
  instead of being misreported as an engine regression. The bench workflow also no longer passes
  an unsupported input to its pinned Rust toolchain action.

## [0.19.0] - 2026-10-18

General-linter phase 0/1a makes the rule registry authoritative for capabilities and generated
schemas, fails incomplete analysis loudly, preserves source relationships and duplicate structured
rows, and adds seven standalone policy kinds plus opt-in versioning and toolchain bundles. It also
makes time-dependent checks reproducible, redacts sensitive evidence, and hardens EditorConfig,
LSP encoding and teardown, dotenv editing, installers, and release tooling.

### Added

- **All 102 canonical rule kinds now declare serializable capability metadata.** Each built-in
  registration carries its dispatch class, accepted selectors, conditional host capabilities,
  content access, formats, changed dependencies, fix operations, and nested-rule support; all 11
  aliases resolve the exact canonical record. Completeness, transition-list parity, subprocess
  source auditing, and rule-instance conformance are enforced in tests.
- **DSL trust and audit decisions now use the rule registry's capability catalog.** Every
  production loader supplies the same borrowed `KindCatalog` used to build rules, including the
  CLI, LSP, export, testkit, benchmark, and facts paths. Process-spawning kinds and fix operations,
  content-injecting fix demotion, nested-config confinement, unconditional-Git coverage, and the
  docs fail-only exemption now derive from metadata. Loading a template,
  inherited rule, or nested rule without a catalog, or with a kind absent from
  that catalog, fails closed; plugin-owned metadata is honored. The former hand
  lists remain exact one-release transition checks.
- **Rule capabilities are now generated into every discovery surface.** `facts.json` format v4
  adds a `kind_meta` record for every canonical kind; aliases resolve through `rule_aliases`.
  `alint rules show <kind>` and `alint explain <id>` now print capabilities, accepted selectors,
  and fix operations in human output and expose the structured lists in JSON. The config JSON
  Schema now derives all 102 kinds' option surfaces from Rust option models and applies each kind's
  metadata-described `paths`, `scope_filter`, `fix`, and `expect_matches` contract to top-level
  and nested rules; generation fails on registry, metadata, or schema drift.
- **Structured extraction now has a duplicate-preserving, source-spanned row API.**
  `alint_core::extract::extract_entry_rows` returns source-ordered object members with their
  structural parent path, key, key span, and optional value span for dotenv, JSON/JSONC, YAML,
  INI, and Java properties. Repeated names remain separate rows; array positions remain explicit
  path segments. TOML, XML, and HCL return a typed unsupported-format result instead of flattening
  away information. The adapters share the existing structured-input byte bound. Existing
  last-wins extraction and all shipped rule results are unchanged; this is the substrate for
  duplicate-key and structured-key-order kinds.
- **Six high-value policy gaps now have standalone rule kinds.** `editorconfig_conforms` checks
  the effective repository-confined EditorConfig policy for indentation, line endings, charset,
  trailing whitespace, final newlines, and line width. CommonMark-backed
  `markdown_images_have_alt`, `markdown_required_headings`,
  `markdown_no_duplicate_headings`, and `markdown_fragments_resolve` enforce accessibility and
  document structure without treating examples as live markup. `spdx_identifier_valid` validates
  strict license expressions against the bundled offline SPDX License List 3.29.0 data. All six
  are check-only, expose precise source ranges, and declare their dispatch, selector, content,
  and dependency capabilities through `KindMeta`.
- **`pinned_references` enforces immutable build inputs across six consumers.** The check-only,
  offline kind validates GitHub Action and reusable-workflow commits, composite-action steps,
  workflow/container service images, pre-commit revisions, CircleCI orbs and images, GitLab
  images/services, Cloud Build step images, Dockerfile base images, and remote `ADD` checksums.
  Findings carry exact YAML scalar or Dockerfile instruction ranges; YAML aliases/merge keys,
  Dockerfile continuations/heredocs, local references, and `scratch` are handled without network
  resolution. `ci/github-actions@v1` keeps its existing `gha-pin-actions-to-sha` rule id, warning
  level, and workflow-step finding identity while migrating to the new kind. Existing users may
  now see warnings for mutable reusable workflows, composite-action steps, and workflow
  container/service images: pin them to a full commit SHA or sha256 digest, or narrow/disable the
  inherited rule when that mutable input is intentional.
- **Two opt-in bundled rulesets enforce cross-file release and toolchain consistency.**
  `alint://bundled/versioning@v1` uses a root `VERSION` authority to compare existing package,
  Cargo, pyproject, Python, Helm, OpenAPI, and changelog values. The
  `alint://bundled/toolchain-pins@v1` policy uses explicit Node, Rust, Python, Ruby, JVM, and .NET
  authorities to compare literal local declarations, supported Docker images, and matching
  GitHub Actions setup inputs. Missing authorities make each policy silent; missing targets are
  optional; malformed or unsafe authorities fail once; ranges, leading-`v` workflow inputs, and
  computed targets remain outside the exact-pin surface. Structured authorities and declaration
  targets are selected from root-qualified nodes, so nested lookalikes cannot activate or satisfy
  policy; Docker `FROM` selectors reuse the Dockerfile parser and ignore heredoc payloads.
  `sync_from` repairs are suggested by bare `fix` and require `--unsafe-fixes` to apply. Repairs
  correlate to the exact live finding (so baseline-suppressed duplicate values in one file stay
  untouched) and enforce extraction-count, syntax-line, edit-count, and output-size ceilings. The
  reusable
  `file_content_matches.extract` fact option applies any of the eight structured formats
  (JSON/YAML/TOML/XML/dotenv/properties/INI/HCL) before matching, while
  `cross_file.targets.matches` filters extracted target strings before comparison or repair.
  Migration: this is additive and affects only repositories that explicitly extend either new URI
  or adopt the new optional fields. Semver-range policy remains C4 and Git-tag consistency remains
  C6.

### Changed
- **Incomplete analysis is now a typed, fail-loud result instead of a policy violation.**
  Evaluation, runtime-condition, resource-read, resource-parse, and repository-derived nested-rule
  build failures live in `analysis_failures`, always force an error exit regardless of the rule's
  configured level, and cannot be baselined or offered to a fixer. All eight check formats, every
  fix report mode (including `--fix-only`, `--dry-run`, and `--diff`), and LSP diagnostics expose
  them; SARIF marks the run unsuccessful, JUnit uses `<error>`, and agent output never supplies a
  remediation command. `alint baseline` refuses to write when analysis is incomplete, while an
  existing baseline can still suppress ordinary violations without hiding a failure. Migration:
  JSON, agent, and fix-report consumers must tolerate the additive `analysis_failures` arrays and
  `total_analysis_failures` summary count. Rust callers constructing `RuleResult` or
  `FixRuleResult` must add `analysis_failures: Vec::new()`; producers that continue after one bad
  resource can use `Violation::as_analysis_failure(AnalysisFailureKind::...)`.

- **The remaining runtime fail-open paths now report explicit outcomes.**
  Manifest-derived `scope_filter` inputs that are missing, unreadable, malformed, over-limit, or
  select non-string nodes produce typed resource analysis failures for every active owning rule;
  they no longer collapse to an empty include set or a full-scope exclude. Active Git
  dependencies derive from each kind's conditional capability metadata—including nested rules,
  `git_tracked_only`, `scope_filter.changed_since`, and `git_untrack`—and an unavailable Git
  executable or repository produces one `resource_read` failure per dependent top-level rule
  while unrelated rules continue. Shared structured extraction no longer drops selected JSON,
  YAML, TOML, or HCL numbers, booleans, nulls, arrays, or objects: manifest path sets reject them
  as unusable input, while `cross_file`, `file_graph`, and `registry_paths_resolve` emit a
  value-free policy violation containing only type counts. `cross_file` stops that affected
  extraction instead of deriving a second empty-set mismatch or offering an unsafe fixer.
  Migration: run Git-backed rules inside a Git work tree (or disable conditionally configured Git
  features), repair unavailable manifest inputs, and narrow string-only relation JSONPaths to
  string nodes. A literal `git_tracked_only: false` remains Git-independent.

- **Informational and bounded-analysis notes now survive every check output format.** Human and
  text output keep the existing stderr count and `--show-notes` detail; JSON carries typed
  per-result notes; SARIF emits informational results or execution notifications; and GitHub,
  GitLab, Markdown, JUnit, and agent output carry an accepted count-and-pointer projection where
  their envelopes cannot preserve full detail. Note-only runs still pass, and notes remain
  excluded from baselines, fixes, fingerprints, and failure counts. Migration: machine-output
  consumers should tolerate the additive JSON/agent summary fields, SARIF notifications, and
  synthetic note summaries documented in the output-format reference. Rust callers constructing
  `alint_core::Violation` must replace `is_note: bool` with
  `disposition: alint_core::FindingDisposition`; use `Violation::as_note()` for informational notes
  and `Violation::as_analysis_truncated_note()` for bounded-analysis truncation.
- **Findings can now identify a primary range and bounded related locations.** The Rust finding
  model carries an optional half-open source range and up to 20 labeled related locations in
  deterministic path/range/label order. Relationship-heavy rules (`cross_file`,
  `registry_paths_resolve`, `unique_by`, `pair`, and `file_graph`, plus the pair hash and
  changed-together kinds) now preserve both sides of a failed relationship. Human output lists
  related paths; SARIF emits primary regions and `relatedLocations`; and LSP diagnostics emit
  `relatedInformation` only when the client advertises support. Ranges and relationships are
  presentation metadata: they never enter baseline fingerprints, and a Linux full-scenario
  golden gate freezes existing policy-finding identities. The v1 JSON report payload intentionally
  remains unchanged until the next release, but `schemas/v1/check-report.json` now accepts
  additive fields at every report object while retaining its required fields and value-type
  checks. Migration: strict consumers should continue validating known fields and tolerate
  additive v1 fields.

- **Secret-bearing evidence is redacted from reports and baselines.** Every rule accepts
  `sensitive: true`, which replaces its complete flattened finding message and fix-report detail
  with a stable, value-free marker before any public report is returned. Dotenv and Java
  properties value extractors enable the same policy automatically for structured-path,
  `cross_file`, `file_graph`, and `registry_paths_resolve` findings. Primary source paths remain
  navigable; secondary locations are omitted from sensitive findings because they may be extracted
  values. Evaluation, severity, exit status, fix applicability, and fix execution are unchanged.
  Proposed replacement bytes are omitted for sensitive findings, including any composed edit for
  the same file, and LSP does not offer sensitive code actions. Sensitive baseline fingerprints
  exclude both the raw value and its digest. A cross-format leak gate covers all eight check
  formats,
  baseline output, human and JSON fix reports, and LSP diagnostics. Migration: add
  `sensitive: true` to any rule whose message can contain credentials or other secrets. Expect
  existing dotenv/properties findings and baseline messages to become `<redacted:…>` markers;
  machine consumers must tolerate missing proposed edits for those findings.

- **Time-dependent analysis now has an explicit, reproducible reference date.** The global
  `--as-of YYYY-MM-DD` option overrides `ALINT_AS_OF`; without either, affected rules use the
  current HEAD commit's committer timestamp instead of the workstation clock. `git_blame_age`
  findings and `alint explain` show the UTC reference date and its source as `explicit`, `head`,
  or `unavailable`. An explicit date removes the HEAD-time lookup, but does not satisfy
  `git_blame_age`'s independent requirement for a usable Git repository and blame input; when
  either dependency is unavailable, it reports a typed, always-error analysis failure. Migration:
  CI jobs that intentionally age findings against a release date should set `--as-of` or
  `ALINT_AS_OF`; repeated checks of the same commit otherwise remain byte-identical. The public
  capability variant and serialized metadata value are now `Capability::AnalysisTime` and
  `analysis_time`; Rust consumers with exhaustive `Capability` matches must replace
  `Capability::WallClock`, and metadata consumers must replace `wall_clock`. Rust callers
  constructing `alint_core::Context` must add `analysis_time: None`, or provide an
  `AnalysisTimeResolution` when evaluating a time-capable rule.

- **Rule fields that their selected kind does not support now fail configuration loading.** Both
  `check` and `validate-config` validate `paths`, every `scope_filter` predicate, `fix` operation,
  `expect_matches`, and metadata-described common options before building the rule, including
  rules nested under `require:`. The error names the incompatible field and kind instead of
  silently discarding the field, and names the template or `extends:` source that supplied it
  after composition. Nested rules also reject `id` and `level`, which are synthesized from the
  parent and were previously ignored. Migration: remove `id` and `level` from nested `require:`
  entries; remove `paths` and `scope_filter` from single-shot `command_idempotent` rules (or use
  per-file `command` when the command genuinely consumes `{path}`); replace
  `scope_filter.changed_since` with top-level `since` on Git commit kinds. A present `fix` must
  select one operation; `fix: null` is now a load error instead of silently disabling the fix.
  Likewise, `scope_filter: null` is an error instead of silently disabling every predicate.
  Remove either key when it is not intended. Bundled rulesets, shipped examples, documentation
  examples, and scenario configs have been migrated.

  The complete W1 migration matrix below covers all 58 newly rejected canonical
  kind/field-or-predicate pairs: 57 were silently ignored before this release, while
  `dir_absent` with `scope_filter.changed_since` silently matched nothing. Kinds and fields not
  listed here keep their prior meaning; aliases inherit their canonical kind's row.

  | Rejected fields | Canonical kinds | Migration |
  |---|---|---|
  | `paths`, `scope_filter` | `changeset_requires_path`; `git_commit_author_allowlist`; `git_commit_gpg_signed`; `git_commit_message`; `git_commit_no_fixup`; `git_commit_signed_off`; `git_commit_subject_matches`; `git_no_denied_paths`; `pair_changed_together` | Remove both fields. Use the kind's own change selectors, such as `since`, `when_changed`, `denied`, `if_changed`, or `then_changed`, where applicable. |
  | `paths`, `scope_filter`, `fix` | `command_idempotent`; `generated_file_fresh` | Remove all three fields. These are repository-level command checks; use `command` when the command must run once per selected path. |
  | `scope_filter.changed_since` | `dir_absent` | Remove `changed_since`; `dir_absent` supports only `scope_filter.has_ancestor`. Use a file-scoped kind when the check must be narrowed to a Git diff. |
  | `paths`, `fix` | `dir_contains`; `dir_only_contains`; `every_matching_has`; `file_graph`; `for_each_dir`; `for_each_file`; `pair`; `registry_paths_resolve`; `unique_by` | Remove `paths` and use the kind-specific selector (`select`, `nodes`, `primary`, or `source`) when narrower input is required. Remove the unsupported top-level `fix`. |
  | `paths` | `cross_file`; `pair_hash` | Remove `paths`; use `source` and the kind's target/partner fields. |
  | `fix` | `commented_out_code`; `dir_absent`; `file_is_text`; `file_max_lines`; `file_max_size`; `file_min_lines`; `file_min_size`; `file_shebang`; `filename_regex`; `for_each_match`; `import_gate`; `markdown_links_resolve`; `markdown_paths_resolve` | Remove the unsupported fix or move remediation to a kind that advertises the required fix operation. |

- **The dotenv engine is now adotenv 0.2.1, with a lossless document model and native verified
  edits.** Parsing accepts bare declarations and maps them to JSON `null` (distinct from `KEY=`,
  which remains `""`). Compared with 0.1.1, it accepts multiline quotes and portability-warning
  key forms, recognizes `export` followed by spaces or tabs, treats lone CR as a line ending,
  and starts an inline comment at `#` after either a space or tab. Double-quoted `\$` decodes to
  a literal `$`, while non-ASCII whitespace at an unquoted value's edges is retained rather than
  trimmed. Non-ASCII keys and text after a closing quote are errors. Migration: rename invalid
  or non-ASCII keys; move any intended quoted tail inside the closing quote (or remove it); write
  `\\$` inside double quotes when the resulting value must retain a backslash before `$`; quote
  values when a tab-before-`#` sequence or non-ASCII edge whitespace is meant literally; and
  account for `null` when querying or schema-validating a bare declaration. `set_value`, value
  `replace`, and `remove_value` now consume adotenv's exact native `TextEdit` ranges and
  replacements; existing quote style is retained where possible, bare declarations can be
  assigned, and multi-key removals are verified atomically.

### Fixed

- **EditorConfig conformance now follows the declared grammar, discovery boundary, and every
  supported target line ending.** Policy files require `=` with LF/CRLF separators while valid
  extension pairs in the preamble remain accepted. A nearer `root = true` policy stops discovery
  before hidden outer policies are read or diagnosed, so a malformed or oversized outer file
  cannot fail a target governed by the nearer root. Glob character classes treat hyphens
  literally, a slash inside a complete class does not incorrectly anchor the section to one
  directory level, and descending numeric ranges no longer match as reversed ranges. CR-only
  target files are split into physical lines for indentation, trailing-whitespace, width, and
  final-newline locations. Tab indentation accepts the specification's canonical sub-tab-width
  space remainder instead of rejecting every space. Unsupported extension properties are discarded
  while parsing instead of being cloned into every target's effective policy, and ancestor
  discovery builds only one target's policy chain at a time instead of retaining every selected
  file's chain.

- **Source locations and line-oriented extraction now agree across LF, CRLF, CR, and BOM-prefixed
  text.** A shared zero-allocation physical-line iterator backs EditorConfig and SPDX checks,
  `lines` extraction, and INI parsing. Half-open source ranges, LSP diagnostic and edit positions,
  and Unicode security scans count CRLF once, recognize lone CR, and do not count a leading UTF-8
  BOM as a visible column. The LSP text cache strips that signature from editor overlays and disk
  text and decodes BOM-marked UTF-16LE/UTF-16BE files before converting scalar columns to UTF-16
  code units. SPDX validation consequently checks every tag in a CR-only file and reports its
  actual physical line.

- **LSP integration teardown and hosted editor coverage no longer leave audit gaps.** The shared
  real-server harness waits for the `shutdown` response and gives `exit` time to terminate the
  child, preventing truncated LLVM profiles from making otherwise-passing coverage runs fail.
  Real-client editor checks now also run on direct `main` pushes that touch an editor, the LSP
  server, the binary, or their workflow—not only on pull requests.

- **Compact human output remains one physical line per finding for untrusted input.** Embedded
  newlines in paths, rule IDs, violation messages, and analysis failures render as inert `\x0a`
  text instead of injecting forged compact records. Grouped output still preserves and re-indents
  intentional message paragraphs, and every line of a top-level CLI error retains an `alint:`
  prefix.

- **The versioning bundle enforces its documented 256-byte source-token ceiling.** A leading
  `v`/`V` now counts toward the limit in the activation fact, source validation rule, and all ten
  target extractors; boundary end-to-end scenarios cover 256-byte acceptance and 257-byte
  rejection.

- **The VS Code extension toolchain no longer carries known npm vulnerabilities.** Open VSX
  publishing dependencies were updated, while the VS Code 1.91-compatible Mocha 11 runner retains
  patched `diff` and `serialize-javascript` transitive versions through exact overrides. Both
  editor CI and the Marketplace/Open VSX release job now block on moderate-or-higher `npm audit`
  findings.

- **Sensitive findings no longer expose internal fix-correlation keys.** Public check, fix, and
  staged-diff reports now clear `fix_key` alongside proposed edits and related evidence, preventing
  extracted secret values from crossing the engine API boundary.

- **Baseline and Markdown identities now follow their lossless consumer semantics.** Baseline
  fingerprints preserve exact Unix path bytes, so distinct non-UTF-8 filenames cannot collapse to
  the same finding. Generated Markdown heading anchors now match GitHub's Unicode-category
  filtering, including punctuation, symbols, tabs, non-breaking spaces, emoji, and combining marks.

- **Mutating generator failures restore partial output before returning.** `generated_file_fresh`
  rescans after a failed or timed-out process, removes newly-created output, restores prior bytes,
  and reports any restoration failure instead of silently leaving the working tree modified.
  Staged fix previews likewise fail on source or metadata read errors rather than fabricating
  empty-file diffs.

- **Installer version inputs are validated before any URL or filesystem use.** `install.sh` and
  the GitHub Action accept only `latest` or complete `vMAJOR.MINOR.PATCH` release tags with optional
  pre-release and build suffixes; branch names, partial versions, and path-like inputs fail early.
  Exact pre-release action refs select themselves, and the documented piped version pin now passes
  `ALINT_VERSION` to `bash` rather than `curl`.

- **Generated Mermaid labels no longer contain wrapped newlines or trailing spaces.** The
  architecture generator joins LikeC4's physical wraps inside quoted labels while preserving
  statement boundaries.

- **Lossless dotenv parsing is bounded before allocating its document.**
  Nonempty parsed documents are limited to 4 MiB of original bytes (including
  all BOMs) and 100,000 physical lines; CRLF counts once, and a terminal ending
  adds no empty line. Native span and fix entry points share these limits.
  Repeated leading BOMs are accepted without changing edit/proposal coordinates
  or removing BOM bytes. Sets budget the complete prospective output before
  native serialization/verification allocations; conservative double-quoted
  size estimates may decline otherwise-valid near-limit edits, while retained
  single quotes use their exact literal size and physical lines. Oversized
  inputs and generated output are declined rather than exhausting memory.
  adotenv 0.2.1 also omits discarded warning diagnostics, duplicate-only
  key tries and CR merge buffers during strict parsing and native verification,
  fixing the below-cap CR warning-density allocation spike while retaining the
  documented multi-megabyte / 100,000-entry workload.

- **Machine-readable edit proposals recognize lone carriage returns as line endings.**
  Dotenv files accepted by adotenv 0.2 now receive correct JSON, agent and SARIF fix
  coordinates instead of an insertion on the preceding physical line. SARIF
  exclusive ends retain the last character's line for both CRLF and lone CR.

- **Machine fix payloads cannot alias distinct non-UTF-8 paths.** SARIF artifact URIs
  percent-encode original Unix filename bytes, while JSON and agent reports omit the optional
  `proposed_edit` field when an edit path has no exact JSON string representation. The agent's
  safe `fix_command` remains available. Git subprocesses also ignore inherited repository-
  redirecting and pathspec-mode environment variables, disable clone-local replacement refs, and
  force literal internal paths, so HEAD time, tracked-path selection, history, blame, and
  Git-backed fixes use the requested repository and exact path.

- **`git_blame_age` no longer inherits clone-local history and blame choices.** HEAD and blame
  disable replacement refs; blame pins the Myers diff algorithm, clears valid global and local
  `blame.ignoreRevsFile` lists, and applies only a tracked, regular
  `.git-blame-ignore-revs`. A worktree symlink at that path is not followed, and full SHA-256
  object IDs are parsed alongside SHA-1 instead of silently dropping every blamed line. These
  controls prevent clones of the same repository input from assigning different line ages
  because of local Git configuration or external ignore-list content.

- **Commit-validation findings no longer inherit Git's `core.abbrev` setting.** HEAD and range
  queries request a seven-hex short object ID, extended only by Git when that prefix is ambiguous,
  so messages and baseline keys do not vary with clone-local abbreviation preferences.

- **The benchmark regression report now includes changed-mode 1k cells as advisories, as
  documented.** Those floor-level cells still never block publication; previously an outer size
  filter silently omitted their deltas despite the gate classifying them as advisory.

- **Minor release preparation now refreshes every version-coupled workspace artifact before
  resolving the workspace.** Cargo's caret semantics do not allow a pre-1.0 `0.18` requirement to
  select a local `0.19` crate, so `bump-version.sh` updates all seven workspace dependency
  requirements when a pre-1.0 minor or any major release crosses a compatibility line. It also
  regenerates the version-bearing `facts.json` contract. Shell regressions cover both omissions
  that previously made the documented release command fail preflight.

## [0.18.4] - 2026-10-15

A performance and correctness patch. `check --changed` and `fix --changed` no longer build
whole-tree lookups that no rule uses (v0.18.3 paid for them on every changed-mode run, about 16%
at 1M files), and `.env` fixes now take their edits from adotenv 0.1.1's verified `set` and
`remove`, which fixes two cases where a fix silently changed a file's meaning.

### Changed

- **`.env` fixes use adotenv 0.1.1's verified edits.** `set_value`, value `replace`, and
  `remove_value` on dotenv files now take their edit from `adotenv::set` and `adotenv::remove`
  (still anchored to the entry's value or line) instead of splicing one fixed encoding. A set
  keeps the quoting its context needs, so a value with text after its closing quote
  (`B="y"tail`) is now fixed instead of only suggested, and several keys removed together are
  checked together.

### Fixed

- **Two `.env` fixes no longer silently change a file's meaning.** A value `replace` on a
  quoted value with text after its closing quote (`D="q"tail` with `^` -> `v`) wrote
  `D=vqtail`, which reads back as `vqtail` and was reported as applied; it now writes
  `D="vq"tail`. And `remove_value` on a file's first line could expose a byte-order mark at
  the start of the next key as the file's BOM, silently renaming that key; such a removal is now
  declined.
- **`check --changed` and `fix --changed` no longer pay for whole-tree lookups they do not
  use.** v0.18.3 built the full tree's path sets on every `--changed` run so that
  `scope_filter.has_ancestor` could see unchanged markers; a run with no such lookup paid for
  it anyway (about 16% at 1M files on the bench host). The file set is now built only when a
  rule with a `has_ancestor` predicate will actually be evaluated against the changed files'
  index (so a repository whose `has_ancestor` rules are all out of the diff's scope pays
  nothing either).

## [0.18.3] - 2026-10-15

A correctness and security patch. `check --changed` and `fix --changed` now judge changed files
against the whole repository (a `scope_filter.has_ancestor` marker outside the diff counts, and
references to unchanged or deleted files resolve correctly), `alint fix` no longer edits files
that `scope_filter` excludes, nested `.alint.yml` configs confine every repository-relative field
(including through root templates and `require:` blocks), and rustls is updated for four new
RustSec advisories. Some nested configurations that never did what they said now fail to load;
see Changed.

### Security

- **rustls updated to 0.23.46** (RUSTSEC-2026-0346, -0347, -0348, -0349). alint uses rustls,
  through `ureq`, only to fetch remote `extends:` rulesets over HTTPS; the release blocks on
  RustSec advisories, so the patch picks up the fixed version.

### Changed

- **Some nested configurations that loaded before are now load errors**, because they never
  did what they said. In a nested `.alint.yml`, kinds whose selectors cannot be confined to the
  subdirectory (`cross_file` and `cross_file_value_equals`, `registry_paths_resolve`,
  `file_graph`, `pair_hash`, `pair_changed_together`, `changeset_requires_path`,
  `git_no_denied_paths`, and the `git_commit_*` kinds) are rejected, at the top level, inside a
  `require:` block, or through a root template; they used to run over the whole repository.
  And a rule nested under `for_each_dir`, `for_each_file`, or `every_matching_has` may no
  longer set `fix`, `git_tracked_only`, or `respect_gitignore`, which were silently discarded.
  Move such rules to the root config (or a top-level rule).

### Fixed

- **`alint fix` no longer edits files that `scope_filter` excludes, and nested structured rules
  honour it in `check`.** The structured `*_path_*` kinds (JSON, YAML, TOML, XML, dotenv,
  properties, INI, HCL) took a fast path for literal `paths:` that skipped `scope_filter`
  whenever the rule was evaluated on its own. That is how `fix` evaluates, so a `set_value` or
  `remove_value` fix could rewrite a listed file that `scope_filter` (for example
  `has_ancestor`) had excluded; and it is how a rule nested under `for_each_dir` is evaluated,
  so `check` reported such files too. The fast path now applies the rule's scope.
- **`check --changed` and `fix --changed` judge changed files against the whole repository.**
  The per-file pass matched scope and resolved references against the changed files only, so:
  a rule with `scope_filter.has_ancestor` never fired when its marker (an unchanged
  `Cargo.toml`) was outside the diff; `markdown_paths_resolve` reported an unchanged file it
  referenced as missing. Scope matching and lookups now see the full tree, while only changed
  files are read.
- **`check --changed` catches violations caused by changes elsewhere.**
  - `no_case_conflicts` and `max_files_per_directory` judge the whole tree but saw only the
    changed files, so adding `Readme.md` next to a committed `README.md`, or a file to a
    directory already at its limit, went unreported. They now evaluate on the full index (and
    are still skipped when no file in their scope changed).
  - `markdown_paths_resolve` now checks every in-scope document, so deleting or renaming a
    file that an unchanged document references is caught.
  - `json_schema_passes` re-validates every in-scope document when only its `schema_path:`
    changes; it used to be skipped.
- **A `git_blame_age` rule nested under `for_each_dir`, `for_each_file`, or
  `every_matching_has` now fires.** The engine built the blame cache only when a top-level rule
  needed it, so a nested `git_blame_age` silently never reported anything.
- **Nested `.alint.yml` configs confine every repository-relative field.** They used to prefix
  only `paths`, `select`, and `primary`, which corrupted the regex `select:` of `for_each_match`
  and `ordered_block` (now left alone) and let other fields escape the subdirectory: the path
  fields of rules nested under `require:`, `import_gate`'s `allow:`, `json_schema_passes`'s
  `schema_path:`, `markdown_links_resolve`'s `root.dir:`, and a literal `pair` `partner:` are
  now prefixed. Confinement also runs after `extends_template:` expansion, so a nested rule can
  no longer pick up an unconfined kind or selector from a root template.
- **`file_exists` with a literal path and `respect_gitignore: false` honours
  `git_tracked_only`.** The direct filesystem check for tracked-but-gitignored files accepted an
  untracked file on disk; it now requires the file to be tracked, as the glob form already did.

## [0.18.2] - 2026-10-13

A security and correctness patch: alint's terminal output now escapes every
invisible character its own rules flag, `.env` parsing and editing moves to the
new `adotenv` crate (fixing a `set_value` edit that silently never applied),
and a leading BOM no longer shifts columns or widths.

### Security

- **Terminal output no longer prints invisible characters that alint itself
  flags.** The sanitizer for the human, Markdown and GitHub-annotation output kept its own
  list of bidi and zero-width characters, and it had drifted from the rules':
  U+180E MONGOLIAN VOWEL SEPARATOR, which `no_zero_width_chars` flags, was
  printed raw. Both now read one definition (`alint_core::invisible`), and the
  sanitizer also escapes other characters that render as nothing or as a line
  break: Unicode tag characters (U+E0000-U+E007F, used to smuggle hidden
  text), U+2028/U+2029, soft hyphen, invisible math operators, Hangul fillers
  and the variation selectors supplement. Emoji variation selectors
  (U+FE00-U+FE0F) and right-to-left text are left alone. Rule results are
  unchanged.

### Changed

- `.env` parsing now comes from [`adotenv`](https://github.com/asamarts/adotenv),
  alint's dotenv parser extracted into its own dependency-free crate: literal
  values, byte spans, and in-place edits. The `dotenv_path_*` rules and the
  `set_value` / `remove_value` fixes read and edit `.env` files exactly as
  before; `structured_fix`'s separate hand-mirrored span scan is gone.

### Fixed

- `set_value` on a `.env` file: a new value starting with `#` was written bare,
  so on a line like `KEY = old # note` it became a comment and read back
  empty. The fix was never written (the edit is verified and was demoted to a
  suggestion), but it silently did not apply. Such values are now quoted.
- `no_bom`: a UTF-8 BOM on a file with a NUL byte past the first 16 bytes was
  marked fixable, but `file_strip_bom` refuses binary files, so `fix` always
  skipped it. Fixability now uses the fixer's own whole-file test (read only
  when a BOM is found). The message and baseline fingerprint are unchanged.
- A leading BOM no longer counts as a column: `no_bidi_controls`,
  `no_zero_width_chars`, `markdown_links_resolve` and `markdown_paths_resolve`
  reported first-line columns one too high in files with a BOM, and
  `line_max_width` counted the BOM toward the first line's width (a false
  positive on a line exactly at the limit). Columns are not part of baseline
  fingerprints.

### Documentation

- `file_starts_with` / `file_ends_with`: documented that binary files are
  checked too (deliberately, for file signatures), with advice to scope
  `paths`, since a paired `file_prepend` / `file_append` refuses binary files.

## [0.18.1] - 2026-10-12

A performance and installer patch. v0.18.0's correctness fixes to the
zero-width, bidi and blank-line content rules had made per-file content
checks about 3% slower and added a git process to every `--changed` run;
this release makes them faster than v0.17 with identical output. `install.sh`
also stops failing with a bare 403 on shared CI runners when it resolves the
latest release.

### Performance

- **Per-file content checks are back to v0.17 speed, and faster.** v0.18.0's
  correctness fixes to `no_zero_width_chars`, `no_bidi_controls` and
  `max_consecutive_blank_lines` (scan non-UTF-8 and NUL-bearing files too)
  decoded every file char by char, costing the per-file content benchmark
  (S2) about 3% wall time. Both character rules now skip files with none of
  the UTF-8 lead bytes their characters use, with a SIMD byte search, and the
  blank-line rule finds newlines the same way. Output is unchanged. On the
  bench host, S2 is 6-7% faster than v0.18.0 at 10k and 100k files, and S4
  (bundled rulesets) 3-4%; instruction counts drop 28% on S2 and 36% on the
  auto-fix scenario.
- **`alint check --changed` without a base runs its two git reads
  concurrently,** recovering most of the ~1.8 ms v0.18.0 added per run
  (18.2 to 16.7 ms on a 1,000-file tree).

### Fixed

- **`install.sh` no longer fails with a bare 403 resolving the latest
  release.** It looked the version up with one unauthenticated GitHub API
  call, which shares a 60-requests-per-hour limit per IP address; on shared
  CI runners that limit ran out and the install (and the GitHub Action's
  default version) failed. It now follows github.com's `/releases/latest`
  redirect, which isn't rate limited, falls back to the API, checks the tag's
  format before using it, and retries transient download errors.

## [0.18.0] - 2026-10-11

A security and correctness release from a whole-repository audit and an
adversarial re-review of its fixes. The config trust gates now judge the rule
that actually runs (no YAML tags, merge keys or placeholder kinds can hide a
field from them), untrusted remote rulesets can no longer declare `ignore:` or
read the environment, crafted YAML and XML files are bounded in memory and CPU,
and RustSec advisories now block CI and releases. It adds the
`markdown_links_resolve` rule kind and per-rule `expect_matches:` scope
assertions, and the LSP now resolves the nearest config per file. Breaking:
three config tightenings (see Changed), MSRV 1.88, and the VS Code extension
now requires VS Code 1.91.

### Security

- **Config trust gates no longer judge a different rule than the one that
  runs.** An `extends:`'d ruleset could execute arbitrary commands through a
  template whose `kind:` was a `{{vars.*}}` placeholder resolving to `command`,
  or through a spawning kind nested in a template's `require:` block; could
  promote `file_remove` to `safe` with a sequence-shaped `file_remove: [safe]`
  or an `applicability: "{{vars.a}}"` placeholder; and a rule from a remote or
  nested config could aim a trusted template's promoted fixer with its own
  `paths:`. All are refused or capped at load.

- **YAML tags and merge keys can no longer hide fields from the trust gates.**
  A tagged key or value (`!x kind: command`, `kind: !x command`) was invisible
  to the checks but still built the real field, so a remote ruleset could run
  commands, promote `file_remove` to `safe`, or read the environment. Rules and
  templates in every config now refuse YAML tags, non-string keys and `<<`
  merge keys.

- **An extended config can no longer reshape one of your process-spawning
  rules.** A rule that shared the id of your `generated_file_fresh` /
  `command_idempotent` rule (or a template it instantiates) field-merged a
  `workdir:` or `paths:` into it and chose what ran. A spawning rule, or a rule
  with a spawning fix op, now fails to load if any `extends:`'d config
  contributed to it. `workdir:` is also confined to the repository, including
  through symlinks.

- **Untrusted remote rulesets** (an `https://` source not in
  `trusted_extends:`) may no longer read the environment through `when:` /
  `when_iter:` (`env.*` directly, a `{{...}}` placeholder in a template's
  `when`, or a var whose value comes from `{{env.X}}`), may no longer splice
  an env-derived template var into a rule field such as `message:`, and may
  no longer declare top-level `ignore:`, which could hide every file from
  every rule.
  Each was a way to leak a secret through which rules fire, or to silence the
  run. Allowlist the URL in `trusted_extends:` to restore the old behavior.

- A remote ruleset can no longer route an environment variable into
  `git_commit_message`'s `since:` (directly, by field-merge onto a user rule,
  or through template `vars:`, including a `${` reference assembled from two
  vars), where it was expanded and echoed in an error.

- Fixer trust now follows the effective rule through field composition and
  template expansion, so neither an untrusted rule instantiating a trusted
  template nor the reverse can keep an untrusted content fixer at its declared
  tier.

- The XML nesting pre-scan now agrees with the XML parser at every skipped
  region (processing instructions, comments, CDATA, the XML declaration) and
  fails closed, so a crafted file can no longer abort alint with a stack
  overflow. The YAML depth and alias guards, which also protect `.alint.yml`
  and `extends:` loading, now follow libyaml's token rules, so comments, block
  scalars, plain-scalar continuations and a leading BOM can no longer hide a
  nesting or alias bomb.

- **Crafted YAML and XML files can no longer exhaust memory or CPU.** The YAML
  alias budget is charged in estimated bytes rather than nodes, so one large
  anchored scalar referenced a few thousand times (a 1 MB file that used to
  need 3.8 GB) or a map-heavy anchor is rejected up front. YAML flow nesting is
  capped at 128 levels, the parser's own limit. XML files with more than 128
  namespace bindings in scope, or with excessive namespace-declaration work,
  are rejected before parsing (a 448 KB file used to take 40 s).

- `no_bidi_controls` and `no_zero_width_chars` no longer skip a file because
  it contains a NUL byte: a binary-looking file that is otherwise valid UTF-8
  is scanned, and its findings are reported but not auto-fixed. Binary files
  that are not valid UTF-8 (images, fonts, archives) are still skipped.

- `file_graph` `require: fresh` no longer reads a target through an in-repo
  symlink that points outside the repository.

- `alint baseline` no longer writes outside the repository through a
  `baseline:` path or a committed symlink (a repo-provided path containing
  `..` is refused), and `alint init` no longer follows a dangling `.alint.yml`
  symlink (both exit 2). A regenerated baseline keeps its file permissions.

- Terminal output from `list`, `explain`, `suggest`, `facts`, `init`,
  `baseline`, `validate-config`, `export-agents-md`, `--show-baselined`,
  `--show-notes` and error messages is sanitized, and the sanitizer now also
  escapes bidi and zero-width characters. `export-agents-md` sanitizes only
  when stdout is a terminal, so `> AGENTS.md` writes the same text as
  `--output`.

- Rule and fact timeouts (`command`, `generated_file_fresh`,
  `command_idempotent`, the `command` fix op, custom facts) no longer wait on a
  background grandchild holding the output pipe. When stdin is not a terminal
  (CI, the LSP, piped runs) on Unix, the child runs in its own process group:
  a timeout kills the whole group, and descendants left holding the output
  pipes after a normal exit are killed too. In an interactive terminal the
  child stays in alint's process group, so Ctrl-C reaches it and password
  prompts work; there a timeout kills only the direct child.

- CI and release workflows no longer expand dispatch inputs or step outputs
  inside shell scripts; the release Docker base image is pinned by digest,
  the Gradle wrapper is checksum-verified, the Homebrew tap push trusts
  GitHub's published SSH host keys instead of `ssh-keyscan`, and Dependabot
  covers the npm, editor, Gradle and Docker dependencies. The script-injection
  gate also catches bracket accessors, function calls and multi-line
  expressions.

- **RustSec vulnerability advisories now fail CI and the release preflight**
  (`cargo deny` and `cargo audit`); unmaintained, unsound and yanked notices
  stay warnings. Waivers go in one place, `deny.toml`
  `[advisories].ignore`, each with an advisory id and a reason.

### Added

- New per-rule `expect_matches: true` scope assertion: after a rule's `when:`
  gate is true, alint reports a violation when `paths:` plus `scope_filter:`
  select no files. The assertion checks the full repository even in
  `--changed` mode, works in nested `require:` rules, and is rejected for
  existence and selector rules whose empty sets already have kind-specific
  meaning.

- New `markdown_links_resolve` rule kind validates Markdown links, images,
  reference definitions, and explicit reference labels against repository
  files and directories. Its CommonMark parser ignores code examples, front
  matter, and HTML comments without misclassifying footnote definitions;
  reports each bad link at its source location; supports a
  `relative: forbid` mode for trailing-slash documentation sites; and can map
  root-absolute URL prefixes back to source directories.

- `alint fix --format json` reports a top-level `dry_run` flag, and skipped
  items carry a `skip_kind` (both optional in the v1 fix-report schema).

- The `when:` expression language accepts `not not x` and negative integer
  literals (`facts.n > -1`). A chain of up to 64 `not x and ...` terms parses.

- `alint lsp` resolves the nearest `.alint.yml` per file (or the outermost
  one with `nested_configs: true`, matching `alint check`), lints every
  workspace folder, honors `baseline:` and `fix_size_limit`, shows fix
  availability and the rule-reference link on hover, and supports
  `--show-notes`. Files outside the workspace folders are not linted.

### Changed

- **Breaking (config):** a template may no longer use a `{{vars.*}}`
  placeholder in `kind:` or in a fix's `applicability:`, and a fix op's
  options must be a mapping (`file_remove: {}`), never a sequence. Rules and
  templates may not use YAML tags, non-string keys or `<<` merge keys.

- **Breaking (config):** an `extends:`'d config may no longer contribute
  fields to a process-spawning rule by sharing its id; rename the rule. An
  untrusted remote ruleset may no longer declare `ignore:` or read the
  environment in `when:` (see Security).

- **Breaking (config):** `registry_paths_resolve` no longer accepts
  `orphans.unreferenced`; orphan findings use the rule's own `level`, and
  directories count as orphan candidates.

- **MSRV raised to Rust 1.88** (from 1.85) for `cargo install alint` and the
  library crates. CI and the release preflight now read the MSRV from
  `Cargo.toml` and gate publishing on it.

- A content rule that cannot read an in-scope file now reports `could not
  read file: <error>` at the rule's level (which fails the run at `level:
  error`) instead of silently passing it, in both `check` and `fix`.

- `alint fix` refuses to replace a file it could not write in place (a
  read-only file, a `0464` file, or one owned by another user), reported once
  as a fix error, instead of silently replacing it through rename.

- `alint fix` lists a finding that `check` does not tag fixable (a UTF-16 BOM,
  a binary bidi finding, an ambiguous reindent) as `unfixable` instead of
  `skipped`, without running its fixer.

- `alint fix --dry-run` reports a `file_create` whose target is ignored or
  outside the rule's `paths:` as unresolved (exit 1), as the real run does. A
  dry run is still one pass; see `alint fix` docs for what only the real run
  can detect.

- **Baselines:** `file_graph` `acyclic` keys findings per strongly connected
  component, and `no_case_conflicts` reports directory-level collisions, so
  baselines covering those findings must be regenerated with `alint baseline`.

- YAML structured rules apply plain `<<` merge keys (a quoted `"<<"` is an
  ordinary key) and accept custom tags (`!Ref`, `!reference`, ...) by keeping
  the tagged value.

- `*_path_equals` compares numbers by value in every format (`1` equals
  `1.0`, exactly, even above 2^53); a string never equals a number.

- `.alint.d/` drop-ins no longer reset top-level settings they leave unset
  (`respect_gitignore`, `fix_size_limit`, `nested_configs`, `version`). An
  `extends:`'d config's values for those settings, which were always
  replaced, are now ignored with a warning.

- `include_manifest_paths` / `exclude_manifest_paths` honor negated
  workspace entries (`!packages/internal`), applied in order with the last
  match winning (as npm does), and large member lists are indexed (5k
  packages over 100k files went from about 4 s to 0.6 s).

- SARIF results without a path are anchored on line 1 of the config file
  (GitHub Code Scanning drops location-less results), the run declares
  `columnKind: "unicodeCodePoints"`, and `:` in artifact URIs is
  percent-encoded.

- `check --changed` without `--base` also includes staged files, and config
  discovery from a relative start path walks every real ancestor directory.

- A `for_each_dir` / `for_each_file` / `every_matching_has` parent's
  `message:` now replaces its nested findings' messages.

- The bundled `agent-context` ruleset's `agent-context-no-stale-paths` rule
  now uses `markdown_paths_resolve`, which can change its findings.

- The GitHub Action carries a baked default binary version and resolves
  commit-SHA refs to it.

- `json_schema_passes` uses `jsonschema` 0.58.

- The VS Code extension requires VS Code 1.91 or newer (was 1.85); older
  editors keep the last compatible extension release. It uses
  `vscode-languageclient` 10, so `alint.trace.server` now takes effect only
  while the `alint` output channel's log level is Trace.

- Building the JetBrains plugin now needs JDK 21 (Gradle 9.8, IntelliJ
  Platform Gradle Plugin 2.19, Kotlin 2.4). The plugin itself still supports
  IntelliJ 2024.2+, and the Marketplace verifier now checks it against
  2024.2 as well as 2024.3 (it previously checked only 2024.3).

- The release job and every downstream publisher (npm, PyPI, VS Code, Open
  VSX, JetBrains) can be re-run after a partial failure without failing on an
  already-published version, and a re-run publishes a release left as a
  draft. A backport release (or a re-run of an older tag) no longer moves
  `v0`, the GitHub "Latest" release, Docker `:latest` / `:X.Y`, Homebrew or
  npm `latest` backwards; an npm backport publishes under `release-X.Y`.
  Release binaries build with a pinned Rust toolchain (1.98.1) instead of the
  day's stable.

- `install.sh` runs nothing if its download is truncated, fails clearly when
  neither `HOME` nor `INSTALL_DIR` is set, and no longer aborts intermittently
  with SIGPIPE while resolving the latest version. The documented one-liner
  uses `curl -fsSL`. `npm install` on Windows on ARM installs the x64 binary
  instead of failing.

- **Rule-kind counts now distinguish implementations from aliases.**
  `facts.json` format v3 reports 95 canonical rule kinds in
  `counts.rule_kinds` and 11 alternative spellings in
  `counts.rule_aliases`; its `rule_kinds` list retains all 106 accepted names
  for compatibility. The generated docs `manifest.json` format is v4 and
  carries the same split as `rule_kinds_total` / `rule_aliases_total`.

### Fixed

- `alint fix` no longer reports a fix as applied (exit 0) when the violation
  still stands on re-check, such as a `file_create` into a gitignored path; it
  is reported as unresolved and exits 1.

- Glob characters in directory names (Next.js `app/[slug]`, a stray `{`, a
  `\` on Unix) are matched literally in nested rules' glob options (`paths:`,
  `select:`, `require:`, ...) and `iter.has_file`, and messages show the real
  path. `{dir}` for a root-level file no longer renders a leading `/`. `pair`
  normalizes `.` and `..` in the partner path and reports a partner that
  resolves outside the repository.

- `command` rules no longer report a false timeout when the tool prints more
  than about 64 KiB.

- The directory-children index builds in linear time (a 20k-directory tree
  went from about 40s to milliseconds).

- `file_content_forbidden` (and its `replace` fix), `no_merge_conflict_markers`,
  `line_max_width` and `max_consecutive_blank_lines` no longer silently skip
  text files that are not valid UTF-8 (Latin-1, for example). Binary files
  are still skipped, and `replace` never edits one.

- `file_graph` `acyclic` reports every file on a cycle, once per strongly
  connected component.

- Structured `set_value` / `remove_value` fixes now apply to JSON and JSONC
  files containing escaped surrogate pairs (`"\ud83d\ude00"`); such files were
  reported fixable but always skipped. The fix side still accepts exactly the
  dialect the check side does (comments and trailing commas only).

- `file_header` matches the header after a leading UTF-8 BOM, so its fixes
  converge; `file_strip_bom` no longer corrupts UTF-16/UTF-32 files, which are
  now reported but not fixed.

- `markdown_paths_resolve` normalizes `./` and `..`, recognizes fences inside
  blockquotes (without closing a fence on a quoted fence line inside it),
  keeps scanning past an unmatched backtick in linear time, and reports
  columns in code points; `markdown_links_resolve` reports `%2F...` targets
  instead of skipping them.

- `no_case_conflicts` detects file/directory and directory/directory case
  collisions, alongside same-directory collisions beneath them;
  `no_illegal_windows_names` follows Microsoft's naming rules
  (control characters, `\`, `COM0`/`LPT0`, superscript ports, `CON .txt`).

- An `extends:` chain that reaches the same file along many paths loads it
  once (a diamond chain used to take exponential time).

- An unknown key in an `extends:` entry (`excpet:`) or a fact with a second
  kind key is a load error instead of being ignored.

- Config parse errors name the file (or remote URL) they came from and no
  longer print the parser message twice.

- The published config schema covers all 26 fix ops, and the
  `applicability` field of the 24 that take options.

- Fixes to files with very long names no longer fail with `ENAMETOOLONG`;
  baseline fingerprints no longer conflate `a\b.txt` with `a/b.txt` on Unix;
  identical same-offset inserts are applied once.

- LSP diagnostics use UTF-16 columns, findings on `.alint.yml` are no longer
  wiped after a check, a full check no longer overwrites diagnostics for
  unsaved buffers, a slow evaluation of an older buffer no longer replaces
  newer results, a broken config clears stale diagnostics, and the server
  exits on `exit` with the spec's exit code.

- A very large `timeout:` no longer crashes alint or the LSP.

- `ALINT_LOG` output follows `--color`, `NO_COLOR` and the stderr TTY; a
  closed stdout (`| head`) exits quietly with the run's normal exit code in
  every subcommand, including `validate-config` and `init`;
  GitHub, GitLab and JUnit output use `/` separators on Windows.

- CI: `cargo deny` runs on `deny.toml`-only changes, packaging files get a
  pre-merge job, the shell harnesses run for every file they guard
  (`install.sh`, the Dockerfile, npm, Dependabot, the Gradle wrapper), the
  action self-test exercises the commit's own `install.sh`, and the CI summary
  fails when change detection fails.

- `markdown_paths_resolve` now recognizes backticked command invocations such
  as `` `tools/run.ts --check` ``: when the complete span is not a path, the
  rule accepts it if its command path resolves and the remainder is
  argument-shaped. Complete spans are resolved first, so real paths containing
  whitespace continue to work, while a missing command path still fails.

- Docs-bundle publishing now keeps generated rule indexes pinned to the
  released rule-page set, rejects dangling rule-index links before publishing,
  serializes every trigger that writes the shared branch, and uses a
  non-forced push so a stale writer cannot overwrite a newer bundle.

- Cache-backed `scope_filter.changed_since` and manifest-derived predicates are
  now resolved for nested `require:` rules as well as top-level rules. Deeply
  nested rule trees are also validated recursively during config loading, so an
  invalid descendant cannot remain hidden when its parent selector matches
  nothing.

- **Documentation audit follow-up.** Search descriptions are plain text and
  capped at 158 characters by a regression test; malformed changelog code
  spans, a stale runtime documentation URL, duplicate migration links, retired
  benchmark routes, stale benchmark headlines, and mixed-era Kubernetes case
  study claims are corrected.

## [0.17.0] - 2026-10-01

This release adds `alint fix`, an auto-fix engine that applies or suggests edits across 26 fix operations.

### Added

- New `replace` auto-fix op for `file_content_forbidden`: rewrites each span
  matching the rule's `pattern:` with a `replacement` template (regex `$1` /
  `${name}` capture references). It is the first *located* fix -- one byte-range
  edit per match, spliced in a single pass -- and is `Unsafe` by default (a regex
  rewrite is not behavior-preserving), so a bare `alint fix` surfaces it as a
  suggestion and `alint fix --unsafe-fixes` applies it. Example: rewrite
  `console.log` to `logger.debug`, or strip a banned token. A replacement that
  would itself still match the forbidden pattern (e.g. `foo` -> `foofoo`, or a
  zero-width pattern) is left unfixed with a warning rather than applied, so the
  fix never loops or grows the file.
- The LSP now offers the located `replace` fix as an editor code action. Its
  byte-range edits are mapped to LSP `TextEdit`s with UTF-16 character positions
  (so a fix after a non-BMP character such as an emoji lands in the right place),
  and every occurrence in the file becomes one `TextEdit` in a single
  `WorkspaceEdit`, so applying the action rewrites them all at once.
- New `trusted_extends:` top-level config key: a list of remote `extends:` URLs
  whose content-injecting fixers (`replace` / `file_create` / `file_prepend` /
  `file_append`) are honored at their declared tier instead of demoted to a
  suggestion (see the security note under Changed). Use it to opt a company's
  internal ruleset host back into auto-applying its own content fixes. Only your
  own top-level config (or a `.alint.d/` drop-in) may grant trust; a ruleset may
  not allowlist itself. A per-rule `applicability:` field was also added to the
  `file_create` / `file_prepend` / `file_append` fix ops.
- New located structured fix ops `set_value` (for the `*_path_equals` kinds) and
  `remove_value` (for the `*_path_absent` kinds): they edit a structured config
  value in place, preserving comments, key order, and line endings by splicing
  only the target node's byte range rather than re-serializing the document.
  `set_value` rewrites the value at the rule's `path:` to its `equals:` (Safe for
  a scalar replacing an existing scalar; object/array/insertion cases decline);
  `remove_value` deletes the matched node with its separator (Unsafe by default).
  Available for **HCL** now; the other structured formats surface the violation
  but decline the fix until their span resolvers land. `set_value` is
  content-injecting, so a remote `extends:` demotes it under the trust boundary
  above.
- New `chmod` auto-fix op for `executable_bit` and `shebang_has_executable`
  (`fix: { chmod: {} }`): sets or clears the Unix executable bits (`0o111`) to
  match the rule, preserving every other permission bit. `executable_bit` picks
  the direction from `require:`; `shebang_has_executable` always sets +x. Safe by
  default (the rule explicitly requires the state, the change is reversible, and
  only `0o111` changes); a per-rule `applicability:` can demote it. Unix-only, as
  the rules are; `alint fix --diff` previews it as a git-style `old mode`/`new
  mode` change.
- New `git_untrack` auto-fix op for `file_absent` (`fix: { git_untrack: {} }`):
  runs `git rm --cached` to drop a committed-but-forbidden path from git's index
  while leaving it on disk, for the "a build artifact got committed and should be
  untracked" hygiene case (pair it with `git_tracked_only: true` so the rule
  converges once the path leaves the tracked set). It is the first *spawning* fix
  op, so -- exactly like a `kind: command` rule -- it is refused from any
  non-top-level source (an `extends:`'d or bundled ruleset, a `templates:` block,
  or a nested `.alint.yml`) to keep an adopted ruleset from shelling out on a bare
  `alint fix`; declare it only in your own top-level config. `Unsafe` by default
  (it restructures the git index), so a bare `alint fix` surfaces it as a
  suggestion and `alint fix --unsafe-fixes` applies it.
- The `command` plugin rule can now carry a fix (`fix: { command: { run: [...] }
  }`): a user-supplied fix command run per violation, with the same `{path}` /
  `{dir}` / `{stem}` templates (and option-injection guard) as the rule's
  `command:`. Pair a checker with its fixer, e.g. check `eslint {path}`, fix
  `eslint --fix {path}`. Like `git_untrack` it is a spawning fix -- refused from
  any non-top-level source, so an adopted ruleset can never run a command on your
  `alint fix` -- and `Unsafe` by default (a bare `alint fix` suggests; a top-level
  `applicability: safe` opts a specific rule into running on a bare fix). Its
  idempotence is the command author's business, so it is exempt from the harness's
  convergence requirement.
- New `dir_create` auto-fix op for `dir_exists` (`fix: { dir_create: {} }`):
  creates the required directory (recursively) when it is missing. `Safe` by
  default (an empty directory is benign). The rule's `paths` must be one literal
  directory (a glob or multiple patterns is ambiguous and rejected at load). Note
  that git does not track an empty directory, so pair it with a `file_create` of a
  `.gitkeep` if the directory must persist in the repo.
- New `sync_from` auto-fix op for `cross_file` (`fix: { sync_from: {} }`), with
  two behaviors by relation:
  - On **`relation: identical`** it overwrites a drifted target with the canonical
    `source:` file so the two are byte-identical again -- the workspace
    LICENSE-mirroring case, where each crate's copy must match the root one.
  - On **`relation: equals`** it PROPAGATES the source's single extracted value
    into each drifting target, preserving the rest of the file -- the "one source
    of truth, propagate to N others" case (e.g. a workspace version to each crate's
    `$.package.version`). Works for a structured target extract (any
    toml/json/yaml/xml/ini/hcl/dotenv/properties JSONPath -- rewrites the located
    node) and a regex target extract (rewrites each match's capture group 1, then
    re-verifies -- e.g. a version in a README badge or a Dockerfile `ARG`).
  `Unsafe` by default (a whole-file overwrite / a ruleset-chosen value can change
  the target), so a bare `alint fix` suggests it and `alint fix --unsafe-fixes`
  applies it. `identical` requires `skip_header_lines: 0`; a set / resolves
  relation is rejected at load. Both the source read and the target write are
  confined to the repo root.
- New `relocate` auto-fix op for `file_absent` (`fix: { relocate: {} }`): moves a
  file the rule flagged in a subdirectory back to the repository root, keeping its
  basename -- the "a lockfile drifted into a member directory" case (a `Cargo.lock`
  / `package-lock.json` / `poetry.lock` belongs at the workspace root, so a rule
  like `paths: "**/*/Cargo.lock"` flags a nested one and `relocate` moves it up).
  `Unsafe` by default (a rename moves a real file and the destination is inferred),
  so a bare `alint fix` suggests it and `alint fix --unsafe-fixes` applies it; a
  per-rule Safe promotion is available from top-level config. Only the unambiguous
  case is fixed: a file already at the root, an occupied root slot, or an
  undecodable basename is reported for a human, never clobbered. Anchor the
  pattern to subdirectories (`**/*/Cargo.lock`, not `**/Cargo.lock`) so the
  relocated root file converges.
- New `cross_file` `relation: registered` + `create_and_register` auto-fix op: a
  workspace member (a filesystem path from `source.file` / `source.files`, mapped
  through `register_as`) must be listed in each target's manifest array. The check
  flags an unregistered member; the fix APPENDS it to the list.

      - id: members-registered
        kind: cross_file
        relation: registered
        source: { files: "crates/*/Cargo.toml" }   # each crate (by its manifest)
        register_as: "{dir}"                        # register its directory
        targets:
          - { file: Cargo.toml, extract: { toml: "$.workspace.members[*]" } }
        fix: { create_and_register: {} }

  The target `extract` selects the array elements with a trailing `[*]` (so the
  check sees each element and the fix locates the array); the array path must be
  static (one array). `register_as` templates the value with `{path}` (default) /
  `{dir}` / `{stem}`. Prefer a manifest-anchored source glob
  (`crates/*/Cargo.toml`, not a bare `crates/*`), since a bare directory glob also
  matches loose files under the directory. `create_and_register` is `Unsafe` by
  default (it mutates a manifest), `Safe`-promotable, and content-injecting (an
  untrusted remote `extends:` demotes it to a suggestion). The fix appends TOML
  lists (format-preserving via `toml_edit`, skipping a member already present). For
  a NAMED `source.file` that is MISSING, a `content:` / `content_from:` on the fix
  also CREATES the member's file -- existence and registration are two independent,
  idempotent postconditions, each repaired only if unmet, so a missing member is
  both created and registered. JSON / YAML list-append and creating a missing
  `members` array are follow-ups.
- New `sort` auto-fix op for `ordered_block` (`fix: { sort: {} }`): reorders the
  entries of each marked block (or, with no `start` / `end` markers, the whole
  file -- the `CODEOWNERS` / allow-list shape) under the rule's `comparator`,
  dropping duplicates when the rule sets `unique:`. It reuses the rule's own
  `start` / `end` / `comparator` / `unique` / `select`, so what `sort` reorders
  is exactly what the check flags out of order; markers, blank lines, and
  `select`-excluded lines (comments, group headers) stay in place, and every
  line keeps its exact terminator (LF vs CRLF) and the file its trailing-newline
  state. A pure reorder is `Safe` (behavior-preserving), so a bare `alint fix`
  applies it; but a `unique` sort DELETES lines -- and equality is on the
  comparator (trimmed / case-folded), so a dropped line need not be byte-identical
  to its survivor -- so `unique` defaults to `Unsafe` like every other deleting
  fixer (a bare `fix` suggests it, `--unsafe-fixes` applies it). A per-rule
  `applicability:` overrides either default. Because a remote `extends:` could aim
  a reorder at an order-significant file such as `.gitignore` (negation order) or
  `CODEOWNERS` (last-match precedence) to change its meaning, a `sort` from an
  untrusted (non-`trusted_extends:`) remote is demoted to a suggestion, exactly
  like `sync_from`; your own top-level config applies it at its tier. The one
  `ordered_block` finding `sort` cannot repair -- an unclosed block (a `start`
  with no `end`) -- is reported but not advertised as auto-fixable, since `sort`
  cannot invent a missing marker.
- New `indent_style` auto-fix op for the `indent_style` rule (`fix: {
  indent_style: {} }`): reindents tab-indented lines to spaces. It is wired ONLY
  for a `style: spaces` + `width: N` rule (the `width` supplies the spaces-per-tab)
  and converts a line whose leading whitespace is PURE TABS to `N` spaces per tab
  (1 tab -> N, 2 tabs -> 2N), preserving the rest of the line, its terminator, and
  the trailing-newline state. It declines the genuinely ambiguous cases -- a mixed
  tab+space lead, or a pure-space run that isn't a multiple of `width` (round up or
  down?) -- so `check` does not advertise those as auto-fixable. A `tabs`-style or
  width-less rule with a `fix` is rejected at load (spaces->tabs has no
  spaces-per-tab). This lifts the previous "reindentation is deferred" rejection.
  **`Unsafe` by default**: a pure-tab reindent is behavior-preserving for the
  common code file, but a mis-aimed one HARD-breaks an indent-significant file --
  a `Makefile` recipe requires a literal tab, so converting it to spaces silently
  breaks the build -- so, like `file_remove`, a bare `alint fix` suggests it and
  `--unsafe-fixes` (or a per-rule `applicability: safe`) applies it. And, like
  `sort`, an untrusted remote `extends:` cannot auto-apply it (it is demoted to a
  suggestion, since a remote could aim it at your files).
- New `require:` field on `ordered_block` + a new `insert_line` auto-fix op
  (`fix: { insert_line: {} }`): `require:` lists exact lines the block must
  contain, and `insert_line` splices a missing one at its SORTED position (using
  the rule's `comparator`) -- the differentiator over `file_append`, which only
  appends at end-of-file. It is the "a managed sorted list (a `CODEOWNERS`, an
  allow-list) must contain these entries, in order" shape; pair it with a second
  `fix: { sort: {} }` rule to also reorder existing entries. Supported for a
  MARKERLESS `ordered_block` only (the whole file is one sorted list); `require:`
  with a `start`/`end` marker is rejected at load (a multi-block insert target is
  ambiguous). A missing-required-line finding is reported and auto-fixable
  independently of the sortedness findings (an out-of-order entry is `sort`'s job,
  not `insert_line`'s). **`Unsafe` by default**: it adds ruleset-authored content
  at a COMPUTED position, which is load-bearing in the order-sensitive formats it
  targets (a `.gitignore` negation must FOLLOW its pattern), so a bare `fix`
  suggests it and `--unsafe-fixes` (or a per-rule `applicability: safe`, e.g. for
  an order-tolerant `CODEOWNERS`) applies it; the `require:` lines are also
  ruleset-authored, so an untrusted remote's `insert_line` is demoted to a
  suggestion. Presence is exact-string (independent of `comparator`); a `require:`
  line that could never round-trip to an entry -- one carrying an embedded line
  break, or (with `select:`) one that does not itself match `select:` -- is
  rejected at load, so a fix can never re-insert it forever.
- New `insert_header` auto-fix op for `file_header` (`fix: { insert_header: {
  content: ... } }`): the position-aware alternative to `file_prepend` (same
  `content` / `content_from`). It inserts the required header AFTER a leading UTF-8
  BOM, shebang (`#!...`), or XML declaration (`<?xml ...?>`), so it never displaces
  a line that must stay first -- unlike `file_prepend`, which would push a shebang
  off line 1. For a file with none of those prefixes it inserts at BOF, exactly
  like `file_prepend`. **`Safe` by default** (the insertion point is the one
  canonical header spot and the content is inert -- at least as safe as the `Safe`
  `file_prepend` it refines, and safer on a file with a shebang / XML declaration);
  the header bytes are ruleset-authored, so an untrusted remote's `insert_header` is
  demoted to a suggestion. Its idempotency guard anchors on both the insertion point
  and the file top, so a repeated fix is a guaranteed no-op even when the header
  content itself begins with a `#!` / `<?xml` prefix. Because the header can land
  below line 1, the rule's `pattern` must be able to match below the first line (use
  an unanchored pattern or `(?m)`, not a `^`-anchored one).

### Changed

- **Breaking:** `file_remove` (the fix for `file_absent`, `no_empty_files`,
  `no_submodules`, `no_symlinks`, and the bundled `hygiene/no-tracked-artifacts`
  ruleset) is now **Unsafe** rather than Safe. A bare `alint fix` no longer
  deletes files; it surfaces the removal as a suggestion. Apply removals with
  `alint fix --unsafe-fixes`, or promote a specific rule back to auto-applied
  with `fix: { file_remove: { applicability: safe } }` in your own top-level
  config (an `extends:`'d ruleset may not promote it). Deleting a whole file
  irreversibly is a poor default for an unattended fix. `--unsafe-fixes`, which
  was previously inert, now gates this tier.
- `alint fix --changed` now SURFACES a required out-of-scope write as a suggestion
  instead of silently dropping it. When a full-index rule (e.g. `file_absent` +
  `file_remove`) would fix a file OUTSIDE the working-tree diff, that fix is no
  longer quietly discarded; it is reported as a suggestion (apply it with a full
  `alint fix`), so `fix --changed` and `check --changed` now agree on which
  violations stand. A file that a fix in scope CREATES during the run joins the
  changed set, so a create-then-fix cascade completes under `--changed` instead of
  leaving the created file half-fixed. Writes to files inside the diff are applied
  exactly as before, and no file OUTSIDE the diff is ever modified or deleted (an
  in-scope fix may still additively create a new file, e.g. a required file, which
  is the one write the confinement model permits beyond the diff).
- **Security / breaking:** a **content-injecting** fixer (`replace`, `file_create`,
  `file_prepend`, `file_append`) reached through a **remote `https:// extends:`**
  now DEMOTES to a suggestion -- it may propose an edit but no longer auto-writes
  third-party-authored bytes into your files. Fixers from your own tree (top-level,
  local-path / nested `extends:`) and first-party bundled rulesets are unaffected,
  as are the fixed-behavior fixers (the hygiene normalizers, `file_remove`,
  `file_rename`, `chmod`) from any source. Opt a specific remote back in with
  `trusted_extends:` (above). This retroactively affects `file_create` /
  `file_prepend` / `file_append` reached via a remote `extends:` (they previously
  auto-applied).
- Baseline fingerprints for `no_zero_width_chars`, `no_bidi_controls`, and
  `max_consecutive_blank_lines` now key on the file path (the file is the unit of
  accepted debt), matching `no_trailing_whitespace`, so `alint fix --baseline`
  never strips a grandfathered occurrence when a new one appears ahead of it. This
  CHANGES those three rules' fingerprint: a baseline recorded by an older alint
  will show a soft "stale entry" note for them on upgrade -- re-run `alint
  baseline` to re-tighten. Other rules' baselines are unaffected.

### Fixed

- The `ordered_block` `numeric` comparator now orders integers larger than
  `i64::MAX` (u64-range identifiers such as Discord/Twitter snowflakes, nanosecond
  timestamps) NUMERICALLY. It parsed the leading integer as `i64`, so a value over
  ~9.2e18 failed to parse and silently fell back to a byte-wise (lexical) order --
  which put `10000000000000000000` before `9999999999999999999`. It now parses as
  `i128` (only a 39+-digit integer still degrades to lexical), so both `check` and
  the new `sort` fix order such lists correctly.
- `alint check` now tags a violation `fixable` (and counts it as "auto-fixable")
  only when a bare `alint fix` would actually resolve that specific violation,
  rather than whenever its rule declares a fixer. Two cases that were falsely
  advertised as fixable no longer are: a `filename_case` violation on a stem with
  no valid target under the requested convention (e.g. `café.rs` under `snake`),
  and an `Unsafe`-tier `file_remove` violation (which a bare `fix` only suggests,
  pending `--unsafe-fixes`). The "N auto-fixable" total, the `fixable` tag, and
  the `agent` format's per-violation `fix_available` / `fix_command` all now match
  what a bare `alint fix` applies.
- A symlink cycle in the tree (a `sub/up -> ..` parent link, a self-loop, or a
  mutual loop) no longer aborts the entire `alint check` / `alint fix` run with
  exit 2. The offending link is skipped, like a dangling symlink, so the rest of
  the repository is still linted and fixed.
- `alint fix --diff` now emits the git extended-header form for an empty-file
  create or delete (a `.keep` / `py.typed` marker, or removing an empty file), so
  `git apply` no longer silently drops it from a multi-file patch.
- `alint fix --fix-only` now surfaces a fix that was attempted and errored (e.g.
  a write into a read-only directory) instead of silently dropping it with the
  benign residuals; previously the run exited nonzero with a report that read as
  a clean success and no cause anywhere.
- `alint fix --diff` now summarizes (rather than echoing) file content that
  carries raw terminal-control bytes (ESC, BEL, ...), so a diff of an untrusted
  repo -- or of a config-controlled `replace` replacement -- can no longer inject
  a screen-clear / banner-forge sequence into the terminal.
- A malformed single-op `fix:` block (a missing required field or an unknown
  field, e.g. `replace:` without `replacement`) now names the offending op in
  the error instead of the misleading "a map with exactly one fix op".

## [0.16.1] - 2026-09-04

This release ships the PyPI distribution channel: alint is now installable via
`uvx` / `pipx` / `uv tool` / `pip`, with a fast `language: python` pre-commit hook.
Each wheel embeds the prebuilt, attested native binary and is published tokenlessly
through PyPI Trusted Publishing with PEP 740 attestations.

### Added

- **PyPI distribution channel.** alint is now installable from PyPI: `uvx alint`,
  `pipx run alint`, `uv tool install alint`, and `pip install alint`, plus a fast
  `language: python` pre-commit hook via the `asamarts/alint-pre-commit` mirror. Each
  per-platform wheel embeds the prebuilt, attested native binary (no source build, no
  PyO3, no Python in the hot path), so it installs cleanly where the npm postinstall
  wrapper cannot (`--ignore-scripts`, `bunx`, offline mirrors). Published tokenlessly via
  PyPI Trusted Publishing with PEP 740 attestations; the wheel payload is byte-identical
  to the release binary, so the release's build provenance transitively covers it. See
  ADR-0016 and the installation guide.

### Changed

- macOS release binaries pin their deployment-target floor (10.12 on x86_64, 11.0 on
  arm64) so it no longer drifts with the Rust toolchain, keeping the PyPI wheel platform
  tags stable.

## [0.16.0] - 2026-09-02

This release completes the config-format coverage arc and hardens every config
parser against crafted-file denial-of-service. XML becomes a first-class target
across every format-aware rule, and dotenv (`.env`), Java `.properties`, INI /
`.cfg`, and HCL (Terraform / Nomad / Packer) join as new config formats, each with
a `*_path_{equals,matches,absent}` rule family, `json_schema_passes` support, and
`extract:` in the cross-file rules; a new `*_path_absent` kind family (JSON / YAML /
TOML / XML) asserts a query matches nothing. Alongside the surface work, a
multi-round adversarial audit bounded every parser against crafted-file DoS
(recursion / nesting / size caps, YAML flow-depth and alias-expansion limits robust
to quote-based evasion, an HCL depth pre-scan matching hcl-rs at every content
boundary, an XML attribute-count cap, a 32 MiB structured-input cap, and graceful
worker-pool fallback), so an untrusted config or repo file can no longer hang or
abort a run.

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
- **HCL (Terraform / Nomad / Packer) as a config format.** New
  `hcl_path_{equals,matches,absent}` rule kinds, `format: hcl` in `json_schema_passes`,
  and `extract: { hcl: ... }` in the cross-file rules. HCL maps via the `hcl-rs` crate,
  which is JSON-native: a block nests by its type then labels
  (`resource "aws_instance" "web" { … }` is `$.resource.aws_instance.web`), values keep
  their HCL type (a number stays a number), and an unevaluated expression (`var.x`,
  `${…}`, a function call) is an opaque string. A block type appearing once is an object
  but repeated is an array (the XML cardinality footgun). `hcl-rs` is a
  recursive-descent parser with no depth limit of its own, so a crafted deeply-nested
  file could otherwise overflow the stack and abort the process; a file that nests too
  deeply (structure, parentheses, or interpolation) or exceeds a size bound is rejected
  as one parse-error violation, and parsing runs on a large-stack thread. Whole numbers
  must be matched as integers (`hcl-rs` normalizes `1.0` to `1`). Auto-detects by the
  `.tf` / `.tfvars` / `.hcl` / `.nomad` extension.

### Fixed

- **Structured-query rules (`*_path_*`, `json_schema_passes`) no longer false-fire on
  a BOM-prefixed or empty file.** Any leading UTF-8 BOM(s) (common on Windows-authored
  `package.json` / `tsconfig.json`) are now stripped for every format before parsing —
  JSON and HCL previously rejected a BOM as a syntax error — and an empty / whitespace-only
  file parses as an empty object `{}` for every format, so `*_path_absent` is satisfied,
  `if_present` stays silent, and `json_schema_passes` validates it against
  `{"type":"object"}` instead of false-firing "null is not of type object". Flagging a
  BOM or an empty file remains the `no_bom` / `no_empty_files` rules' job.
- **Format auto-detection recognizes uppercase extensions and defers `.env.*` to a real
  extension.** `json_schema_passes` (which auto-detects the target format) no longer
  reports a false "could not detect format" on a `.JSON` / `.XML` / `.CSPROJ` file
  (common on Windows and case-insensitive filesystems), and a `.env.json` / `.env.yaml`
  is now parsed by its real format rather than shadowed by the `.env.*` dotenv rule.
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

### Security

- **Parser hardening: a crafted config file can no longer abort an `alint` run.**
  `hcl-rs` is a recursive-descent parser with no depth limit of its own, so a
  deeply-nested or expression-bomb `.tf` (structure, parentheses, unary runs, or long
  binary / ternary operator chains, including inside `${...}` interpolation) could
  overflow the stack and abort the whole process (SIGABRT). HCL parsing now runs behind
  a depth pre-scan plus a 64 KiB size cap on a dedicated large-stack thread, so such a
  file becomes one per-file parse-error violation instead of a crash. The XML depth
  guard margin was also widened (`MAX_XML_DEPTH` 256 -> 128) to stay well below
  `roxmltree`'s overflow point on a constrained worker stack. JSON, YAML, and TOML
  already enforce their own recursion limits.
- **XML parsing is no longer quadratic in attribute count.** `roxmltree` 0.20 validates
  per-element attribute uniqueness in O(n^2), so a single element bearing tens of
  thousands of attributes (a ~1.5 MB crafted `.xml` / `.csproj`) could stall a worker
  for minutes -- an algorithmic-complexity DoS that no nesting guard caught. The pre-scan
  now also caps attributes per element, restoring linear-time parsing; an over-wide
  element is one per-file parse-error violation.
- **A remote `extends:` ruleset can no longer hang the run with a YAML flow bomb.** The
  flow-depth guard that protects a local config was not applied to a remote (or bundled)
  `extends:` body, so a deeply-nested-flow remote ruleset made `serde_yaml_ng` process it
  super-linearly. Remote and bundled bodies now go through the same guard.
- **HCL parsing no longer panics when the parse thread can't be spawned.** Under address-
  space / thread pressure (hardened CI, containers) the large-stack HCL parse thread could
  fail to spawn and `.expect`-panic with the crash banner; it now surfaces as one ordinary
  per-file parse-error violation.
- **The HCL depth pre-scan can no longer be evaded through a heredoc terminator or a
  comment inside an interpolation.** The scan skips string / heredoc / comment content
  while counting expression nesting, but two boundary mismatches with `hcl-rs` let a bomb
  hide as "content": `hcl-rs` ends a heredoc on a terminator line carrying leading and/or
  trailing whitespace or a trailing comment (`  EOT`, `EOT `, `EOT # c`), which the scan
  did not recognize (so it kept skipping the real expression after the terminator); and
  `hcl-rs` honors `#` / `//` /
  `/* */` comments inside a `${...}` interpolation, so a `}` in such a comment made the
  scan close the interpolation early and stop counting. Either could carry a deeply-nested
  expression on to `hcl-rs` and stack-abort the process. The scan now matches `hcl-rs`'s
  terminator leniency and skips interpolation comments, both pinned by tests.
- **A YAML alias-expansion bomb can no longer hang the run.** A single anchor referenced
  many times (`&a [..]` + thousands of `*a`, or a `<<: *a` merge) expands to tens of
  millions of nodes from a ~150 KB file -- serde_yaml_ng's own limits catch only NESTED
  aliases (billion-laughs), not this shape, and the flow-depth guard only bounds nesting.
  A cheap discard-only pre-count now bounds alias expansion for every YAML surface
  (`yaml_path_*`, `json_schema_passes`, `extract`, and config / `extends:` loading), so a
  bomb becomes one per-file parse-error instead of a multi-second, multi-GB hang. Alias-free
  YAML is unaffected (the check short-circuits) and every legit document parses unchanged.
- **The alias-expansion short-circuit can no longer be evaded with a plain-scalar quote.**
  The bomb guard above skips its budget pass when the document uses no alias, and decided
  that by skipping quoted scalars -- but a plain scalar that merely contains a quote
  (`desc: it's fine`, `size: 12" wide`; valid, common YAML) made the scan skip past a real
  `*alias` after it and wrongly conclude "no alias", so a hidden bomb slipped through. The
  alias detector and the flow-depth scanner (which shared the flaw, letting a flow bomb on a
  later line hide behind an earlier mid-scalar quote) now treat a quote as a scalar delimiter
  only when it opens at a node position (line start, after `:`/`-`/`,`/`[`/`{`/`?`, or after an
  anchor / tag), so the guards can no longer be side-stepped by an innocuous leading line.
- **Every YAML entry point is now flow-guarded.** The public `alint_dsl::parse()` API, the
  `{{env.X}}`-interpolated scalar re-parse, and the `alint suggest` config reader each
  deserialized untrusted YAML without the flow-depth / alias guards the file loader
  applies; all now share them, closing the last unguarded parse sites.
- **Structured parsing is bounded against parse-tree memory blow-up.** A parsed value tree
  costs far more RSS than its input -- up to ~30x for attribute-heavy XML, ~6-10x for JSON --
  so a near-read-cap (256 MiB) structured file could balloon to multiple GB. Structured input
  is now capped at 32 MiB (`MAX_STRUCTURED_BYTES`; HCL keeps its tighter 64 KiB cap), bounding
  a single file's tree to ~1 GB worst case. Config and manifest files are far smaller; an
  oversize structured file is one parse-error.
- **Parallel rule dispatch no longer panics when the OS refuses worker threads.** Under
  `RLIMIT_NPROC` / pids pressure (hardened CI, containers) rayon's lazy global-pool init
  would `.expect`-panic with the "alint crashed" banner; the engine now runs `par_iter`
  inside an explicitly-built pool (built once and cached) that degrades through a single
  spawned worker to a zero-new-thread pool running on the calling thread, with identical,
  order-preserving results.

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
  whose name contains a `\n`/`\r` (legal on Unix) broke out of the `` ## `path` ``
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
  [variable interpolation](https://alint.org/docs/configuration/variable-interpolation/).
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
  `crates/alint-dsl/rulesets/v1/`, parses every `` ### `alint://bundled/...@v1` ``
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
[`docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/v0.9.13/`](docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/v0.9.13/)
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
  [`docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/v0.9.10/`](docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/v0.9.10/).

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
  [`docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/v0.9.9/`](docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/v0.9.9/).

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
  [`docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/v0.9.8/`](docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/v0.9.8/).

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
[`docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/v0.9.5/`](docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/v0.9.5/).

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
  `docs/benchmarks/micro/results/linux-x86_64-ryzen-3900x/v0.9.4/criterion/`.

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
  `docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/v0.5.7/`. Same
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
  `docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/v0.5.6/1m/results.md`.
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
  `docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/v0.5.7/` with 18 rows
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
  derived from `git diff --name-only --relative <base>...HEAD`
  (three-dot — merge-base diff, the right
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
  [walker-and-git concept page](https://alint.org/docs/concepts/targeting/the-walker-and-git/):
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
  auto-fixable` + a call-to-action line ``→ run `alint fix` to
  resolve N fixable violation(s).`` when anything's auto-fixable.
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

[Unreleased]: https://github.com/asamarts/alint/compare/v0.23.0...HEAD
[0.23.0]: https://github.com/asamarts/alint/compare/v0.22.1...v0.23.0
[0.22.1]: https://github.com/asamarts/alint/compare/v0.22.0...v0.22.1
[0.22.0]: https://github.com/asamarts/alint/compare/v0.21.2...v0.22.0
[0.21.2]: https://github.com/asamarts/alint/compare/v0.21.1...v0.21.2
[0.21.1]: https://github.com/asamarts/alint/compare/v0.21.0...v0.21.1
[0.21.0]: https://github.com/asamarts/alint/compare/v0.20.1...v0.21.0
[0.20.1]: https://github.com/asamarts/alint/compare/v0.20.0...v0.20.1
[0.20.0]: https://github.com/asamarts/alint/compare/v0.19.0...v0.20.0
[0.19.0]: https://github.com/asamarts/alint/compare/v0.18.4...v0.19.0
[0.18.4]: https://github.com/asamarts/alint/compare/v0.18.3...v0.18.4
[0.18.3]: https://github.com/asamarts/alint/compare/v0.18.2...v0.18.3
[0.18.2]: https://github.com/asamarts/alint/compare/v0.18.1...v0.18.2
[0.18.1]: https://github.com/asamarts/alint/compare/v0.18.0...v0.18.1
[0.18.0]: https://github.com/asamarts/alint/compare/v0.17.0...v0.18.0
[0.17.0]: https://github.com/asamarts/alint/compare/v0.16.1...v0.17.0
[0.16.1]: https://github.com/asamarts/alint/compare/v0.16.0...v0.16.1
[0.16.0]: https://github.com/asamarts/alint/compare/v0.15.2...v0.16.0
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
