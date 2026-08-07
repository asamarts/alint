use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::Deserialize;

use crate::facts::FactSpec;
use crate::level::Level;

/// Parsed form of a `.alint.yml` file.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    /// Other config files this one inherits from. Entries resolved
    /// left-to-right; later entries override earlier ones; the
    /// current file's own definitions override everything it extends.
    ///
    /// Each entry is either a bare string (local path, `https://`
    /// URL with SRI, or `alint://bundled/...`) or a mapping with
    /// `url:` and optional `only:` / `except:` filters.
    #[serde(default)]
    pub extends: Vec<ExtendsEntry>,
    #[serde(default)]
    pub ignore: Vec<String>,
    #[serde(default = "default_respect_gitignore")]
    pub respect_gitignore: bool,
    /// Free-form string variables referenced from rule messages and
    /// `when` expressions as `{{vars.<name>}}` and `vars.<name>`.
    #[serde(default)]
    pub vars: HashMap<String, String>,
    /// Repository properties evaluated once per run and referenced from
    /// `when` clauses as `facts.<id>`.
    #[serde(default)]
    pub facts: Vec<FactSpec>,
    #[serde(default)]
    pub rules: Vec<RuleSpec>,
    /// Maximum file size, in bytes, that content-editing fixes
    /// will read and rewrite. Files over this limit are reported
    /// as `Skipped` in the fix report and a one-line warning is
    /// printed to stderr. Defaults to 1 MiB; set explicitly to
    /// `null` to disable the cap entirely.
    ///
    /// Path-only fixes (`file_create`, `file_remove`,
    /// `file_rename`) ignore the cap — they don't read content.
    #[serde(default = "default_fix_size_limit")]
    pub fix_size_limit: Option<u64>,
    /// Opt in to discovery of `.alint.yml` / `.alint.yaml` files
    /// in subdirectories. When `true`, the loader walks the
    /// repository tree (from the root config's directory,
    /// respecting `.gitignore` and `ignore:`) and finds any
    /// nested config files; each nested rule's path-like fields
    /// (`paths`, `select`, `primary`) are prefixed with the
    /// directory that nested config lives in, so the rule
    /// auto-scopes to that subtree. Default `false`.
    ///
    /// Only the user's top-level config may set this — nested
    /// configs themselves cannot spawn further nested discovery.
    #[serde(default)]
    pub nested_configs: bool,
    /// Resolved `allow_out_of_root:` policy — which rules may read a
    /// config-declared path that escapes the repo root. Set by the
    /// loader's `finalize()` from the user's *top-level* config only
    /// (rejected from `extends:`); `#[serde(skip)]` so a directly
    /// deserialized or bundled `Config` can never set it. Default
    /// [`AllowOutOfRoot::Confined`]. See
    /// `docs/design/v0.12/allow_out_of_root.md`.
    #[serde(skip)]
    pub allow_out_of_root: AllowOutOfRoot,
    /// Resolved `baseline:` path — the committed baseline file that `check`
    /// suppresses against when no `--baseline` flag is given (the flag
    /// overrides it). Like `allow_out_of_root`, set by the loader from the
    /// user's *top-level* config only (rejected from `extends:` and nested
    /// configs); `#[serde(skip)]` so a bundled/inherited `Config` can never
    /// carry it. A trusted top-level input like `-c`, resolved relative to the
    /// repo root and not subject to read-path confinement. See
    /// `docs/design/baseline.md` §2.3.
    #[serde(skip)]
    pub baseline: Option<PathBuf>,
}

// Returning `Option<u64>` (rather than bare `u64`) keeps the
// YAML-facing type consistent with `Config.fix_size_limit`:
// users set `null` in YAML to mean "no limit". The Option is
// load-bearing at the field level, so clippy's warning on the
// default fn is noise here.
#[allow(clippy::unnecessary_wraps)]
fn default_fix_size_limit() -> Option<u64> {
    Some(1 << 20)
}

fn default_respect_gitignore() -> bool {
    true
}

impl Config {
    pub const CURRENT_VERSION: u32 = 1;
}

/// Which rules may *read* a config-declared path that escapes the
/// repo root — the parsed form of the top-level `allow_out_of_root:`
/// key. Default [`Confined`](Self::Confined) (hard confinement, the
/// secure default). Honored only from the user's own top-level config;
/// the loader rejects it from any `extends:`'d ruleset (the same trust
/// model as the spawning-rule gate). See
/// `docs/design/v0.12/allow_out_of_root.md`.
///
/// YAML forms: `true` (all rules), or `{ kinds: [...], rules: [...] }`
/// (a rule is permitted if its kind or id is listed). Absent / `false`
/// → `Confined`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AllowOutOfRoot {
    /// No rule may read outside the repo root (default).
    #[default]
    Confined,
    /// Every rule may (`allow_out_of_root: true`).
    All,
    /// Only rules whose `kind` ∈ `kinds` or `id` ∈ `rules` may.
    Selective {
        kinds: HashSet<String>,
        rules: HashSet<String>,
    },
}

