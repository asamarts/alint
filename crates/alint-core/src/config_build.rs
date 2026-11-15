//! The constructors and setters of [`Config`] (ADR-0030 decision 2), and of
//! [`RuleSpec`] and [`NestedRuleSpec`] (ADR-0032 decision 3), for code that
//! builds a configuration instead of loading one.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use crate::config::{
    AllowOutOfRoot, Config, ExtendsEntry, FixSpec, NestedRuleSpec, PathsSpec, RuleSpec,
    default_fix_size_limit, default_respect_gitignore,
};
use crate::facts::FactSpec;
use crate::level::Level;
use crate::scope_filter::ScopeFilterSpec;

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

impl RuleSpec {
    /// A rule `id` of `kind` at `level`, with every other field as a rule that
    /// sets only those three would load: no paths, message, policy URL, `when:`,
    /// fix or scope filter, not sensitive, and no options.
    #[must_use]
    pub fn new(id: impl Into<String>, kind: impl Into<String>, level: Level) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            level,
            paths: None,
            message: None,
            policy_url: None,
            when: None,
            expect_matches: false,
            sensitive: false,
            sensitive_fields: BTreeSet::new(),
            fix: None,
            scope_filter: None,
            extra: crate::yaml::Mapping::new(),
        }
    }

    /// Sets [`Self::paths`].
    #[must_use]
    pub fn with_paths(mut self, paths: Option<PathsSpec>) -> Self {
        self.paths = paths;
        self
    }

    /// Sets [`Self::message`].
    #[must_use]
    pub fn with_message(mut self, message: Option<String>) -> Self {
        self.message = message;
        self
    }

    /// Sets [`Self::policy_url`].
    #[must_use]
    pub fn with_policy_url(mut self, policy_url: Option<String>) -> Self {
        self.policy_url = policy_url;
        self
    }

    /// Sets [`Self::when`].
    #[must_use]
    pub fn with_when(mut self, when: Option<String>) -> Self {
        self.when = when;
        self
    }

    /// Sets [`Self::expect_matches`].
    #[must_use]
    pub fn with_expect_matches(mut self, expect_matches: bool) -> Self {
        self.expect_matches = expect_matches;
        self
    }

    /// Sets [`Self::sensitive`].
    #[must_use]
    pub fn with_sensitive(mut self, sensitive: bool) -> Self {
        self.sensitive = sensitive;
        self
    }

    /// Sets [`Self::fix`].
    #[must_use]
    pub fn with_fix(mut self, fix: Option<FixSpec>) -> Self {
        self.fix = fix;
        self
    }

    /// Sets [`Self::scope_filter`].
    #[must_use]
    pub fn with_scope_filter(mut self, scope_filter: Option<ScopeFilterSpec>) -> Self {
        self.scope_filter = scope_filter;
        self
    }

    /// Sets [`Self::extra`], the kind's options.
    #[must_use]
    pub fn with_extra(mut self, extra: crate::yaml::Mapping) -> Self {
        self.extra = extra;
        self
    }
}

