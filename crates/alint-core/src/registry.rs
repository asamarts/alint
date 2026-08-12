use std::collections::HashMap;

use crate::config::RuleSpec;
use crate::did_you_mean;
use crate::error::{Error, Result};
use crate::rule::Rule;

pub type RuleBuilder = fn(&RuleSpec) -> Result<Box<dyn Rule>>;

/// Internal storage form: a boxed builder, so `register_optionless` can wrap a
/// plain [`RuleBuilder`] with option-validation. `Send + Sync` because the
/// registry is shared across the engine's worker threads.
type BoxedBuilder = Box<dyn Fn(&RuleSpec) -> Result<Box<dyn Rule>> + Send + Sync>;

/// Map from `kind` string → factory function. Built-in rule crates register
/// themselves here at startup, and plugin rules (in later phases) will too.
///
/// A kind may be registered under more than one spelling: `register_alias`
/// records that the alias resolves to a canonical kind, so `canonical_kind`
/// can collapse the two. The alias still builds the same rule.
#[derive(Default)]
pub struct RuleRegistry {
    builders: HashMap<String, BoxedBuilder>,
    /// alias kind → canonical kind. Only populated by `register_alias*`; a
    /// canonical (or unregistered) name is absent and resolves to itself.
    aliases: HashMap<String, String>,
}

impl std::fmt::Debug for RuleRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuleRegistry")
            .field("kinds", &self.builders.keys().collect::<Vec<_>>())
            .field("aliases", &self.aliases)
            .finish()
    }
}

impl RuleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, kind: &str, builder: RuleBuilder) {
        self.builders.insert(kind.to_string(), Box::new(builder));
    }

    /// Register an OPTION-LESS rule kind — one that takes no kind-specific
    /// options. Wraps the builder so any leftover field on the spec is rejected
    /// ([`RuleSpec::deny_unknown_options`]): a typo'd option must fail loudly,
    /// not silently no-op, the same way every option-bearing kind rejects
    /// unknown fields via its `deserialize_options::<Options>()`.
    pub fn register_optionless(&mut self, kind: &str, builder: RuleBuilder) {
        self.builders.insert(
            kind.to_string(),
            Box::new(move |spec: &RuleSpec| {
                spec.deny_unknown_options()?;
                builder(spec)
            }),
        );
    }

    /// Register `alias` as another spelling of `canonical`: it builds the same
    /// rule (same `builder`, as [`register`](Self::register) would) AND records
    /// the alias → canonical mapping that [`canonical_kind`](Self::canonical_kind)
    /// and [`canonical_kinds`](Self::canonical_kinds) expose. `canonical` should
    /// itself be registered (as a non-alias) so an alias always resolves to a
    /// real page/kind; the built-in registry's consistency test enforces this.
    pub fn register_alias(&mut self, alias: &str, canonical: &str, builder: RuleBuilder) {
        self.register(alias, builder);
        self.aliases
            .insert(alias.to_string(), canonical.to_string());
    }

    /// [`register_alias`](Self::register_alias) for an OPTION-LESS canonical
    /// kind: the alias gets the same unknown-option rejection as
    /// [`register_optionless`](Self::register_optionless).
    pub fn register_alias_optionless(
        &mut self,
        alias: &str,
        canonical: &str,
        builder: RuleBuilder,
    ) {
        self.register_optionless(alias, builder);
        self.aliases
            .insert(alias.to_string(), canonical.to_string());
    }

    /// Resolve `name` to its canonical kind: the target of an alias registered
    /// via [`register_alias`](Self::register_alias), or `name` itself when it is
    /// already canonical (or not registered at all). Alias-aware equality —
    /// `canonical_kind(a) == canonical_kind(b)` — is how a caller tells whether
    /// two spellings name the same rule.
    pub fn canonical_kind<'a>(&'a self, name: &'a str) -> &'a str {
        self.aliases.get(name).map_or(name, String::as_str)
    }

    /// Every registered kind that is NOT an alias, in arbitrary order — the
    /// canonical set the coverage audits enumerate (each alias collapses into
    /// the canonical it points at).
    pub fn canonical_kinds(&self) -> impl Iterator<Item = &str> {
        self.builders
            .keys()
            .map(String::as_str)
            .filter(|k| !self.aliases.contains_key(*k))
    }

    pub fn build(&self, spec: &RuleSpec) -> Result<Box<dyn Rule>> {
        let builder = self
            .builders
            .get(&spec.kind)
            .ok_or_else(|| Error::UnknownRuleKind(spec.kind.clone()))?;
        builder(spec).map_err(|e| enrich_error(e, &spec.kind))
    }

    pub fn known_kinds(&self) -> impl Iterator<Item = &str> {
        self.builders.keys().map(String::as_str)
    }
}

/// Apply [`did_you_mean::enrich`] to the message of a `RuleConfig`
/// error. Other error variants pass through unchanged.
fn enrich_error(err: Error, kind: &str) -> Error {
    match err {
        Error::RuleConfig { rule_id, message } => {
            let enriched = did_you_mean::enrich(kind, &message);
            Error::RuleConfig {
                rule_id,
                message: enriched,
            }
        }
        other => other,
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
    fn register_alias_records_canonical_and_still_builds() {
        let mut r = RuleRegistry::new();
        r.register("file_content_matches", ok_builder);
        r.register_alias("content_matches", "file_content_matches", ok_builder);
        // The alias is a real, buildable kind (both spellings are known).
        assert_eq!(r.known_kinds().count(), 2);
        // canonical_kind collapses the alias; a canonical / unknown name is itself.
        assert_eq!(r.canonical_kind("content_matches"), "file_content_matches");
        assert_eq!(
            r.canonical_kind("file_content_matches"),
            "file_content_matches"
        );
        assert_eq!(r.canonical_kind("not_registered"), "not_registered");
        // canonical_kinds omits the alias.
        let mut canon: Vec<&str> = r.canonical_kinds().collect();
        canon.sort_unstable();
        assert_eq!(canon, vec!["file_content_matches"]);
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