impl AllowOutOfRoot {
    /// Whether a rule with this `id` / `kind` may read out of root.
    #[must_use]
    pub fn allows(&self, id: &str, kind: &str) -> bool {
        match self {
            Self::Confined => false,
            Self::All => true,
            Self::Selective { kinds, rules } => kinds.contains(kind) || rules.contains(id),
        }
    }

    /// `true` when nothing is permitted (the default). The `extends:`
    /// trust gate rejects an inherited ruleset whose value is not this.
    #[must_use]
    pub fn is_confined(&self) -> bool {
        matches!(self, Self::Confined)
    }
}

impl<'de> Deserialize<'de> for AllowOutOfRoot {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct SelectiveSpec {
            #[serde(default)]
            kinds: Vec<String>,
            #[serde(default)]
            rules: Vec<String>,
        }
        #[derive(Deserialize)]
        #[serde(
            untagged,
            expecting = "a boolean, or a map with optional `kinds` and `rules` lists"
        )]
        enum Raw {
            Flag(bool),
            Selective(SelectiveSpec),
        }
        Ok(match Raw::deserialize(deserializer)? {
            Raw::Flag(true) => Self::All,
            Raw::Flag(false) => Self::Confined,
            Raw::Selective(s) => Self::Selective {
                kinds: s.kinds.into_iter().collect(),
                rules: s.rules.into_iter().collect(),
            },
        })
    }
}

/// A single `extends:` entry. Accepts either a bare string (the
/// classic form — a local path, `https://` URL with SRI, or
/// `alint://bundled/<name>@<rev>`) or a mapping that adds
/// `only:` / `except:` filters on the inherited rule set.
///
/// ```yaml
/// extends:
///   - alint://bundled/oss-baseline@v1             # classic form
///   - url: alint://bundled/rust@v1                # filtered form
///     except: [rust-no-target-dir]                # drop by id
///   - url: ./team-defaults.yml
///     only: [team-copyright-header]               # keep by id
/// ```
///
/// Filters resolve against the *fully-resolved* rule set of the
/// entry (i.e. anything it transitively extends). `only:` and
/// `except:` are mutually exclusive on a single entry; listing an
/// unknown rule id is a config error so typos surface at load
/// time.
#[derive(Debug, Clone, Deserialize)]
#[serde(
    untagged,
    expecting = "a URL or path string, or a `{ url, only?, except? }` map"
)]
pub enum ExtendsEntry {
    Url(String),
    Filtered {
        url: String,
        #[serde(default)]
        only: Option<Vec<String>>,
        #[serde(default)]
        except: Option<Vec<String>>,
    },
}

impl ExtendsEntry {
    /// The URL / path of the extended config. Uniform across both
    /// enum variants.
    pub fn url(&self) -> &str {
        match self {
            Self::Url(s) | Self::Filtered { url: s, .. } => s,
        }
    }

    /// Rule ids to keep (drop everything else). `None` when no
    /// `only:` filter is specified.
    pub fn only(&self) -> Option<&[String]> {
        match self {
            Self::Filtered { only: Some(v), .. } => Some(v),
            _ => None,
        }
    }

    /// Rule ids to drop. `None` when no `except:` filter is
    /// specified.
    pub fn except(&self) -> Option<&[String]> {
        match self {
            Self::Filtered {
                except: Some(v), ..
            } => Some(v),
            _ => None,
        }
    }
}

/// YAML shape for a rule's `paths:` field — a single glob, an array (with
/// optional `!pattern` negations), or an explicit `{include, exclude}` pair.
/// For the include/exclude form, each field accepts either a single string
/// or a list of strings.
#[derive(Debug, Clone, Deserialize)]
#[serde(
    untagged,
    expecting = "a glob string, a list of globs, or an `{ include, exclude }` map"
)]
pub enum PathsSpec {
    Single(String),
    Many(Vec<String>),
    IncludeExclude {
        #[serde(default, deserialize_with = "string_or_vec")]
        include: Vec<String>,
        #[serde(default, deserialize_with = "string_or_vec")]
        exclude: Vec<String>,
    },
}

impl PathsSpec {
    /// Human-readable one-line rendering of the scope, for `alint explain`.
    #[must_use]
    pub fn render_scope(&self) -> String {
        match self {
            PathsSpec::Single(s) => s.clone(),
            PathsSpec::Many(v) => v.join(", "),
            PathsSpec::IncludeExclude { include, exclude } => {
                let inc = if include.is_empty() {
                    "**".to_string()
                } else {
                    include.join(", ")
                };
                if exclude.is_empty() {
                    inc
                } else {
                    format!("{inc}  (excluding {})", exclude.join(", "))
                }
            }
        }
    }

