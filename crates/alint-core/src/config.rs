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
    /// Each entry is a local path (relative to the containing file
    /// or absolute). Remote `https://` URLs are reserved but not yet
    /// supported; the loader rejects them with a clear error.
    #[serde(default)]
    pub extends: Vec<String>,
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
    /// Content to write. Required — there is no implicit empty default;
    /// for an empty file, pass `content: ""` explicitly.
    pub content: String,
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
    /// Bytes to insert at the beginning of each violating file. A
    /// trailing newline in `content` is the caller's responsibility.
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileAppendFixSpec {
    /// Bytes to append to each violating file. A leading newline in
    /// `content` is the caller's responsibility.
    pub content: String,
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
            extra: crate::template::render_mapping(self.extra.clone(), tokens),
        }
    }
}
