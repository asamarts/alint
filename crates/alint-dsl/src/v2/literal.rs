//! Names stay literal in a version 2 source (design section 10, ADR-0037
//! decision 3): selector and relation names, `from:`, `input:`, `left:`,
//! `right:`, `as:`, field names, `key:`, `group_by:` and `on:`. `{{...}}` in
//! any of them is a load error, so what a name refers to cannot change with
//! the environment. The check reads the document before interpolation, which
//! would otherwise erase the difference; every other value interpolates as in
//! v1.

use alint_core::{Error, Result};
use ayaml::Value;

/// Keys of a `constraint` rule's `assert:` whose values are field or relation
/// names.
const ASSERT_NAME_KEYS: &[&str] = &[
    "key",
    "group_by",
    "field",
    "fields",
    "determines",
    "relation",
    "version",
    "range",
];

/// Refuse `{{` in any name, alias or reference of `document`, a version 2
/// source as written. `display` names the source in the error.
pub(crate) fn reject_interpolated_names(document: &Value, display: &str) -> Result<()> {
    let Some(root) = document.as_mapping() else {
        return Ok(());
    };
    let check = Check { display };
    for (key, value) in root {
        match key.as_str() {
            Some("selectors") => {
                for name in value
                    .as_mapping()
                    .into_iter()
                    .flat_map(ayaml::Mapping::keys)
                {
                    check.key("selectors", name)?;
                }
            }
            Some("relations") => {
                for (name, relation) in value.as_mapping().into_iter().flatten() {
                    let path = check.key("relations", name)?;
                    check.relation(relation, &path)?;
                }
            }
            Some("extends") => {
                for (index, entry) in sequence(value).iter().enumerate() {
                    if let Some(alias) = entry.get("as") {
                        check.value(alias, &format!("extends[{index}].as"))?;
                    }
                }
            }
            // Every rule and template, whatever its `kind:`: an override by
            // id and a template's instance write none, and a later override
            // can make a rule a `constraint`, so any of them can end up one.
            // No version 1 kind has an `input:` or `assert:` field, so this
            // refuses nothing a version 1 kind reads.
            Some(section @ ("rules" | "templates")) => {
                for (index, rule) in sequence(value).iter().enumerate() {
                    check.constraint(rule, &format!("{section}[{index}]"))?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn sequence(value: &Value) -> &[Value] {
    value.as_sequence().map_or(&[], Vec::as_slice)
}

fn interpolated(text: &str) -> bool {
    text.contains("{{")
}

struct Check<'d> {
    display: &'d str,
}

impl Check<'_> {
    fn error(&self, path: &str) -> Error {
        Error::Other(format!(
            "{}: `{path}`: `{{{{...}}}}` is not allowed in a name, alias or reference of a \
             version 2 config, which must name the same thing in every environment; write \
             it literally",
            self.display
        ))
    }

    /// A name written as a map key under `parent`; the key's path.
    fn key(&self, parent: &str, key: &Value) -> Result<String> {
        let text = super::decode::key_text(key);
        let path = format!("{parent}.{text}");
        if key.as_str().is_some_and(interpolated) {
            return Err(self.error(&path));
        }
        Ok(path)
    }

    /// A name written as a value, or a list of names.
    fn value(&self, value: &Value, path: &str) -> Result<()> {
        match value {
            Value::String(text) if interpolated(text) => Err(self.error(path)),
            Value::Sequence(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.value(item, &format!("{path}[{index}]"))?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn keys(&self, value: Option<&Value>, path: &str) -> Result<()> {
        for key in value
            .and_then(Value::as_mapping)
            .into_iter()
            .flat_map(ayaml::Mapping::keys)
        {
            self.key(path, key)?;
        }
        Ok(())
    }

    /// A relation, named or inline. An inline selector holds no names.
    fn relation(&self, relation: &Value, path: &str) -> Result<()> {
        let Some(map) = relation.as_mapping() else {
            return Ok(());
        };
        if let Some(from @ Value::String(_)) = map.get("from") {
            self.value(from, &format!("{path}.from"))?;
        }
        for (index, part) in map
            .get("union")
            .map_or(&[][..], sequence)
            .iter()
            .enumerate()
        {
            let part_path = format!("{path}.union[{index}]");
            if let Some(from @ Value::String(_)) = part.get("from") {
                self.value(from, &format!("{part_path}.from"))?;
            }
            if let Some(fields) = part.get("fields") {
                self.fields(fields, &format!("{part_path}.fields"))?;
            }
        }
        if let Some(join) = map.get("join") {
            for side in ["left", "right"] {
                if let Some(name) = join.get(side) {
                    self.value(name, &format!("{path}.join.{side}"))?;
                }
            }
            let on_path = format!("{path}.join.on");
            for (left, right) in join
                .get("on")
                .and_then(Value::as_mapping)
                .into_iter()
                .flatten()
            {
                let pair = self.key(&on_path, left)?;
                self.value(right, &pair)?;
            }
        }
        if let Some(fields) = map.get("fields") {
            self.fields(fields, &format!("{path}.fields"))?;
        }
        self.keys(map.get("transform"), &format!("{path}.transform"))?;
        let where_path = format!("{path}.where");
        for (field, predicates) in map
            .get("where")
            .and_then(Value::as_mapping)
            .into_iter()
            .flatten()
        {
            let field_path = self.key(&where_path, field)?;
            for list in ["in", "not_in"] {
                let list_path = format!("{field_path}.{list}");
                for (index, item) in predicates
                    .get(list)
                    .map_or(&[][..], sequence)
                    .iter()
                    .enumerate()
                {
                    for name in ["relation", "field"] {
                        if let Some(reference) = item.get(name) {
                            self.value(reference, &format!("{list_path}[{index}].{name}"))?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Check one `fields:` map (top-level or per-part in a `union:`): field
    /// names and non-query field sources stay literal; `bound:` and `record:`
    /// `JSONPath` queries are values and may interpolate.
    fn fields(&self, fields: &Value, fields_path: &str) -> Result<()> {
        for (name, field) in fields.as_mapping().into_iter().flatten() {
            let field_path = self.key(fields_path, name)?;
            let (source, source_path) = match field {
                Value::Mapping(field) => (field.get("source"), format!("{field_path}.source")),
                other => (Some(other), field_path),
            };
            if let Some(Value::String(text)) = source
                && interpolated(text)
                && !text.starts_with("bound:")
                && !text.starts_with("record:")
            {
                return Err(self.error(&source_path));
            }
        }
        Ok(())
    }

    /// A rule as a `constraint` reads it: its `input:`, and the names its
    /// `assert:` holds.
    fn constraint(&self, rule: &Value, path: &str) -> Result<()> {
        match rule.get("input") {
            Some(input @ Value::String(_)) => self.value(input, &format!("{path}.input"))?,
            Some(input @ Value::Mapping(_)) => self.inline(input, &format!("{path}.input"))?,
            _ => {}
        }
        if let Some(assert) = rule.get("assert") {
            self.assertion(assert, &format!("{path}.assert"))?;
        }
        Ok(())
    }

    /// An inline relation (a rule's `input:` map) is literal throughout: the
    /// loader decodes it as a relation of the source after interpolation, and
    /// none of it records that it read the environment, as a named relation's
    /// definition does. A relation that reads the environment is declared
    /// under `relations:` and named.
    fn inline(&self, value: &Value, path: &str) -> Result<()> {
        // A name gets the error about names; any other value, this one.
        self.relation(value, path)?;
        self.literal(value, path)
    }

    fn literal(&self, value: &Value, path: &str) -> Result<()> {
        match value {
            Value::String(text) if interpolated(text) => Err(Error::Other(format!(
                "{}: `{path}`: `{{{{...}}}}` is not allowed in an inline `input:` relation; \
                 declare the relation under `relations:`, where a value may read the \
                 environment, and name it",
                self.display
            ))),
            Value::Sequence(items) => items
                .iter()
                .enumerate()
                .try_for_each(|(index, item)| self.literal(item, &format!("{path}[{index}]"))),
            Value::Mapping(map) => map.iter().try_for_each(|(key, item)| {
                let item_path = format!("{path}.{}", super::decode::key_text(key));
                self.literal(key, &item_path)?;
                self.literal(item, &item_path)
            }),
            _ => Ok(()),
        }
    }

    fn assertion(&self, value: &Value, path: &str) -> Result<()> {
        match value {
            Value::Mapping(map) => {
                for (key, inner) in map {
                    let inner_path = self.key(path, key)?;
                    // `to: { row: { F: literal } }`: each key names a field and
                    // each value is a literal, which interpolates as any value
                    // does.
                    if key.as_str() == Some("row") {
                        self.keys(Some(inner), &inner_path)?;
                        continue;
                    }
                    if key
                        .as_str()
                        .is_some_and(|key| ASSERT_NAME_KEYS.contains(&key))
                    {
                        self.value(inner, &inner_path)?;
                    }
                    self.assertion(inner, &inner_path)?;
                }
                Ok(())
            }
            Value::Sequence(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.assertion(item, &format!("{path}[{index}]"))?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
