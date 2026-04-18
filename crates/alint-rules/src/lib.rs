//! Built-in rule implementations for alint.
//!
//! Rules are registered into an [`alint_core::RuleRegistry`] via
//! [`register_builtin`]. Each kind has its own submodule.

use alint_core::RuleRegistry;

pub mod file_exists;

/// Register every built-in rule kind into the given registry.
pub fn register_builtin(registry: &mut RuleRegistry) {
    registry.register("file_exists", file_exists::build);
}

/// Convenience constructor that returns a fresh registry pre-populated with
/// every built-in rule.
pub fn builtin_registry() -> RuleRegistry {
    let mut r = RuleRegistry::new();
    register_builtin(&mut r);
    r
}
