use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::Deserialize;

use crate::facts::FactSpec;
use crate::level::Level;

fn deserialize_present<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Parsed form of a `.alint.yml` file.
///
/// Outside deserialization, built with [`Config::new`] and the `with_*`
/// setters, so a later field is an addition rather than a breaking change; the
/// fields stay public, to read and to assign.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Config {
    pub version: u32,
    /// Highest minimum stable alint release required by any composed source.
    /// Per-source provenance is retained in [`Self::compatibility`].
    #[serde(default, deserialize_with = "deserialize_present")]
    pub requires: Option<crate::ConfigRequirements>,
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
    /// Per-source compatibility records retained by the recursive loader.
    /// Source-local requirements never participate in config precedence.
    #[serde(skip)]
    pub compatibility: Vec<crate::ConfigCompatibility>,
    /// Values the environment supplied to the top-level fields of the user's
    /// configs (`extends:`, `ignore:`, `facts:`, `baseline:` and the rest;
    /// not `rules:`, `templates:` or `vars:`, whose errors are redacted per
    /// rule), recorded by the loader. An embedder removes them from an error it
    /// shows with [`Self::redact_environment_values`], since such an error (an
    /// invalid ignore pattern, an unreadable baseline) can quote the value.
    #[serde(skip)]
    #[doc(hidden)]
    pub environment_values: Vec<String>,
    /// The resolved local files the configuration came from: the top-level
    /// file, its `.alint.d/` drop-ins and every local `extends:` target,
    /// recorded by the loader. A fix from an `extends:`'d ruleset never writes
    /// one ([`crate::FixContext::config_files`]).
    #[serde(skip)]
    pub config_files: Vec<PathBuf>,
    /// The resolved selectors and relations of a version 2 configuration
    /// (`None` for version 1), recorded by the loader once every name has
    /// resolved and the plan has been checked. Hidden from the documented
    /// API because version 2 is experimental (ADR-0034 decision 1) and the
    /// type belongs to the hidden relation layer; public so the CLI's `list`
    /// and the rule kinds that read relations can reach it.
    #[serde(skip)]
    #[doc(hidden)]
    pub relation_plan: Option<std::sync::Arc<crate::relation::plan::Plan>>,
}

// Returning `Option<u64>` (rather than bare `u64`) keeps the
// YAML-facing type consistent with `Config.fix_size_limit`:
// users set `null` in YAML to mean "no limit". The Option is
// load-bearing at the field level, so clippy's warning on the
// default fn is noise here.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn default_fix_size_limit() -> Option<u64> {
    Some(1 << 20)
}

pub(crate) fn default_respect_gitignore() -> bool {
    true
}

impl Config {
    pub const MIN_SUPPORTED_VERSION: u32 = 1;
    pub const CURRENT_VERSION: u32 = 1;

    #[must_use]
    pub const fn supports_version(version: u32) -> bool {
        version >= Self::MIN_SUPPORTED_VERSION && version <= Self::CURRENT_VERSION
    }
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
    /// Whether the effective rule `spec` may read, and its fix read or write,
    /// outside the root: only a rule of the user's own top-level config the
    /// grant names ([`Self::allows`]). A rule an `extends:`'d ruleset or a
    /// nested config shaped ([`RuleSpec::extended_source`]: it declared the
    /// rule, contributed a field to it, or defined its template) never may,
    /// whatever the grant says, even when the user's config overrides it by
    /// id or lists that id under `rules:`.
    #[must_use]
    pub fn allows_rule(&self, spec: &RuleSpec) -> bool {
        !spec.extended_source && self.allows(&spec.id, &spec.kind)
    }

    /// Whether a rule with this `id` / `kind` may read out of root, by the
    /// grant alone. A builder asks [`Self::allows_rule`], which also keeps a
    /// rule from an `extends:`'d ruleset confined.
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
#[derive(Debug, Clone)]
pub enum ExtendsEntry {
    Url(String),
    Filtered {
        url: String,
        only: Option<Vec<String>>,
        except: Option<Vec<String>>,
    },
}

/// The mapping form of [`ExtendsEntry`], deserialized with
/// `deny_unknown_fields` so a typo such as `excpet:` is a load error rather
/// than a silently-ignored filter (an untagged enum variant cannot deny
/// unknown fields itself).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FilteredExtendsEntry {
    url: String,
    #[serde(default)]
    only: Option<Vec<String>>,
    #[serde(default)]
    except: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for ExtendsEntry {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;
        match ayaml::Value::deserialize(deserializer)? {
            ayaml::Value::String(url) => Ok(Self::Url(url)),
            ayaml::Value::Tagged(tagged) => Err(D::Error::custom(crate::yaml_decode::tag_refused(
                &tagged.tag,
            ))),
            value @ ayaml::Value::Mapping(_) => {
                let FilteredExtendsEntry { url, only, except } =
                    crate::yaml_decode::from_value(value).map_err(D::Error::custom)?;
                Ok(Self::Filtered { url, only, except })
            }
            _ => Err(D::Error::custom(
                "an `extends:` entry must be a URL or path string, or a \
                 `{ url, only?, except? }` map",
            )),
        }
    }
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
    /// Split a raw pattern list into `(include, exclude)` exactly as the scope
    /// matcher does (`Scope::from_patterns`): a leading `!` marks an exclusion,
    /// with the `!` stripped. Keeps `alint explain` honest about a rule that
    /// uses inline negations (`paths: ["src/**", "!src/tests/**"]`).
    fn split_patterns(patterns: &[String]) -> (Vec<String>, Vec<String>) {
        let mut include = Vec::new();
        let mut exclude = Vec::new();
        for p in patterns {
            if let Some(rest) = p.strip_prefix('!') {
                exclude.push(rest.to_string());
            } else {
                include.push(p.clone());
            }
        }
        (include, exclude)
    }

    /// Normalised `(include, exclude)` glob lists, for machine output. Mirrors
    /// how `Scope::from_paths_spec` classifies each form, so inline `!`
    /// negations report as excludes rather than includes.
    #[must_use]
    pub fn include_exclude(&self) -> (Vec<String>, Vec<String>) {
        match self {
            PathsSpec::Single(s) => Self::split_patterns(std::slice::from_ref(s)),
            PathsSpec::Many(v) => Self::split_patterns(v),
            PathsSpec::IncludeExclude { include, exclude } => {
                // The include list may itself carry `!` negations (the matcher
                // folds them in); split it, then append the explicit excludes.
                let (inc, mut exc) = Self::split_patterns(include);
                exc.extend(exclude.iter().cloned());
                (inc, exc)
            }
        }
    }