impl NestedRuleSpec {
    /// A nested rule of `kind`, with every other field as a nested rule that
    /// sets only `kind` would load.
    #[must_use]
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            paths: None,
            message: None,
            policy_url: None,
            when: None,
            expect_matches: false,
            sensitive: false,
            scope_filter: None,
            extra: crate::yaml::Mapping::new(),
        }
    }

    /// Sets [`Self::paths`].
    #[must_use]
    pub fn with_paths(mut self, paths: Option<PathsSpec>) -> Self {
        self.paths = paths;
        self
    }

    /// Sets [`Self::message`].
    #[must_use]
    pub fn with_message(mut self, message: Option<String>) -> Self {
        self.message = message;
        self
    }

    /// Sets [`Self::policy_url`].
    #[must_use]
    pub fn with_policy_url(mut self, policy_url: Option<String>) -> Self {
        self.policy_url = policy_url;
        self
    }

    /// Sets [`Self::when`].
    #[must_use]
    pub fn with_when(mut self, when: Option<String>) -> Self {
        self.when = when;
        self
    }

    /// Sets [`Self::expect_matches`].
    #[must_use]
    pub fn with_expect_matches(mut self, expect_matches: bool) -> Self {
        self.expect_matches = expect_matches;
        self
    }

    /// Sets [`Self::sensitive`].
    #[must_use]
    pub fn with_sensitive(mut self, sensitive: bool) -> Self {
        self.sensitive = sensitive;
        self
    }

    /// Sets [`Self::scope_filter`].
    #[must_use]
    pub fn with_scope_filter(mut self, scope_filter: Option<ScopeFilterSpec>) -> Self {
        self.scope_filter = scope_filter;
        self
    }

    /// Sets [`Self::extra`], the kind's options.
    #[must_use]
    pub fn with_extra(mut self, extra: crate::yaml::Mapping) -> Self {
        self.extra = extra;
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

    fn options() -> crate::yaml::Mapping {
        crate::yaml::from_str("pattern: x\n").unwrap()
    }

    fn scope_filter() -> ScopeFilterSpec {
        crate::yaml::from_str("has_ancestor: Cargo.toml\n").unwrap()
    }

    #[test]
    fn every_rule_spec_setter_sets_its_field() {
        let bare = RuleSpec::new("r", "file_exists", Level::Warning);
        assert_eq!((bare.id.as_str(), bare.kind.as_str()), ("r", "file_exists"));
        assert_eq!(bare.level, Level::Warning);
        assert!(bare.paths.is_none() && bare.message.is_none() && bare.policy_url.is_none());
        assert!(bare.when.is_none() && bare.fix.is_none() && bare.scope_filter.is_none());
        assert!(!bare.expect_matches && !bare.sensitive);
        assert!(bare.sensitive_fields.is_empty() && bare.extra.is_empty());

        let spec = bare
            .with_paths(Some(PathsSpec::Single("**/*.rs".into())))
            .with_message(Some("m".into()))
            .with_policy_url(Some("https://example.invalid/p".into()))
            .with_when(Some("facts.x".into()))
            .with_expect_matches(true)
            .with_sensitive(true)
            .with_fix(Some(
                crate::yaml::from_str("file_create: {content: x}\n").unwrap(),
            ))
            .with_scope_filter(Some(scope_filter()))
            .with_extra(options());
        assert!(matches!(&spec.paths, Some(PathsSpec::Single(p)) if p == "**/*.rs"));
        assert_eq!(spec.message.as_deref(), Some("m"));
        assert_eq!(
            spec.policy_url.as_deref(),
            Some("https://example.invalid/p")
        );
        assert_eq!(spec.when.as_deref(), Some("facts.x"));
        assert!(spec.expect_matches && spec.sensitive);
        assert_eq!(spec.fix.as_ref().map(FixSpec::op_name), Some("file_create"));
        assert!(spec.scope_filter.is_some());
        assert_eq!(spec.extra, options());

        // Each flag setter sets its own flag only.
        let expect = RuleSpec::new("r", "file_exists", Level::Warning).with_expect_matches(true);
        assert!(expect.expect_matches && !expect.sensitive);
        let sensitive = RuleSpec::new("r", "file_exists", Level::Warning).with_sensitive(true);
        assert!(!sensitive.expect_matches && sensitive.sensitive);
    }

    #[test]
    fn every_nested_rule_spec_setter_sets_its_field() {
        let bare = NestedRuleSpec::new("file_exists");
        assert_eq!(bare.kind, "file_exists");
        assert!(bare.paths.is_none() && bare.message.is_none() && bare.policy_url.is_none());
        assert!(bare.when.is_none() && bare.scope_filter.is_none() && bare.extra.is_empty());
        assert!(!bare.expect_matches && !bare.sensitive);

        let spec = bare
            .with_paths(Some(PathsSpec::Single("{path}/README.md".into())))
            .with_message(Some("m".into()))
            .with_policy_url(Some("https://example.invalid/p".into()))
            .with_when(Some("iter.has_file(\"x\")".into()))
            .with_expect_matches(true)
            .with_sensitive(true)
            .with_scope_filter(Some(scope_filter()))
            .with_extra(options());
        assert!(matches!(&spec.paths, Some(PathsSpec::Single(p)) if p == "{path}/README.md"));
        assert_eq!(spec.message.as_deref(), Some("m"));
        assert_eq!(
            spec.policy_url.as_deref(),
            Some("https://example.invalid/p")
        );
        assert_eq!(spec.when.as_deref(), Some("iter.has_file(\"x\")"));
        assert!(spec.expect_matches && spec.sensitive);
        assert!(spec.scope_filter.is_some());
        assert_eq!(spec.extra, options());

        // Each flag setter sets its own flag only.
        let expect = NestedRuleSpec::new("file_exists").with_expect_matches(true);
        assert!(expect.expect_matches && !expect.sensitive);
        let sensitive = NestedRuleSpec::new("file_exists").with_sensitive(true);
        assert!(!sensitive.expect_matches && sensitive.sensitive);
    }
}
