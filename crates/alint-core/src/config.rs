use serde::Deserialize;

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
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PathsSpec {
    Single(String),
    Many(Vec<String>),
    IncludeExclude {
        #[serde(default)]
        include: Vec<String>,
        #[serde(default)]
        exclude: Vec<String>,
    },
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
    /// The entire YAML mapping, retained so each rule builder can deserialize
    /// its kind-specific fields without every option being represented here.
    #[serde(flatten)]
    pub extra: serde_yaml_ng::Mapping,
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
        Ok(serde_yaml_ng::from_value(
            serde_yaml_ng::Value::Mapping(self.extra.clone()),
        )?)
    }
}
