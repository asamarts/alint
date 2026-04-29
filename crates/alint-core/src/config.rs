use std::collections::HashMap;
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
#[serde(untagged)]
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
#[serde(untagged)]
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

fn string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
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
    /// Restrict the rule to files / directories tracked in git's index.
    /// When `true`, the rule's `paths`-matched entries are intersected
    /// with the set of git-tracked files; entries that exist in the
    /// walked tree but aren't in `git ls-files` output are skipped.
    /// Only meaningful for rule kinds that opt in (currently the
    /// existence family — `file_exists`, `file_absent`, `dir_exists`,
    /// `dir_absent`); rule kinds that don't support it surface a clean
    /// config error when this is `true` so silent mis-configuration
    /// doesn't slip through.
    ///
    /// Default `false`. Has no effect outside a git repo.
    #[serde(default)]
    pub git_tracked_only: bool,
    /// The entire YAML mapping, retained so each rule builder can deserialize
    /// its kind-specific fields without every option being represented here.
    #[serde(flatten)]
    pub extra: serde_yaml_ng::Mapping,
}

/// The `fix:` block on a rule. Exactly one op key must be present —
/// alint errors at load time when the op and rule kind are incompatible.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
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
/// (U+200B / U+200C / U+200D / U+FEFF) from the file's content,
/// *except* a leading BOM (U+FEFF at position 0) — that's the
/// responsibility of the `no_bom` rule.
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
    #[serde(flatten)]
    pub extra: serde_yaml_ng::Mapping,
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
            // Nested rules don't currently expose
            // `git_tracked_only` from their parent's spec — the
            // option is meaningful on top-level rules only for
            // now. If/when `for_each_dir`'s nested rules need it,
            // plumb it through here.
            git_tracked_only: false,
            extra: crate::template::render_mapping(self.extra.clone(), tokens),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::PathTokens;
    use std::path::Path;

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
        let cfg: Config =
            serde_yaml_ng::from_str("version: 1\nfix_size_limit: null\n").unwrap();
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

        let many: PathsSpec =
            serde_yaml_ng::from_str("[\"src/**\", \"!src/vendor/**\"]").unwrap();
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
        let p = Some(PathBuf::from("LICENSE"));
        let resolved = resolve_content_source("r", "file_create", &None, &p).unwrap();
        assert!(matches!(resolved, ContentSourceSpec::File(p) if p == Path::new("LICENSE")));
    }

    #[test]
    fn resolve_content_source_rejects_both_set() {
        let err = resolve_content_source(
            "r",
            "file_prepend",
            &Some("x".into()),
            &Some(PathBuf::from("y")),
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
        // Nested rules don't propagate git_tracked_only — the
        // option is meaningful on top-level rules only.
        assert!(!spec.git_tracked_only);
    }
}