    /// Normalised `(include, exclude)` glob lists, for machine output.
    #[must_use]
    pub fn include_exclude(&self) -> (Vec<String>, Vec<String>) {
        match self {
            PathsSpec::Single(s) => (vec![s.clone()], Vec::new()),
            PathsSpec::Many(v) => (v.clone(), Vec::new()),
            PathsSpec::IncludeExclude { include, exclude } => (include.clone(), exclude.clone()),
        }
    }
}

fn string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged, expecting = "a string, or a list of strings")]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(s) => Ok(vec![s]),
        OneOrMany::Many(v) => Ok(v),
    }
}

/// YAML-level description of a rule before it is instantiated into a `Box<dyn Rule>`
/// by a [`RuleBuilder`](crate::registry::RuleBuilder).
#[derive(Debug, Clone, Deserialize)]
pub struct RuleSpec {
    pub id: String,
    pub kind: String,
    pub level: Level,
    #[serde(default)]
    pub paths: Option<PathsSpec>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub policy_url: Option<String>,
    #[serde(default)]
    pub when: Option<String>,
    /// Optional mechanical-fix strategy. Rules whose builders understand
    /// the chosen op attach a [`Fixer`](crate::Fixer) to the built rule;
    /// rules whose kind is incompatible with the op return a config error
    /// at build time.
    #[serde(default)]
    pub fix: Option<FixSpec>,
    // Neither `git_tracked_only` nor `respect_gitignore` is a RuleSpec field:
    // both are kind-specific options (ADR-0008). `git_tracked_only` lives in
    // each existence kind's `Options` struct (`file_exists`/`file_absent`/
    // `dir_exists`/`dir_absent`) — the engine reads it via the
    // `Rule::git_tracked_mode()` trait method, never a RuleSpec field.
    // `respect_gitignore` lives only in `file_exists`'s `Options` — the sole
    // kind that honours a per-rule override (the bazel-style "tracked AND
    // gitignored" pitfall #18). Keeping both off `rule_common` means a rule of
    // any other kind that sets either is rejected at load by
    // `deny_unknown_fields`, rather than silently ignored.
    /// Per-file ancestor-manifest gate. When set, the rule
    /// only fires on files that have at least one ancestor
    /// directory (including the file's own directory)
    /// containing a file matching the configured
    /// `has_ancestor` name(s). Composes AND with `paths:`
    /// and `git_tracked_only:`.
    ///
    /// Only meaningful for per-file rules; cross-file rule
    /// builders MUST reject this field at build time
    /// (see the design doc for the cross-file alternative
    /// via `for_each_dir + when_iter:`).
    ///
    /// Default `None` (no scope filter; existing rules
    /// preserve their pre-v0.9.6 behaviour).
    #[serde(default)]
    pub scope_filter: Option<crate::ScopeFilterSpec>,
    /// The entire YAML mapping, retained so each rule builder can deserialize
    /// its kind-specific fields without every option being represented here.
    #[serde(flatten)]
    pub extra: serde_yaml_ng::Mapping,
}

/// The `fix:` block on a rule. Exactly one op key must be present —
/// alint errors at load time when the op and rule kind are incompatible.
#[derive(Debug, Clone, Deserialize)]
#[serde(
    untagged,
    expecting = "a map with exactly one fix op (e.g. `file_create`, `file_remove`, `file_prepend`)"
)]
pub enum FixSpec {
    FileCreate {
        file_create: FileCreateFixSpec,
    },
    FileRemove {
        file_remove: FileRemoveFixSpec,
    },
    FilePrepend {
        file_prepend: FilePrependFixSpec,
    },
    FileAppend {
        file_append: FileAppendFixSpec,
    },
    FileRename {
        file_rename: FileRenameFixSpec,
    },
    FileTrimTrailingWhitespace {
        file_trim_trailing_whitespace: FileTrimTrailingWhitespaceFixSpec,
    },
    FileAppendFinalNewline {
        file_append_final_newline: FileAppendFinalNewlineFixSpec,
    },
    FileNormalizeLineEndings {
        file_normalize_line_endings: FileNormalizeLineEndingsFixSpec,
    },
    FileStripBidi {
        file_strip_bidi: FileStripBidiFixSpec,
    },
    FileStripZeroWidth {
        file_strip_zero_width: FileStripZeroWidthFixSpec,
    },
    FileStripBom {
        file_strip_bom: FileStripBomFixSpec,
    },
    FileCollapseBlankLines {
        file_collapse_blank_lines: FileCollapseBlankLinesFixSpec,
    },
}

