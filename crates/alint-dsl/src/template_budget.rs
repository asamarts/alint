//! The budget `extends_template:` expansion runs under.
//!
//! Each instance clones its template's body and renders `{{vars.<name>}}` in
//! every string, so without a bound a config of a megabyte (a template with a
//! 1 MiB `message:` and 4,000 instances, or one message of 100,000
//! placeholders for a 20 KB var) expands to gigabytes. A config is untrusted
//! for its use of resources, so expansion is charged against a budget for the
//! whole config, and a single rendered string is held to the size of a whole
//! config.

use std::collections::HashMap;

use ayaml::{Mapping, Value};

/// What all the `extends_template:` instances of one config may expand to,
/// in the estimate of [`size_of_mapping`]. No bundled ruleset or example uses
/// templates; an instance of the documented example costs about 300 bytes, so
/// the budget holds some 220,000 of them.
pub(crate) const MAX_TEMPLATE_EXPANSION_BYTES: usize = 64 * 1024 * 1024;

/// The longest string one instance may render: no longer than a whole config
/// may be ([`alint_core::MAX_STRUCTURED_BYTES`]).
pub(crate) const MAX_RENDERED_TEMPLATE_STRING_BYTES: usize = alint_core::MAX_STRUCTURED_BYTES;

/// What each node costs on top of its text, so a body of many small nodes is
/// charged too.
const NODE_BYTES: usize = 16;

/// The estimated size of `value` once materialized: its strings' bytes plus
/// [`NODE_BYTES`] per node.
fn size_of_value(value: &Value) -> usize {
    match value {
        Value::String(text) => NODE_BYTES + text.len(),
        Value::Sequence(items) => items.iter().fold(NODE_BYTES, |sum, item| {
            sum.saturating_add(size_of_value(item))
        }),
        Value::Mapping(mapping) => NODE_BYTES.saturating_add(size_of_mapping(mapping)),
        Value::Tagged(tagged) => NODE_BYTES.saturating_add(size_of_value(&tagged.value)),
        _ => NODE_BYTES,
    }
}

/// [`size_of_mapping`] of a template's body without the loader's private
/// markers (`__alint_internal_*`), which say where its fields came from and
/// are no part of what an instance expands to: the same template costs the
/// same whichever source declares it, and whatever version that source is.
pub(crate) fn size_of_template(template: &Mapping) -> usize {
    template
        .iter()
        .filter(|(key, _)| {
            !key.as_str()
                .is_some_and(|key| key.starts_with(crate::trust::INTERNAL_FIELD_PREFIX))
        })
        .fold(0, |sum, (key, value)| {
            sum.saturating_add(size_of_value(key))
                .saturating_add(size_of_value(value))
        })
}

/// [`size_of_value`] of a mapping's entries.
pub(crate) fn size_of_mapping(mapping: &Mapping) -> usize {
    mapping.iter().fold(0, |sum, (key, value)| {
        sum.saturating_add(size_of_value(key))
            .saturating_add(size_of_value(value))
    })
}

/// Why an expansion was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Overrun {
    /// The config's instances together expand past
    /// [`MAX_TEMPLATE_EXPANSION_BYTES`].
    Total,
    /// One string renders to this many bytes, past
    /// [`MAX_RENDERED_TEMPLATE_STRING_BYTES`].
    String(usize),
}

impl Overrun {
    /// The refusal, for an error about the rule the template expands for;
    /// `template` names it (as ``template `id` ``, or as `a template` when its
    /// id is read from the environment).
    pub(crate) fn message(self, template: &str) -> String {
        match self {
            Self::Total => format!(
                "expanding {template} exceeds the template expansion budget: all the \
                 `extends_template:` instances of a config may expand to at most \
                 {MAX_TEMPLATE_EXPANSION_BYTES} bytes"
            ),
            Self::String(len) => format!(
                "{template} renders a string of {len} bytes, more than the \
                 {MAX_RENDERED_TEMPLATE_STRING_BYTES} one rendered value may hold"
            ),
        }
    }
}

/// What expansion may still produce, in bytes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Budget(usize);

impl Default for Budget {
    fn default() -> Self {
        Self(MAX_TEMPLATE_EXPANSION_BYTES)
    }
}

impl Budget {
    /// Spend `bytes`, or refuse once the budget is spent.
    pub(crate) fn charge(&mut self, bytes: usize) -> Result<(), Overrun> {
        self.0 = self.0.checked_sub(bytes).ok_or(Overrun::Total)?;
        Ok(())
    }

    /// Render the `{{vars.<name>}}` placeholders of `text` from `vars`, after
    /// charging what the rendering adds to the text, and refusing a result
    /// over [`MAX_RENDERED_TEMPLATE_STRING_BYTES`] before it is built.
    pub(crate) fn render(
        &mut self,
        text: &str,
        vars: &HashMap<String, String>,
    ) -> Result<String, Overrun> {
        // The length first, from a rendering with every var empty plus the
        // vars' lengths, so a huge result is refused without building it.
        let added = std::cell::Cell::new(0usize);
        let skeleton = alint_core::template::render_message(text, |namespace, name| {
            let value = vars.get(name).filter(|_| namespace == "vars")?;
            added.set(added.get().saturating_add(value.len()));
            Some(String::new())
        });
        let len = skeleton.len().saturating_add(added.get());
        if len > MAX_RENDERED_TEMPLATE_STRING_BYTES {
            return Err(Overrun::String(len));
        }
        self.charge(len.saturating_sub(text.len()))?;
        Ok(crate::render_template_vars(text, vars))
    }
}
