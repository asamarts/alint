use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

use crate::facts::FactSpec;
use crate::level::Level;

/// Parsed form of a `.alint.yml` file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
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

/// The `fix:` block on a rule. The op name is the discriminator key —
/// exactly one of `file_create` or `file_remove` must be present.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum FixSpec {
    FileCreate { file_create: FileCreateFixSpec },
    FileRemove { file_remove: FileRemoveFixSpec },
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