    /// Human-readable one-line rendering of the scope, for `alint explain`.
    /// Built from [`include_exclude`](Self::include_exclude) so it classifies
    /// inline `!` negations the same way the matcher does.
    #[must_use]
    pub fn render_scope(&self) -> String {
        let (include, exclude) = self.include_exclude();
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
/// by a [`RuleBuilder`](crate::registry::RuleBuilder). Code builds one with
/// [`RuleSpec::new`] and the `with_*` setters; its public fields can be assigned.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
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
    /// Require the rule's effective file scope (`paths:` plus any
    /// `scope_filter:` predicates) to contain at least one file. Evaluated only
    /// after `when:` is true and against the full repository index, including
    /// during `--changed` runs. Unsupported for existence and selector rules,
    /// whose empty sets already have kind-specific meaning.
    #[serde(default)]
    pub expect_matches: bool,
    /// Redact this rule's flattened finding messages and fix detail before any
    /// report or baseline is exposed. Defaults to `false`.
    #[serde(default)]
    pub sensitive: bool,
    /// Effective top-level fields whose value was derived from environment
    /// interpolation. The DSL loader populates this sparse set after merge and
    /// template expansion; it is never deserialized from user input.
    #[serde(skip)]
    #[doc(hidden)]
    pub sensitive_fields: std::collections::BTreeSet<String>,
    /// Whether an `extends:`'d ruleset or a nested config contributed to this
    /// effective rule (declared it, shares its id, or defines the template it
    /// instantiates). Neither source may run commands, so a fix of the rule
    /// never writes alint's own configuration, which may, and never outside
    /// the root ([`FixContext::extended_source`](crate::FixContext::extended_source)).
    /// The DSL loader sets it after composition; it is never deserialized from
    /// user input, and a rule built in code is the user's own (`false`).
    #[serde(skip)]
    #[doc(hidden)]
    pub extended_source: bool,
    /// Whether an iteration rendered this spec ([`NestedRuleSpec::instantiate`]):
    /// every path token in it, including in the `require:` lists of the rules
    /// nested inside it, holds that iteration's value. Those lists are then
    /// compiled already rendered ([`CompiledNestedSpec::rendered`]), and a
    /// template the rule renders per entry is final
    /// ([`PathTemplate`](crate::template::PathTemplate)), so a token spelled
    /// in a value (a directory named `x{stem}`) stays text. Set only by
    /// instantiation; never deserialized from user input.
    #[serde(skip)]
    #[doc(hidden)]
    pub rendered: bool,
    /// The resolved plan of the version 2 configuration this rule belongs to,
    /// set by the DSL loader once every reference has resolved, on a rule
    /// whose fields reference a relation (the `constraint` kind's `input:`):
    /// [`Plan::rule`](crate::relation::plan::Plan::rule) holds what each
    /// field resolved to. `None` for every other rule, and for a rule a
    /// version 1 config or code built. Hidden, as the relation layer is:
    /// version 2 is experimental (ADR-0034 decision 1). Never deserialized
    /// from user input.
    #[serde(skip)]
    #[doc(hidden)]
    pub relation_plan: Option<std::sync::Arc<crate::relation::plan::Plan>>,
    /// Optional mechanical-fix strategy. Rules whose builders understand
    /// the chosen op attach a [`Fixer`](crate::Fixer) to the built rule;
    /// rules whose kind is incompatible with the op return a config error
    /// at build time. A block with more than one op key is rejected at
    /// load (R-TWOOP) by `deserialize_fix_spec`.
    #[serde(default, deserialize_with = "deserialize_fix_spec")]
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
    pub extra: crate::yaml::Mapping,
}

/// The `fix:` block on a rule. Exactly one op key must be present —
/// alint errors at load time when the op and rule kind are incompatible.
///
/// Decoded by its op key ([`FixSpec::deserialize`]), not as a serde
/// `untagged` enum: an untagged variant reads its options from serde's
/// buffered copy of the value, which takes a one-entry mapping for an enum
/// variant without data (`applicability: {safe: ~}` for `safe`), past the
/// checked decoder that refuses one.
#[derive(Debug, Clone)]
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
    Replace {
        replace: ReplaceFixSpec,
    },
    SetValue {
        set_value: SetValueFixSpec,
    },
    RemoveValue {
        remove_value: RemoveValueFixSpec,
    },
    Chmod {
        chmod: ChmodFixSpec,
    },
    GitUntrack {
        git_untrack: GitUntrackFixSpec,
    },
    Command {
        command: CommandFixSpec,
    },
    DirCreate {
        dir_create: DirCreateFixSpec,
    },
    SyncFrom {
        sync_from: SyncFromFixSpec,
    },
    Relocate {
        relocate: RelocateFixSpec,
    },
    CreateAndRegister {
        create_and_register: CreateAndRegisterFixSpec,
    },
    Sort {
        sort: SortFixSpec,
    },
    IndentStyle {
        indent_style: IndentStyleFixSpec,
    },
    InsertLine {
        insert_line: InsertLineFixSpec,
    },
    InsertHeader {
        insert_header: InsertHeaderFixSpec,
    },
}