impl FixSpec {
    /// The op name as it appears in YAML — used in config-error messages.
    pub fn op_name(&self) -> &'static str {
        match self {
            Self::FileCreate { .. } => "file_create",
            Self::FileRemove { .. } => "file_remove",
            Self::FilePrepend { .. } => "file_prepend",
            Self::FileAppend { .. } => "file_append",
            Self::FileRename { .. } => "file_rename",
            Self::FileTrimTrailingWhitespace { .. } => "file_trim_trailing_whitespace",
            Self::FileAppendFinalNewline { .. } => "file_append_final_newline",
            Self::FileNormalizeLineEndings { .. } => "file_normalize_line_endings",
            Self::FileStripBidi { .. } => "file_strip_bidi",
            Self::FileStripZeroWidth { .. } => "file_strip_zero_width",
            Self::FileStripBom { .. } => "file_strip_bom",
            Self::FileCollapseBlankLines { .. } => "file_collapse_blank_lines",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileCreateFixSpec {
    /// Inline content to write. Mutually exclusive with
    /// `content_from`; exactly one of the two must be set. For
    /// an empty file, pass `content: ""` explicitly.
    #[serde(default)]
    pub content: Option<String>,
    /// Path to a file (relative to the lint root) whose bytes
    /// will be the content. Mutually exclusive with `content`.
    /// Read at fix-apply time; missing source produces a
    /// `Skipped` outcome rather than a panic. Useful for
    /// LICENSE / NOTICE / CONTRIBUTING boilerplate that's too
    /// long to inline in YAML.
    #[serde(default)]
    pub content_from: Option<PathBuf>,
    /// Path to create, relative to the repo root. When omitted, the
    /// rule builder substitutes the first literal entry from the rule's
    /// `paths:` list.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Whether to create intermediate directories. Defaults to true.
    #[serde(default = "default_create_parents")]
    pub create_parents: bool,
}

fn default_create_parents() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileRemoveFixSpec {}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilePrependFixSpec {
    /// Inline bytes to insert at the beginning of each
    /// violating file. Mutually exclusive with `content_from`.
    /// A trailing newline is the caller's responsibility.
    #[serde(default)]
    pub content: Option<String>,
    /// Path to a file (relative to the lint root) whose bytes
    /// will be prepended. Mutually exclusive with `content`.
    #[serde(default)]
    pub content_from: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileAppendFixSpec {
    /// Inline bytes to append to each violating file. Mutually
    /// exclusive with `content_from`. A leading newline is the
    /// caller's responsibility.
    #[serde(default)]
    pub content: Option<String>,
    /// Path to a file (relative to the lint root) whose bytes
    /// will be appended. Mutually exclusive with `content`.
    #[serde(default)]
    pub content_from: Option<PathBuf>,
}

/// Resolution of an `(content, content_from)` pair to a single
/// content source. Used by the three fixers that take either.
/// Errors when neither or both are set.
pub fn resolve_content_source(
    rule_id: &str,
    op_name: &str,
    inline: &Option<String>,
    from: &Option<PathBuf>,
) -> crate::error::Result<ContentSourceSpec> {
    match (inline, from) {
        (Some(_), Some(_)) => Err(crate::error::Error::rule_config(
            rule_id,
            format!("fix.{op_name}: `content` and `content_from` are mutually exclusive"),
        )),
        (None, None) => Err(crate::error::Error::rule_config(
            rule_id,
            format!("fix.{op_name}: one of `content` or `content_from` is required"),
        )),
        (Some(s), None) => Ok(ContentSourceSpec::Inline(s.clone())),
        (None, Some(p)) => Ok(ContentSourceSpec::File(p.clone())),
    }
}

/// Pre-validated content source — exactly one of inline or
/// from-file. Resolved at config-parse time so fixers don't
/// need to reproduce the XOR check at apply time.
#[derive(Debug, Clone)]
pub enum ContentSourceSpec {
    /// Inline string body.
    Inline(String),
    /// Path relative to the lint root; bytes are read at fix-
    /// apply time.
    File(PathBuf),
}

impl From<String> for ContentSourceSpec {
    fn from(s: String) -> Self {
        Self::Inline(s)
    }
}

impl From<&str> for ContentSourceSpec {
    fn from(s: &str) -> Self {
        Self::Inline(s.to_string())
    }
}

/// Empty marker: `file_rename` takes no parameters. The target name
/// is derived from the parent rule (e.g. `filename_case` converts the
/// stem to its configured case; the extension is preserved).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileRenameFixSpec {}

/// Empty marker. Behavior: read file (subject to `fix_size_limit`),
/// strip trailing space/tab on every line, write back.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileTrimTrailingWhitespaceFixSpec {}

/// Empty marker. Behavior: if the file has content and does not
/// end with `\n`, append one.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileAppendFinalNewlineFixSpec {}

/// Empty marker. Behavior: rewrite the file with every line ending
/// replaced by the parent rule's configured target (`lf` or `crlf`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileNormalizeLineEndingsFixSpec {}

/// Empty marker. Behavior: remove every Unicode bidi control
/// character (U+202A–202E, U+2066–2069) from the file's content.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileStripBidiFixSpec {}

/// Empty marker. Behavior: remove every zero-width character
/// (U+200B / U+200C / U+200D / U+2060 / U+180E / U+FEFF) from the
/// file's content, *except* a leading BOM (U+FEFF at position 0) —
/// that's the responsibility of the `no_bom` rule.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileStripZeroWidthFixSpec {}

/// Empty marker. Behavior: remove a leading UTF-8/UTF-16/UTF-32
/// BOM byte sequence if present; otherwise a no-op.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileStripBomFixSpec {}

/// Empty marker. Behavior: collapse runs of blank lines longer than
/// the parent rule's `max` down to exactly `max` blank lines.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileCollapseBlankLinesFixSpec {}

impl RuleSpec {
    /// Deserialize the full spec (common + kind-specific fields) into a typed
    /// options struct. Common fields are reconstructed into the mapping so
    /// the target struct can `#[derive(Deserialize)]` against the whole shape
    /// when convenient.
    pub fn deserialize_options<T>(&self) -> crate::error::Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        Ok(serde_yaml_ng::from_value(serde_yaml_ng::Value::Mapping(
            self.extra.clone(),
        ))?)
    }

    /// Reject any leftover option key on this spec — for rule kinds that take
    /// NO kind-specific options. Option-bearing kinds reject unknown fields via
    /// their `deserialize_options::<Options>()` (a `deny_unknown_fields`
    /// struct); this is the equivalent loud failure for option-less kinds,
    /// used by `RuleRegistry::register_optionless`. Without it, a typo'd option
    /// on an option-less rule silently no-ops.
    pub fn deny_unknown_options(&self) -> crate::error::Result<()> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct NoOptions {}
        self.deserialize_options::<NoOptions>()
            .map(|_: NoOptions| ())
    }

    /// Parse and validate this spec's optional `scope_filter:`
    /// field into a built [`ScopeFilter`](crate::ScopeFilter).
    /// Returns `Ok(None)` when the spec has no `scope_filter`
    /// set (the common case).
    ///
    /// Per-file rule builders typically don't call this directly
    /// since v0.9.10 — they use
    /// [`Scope::from_spec`](crate::Scope::from_spec) instead,
    /// which bundles `paths:` + `scope_filter:` parsing into one
    /// call. The Scope owns the parsed filter and consults it
    /// inside [`Scope::matches`](crate::Scope::matches), so the
    /// engine doesn't need a separate per-rule accessor any more.
    /// Cross-file rules MUST NOT call this — they call
    /// [`reject_scope_filter_on_cross_file`](crate::reject_scope_filter_on_cross_file)
    /// instead so a misconfigured `scope_filter:` on a cross-
    /// file rule surfaces as a clear build-time error rather
    /// than a silently-ignored field.
    pub fn parse_scope_filter(&self) -> crate::error::Result<Option<crate::ScopeFilter>> {
        match &self.scope_filter {
            Some(spec) => Ok(Some(crate::ScopeFilter::from_spec(&self.id, spec.clone())?)),
            None => Ok(None),
        }
    }
}

/// Rule specification for nested rules (e.g. the `require:` block of
/// `for_each_dir`). Unlike [`RuleSpec`], `id` and `level` are synthesized
/// from the parent rule — users just supply the `kind` plus kind-specific
/// options, optionally with a `message` / `policy_url` / `when`.
#[derive(Debug, Clone, Deserialize)]
pub struct NestedRuleSpec {
    pub kind: String,
    #[serde(default)]
    pub paths: Option<PathsSpec>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub policy_url: Option<String>,
    #[serde(default)]
    pub when: Option<String>,
    /// Per-file scope filter — see [`RuleSpec::scope_filter`]
    /// for semantics. Inherited unchanged when
    /// [`NestedRuleSpec::instantiate`] synthesises a full
    /// `RuleSpec` per-iteration.
    #[serde(default)]
    pub scope_filter: Option<crate::ScopeFilterSpec>,
    #[serde(flatten)]
    pub extra: serde_yaml_ng::Mapping,
}

/// A [`NestedRuleSpec`] with its `when:` source pre-compiled
/// into a [`crate::when::WhenExpr`] at rule-build time.
///
/// Mirrors the v0.9.5-era pattern for `when_iter:` on cross-
/// file iteration rules: parse the source once at build, then
/// evaluate per iteration with a fresh `iter` context. v0.9.12
/// closed the gap: pre-v0.9.12 the nested `when:` source string
/// was re-parsed inside `evaluate_for_each` on every iteration
/// (one parse per (entry, nested-rule) pair, sometimes
/// thousands of redundant parses per cross-file rule eval). The
/// `Option<WhenExpr>` on this struct is parsed exactly once.
///
/// Build sites (`for_each_dir`, `for_each_file`,
/// `every_matching_has` in `alint-rules`) construct a
/// `Vec<CompiledNestedSpec>` from `Vec<NestedRuleSpec>` in
/// their `build` impl; `evaluate_for_each` consumes the
/// compiled form.
#[derive(Debug)]
pub struct CompiledNestedSpec {
    /// The original nested-rule spec — passed to
    /// [`NestedRuleSpec::instantiate`] per iteration to get a
    /// per-iteration full `RuleSpec` with template tokens
    /// substituted.
    pub spec: NestedRuleSpec,
    /// Pre-compiled `when:` expression. `None` when the nested
    /// spec carried no `when:` clause.
    pub when: Option<crate::when::WhenExpr>,
}

impl CompiledNestedSpec {
    /// Compile a [`NestedRuleSpec`] — parsing its `when:`
    /// source string once. Surfaces a build-time config error
    /// (`"<parent_id>: nested rule #<idx>: invalid when: ..."`)
    /// when the source fails to parse, so misconfigured
    /// nested-when clauses fail at config-load time instead of
    /// per-iteration during evaluation.
    pub fn compile(
        spec: NestedRuleSpec,
        parent_id: &str,
        idx: usize,
    ) -> crate::error::Result<Self> {
        let when = match spec.when.as_deref() {
            Some(src) => Some(crate::when::parse(src).map_err(|e| {
                crate::error::Error::rule_config(
                    parent_id,
                    format!("nested rule #{idx}: invalid when: {e}"),
                )
            })?),
            None => None,
        };
        Ok(Self { spec, when })
    }
}

impl NestedRuleSpec {
    /// Synthesize a full [`RuleSpec`] for a single iteration, applying
    /// path-template substitution (using the iterated entry's tokens) to
    /// every string field. The resulting spec has `id =
    /// "{parent_id}.require[{idx}]"` and inherits `level` from the parent.
    pub fn instantiate(
        &self,
        parent_id: &str,
        idx: usize,
        level: Level,
        tokens: &crate::template::PathTokens,
    ) -> RuleSpec {
        RuleSpec {
            id: format!("{parent_id}.require[{idx}]"),
            kind: self.kind.clone(),
            level,
            paths: self
                .paths
                .as_ref()
                .map(|p| crate::template::render_paths_spec(p, tokens)),
            message: self
                .message
                .as_deref()
                .map(|m| crate::template::render_path(m, tokens)),
            policy_url: self.policy_url.clone(),
            when: self.when.clone(),
            fix: None,
            // `git_tracked_only` and `respect_gitignore` are both kind-specific
            // options now (ADR-0008), stripped from the nested `extra` via
            // PARENT_FIELDS below, so a nested existence rule doesn't silently
            // inherit either. If/when nested rules need one, drop it from
            // PARENT_FIELDS and let it flow into the leaf's option set.
            scope_filter: self.scope_filter.clone(),
            // `NestedRuleSpec` doesn't name `id`/`level` (synthesized from the
            // parent) or the top-level-only `fix`/git toggles, so a nested
            // config that supplies them leaves them in `extra`. Strip them
            // before they reach the leaf rule's option set — they are not
            // options, and would otherwise trip the leaf's unknown-option
            // validation (a deny-unknown-fields `Options` struct, or an
            // option-less rule's `deny_unknown_options`).
            extra: {
                const PARENT_FIELDS: &[&str] = &[
                    "id",
                    "level",
                    "fix",
                    "git_tracked_only",
                    "respect_gitignore",
                ];
                crate::template::render_mapping(self.extra.clone(), tokens)
                    .into_iter()
                    .filter(|(k, _)| k.as_str().is_none_or(|s| !PARENT_FIELDS.contains(&s)))
                    .collect()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::PathTokens;
    use std::path::Path;

    #[test]
    fn allow_out_of_root_policy_resolves() {
        assert!(!AllowOutOfRoot::Confined.allows("r", "k"));
        assert!(AllowOutOfRoot::All.allows("r", "k"));
        let sel = AllowOutOfRoot::Selective {
            kinds: ["json_schema_passes".to_string()].into_iter().collect(),
            rules: ["my-rule".to_string()].into_iter().collect(),
        };
        assert!(sel.allows("anything", "json_schema_passes"), "by kind");
        assert!(sel.allows("my-rule", "other_kind"), "by id");
        assert!(!sel.allows("nope", "other_kind"), "neither id nor kind");
        assert!(AllowOutOfRoot::Confined.is_confined());
        assert!(!AllowOutOfRoot::All.is_confined());
    }

    #[test]
    fn allow_out_of_root_deserializes_bool_and_map() {
        let t: AllowOutOfRoot = serde_yaml_ng::from_str("true").unwrap();
        assert_eq!(t, AllowOutOfRoot::All);
        let f: AllowOutOfRoot = serde_yaml_ng::from_str("false").unwrap();
        assert_eq!(f, AllowOutOfRoot::Confined);
        let m: AllowOutOfRoot = serde_yaml_ng::from_str("kinds: [pair_hash]\nrules: [x]").unwrap();
        match m {
            AllowOutOfRoot::Selective { kinds, rules } => {
                assert!(kinds.contains("pair_hash"));
                assert!(rules.contains("x"));
            }
            other => panic!("expected Selective, got {other:?}"),
        }
        // an unknown key in the map form is rejected with a message that names
        // the accepted forms, not the internal untagged-enum type.
        let err = serde_yaml_ng::from_str::<AllowOutOfRoot>("bogus: 1").unwrap_err();
        assert!(
            err.to_string().contains("a boolean, or a map"),
            "expected a friendly message, got: {err}"
        );
        assert!(
            !err.to_string().contains("Raw"),
            "message leaks the internal enum name: {err}"
        );
    }

    #[test]
    fn untagged_config_enums_name_accepted_forms_not_internal_type() {
        // A value matching no variant reports the accepted forms (via
        // `#[serde(expecting)]`), never the internal untagged-enum type name.
        let p = serde_yaml_ng::from_str::<PathsSpec>("42")
            .unwrap_err()
            .to_string();
        assert!(
            p.contains("a glob string") && !p.contains("PathsSpec"),
            "PathsSpec: {p}"
        );
        let e = serde_yaml_ng::from_str::<ExtendsEntry>("42")
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("a URL or path") && !e.contains("ExtendsEntry"),
            "ExtendsEntry: {e}"
        );
        let f = serde_yaml_ng::from_str::<FixSpec>("42")
            .unwrap_err()
            .to_string();
        assert!(
            f.contains("exactly one fix op") && !f.contains("FixSpec"),
            "FixSpec: {f}"
        );
    }

    #[test]
    fn config_default_respects_gitignore_and_caps_fix_size() {
        // Round-trip the documented defaults through serde to
        // catch silent default drift.
        let cfg: Config = serde_yaml_ng::from_str("version: 1\n").expect("minimal config");
        assert_eq!(cfg.version, 1);
        assert!(cfg.respect_gitignore);
        assert_eq!(cfg.fix_size_limit, Some(1 << 20));
        assert!(!cfg.nested_configs);
        assert!(cfg.extends.is_empty());
        assert!(cfg.rules.is_empty());
    }

    #[test]
    fn config_rejects_unknown_top_level_field() {
        let err = serde_yaml_ng::from_str::<Config>("version: 1\nignored_typo: true\n");
        assert!(err.is_err(), "deny_unknown_fields should reject typos");
    }

    #[test]
    fn config_explicit_null_disables_fix_size_limit() {
        let cfg: Config = serde_yaml_ng::from_str("version: 1\nfix_size_limit: null\n").unwrap();
        assert_eq!(cfg.fix_size_limit, None);
    }

    #[test]
    fn extends_entry_url_form_has_no_filters() {
        let e = ExtendsEntry::Url("alint://bundled/oss-baseline@v1".into());
        assert_eq!(e.url(), "alint://bundled/oss-baseline@v1");
        assert!(e.only().is_none());
        assert!(e.except().is_none());
    }

    #[test]
    fn extends_entry_filtered_form_exposes_only_and_except() {
        let e = ExtendsEntry::Filtered {
            url: "alint://bundled/rust@v1".into(),
            only: Some(vec!["rust-edition".into()]),
            except: None,
        };
        assert_eq!(e.url(), "alint://bundled/rust@v1");
        assert_eq!(e.only(), Some(&["rust-edition".to_string()][..]));
        assert!(e.except().is_none());
    }

    #[test]
    fn extends_entry_filtered_form_supports_except_only() {
        let e = ExtendsEntry::Filtered {
            url: "./team.yml".into(),
            only: None,
            except: Some(vec!["legacy-rule".into()]),
        };
        assert_eq!(e.except(), Some(&["legacy-rule".to_string()][..]));
        assert!(e.only().is_none());
    }

    #[test]
    fn paths_spec_accepts_three_shapes() {
        let single: PathsSpec = serde_yaml_ng::from_str("\"src/**\"").unwrap();
        assert!(matches!(single, PathsSpec::Single(s) if s == "src/**"));

        let many: PathsSpec = serde_yaml_ng::from_str("[\"src/**\", \"!src/vendor/**\"]").unwrap();
        assert!(matches!(many, PathsSpec::Many(v) if v.len() == 2));

        let inc_exc: PathsSpec =
            serde_yaml_ng::from_str("include: src/**\nexclude: src/vendor/**\n").unwrap();
        match inc_exc {
            PathsSpec::IncludeExclude { include, exclude } => {
                assert_eq!(include, vec!["src/**"]);
                assert_eq!(exclude, vec!["src/vendor/**"]);
            }
            _ => panic!("expected include/exclude shape"),
        }
    }

    #[test]
    fn paths_spec_include_accepts_string_or_vec() {
        let from_string: PathsSpec =
            serde_yaml_ng::from_str("include: a\nexclude:\n  - b\n  - c\n").unwrap();
        let PathsSpec::IncludeExclude { include, exclude } = from_string else {
            panic!("expected include/exclude shape");
        };
        assert_eq!(include, vec!["a"]);
        assert_eq!(exclude, vec!["b", "c"]);
    }

    #[test]
    fn rule_spec_deserialize_options_picks_up_kind_specific_fields() {
        #[derive(Deserialize, Debug)]
        struct PatternOnly {
            pattern: String,
        }
        let spec: RuleSpec = serde_yaml_ng::from_str(
            "id: r\nkind: file_content_matches\nlevel: error\npaths: src/**\npattern: TODO\n",
        )
        .unwrap();
        let opts: PatternOnly = spec.deserialize_options().unwrap();
        assert_eq!(opts.pattern, "TODO");
    }

    #[test]
    fn fix_spec_op_name_covers_every_variant() {
        // Round-trip every documented op name through YAML; any
        // future fix variant added without a corresponding
        // op_name arm will fall through serde and trip this test.
        let cases = [
            ("file_create:\n  content: x\n", "file_create"),
            ("file_remove: {}", "file_remove"),
            ("file_prepend:\n  content: x\n", "file_prepend"),
            ("file_append:\n  content: x\n", "file_append"),
            ("file_rename: {}", "file_rename"),
            (
                "file_trim_trailing_whitespace: {}",
                "file_trim_trailing_whitespace",
            ),
            ("file_append_final_newline: {}", "file_append_final_newline"),
            (
                "file_normalize_line_endings: {}",
                "file_normalize_line_endings",
            ),
            ("file_strip_bidi: {}", "file_strip_bidi"),
            ("file_strip_zero_width: {}", "file_strip_zero_width"),
            ("file_strip_bom: {}", "file_strip_bom"),
            ("file_collapse_blank_lines: {}", "file_collapse_blank_lines"),
        ];
        for (yaml, expected) in cases {
            let spec: FixSpec =
                serde_yaml_ng::from_str(yaml).unwrap_or_else(|e| panic!("{yaml}: {e}"));
            assert_eq!(spec.op_name(), expected);
        }
    }

    #[test]
    fn resolve_content_source_inline_only() {
        let s = Some("hello".to_string());
        let resolved = resolve_content_source("r", "file_create", &s, &None).unwrap();
        assert!(matches!(resolved, ContentSourceSpec::Inline(b) if b == "hello"));
    }

    #[test]
    fn resolve_content_source_file_only() {
        let p = Some(Path::new("LICENSE").into());
        let resolved = resolve_content_source("r", "file_create", &None, &p).unwrap();
        assert!(matches!(resolved, ContentSourceSpec::File(p) if p == Path::new("LICENSE")));
    }

    #[test]
    fn resolve_content_source_rejects_both_set() {
        let err = resolve_content_source(
            "r",
            "file_prepend",
            &Some("x".into()),
            &Some(Path::new("y").into()),
        )
        .unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn resolve_content_source_rejects_neither_set() {
        let err = resolve_content_source("r", "file_append", &None, &None).unwrap_err();
        assert!(err.to_string().contains("required"));
    }

    #[test]
    fn content_source_spec_from_string_variants() {
        let from_owned: ContentSourceSpec = String::from("hi").into();
        assert!(matches!(from_owned, ContentSourceSpec::Inline(s) if s == "hi"));
        let from_str: ContentSourceSpec = "hi".into();
        assert!(matches!(from_str, ContentSourceSpec::Inline(s) if s == "hi"));
    }

    #[test]
    fn nested_rule_spec_instantiate_synthesizes_id_and_inherits_level() {
        let nested: NestedRuleSpec = serde_yaml_ng::from_str(
            "kind: file_exists\npaths: \"{path}/README.md\"\nmessage: missing in {path}\n",
        )
        .unwrap();
        let tokens = PathTokens::from_path(Path::new("packages/foo"));
        let spec = nested.instantiate("every-pkg-has-readme", 0, Level::Error, &tokens);

        assert_eq!(spec.id, "every-pkg-has-readme.require[0]");
        assert_eq!(spec.kind, "file_exists");
        assert_eq!(spec.level, Level::Error);
        // Path template should have been rendered for both
        // `paths:` and `message:` from the iterated tokens.
        match spec.paths {
            Some(PathsSpec::Single(p)) => assert_eq!(p, "packages/foo/README.md"),
            other => panic!("unexpected paths shape: {other:?}"),
        }
        assert_eq!(spec.message.as_deref(), Some("missing in packages/foo"));
        // Nested rules don't propagate git_tracked_only: it is a kind-specific
        // option (ADR-0008) stripped from the nested `extra` via PARENT_FIELDS,
        // so it never reaches the leaf rule's options.
        assert!(
            !spec
                .extra
                .keys()
                .any(|k| k.as_str() == Some("git_tracked_only")),
            "git_tracked_only should be stripped from nested extra"
        );
    }
}
