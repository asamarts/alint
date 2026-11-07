//! [`Config`]'s constructor and setters, for code that builds a configuration
//! instead of loading one (ADR-0030 decision 2).

use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::{
    AllowOutOfRoot, Config, ExtendsEntry, RuleSpec, default_fix_size_limit,
    default_respect_gitignore,
};
use crate::facts::FactSpec;

impl Config {
    /// A configuration of `version` with `rules`, and every other field as a
    /// file that sets only those two would load: nothing extended, ignored or
    /// declared, `.gitignore` respected, the 1 MiB fix size cap, no nested
    /// configs, root confinement, and no baseline.
    #[must_use]
    pub fn new(version: u32, rules: Vec<RuleSpec>) -> Self {
        Self {
            version,
            rules,
            respect_gitignore: default_respect_gitignore(),
            fix_size_limit: default_fix_size_limit(),
            ..Self::default()
        }
    }

    /// Sets [`Self::requires`].
    #[must_use]
    pub fn with_requires(mut self, requires: Option<crate::ConfigRequirements>) -> Self {
        self.requires = requires;
        self
    }

    /// Sets [`Self::extends`].
    #[must_use]
    pub fn with_extends(mut self, extends: Vec<ExtendsEntry>) -> Self {
        self.extends = extends;
        self
    }

    /// Sets [`Self::ignore`].
    #[must_use]
    pub fn with_ignore(mut self, ignore: Vec<String>) -> Self {
        self.ignore = ignore;
        self
    }

    /// Sets [`Self::respect_gitignore`].
    #[must_use]
    pub fn with_respect_gitignore(mut self, respect_gitignore: bool) -> Self {
        self.respect_gitignore = respect_gitignore;
        self
    }

    /// Sets [`Self::vars`].
    #[must_use]
    pub fn with_vars(mut self, vars: HashMap<String, String>) -> Self {
        self.vars = vars;
        self
    }

    /// Sets [`Self::facts`].
    #[must_use]
    pub fn with_facts(mut self, facts: Vec<FactSpec>) -> Self {
        self.facts = facts;
        self
    }

    /// Sets [`Self::fix_size_limit`].
    #[must_use]
    pub fn with_fix_size_limit(mut self, fix_size_limit: Option<u64>) -> Self {
        self.fix_size_limit = fix_size_limit;
        self
    }

    /// Sets [`Self::nested_configs`].
    #[must_use]
    pub fn with_nested_configs(mut self, nested_configs: bool) -> Self {
        self.nested_configs = nested_configs;
        self
    }

    /// Sets [`Self::allow_out_of_root`].
    #[must_use]
    pub fn with_allow_out_of_root(mut self, allow_out_of_root: AllowOutOfRoot) -> Self {
        self.allow_out_of_root = allow_out_of_root;
        self
    }

    /// Sets [`Self::baseline`].
    #[must_use]
    pub fn with_baseline(mut self, baseline: Option<PathBuf>) -> Self {
        self.baseline = baseline;
        self
    }

    /// Sets [`Self::compatibility`].
    #[must_use]
    pub fn with_compatibility(mut self, compatibility: Vec<crate::ConfigCompatibility>) -> Self {
        self.compatibility = compatibility;
        self
    }

    /// Sets [`Self::environment_values`].
    #[doc(hidden)]
    #[must_use]
    pub fn with_environment_values(mut self, environment_values: Vec<String>) -> Self {
        self.environment_values = environment_values;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compatibility::{
        ConfigCompatibility, ConfigRequirements, ConfigSourceClass, StableVersion,
    };
    use crate::facts::FactKind;

    #[test]
    fn every_setter_sets_its_field() {
        let requires = ConfigRequirements {
            alint: StableVersion::new(0, 22, 0),
        };
        let compatibility = ConfigCompatibility {
            source: "base.yml".into(),
            source_class: ConfigSourceClass::LocalExtend,
            config_version: 1,
            requires_alint: Some(StableVersion::new(0, 21, 0)),
            consumer_version: None,
        };
        let config = Config::new(1, Vec::new())
            .with_requires(Some(requires))
            .with_extends(vec![ExtendsEntry::Url("base.yml".into())])
            .with_ignore(vec!["target/**".into()])
            .with_respect_gitignore(false)
            .with_vars(HashMap::from([("org".into(), "acme".into())]))
            .with_facts(vec![FactSpec {
                id: "rust_files".into(),
                kind: FactKind::CountFiles {
                    count_files: "**/*.rs".into(),
                },
            }])
            .with_fix_size_limit(Some(4096))
            .with_nested_configs(true)
            .with_allow_out_of_root(AllowOutOfRoot::All)
            .with_baseline(Some(PathBuf::from(".alint-baseline.json")))
            .with_compatibility(vec![compatibility.clone()])
            .with_environment_values(vec!["secret".into()]);

        assert_eq!(config.requires, Some(requires));
        assert!(matches!(&config.extends[..], [ExtendsEntry::Url(u)] if u == "base.yml"));
        assert_eq!(config.ignore, ["target/**"]);
        assert!(!config.respect_gitignore);
        assert_eq!(config.vars["org"], "acme");
        assert!(matches!(
            &config.facts[..],
            [FactSpec { id, kind: FactKind::CountFiles { count_files } }]
                if id == "rust_files" && count_files == "**/*.rs"
        ));
        assert_eq!(config.fix_size_limit, Some(4096));
        assert!(config.nested_configs);
        assert!(matches!(config.allow_out_of_root, AllowOutOfRoot::All));
        assert_eq!(
            config.baseline.as_deref(),
            Some(std::path::Path::new(".alint-baseline.json"))
        );
        assert_eq!(config.compatibility, [compatibility]);
        assert_eq!(config.environment_values, ["secret"]);
    }
}