/// Deserialize a rule's `fix:` block, rejecting a block with more than one
/// op key (R-TWOOP), naming the keys. A file-mutating feature must not pick
/// one op and drop its siblings, so a two-op block is a hard config error
/// rather than a silent first-wins.
fn deserialize_fix_spec<'de, D>(deserializer: D) -> std::result::Result<Option<FixSpec>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    let value = ayaml::Value::deserialize(deserializer)?;
    if value.is_null() {
        return Ok(None);
    }
    if let ayaml::Value::Mapping(m) = &value
        && m.len() != 1
    {
        let mut keys: Vec<String> = m
            .keys()
            .map(|k| match k {
                ayaml::Value::String(s) => s.clone(),
                other => format!("{other:?}"),
            })
            .collect();
        keys.sort();
        return Err(D::Error::custom(format!(
            "a `fix:` block must have exactly one op key, found {}: {}. \
                 Each rule declares a single fix op; split them into separate rules.",
            m.len(),
            keys.join(", ")
        )));
    }
    // Every op's options must be a mapping. The derived op structs would also
    // accept a YAML sequence (filling fields by position), and the DSL trust
    // gates -- which refuse an inherited `applicability: safe` promotion and
    // demote an untrusted remote's content fixers -- inspect mappings only, so a
    // positional `file_remove: [safe]` would load as an unreviewed promotion.
    if let ayaml::Value::Mapping(m) = &value
        && let Some((op, args)) = m.iter().next()
        && !args.is_mapping()
    {
        let op = op.as_str().unwrap_or("<fix>");
        return Err(D::Error::custom(format!(
            "`{op}`: a fix op's options must be a mapping (write `{op}: {{}}` for an op \
             with no options)"
        )));
    }
    crate::yaml_decode::from_value(value)
        .map(Some)
        .map_err(D::Error::custom)
}

/// What a `fix:` block that is not one op key holds.
const FIX_SPEC_EXPECTING: &str =
    "a map with exactly one fix op (e.g. `file_create`, `file_remove`, `file_prepend`)";

