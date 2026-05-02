use std::collections::HashMap;

use crate::config::RuleSpec;
use crate::error::{Error, Result};
use crate::rule::Rule;

pub type RuleBuilder = fn(&RuleSpec) -> Result<Box<dyn Rule>>;

/// Map from `kind` string → factory function. Built-in rule crates register
/// themselves here at startup, and plugin rules (in later phases) will too.
#[derive(Default)]
pub struct RuleRegistry {
    builders: HashMap<String, RuleBuilder>,
}

impl std::fmt::Debug for RuleRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuleRegistry")
            .field("kinds", &self.builders.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl RuleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, kind: &str, builder: RuleBuilder) {
        self.builders.insert(kind.to_string(), builder);
    }

    pub fn build(&self, spec: &RuleSpec) -> Result<Box<dyn Rule>> {
        let builder = self
            .builders
            .get(&spec.kind)
            .ok_or_else(|| Error::UnknownRuleKind(spec.kind.clone()))?;
        builder(spec)
    }

    pub fn known_kinds(&self) -> impl Iterator<Item = &str> {
        self.builders.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::Level;

    fn fake_spec(kind: &str) -> RuleSpec {
        RuleSpec {
            id: "t".into(),
            kind: kind.into(),
            level: Level::Error,
            paths: None,
            message: None,
            policy_url: None,
            when: None,
            fix: None,
            git_tracked_only: false,
            scope_filter: None,
            extra: serde_yaml_ng::Mapping::new(),
        }
    }

    fn ok_builder(_spec: &RuleSpec) -> Result<Box<dyn Rule>> {
        // Trait object can't be a unit struct without an `impl
        // Rule for ()` somewhere; the build path doesn't actually
        // call this in the unknown-kind tests so we leave it
        // unreachable on the happy path.
        unreachable!("test should not call this builder")
    }

    #[test]
    fn new_registry_has_no_kinds() {
        let r = RuleRegistry::new();
        assert_eq!(r.known_kinds().count(), 0);
    }

    #[test]
    fn register_inserts_a_kind() {
        let mut r = RuleRegistry::new();
        r.register("my_kind", ok_builder);
        let kinds: Vec<&str> = r.known_kinds().collect();
        assert_eq!(kinds, vec!["my_kind"]);
    }

    #[test]
    fn register_overwrites_existing_kind() {
        // Last-registered-wins. Plugin loaders may rely on this
        // to override a built-in's behaviour.
        let mut r = RuleRegistry::new();
        r.register("my_kind", ok_builder);
        r.register("my_kind", ok_builder);
        assert_eq!(r.known_kinds().count(), 1);
    }

    #[test]
    fn build_rejects_unknown_kind_with_clear_error() {
        let r = RuleRegistry::new();
        let err = r.build(&fake_spec("not_real")).unwrap_err();
        match err {
            Error::UnknownRuleKind(name) => assert_eq!(name, "not_real"),
            other => panic!("expected UnknownRuleKind, got {other:?}"),
        }
    }

    #[test]
    fn known_kinds_iterator_lists_all_registered() {
        let mut r = RuleRegistry::new();
        r.register("a", ok_builder);
        r.register("b", ok_builder);
        r.register("c", ok_builder);
        let mut kinds: Vec<&str> = r.known_kinds().collect();
        kinds.sort_unstable();
        assert_eq!(kinds, vec!["a", "b", "c"]);
    }
}
