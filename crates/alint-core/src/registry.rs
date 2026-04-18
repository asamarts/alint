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