impl<'de> Deserialize<'de> for FixSpec {
    /// Read the block's one op key, then decode its options as that op's
    /// struct through the checked decoder (`yaml_decode::from_value`),
    /// so a field holding an enum takes only a variant's name, and an error
    /// names the op and the field (`` `replace`: missing field `replacement` ``).
    // One arm per fix op.
    #[allow(clippy::too_many_lines)]
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        use serde::de::Error as _;
        fn options<T: serde::de::DeserializeOwned, E: serde::de::Error>(
            op: &str,
            args: ayaml::Value,
        ) -> std::result::Result<T, E> {
            crate::yaml_decode::from_value(args)
                .map_err(|error| E::custom(format!("`{op}`: {error}")))
        }
        let ayaml::Value::Mapping(mapping) = ayaml::Value::deserialize(deserializer)? else {
            return Err(D::Error::custom(FIX_SPEC_EXPECTING));
        };
        let mut entries = mapping.into_iter();
        let (Some((op, args)), None) = (entries.next(), entries.next()) else {
            return Err(D::Error::custom(FIX_SPEC_EXPECTING));
        };
        let Some(op) = op.as_str() else {
            return Err(D::Error::custom(FIX_SPEC_EXPECTING));
        };
        Ok(match op {
            "file_create" => Self::FileCreate {
                file_create: options(op, args)?,
            },
            "file_remove" => Self::FileRemove {
                file_remove: options(op, args)?,
            },
            "file_prepend" => Self::FilePrepend {
                file_prepend: options(op, args)?,
            },
            "file_append" => Self::FileAppend {
                file_append: options(op, args)?,
            },
            "file_rename" => Self::FileRename {
                file_rename: options(op, args)?,
            },
            "file_trim_trailing_whitespace" => Self::FileTrimTrailingWhitespace {
                file_trim_trailing_whitespace: options(op, args)?,
            },
            "file_append_final_newline" => Self::FileAppendFinalNewline {
                file_append_final_newline: options(op, args)?,
            },
            "file_normalize_line_endings" => Self::FileNormalizeLineEndings {
                file_normalize_line_endings: options(op, args)?,
            },
            "file_strip_bidi" => Self::FileStripBidi {
                file_strip_bidi: options(op, args)?,
            },
            "file_strip_zero_width" => Self::FileStripZeroWidth {
                file_strip_zero_width: options(op, args)?,
            },
            "file_strip_bom" => Self::FileStripBom {
                file_strip_bom: options(op, args)?,
            },
            "file_collapse_blank_lines" => Self::FileCollapseBlankLines {
                file_collapse_blank_lines: options(op, args)?,
            },
            "replace" => Self::Replace {
                replace: options(op, args)?,
            },
            "set_value" => Self::SetValue {
                set_value: options(op, args)?,
            },
            "remove_value" => Self::RemoveValue {
                remove_value: options(op, args)?,
            },
            "chmod" => Self::Chmod {
                chmod: options(op, args)?,
            },
            "git_untrack" => Self::GitUntrack {
                git_untrack: options(op, args)?,
            },
            "command" => Self::Command {
                command: options(op, args)?,
            },
            "dir_create" => Self::DirCreate {
                dir_create: options(op, args)?,
            },
            "sync_from" => Self::SyncFrom {
                sync_from: options(op, args)?,
            },
            "relocate" => Self::Relocate {
                relocate: options(op, args)?,
            },
            "create_and_register" => Self::CreateAndRegister {
                create_and_register: options(op, args)?,
            },
            "sort" => Self::Sort {
                sort: options(op, args)?,
            },
            "indent_style" => Self::IndentStyle {
                indent_style: options(op, args)?,
            },
            "insert_line" => Self::InsertLine {
                insert_line: options(op, args)?,
            },
            "insert_header" => Self::InsertHeader {
                insert_header: options(op, args)?,
            },
            other => {
                return Err(D::Error::custom(format!(
                    "`{other}` is not a fix op (one of {})",
                    Self::ALL_OP_NAMES
                        .iter()
                        .map(|name| format!("`{name}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        })
    }
}

impl FixSpec {
    /// Every fix op's YAML key, in declaration order. The single source of
    /// truth the fix-coverage gate (`coverage_audit_fix_coverage`)
    /// enumerates. A new enum variant forces a new [`op_name`](Self::op_name)
    /// arm (the match is exhaustive) and must be added here in tandem;
    /// `fix_spec_op_name_covers_every_variant` asserts the two agree.
    pub const ALL_OP_NAMES: &'static [&'static str] = &[
        "file_create",
        "file_remove",
        "file_prepend",
        "file_append",
        "file_rename",
        "file_trim_trailing_whitespace",
        "file_append_final_newline",
        "file_normalize_line_endings",
        "file_strip_bidi",
        "file_strip_zero_width",
        "file_strip_bom",
        "file_collapse_blank_lines",
        "replace",
        "set_value",
        "remove_value",
        "chmod",
        "git_untrack",
        "command",
        "dir_create",
        "sync_from",
        "relocate",
        "create_and_register",
        "sort",
        "indent_style",
        "insert_line",
        "insert_header",
    ];

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
            Self::Replace { .. } => "replace",
            Self::SetValue { .. } => "set_value",
            Self::RemoveValue { .. } => "remove_value",
            Self::Chmod { .. } => "chmod",
            Self::GitUntrack { .. } => "git_untrack",
            Self::Command { .. } => "command",
            Self::DirCreate { .. } => "dir_create",
            Self::SyncFrom { .. } => "sync_from",
            Self::Relocate { .. } => "relocate",
            Self::CreateAndRegister { .. } => "create_and_register",
            Self::Sort { .. } => "sort",
            Self::IndentStyle { .. } => "indent_style",
            Self::InsertLine { .. } => "insert_line",
            Self::InsertHeader { .. } => "insert_header",
        }
    }

    /// Whether the op leaves every path that exists the kind of node it was:
    /// it edits, creates or changes the mode of files and makes directories,
    /// but never moves or removes a file or runs a command, any of which can
    /// leave a symlink where the walk saw a file. Until a pass applies an op
    /// that may, the walk still answers which paths a symlink is on
    /// ([`FixContext::index`](crate::FixContext::index)). A new op decides here.
    pub(crate) fn keeps_paths_in_place(&self) -> bool {
        match self {
            Self::FileRemove { .. }
            | Self::FileRename { .. }
            | Self::Relocate { .. }
            | Self::GitUntrack { .. }
            | Self::Command { .. } => false,
            Self::FileCreate { .. }
            | Self::FilePrepend { .. }
            | Self::FileAppend { .. }
            | Self::FileTrimTrailingWhitespace { .. }
            | Self::FileAppendFinalNewline { .. }
            | Self::FileNormalizeLineEndings { .. }
            | Self::FileStripBidi { .. }
            | Self::FileStripZeroWidth { .. }
            | Self::FileStripBom { .. }
            | Self::FileCollapseBlankLines { .. }
            | Self::Replace { .. }
            | Self::SetValue { .. }
            | Self::RemoveValue { .. }
            | Self::Chmod { .. }
            | Self::DirCreate { .. }
            | Self::SyncFrom { .. }
            | Self::CreateAndRegister { .. }
            | Self::Sort { .. }
            | Self::IndentStyle { .. }
            | Self::InsertLine { .. }
            | Self::InsertHeader { .. } => true,
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
    /// Per-rule applicability override (auto-fix.md 5.5). Defaults to `Safe`
    /// (creating a required file is behavior-preserving). W2 uses this to demote a
    /// `file_create` from an untrusted remote `extends:` to `suggestion` -- its
    /// content is third-party-authored. An extends'd ruleset may only DEMOTE; a
    /// promotion is rejected upstream by the DSL trust gate.
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

fn default_create_parents() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileRemoveFixSpec {
    /// Per-rule applicability override (auto-fix.md 5.5). `file_remove` defaults
    /// to `Unsafe` -- deleting a whole file irreversibly is a poor default for a
    /// bare `alint fix`, so it is surfaced as a suggestion and applied only with
    /// `--unsafe-fixes`. A user may promote it back to `safe` on a specific rule
    /// (`fix: { file_remove: { applicability: safe } }`) in their OWN top-level
    /// config; an extends'd ruleset attempting the promotion is rejected upstream
    /// by the DSL trust gate (inherited fixers may be demoted, never promoted).
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

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
    /// Per-rule applicability override (auto-fix.md 5.5). Defaults to `Safe`. W2
    /// demotes a `file_prepend` from an untrusted remote `extends:` to `suggestion`
    /// (the header bytes are third-party-authored); demote-only, promotion refused.
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
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
    /// Per-rule applicability override (auto-fix.md 5.5). Defaults to `Safe`. W2
    /// demotes a `file_append` from an untrusted remote `extends:` to `suggestion`
    /// (the footer bytes are third-party-authored); demote-only, promotion refused.
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// Resolution of an `(content, content_from)` pair to a single
/// content source. Used by the three fixers that take either.
/// Errors when neither or both are set, or when `content_from` names a Git
/// directory, which no rule reads.
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
        // A rule never reads a Git directory: `.git/config` can hold
        // credentials, which a created file would copy into the worktree.
        (None, Some(p)) if crate::path_names_git_directory(p) => {
            Err(crate::error::Error::rule_config(
                rule_id,
                format!(
                    "`fix.{op_name}.content_from` {}",
                    crate::git_directory_read_refusal(p)
                ),
            ))
        }
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

/// The target name is derived from the parent rule (e.g. `filename_case` converts
/// the stem to its configured case; the extension is preserved). **`Safe` by
/// default**; **content-injecting** in the W2 partition (no ruleset bytes, but a
/// remote's `paths:` can AIM a mass case-rename at the victim's source, breaking
/// case-sensitive imports -- so an untrusted remote demotes it to a suggestion).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileRenameFixSpec {
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// Behavior: read file (subject to `fix_size_limit`), strip trailing space/tab on
/// every line, write back. **`Safe` by default**; **content-injecting** in the W2
/// partition (no ruleset bytes, but a remote's `paths:` can AIM a trim at a file
/// where trailing whitespace is significant -- e.g. a Markdown hard line break
/// `two spaces` -- so an untrusted remote demotes it to a suggestion).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileTrimTrailingWhitespaceFixSpec {
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// Behavior: if the file has content and does not end with `\n`, append one.
/// **`Safe` by default**; **content-injecting** in the W2 partition (no ruleset
/// bytes, but a remote's `paths:` can AIM the append at a file where the final byte
/// matters, so an untrusted remote demotes it to a suggestion).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileAppendFinalNewlineFixSpec {
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// Behavior: rewrite the file with every line ending replaced by the parent rule's
/// configured target (`lf` or `crlf`). **`Safe` by default**; **content-injecting**
/// in the W2 partition (no ruleset bytes, but a remote's `paths:` can AIM a CRLF/LF
/// rewrite at a line-ending-significant file -- CRLF on a `#!/bin/sh` shebang breaks
/// it -- so an untrusted remote demotes it to a suggestion).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileNormalizeLineEndingsFixSpec {
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

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

/// Behavior: remove a leading UTF-8/UTF-16/UTF-32 BOM byte sequence if present;
/// otherwise a no-op. **`Safe` by default**; **content-injecting** in the W2
/// partition (no ruleset bytes, but a remote's `paths:` can AIM a BOM strip at a
/// file whose encoding signature matters, so an untrusted remote demotes it to a
/// suggestion).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileStripBomFixSpec {
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// Behavior: collapse runs of blank lines longer than the parent rule's `max` down
/// to exactly `max` blank lines. **`Safe` by default**; **content-injecting** in the
/// W2 partition (no ruleset bytes, but a remote's `paths:` can AIM the collapse at a
/// file where blank-line runs are significant -- e.g. paragraph breaks -- so an
/// untrusted remote demotes it to a suggestion).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileCollapseBlankLinesFixSpec {
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// The `replace` op (Phase 1): rewrite each span the host rule's `pattern:`
/// matches with `replacement`. Wired to `file_content_forbidden` only -- the
/// pattern comes from the host rule, not a field here. (`file_content_matches`
/// violates on the pattern's *absence*, so `replace` -- which rewrites matches
/// -- has nothing to act on there and rejects the op.) `replacement` is a byte
/// template supporting `$1` / `${name}`
/// capture references (the regex-crate substitution syntax); a literal `$` is
/// written `$$`. This is a *located* op (one [`crate::FixEdit::ReplaceRange`] per
/// match), Unsafe by default (a regex rewrite is not behavior-preserving in
/// general), applied only under `--unsafe-fixes` unless promoted per-rule.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaceFixSpec {
    /// The search regex, applied to the SCALAR VALUE at the rule's `path:`. Used
    /// (and REQUIRED) by `*_path_matches` only: there the rule's `matches:` is the
    /// check + re-verify target, so the transformation needs its own search
    /// pattern. `file_content_forbidden` / `file_content_matches` leave this
    /// unset -- they use the HOST rule's own pattern as the whole-file search.
    #[serde(default)]
    pub pattern: Option<String>,
    /// The replacement template written in place of each match. Supports the
    /// regex crate's `$1` / `${name}` capture references; `$$` is a literal `$`.
    pub replacement: String,
    /// Per-rule applicability override (auto-fix.md 5.5). `replace` defaults to
    /// `Unsafe`; a user may set `safe` in their OWN top-level config when the
    /// rewrite is provably a normalization (an inherited config may only demote,
    /// enforced by the DSL trust gate).
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// `set_value` (Phase 2): the located structured-value setter. Hosts on the
/// eight `*_path_equals` kinds and reads BOTH its parameters -- the `JSONPath`
/// `path:` and the expected value `equals:` -- from the host rule, so the fix
/// restates nothing the rule already carries. Format-preservingly rewrites the
/// value at `path` to `equals` via a byte-range splice against a span-resolving
/// parser (auto-fix.md 5.3/5.4). Content-injecting (it writes the rule's
/// `equals` bytes), so a remote `extends:` demotes it under W2.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetValueFixSpec {
    /// Per-rule applicability override. `set_value` defaults to `Safe`, but ONLY
    /// a scalar replacing an existing scalar node is ever applied at tier: the
    /// fixer itself demotes object/array values, zero-match insertion, and
    /// nested creation to `Suggestion` regardless of this field (the re-parse
    /// postcondition is undefined there, auto-fix.md 5.8). An inherited config
    /// may only demote (the DSL trust gate), never promote.
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// `remove_value` (Phase 2): the located structured-value remover. Hosts on the
/// eight `*_path_absent` kinds and reads its `path:` from the host rule.
/// Deletes the node the `JSONPath` selects, along with its format-specific
/// separator (a trailing comma, a whole `key = value` line, an XML element),
/// via a byte-range splice. Fixed-behavior (it injects no ruleset bytes), so W2
/// never demotes it; `Unsafe` by default because a deletion is not
/// behavior-preserving in general.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoveValueFixSpec {
    /// Per-rule applicability override. `remove_value` defaults to `Unsafe`; a
    /// user may promote a specific rule to `Safe` in their OWN top-level config.
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// `chmod`: set or clear the Unix executable bits (`0o111`) on the violating
/// file, preserving every other permission bit. The host rule (`executable_bit`,
/// `shebang_has_executable`) picks the direction. Unix-only; a no-op on platforms
/// without an executable bit, where the host rule never fires. `Safe` by default:
/// the rule explicitly requires the +x state, the change is reversible, and it
/// touches only the executable bits.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChmodFixSpec {
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// `git_untrack`: remove the violating path from git's index (`git rm --cached`)
/// while leaving it on disk, for the "a committed artifact should be untracked"
/// hygiene case (host `file_absent` with `git_tracked_only: true`, so the rule
/// converges once the path leaves the tracked set). A **spawning** fix op: it
/// shells out to `git`, so it is refused from any non-top-level source
/// (`extends:`/nested/bundled) exactly like a spawning rule kind (auto-fix.md
/// 5.5), and it is **`Unsafe` by default** (irreversibly restructures the index
/// on a bare `alint fix`; surfaced as a suggestion, applied only with
/// `--unsafe-fixes`). A user may promote a specific rule to `Safe` in their OWN
/// top-level config.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitUntrackFixSpec {
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// `dir_create`: create the (single, literal) directory the host `dir_exists`
/// rule requires. A **fixed-behavior** op: it writes no ruleset bytes and does not
/// spawn, so it is honored at its tier from any source. **`Safe` by default** (an
/// empty directory is benign and easily removed). Only valid when the rule's
/// `paths` is one literal directory (a glob or multiple patterns is ambiguous and
/// rejected at load). Note: git does not track an empty directory, so this creates
/// it for local tooling; pair with a `file_create` of a `.gitkeep` to persist it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirCreateFixSpec {
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// `sync_from`: overwrite a drifted target with its canonical source, so a
/// `cross_file` `relation: identical` rule converges. A **content-injecting** op:
/// it writes another file's bytes wholesale, and the host rule's `source:`
/// decides which file overwrites which, so a remote `extends:` the user has not
/// trusted demotes it to `suggestion` (auto-fix.md 5.5). **`Unsafe` by default**
/// (a whole-file overwrite can discard uncommitted target content). Only valid on
/// `relation: identical` with `skip_header_lines: 0` (a header-preserving sync is
/// a deferred follow-up) and a single-file `source:` (not a glob-union); other
/// shapes are rejected at load. Takes its source from the host rule, not a new
/// field.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncFromFixSpec {
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// `relocate`: move a file the host `file_absent` rule flagged in a subdirectory
/// back to the repository root, keeping its basename (a lockfile that drifted
/// into a member directory belongs at the workspace root). A **fixed-behavior**
/// op: it writes no ruleset bytes and does not spawn, so it is honored at its
/// tier from any source. **`Unsafe` by default** (a rename is easy to review but
/// moves a real file, and the destination is inferred), `Safe`-promotable. Only
/// the unambiguous case is fixed: the flagged file is below the root and the root
/// slot (`<basename>` at the repo root) is free. A file already at the root, a
/// taken root slot, or an undeterminable basename is left for a human (reported,
/// not fixed). Takes its source path from the violation, not a new field.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelocateFixSpec {
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// `create_and_register`: ensure a `cross_file` `relation: registered` member both
/// exists and is listed in each target's manifest array, applying only the unmet
/// repair. An existing-but-unregistered member is APPENDED to the list (a
/// single-file structured edit); a MISSING NAMED member is CREATED from `content` /
/// `content_from` below and then registered (two independent idempotent fixes the
/// fixpoint applies in order -- no multi-file transaction). A **content-injecting**
/// op (it writes a ruleset-chosen value into a manifest, and a ruleset-authored
/// file), so an untrusted remote `extends:` demotes it to a suggestion (auto-fix.md
/// 5.5). **`Unsafe` by default** (mutates a manifest / creates a file),
/// `Safe`-promotable. Members + targets come from the host rule.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateAndRegisterFixSpec {
    /// Inline bytes for a MISSING named member (the create half). Mutually
    /// exclusive with `content_from`; both absent = register-only (a glob source
    /// never has a missing member, so content is inert there and rejected at load).
    #[serde(default)]
    pub content: Option<String>,
    /// A `content_from` path (read at apply time, confined) for a missing named
    /// member. Mutually exclusive with `content`.
    #[serde(default)]
    pub content_from: Option<std::path::PathBuf>,
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// The `sort` op (Phase 4): rewrite each of the host `ordered_block` rule's
/// marked blocks with its entries sorted under the rule's `comparator` (and
/// deduped when the rule sets `unique`), preserving non-entry lines (markers,
/// blanks, `select`-excluded comments) in place. Behavior comes entirely from
/// the host rule (`start`/`end`/`comparator`/`unique`/`select`); this spec adds
/// only the optional tier override. **`Safe` by default** — a keep-sorted
/// block's meaning is order-independent (and a `unique` block declared its
/// duplicates redundant), so sorting is behavior-preserving.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SortFixSpec {
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// The `indent_style` op (Phase 4): reindent the host `indent_style` rule's
/// violating lines. Wired ONLY for `style: spaces` + a `width: N` (the
/// spaces-per-tab): it rewrites a PURE-TAB leading run (K tabs) to K*N spaces, and
/// leaves the genuinely ambiguous cases (a mixed tab+space lead, a pure-space
/// width mismatch). Behavior comes from the host rule (`style` / `width`); this
/// spec adds only the optional tier override. **`Unsafe` by default** (a mis-aimed
/// reindent HARD-breaks an indent-significant file such as a `Makefile` recipe;
/// audit round-3), so a bare `fix` suggests it and `--unsafe-fixes` applies it;
/// **content-injecting** in the W2 partition (a remote can aim it at such a file).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct IndentStyleFixSpec {
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// The `insert_line` op (Phase 4): insert a missing `require:` line of the host
/// `ordered_block` rule at its SORTED position (using the rule's `comparator`).
/// The required lines + comparator come from the host rule; this spec adds only
/// the optional tier override. **`Unsafe` by default** (it adds ruleset-authored
/// content at a COMPUTED position, load-bearing in the order-sensitive formats it
/// targets); **content-injecting** in the W2 partition (the `require:` lines are
/// ruleset-authored, so an untrusted remote demotes it).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct InsertLineFixSpec {
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// The `insert_header` op (Phase 4): insert the host `file_header` rule's required
/// header at the top of a violating file, AFTER any leading BOM, shebang
/// (`#!...`), or XML declaration (`<?xml ...?>`) -- the differentiator over
/// `file_prepend`, which prepends at BOF (after any BOM, but blind to a shebang /
/// XML declaration) and would push a shebang off line 1. Content comes from
/// `content` / `content_from` (as `file_prepend`). **`Safe` by default** (the
/// insertion point is the one canonical header spot and the content is inert; at
/// least as safe as the `Safe` `file_prepend` it refines, and safer on a file with
/// a shebang / XML declaration, which `file_prepend` would displace);
/// **content-injecting** in the W2 partition (the header bytes are ruleset-authored,
/// so an untrusted remote demotes it). NOTE: because the header can land below line
/// 1, the host rule's `pattern` must be able to match below the first line (use an
/// unanchored pattern or `(?m)`); a `^`-anchored pattern that only matches line 1
/// cannot be satisfied once the header sits under a shebang.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InsertHeaderFixSpec {
    /// Inline header bytes to insert. Mutually exclusive with `content_from`.
    /// A trailing newline is the caller's responsibility.
    #[serde(default)]
    pub content: Option<String>,
    /// Path to a file (relative to the lint root) whose bytes are the header.
    /// Mutually exclusive with `content`.
    #[serde(default)]
    pub content_from: Option<PathBuf>,
    /// Per-rule applicability override (auto-fix.md 5.5). Defaults to `Safe`. W2
    /// demotes an `insert_header` from an untrusted remote `extends:` to
    /// `suggestion` (the header bytes are third-party-authored); demote-only.
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

/// `command`: run a user-supplied fix command per violation on a `command` rule
/// (e.g. check `eslint {path}`, fix `eslint --fix {path}`). A **spawning** fix op:
/// it shells out, so it is refused from any non-top-level source (auto-fix.md 5.5,
/// like the `command` rule kind itself) and is **`Unsafe` by default** (running an
/// arbitrary command on a bare `alint fix` is opt-in). A user may promote a
/// specific rule to `Safe` in their OWN top-level config.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandFixSpec {
    /// The fix command argv. The first token is the program (PATH-looked-up if a
    /// bare name). EVERY token -- the program included -- accepts the same `{path}`
    /// / `{dir}` / `{stem}` / ... templates (with the option-injection guard) as
    /// the `command` rule's `command:`; put a fixed program name first unless you
    /// intend to run a repo file. Must be non-empty.
    pub run: Vec<String>,
    /// Per-invocation timeout in seconds (default 30, matching the `command`
    /// rule). Past it the child is killed and the fix reports an error.
    #[serde(default)]
    pub timeout: Option<u64>,
    #[serde(default)]
    pub applicability: Option<crate::rule::Applicability>,
}

impl RuleSpec {
    /// Deserialize the full spec (common + kind-specific fields) into a typed
    /// options struct. Common fields are reconstructed into the mapping so
    /// the target struct can `#[derive(Deserialize)]` against the whole shape
    /// when convenient.
    pub fn deserialize_options<T>(&self) -> crate::error::Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        Ok(crate::yaml::from_value(self.extra.clone().into())?)
    }

    /// Record environment-derived effective fields after config composition.
    #[doc(hidden)]
    pub fn set_sensitive_fields(&mut self, fields: std::collections::BTreeSet<String>) {
        self.sensitive_fields = fields;
    }

    /// Record that an `extends:`'d ruleset or a nested config contributed to
    /// this rule ([`Self::extended_source`]), after config composition.
    #[doc(hidden)]
    pub fn set_extended_source(&mut self, extended_source: bool) {
        self.extended_source = extended_source;
    }

    /// Record the version 2 plan this rule's reference fields resolved in
    /// ([`Self::relation_plan`]), after config composition.
    #[doc(hidden)]
    pub fn set_relation_plan(&mut self, plan: Option<std::sync::Arc<crate::relation::plan::Plan>>) {
        self.relation_plan = plan;
    }

    /// Whether one effective top-level field must be hidden from public output.
    #[must_use]
    pub fn field_is_sensitive(&self, field: &str) -> bool {
        self.sensitive || self.sensitive_fields.contains(field)
    }

    /// Whether the effective field's winning value came from environment
    /// interpolation, directly or through a template variable.
    #[must_use]
    #[doc(hidden)]
    pub fn field_is_environment_derived(&self, field: &str) -> bool {
        self.sensitive_fields.contains(field)
    }

    /// Whether dynamic evidence from this rule must use the sensitive-result policy.
    #[must_use]
    pub fn has_sensitive_evidence(&self) -> bool {
        self.sensitive
            || self
                .sensitive_fields
                .iter()
                .any(|field| field != "policy_url")
    }

    /// Whether any configured field can carry protected source material.
    #[must_use]
    #[doc(hidden)]
    pub fn has_sensitive_configuration(&self) -> bool {
        self.sensitive || !self.sensitive_fields.is_empty()
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
/// options, optionally with a `message` / `policy_url` / `when`. Code builds one
/// with [`NestedRuleSpec::new`] and the `with_*` setters.
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
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
    /// Nested-rule equivalent of [`RuleSpec::expect_matches`]. Checked per
    /// selected iteration after the nested `when:` gate is true.
    #[serde(default)]
    pub expect_matches: bool,
    /// Nested-rule equivalent of [`RuleSpec::sensitive`].
    #[serde(default)]
    pub sensitive: bool,
    /// Per-file scope filter — see [`RuleSpec::scope_filter`]
    /// for semantics. Inherited unchanged when
    /// [`NestedRuleSpec::instantiate`] synthesises a full
    /// `RuleSpec` per-iteration.
    #[serde(default)]
    pub scope_filter: Option<crate::ScopeFilterSpec>,
    #[serde(flatten)]
    pub extra: crate::yaml::Mapping,
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
///
/// Built only by [`CompiledNestedSpec::compile`]; its fields stay public.
#[derive(Debug)]
#[non_exhaustive]
pub struct CompiledNestedSpec {
    /// The original nested-rule spec — passed to
    /// [`NestedRuleSpec::instantiate`] per iteration to get a
    /// per-iteration full `RuleSpec` with template tokens
    /// substituted.
    pub spec: NestedRuleSpec,
    /// Pre-compiled `when:` expression. `None` when the nested
    /// spec carried no `when:` clause.
    pub when: Option<crate::when::WhenExpr>,
    /// Whether the parent's `require:` may hold a secret: a value in it came
    /// from the environment, or the parent is `sensitive: true`. Which nested
    /// field such a value reached is not recorded, so
    /// [`Self::instantiate`] treats every field of the nested rule as one.
    pub sensitive_configuration: bool,
    /// Whether the enclosing iteration already rendered this spec's strings:
    /// it is nested two or more levels deep, and the outermost iteration
    /// renders every level below it with its own values ([`RuleSpec::rendered`]
    /// on the parent). [`Self::instantiate`] then leaves them as they are, so
    /// a value is never rendered twice: a `{stem}` that came from a file name
    /// is text, not a token.
    pub rendered: bool,
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
        Ok(Self {
            spec,
            when,
            sensitive_configuration: false,
            rendered: false,
        })
    }

    /// [`NestedRuleSpec::instantiate`], with every configured field of the
    /// result sensitive ([`RuleSpec::field_is_sensitive`]) when
    /// [`Self::sensitive_configuration`] is set, as an environment-derived
    /// field of a top-level rule is: its errors and warnings leave the value
    /// out.
    // A thin wrapper on the per-entry path of `for_each_dir` and
    // `every_matching_has`, in another crate: with only `#[inline]` it stays a
    // call, and those benches measure 0.4% more instructions.
    #[allow(clippy::inline_always)]
    #[inline(always)]
    pub fn instantiate(
        &self,
        parent_id: &str,
        idx: usize,
        level: Level,
        tokens: &crate::template::PathTokens,
    ) -> RuleSpec {
        // Built in place: a `RuleSpec` moved out of a second call costs a
        // copy per iterated entry, which `for_each_dir` measures.
        let sensitive_fields = if self.sensitive_configuration {
            ["paths", "message", "policy_url", "when", "scope_filter"]
                .into_iter()
                .map(str::to_string)
                .chain(
                    self.spec
                        .extra
                        .keys()
                        .filter_map(|key| key.as_str())
                        .filter(|key| !NestedRuleSpec::PARENT_FIELDS.contains(key))
                        .map(str::to_string),
                )
                .collect()
        } else {
            std::collections::BTreeSet::new()
        };
        self.spec.instantiate_with(
            parent_id,
            idx,
            level,
            (!self.rendered).then_some(tokens),
            sensitive_fields,
        )
    }
}

impl NestedRuleSpec {
    /// Fields a nested config may write that belong to the parent (`id` and
    /// `level` are synthesized from it, `fix` and the git toggles are
    /// top-level only): they stay out of the leaf's option set.
    const PARENT_FIELDS: &[&str] = &[
        "id",
        "level",
        "fix",
        "git_tracked_only",
        "respect_gitignore",
    ];

    /// Synthesize a full [`RuleSpec`] for a single iteration, applying
    /// path-template substitution (using the iterated entry's tokens) to
    /// every string field. The resulting spec has `id =
    /// "{parent_id}.require[{idx}]"` and inherits `level` from the parent.
    /// Every path token any rendered field of this spec refers to (`paths`,
    /// `message` and every string in its options), so callers instantiating
    /// it per entry derive only those. Over-inclusive by construction.
    #[must_use]
    pub fn path_tokens(&self) -> crate::template::TokenSet {
        let mut acc = crate::template::TokenSet::NONE;
        self.for_each_template(&mut |s| acc = acc.union(crate::template::TokenSet::of(s)));
        acc
    }

    /// Every string of this spec that an iteration renders (`paths`, `message`,
    /// each option string and mapping key), for alint's own crates: preparing a
    /// nested rule reads them ([`crate::template::Preparation`]).
    #[doc(hidden)]
    #[must_use]
    pub fn templates(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.for_each_template(&mut |s| out.push(s));
        out
    }

    fn for_each_template<'a>(&'a self, visit: &mut dyn FnMut(&'a str)) {
        fn walk<'a>(v: &'a ayaml::Value, visit: &mut dyn FnMut(&'a str)) {
            match v {
                ayaml::Value::String(s) => visit(s),
                ayaml::Value::Sequence(items) => items.iter().for_each(|x| walk(x, visit)),
                ayaml::Value::Mapping(m) => m.iter().for_each(|(k, x)| {
                    walk(k, visit);
                    walk(x, visit);
                }),
                ayaml::Value::Tagged(tagged) => walk(&tagged.value, visit),
                _ => {}
            }
        }
        match &self.paths {
            Some(PathsSpec::Single(s)) => visit(s),
            Some(PathsSpec::Many(v)) => v.iter().for_each(|s| visit(s)),
            Some(PathsSpec::IncludeExclude { include, exclude }) => {
                include.iter().chain(exclude).for_each(|s| visit(s));
            }
            None => {}
        }
        if let Some(message) = &self.message {
            visit(message);
        }
        for (k, v) in self.extra.parser() {
            walk(k, visit);
            walk(v, visit);
        }
    }

    // A thin wrapper, as [`CompiledNestedSpec::instantiate`] is.
    #[allow(clippy::inline_always)]
    #[inline(always)]
    pub fn instantiate(
        &self,
        parent_id: &str,
        idx: usize,
        level: Level,
        tokens: &crate::template::PathTokens,
    ) -> RuleSpec {
        self.instantiate_with(
            parent_id,
            idx,
            level,
            Some(tokens),
            std::collections::BTreeSet::new(),
        )
    }

    /// [`Self::instantiate`] with `sensitive_fields` set on the result. With
    /// no `tokens` the strings are taken as they are: an enclosing iteration
    /// rendered them ([`CompiledNestedSpec::rendered`]).
    fn instantiate_with(
        &self,
        parent_id: &str,
        idx: usize,
        level: Level,
        tokens: Option<&crate::template::PathTokens>,
        sensitive_fields: std::collections::BTreeSet<String>,
    ) -> RuleSpec {
        RuleSpec {
            id: format!("{parent_id}.require[{idx}]"),
            kind: self.kind.clone(),
            level,
            paths: self.paths.as_ref().map(|p| match tokens {
                Some(tokens) => crate::template::render_paths_spec(p, tokens),
                None => p.clone(),
            }),
            message: self.message.as_deref().map(|m| match tokens {
                Some(tokens) => crate::template::render_nested_message(&self.kind, m, tokens),
                None => m.to_owned(),
            }),
            policy_url: self.policy_url.clone(),
            when: self.when.clone(),
            expect_matches: self.expect_matches,
            sensitive: self.sensitive,
            sensitive_fields,
            // A nested rule has no fix of its own.
            extended_source: false,
            // This iteration renders the `require:` lists inside the spec too.
            rendered: true,
            // A nested rule reads no relation (the loader refuses one that
            // would).
            relation_plan: None,
            fix: None,
            // `git_tracked_only` and `respect_gitignore` are both kind-specific
            // options now (ADR-0008), stripped from the nested `extra` via
            // `PARENT_FIELDS`, so a nested existence rule doesn't silently
            // inherit either. If/when nested rules need one, drop it from
            // `PARENT_FIELDS` and let it flow into the leaf's option set.
            scope_filter: self.scope_filter.clone(),
            // `NestedRuleSpec` doesn't name `id`/`level` (synthesized from the
            // parent) or the top-level-only `fix`/git toggles, so a nested
            // config that supplies them leaves them in `extra`. Strip them
            // before they reach the leaf rule's option set — they are not
            // options, and would otherwise trip the leaf's unknown-option
            // validation (a deny-unknown-fields `Options` struct, or an
            // option-less rule's `deny_unknown_options`).
            extra: crate::template::render_nested_extra(
                &self.kind,
                &self.extra,
                tokens,
                Self::PARENT_FIELDS,
            ),
        }
    }
}

#[path = "config_redaction.rs"]
mod redaction;
#[doc(hidden)]
pub use redaction::{
    CONFIGURATION_MASK, NumberMask, REDACTED_CONFIGURATION_ERROR,
    configuration_field_may_be_secret, mask_configuration_field, mask_configuration_values,
    redact_environment_values, redact_environment_values_in_error,
};

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "config_redaction_tests.rs"]
mod redaction_tests;
