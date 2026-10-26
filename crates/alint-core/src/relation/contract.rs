//! Bridge between the internal relation traits and the std-only plugin
//! contract. The contract types live in `crate::plugin_api`; this module is
//! the only place allowed to know both layouts.

use crate::extract::NonStringValueKind;
use crate::plugin_api::{
    Adapter as ContractAdapterTrait, AdapterError as ContractError, AdapterInput as ContractInput,
    AdapterOutput as ContractOutput, Limits as ContractLimits, PointerSegment,
    Resource as ContractResource, Row as ContractRow, RowState as ContractRowState,
};

use super::adapter::{Adapter, AdapterError, AdapterNote, AdapterOutput};
use super::limits::LimitKind;
use super::pointer::{Pointer, Segment};
use super::resource::{Origin, Resource};
use super::row::{Row, RowState};
use super::value::Value;

/// Wraps a contract adapter so the engine sees an internal [`Adapter`].
pub struct ContractAdapter<A> {
    inner: A,
}

impl<A: ContractAdapterTrait> std::fmt::Debug for ContractAdapter<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContractAdapter")
            .field("id", &self.inner.id())
            .finish()
    }
}

impl<A> ContractAdapter<A> {
    #[must_use]
    pub fn new(inner: A) -> Self {
        Self { inner }
    }
}

impl<A: ContractAdapterTrait> Adapter for ContractAdapter<A> {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn redacts_by_default(&self) -> bool {
        self.inner.redacts_by_default()
    }

    fn extract(
        &self,
        resource: &Resource,
        bytes: &[u8],
        limits: &super::limits::Limits,
    ) -> Result<AdapterOutput, AdapterError> {
        let resource = contract_resource(resource);
        let output = self
            .inner
            .extract(ContractInput {
                resource: &resource,
                bytes,
                limits: ContractLimits {
                    max_rows: limits.get(LimitKind::RowsPerExtractor),
                },
            })
            .map_err(from_contract_error)?;
        from_contract_output(output)
    }
}

/// Wraps an internal adapter as a contract adapter. This is the reverse bridge
/// used to prove built-in adapters travel through the contract and back
/// without changing their rows.
pub struct InternalContractAdapter<A> {
    inner: A,
}

impl<A: Adapter> std::fmt::Debug for InternalContractAdapter<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InternalContractAdapter")
            .field("id", &self.inner.id())
            .finish()
    }
}

impl<A> InternalContractAdapter<A> {
    #[must_use]
    pub fn new(inner: A) -> Self {
        Self { inner }
    }
}

impl<A: Adapter> ContractAdapterTrait for InternalContractAdapter<A> {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn redacts_by_default(&self) -> bool {
        self.inner.redacts_by_default()
    }

    fn extract(&self, input: ContractInput<'_>) -> Result<ContractOutput, ContractError> {
        let resource = internal_resource(input.resource).ok_or_else(|| {
            ContractError::Parse(format!(
                "resource path {:?} is not a normalized repository path",
                input.resource.path
            ))
        })?;
        let limits = super::limits::Limits::default()
            .with(LimitKind::RowsPerExtractor, input.limits.max_rows)
            .map_err(|error| ContractError::Limit {
                limit: input.limits.max_rows,
                reached: error.value,
            })?;
        let output = self
            .inner
            .extract(&resource, input.bytes, &limits)
            .map_err(to_contract_error)?;
        Ok(to_contract_output(output))
    }
}

fn contract_resource(resource: &Resource) -> ContractResource {
    ContractResource {
        path: resource.path().as_str().to_string(),
        indexed: matches!(resource.origin(), Origin::Index(_)),
        index: match resource.origin() {
            Origin::Index(id) => Some(u32::try_from(id.index()).unwrap_or(u32::MAX)),
            Origin::Literal => None,
        },
    }
}

fn internal_resource(resource: &ContractResource) -> Option<Resource> {
    let path = super::RelPath::parse(&resource.path)?;
    Some(match (resource.indexed, resource.index) {
        (true, Some(index)) => {
            Resource::from_parts(Origin::Index(super::ResourceId::from_index(index)), path)
        }
        _ => Resource::literal(path),
    })
}

fn from_contract_error(error: ContractError) -> AdapterError {
    match error {
        ContractError::Parse(message) => AdapterError::Parse(message),
        ContractError::Limit { limit, reached } => {
            AdapterError::Limit(super::limits::LimitExceeded {
                kind: LimitKind::RowsPerExtractor,
                limit,
                reached,
            })
        }
    }
}

fn to_contract_error(error: AdapterError) -> ContractError {
    match error {
        AdapterError::Parse(message) => ContractError::Parse(message),
        AdapterError::Limit(limit) => ContractError::Limit {
            limit: limit.limit,
            reached: limit.reached,
        },
    }
}

fn from_contract_output(output: ContractOutput) -> Result<AdapterOutput, AdapterError> {
    let mut rows = Vec::with_capacity(output.rows.len());
    for row in output.rows {
        rows.push(from_contract_row(row)?);
    }
    let non_strings = output
        .non_strings
        .iter()
        .filter_map(|kind| match kind.as_str() {
            "null" => Some(NonStringValueKind::Null),
            "boolean" => Some(NonStringValueKind::Boolean),
            "number" => Some(NonStringValueKind::Number),
            "array" => Some(NonStringValueKind::Array),
            "object" => Some(NonStringValueKind::Object),
            _ => None,
        })
        .collect();
    let notes = output
        .notes
        .into_iter()
        .map(|value| AdapterNote::NonLiteral { value })
        .collect();
    Ok(AdapterOutput {
        rows,
        non_strings,
        notes,
    })
}

fn from_contract_row(row: ContractRow) -> Result<Row, AdapterError> {
    let resource = internal_resource(&row.resource).ok_or_else(|| {
        AdapterError::Parse(format!(
            "resource path {:?} is not a normalized repository path",
            row.resource.path
        ))
    })?;
    let pointer = row.pointer.map(|segments| {
        Pointer::from_segments(
            segments
                .into_iter()
                .map(|segment| match segment {
                    PointerSegment::Key(key) => Segment::Key(key.into()),
                    PointerSegment::Index(index) => {
                        Segment::Index(usize::try_from(index).unwrap_or(usize::MAX))
                    }
                })
                .collect(),
        )
    });
    let state = match row.state {
        ContractRowState::Value(value) => RowState::Value(
            Value::from_contract(&value)
                .map_err(|error| AdapterError::Parse(format!("invalid contract value: {error}")))?,
        ),
        ContractRowState::Key => RowState::Key,
        ContractRowState::Missing => RowState::Missing,
        ContractRowState::Error(message) => RowState::Error(message.into()),
    };
    let original = row
        .original
        .as_ref()
        .map(Value::from_contract)
        .transpose()
        .map_err(|error| AdapterError::Parse(format!("invalid contract value: {error}")))?;
    let mut converted = Row::new(resource, pointer, row.ordinal, state);
    converted.label = row.label.map(Into::into);
    converted.span = match (row.start, row.end) {
        (Some(start), Some(end)) => Some(crate::extract::ByteSpan::new(
            usize::try_from(start)
                .map_err(|_| AdapterError::Parse("span start is too large".into()))?,
            usize::try_from(end)
                .map_err(|_| AdapterError::Parse("span end is too large".into()))?,
        )),
        _ => None,
    };
    converted.original = original;
    Ok(converted)
}

fn to_contract_output(output: AdapterOutput) -> ContractOutput {
    ContractOutput {
        rows: output.rows.into_iter().map(to_contract_row).collect(),
        non_strings: output
            .non_strings
            .into_iter()
            .map(|kind| kind.as_str().to_string())
            .collect(),
        notes: output
            .notes
            .into_iter()
            .map(|note| match note {
                AdapterNote::NonLiteral { value } => value,
            })
            .collect(),
    }
}

fn to_contract_row(row: Row) -> ContractRow {
    let resource = contract_resource(&row.resource);
    let pointer = row.pointer.as_ref().map(|pointer| {
        pointer
            .segments()
            .iter()
            .map(|segment| match segment {
                Segment::Key(key) => PointerSegment::Key(key.to_string()),
                Segment::Index(index) => PointerSegment::Index(*index as u64),
            })
            .collect()
    });
    let state = match row.state {
        RowState::Value(value) => ContractRowState::Value(value.to_contract()),
        RowState::Key => ContractRowState::Key,
        RowState::Missing => ContractRowState::Missing,
        RowState::Error(message) => ContractRowState::Error(message.to_string()),
    };
    ContractRow {
        resource,
        pointer,
        label: row.label.map(|label| label.to_string()),
        ordinal: row.ordinal,
        start: row.span.map(|span| span.start as u64),
        end: row.span.map(|span| span.end as u64),
        state,
        original: row.original.map(|value| value.to_contract()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relation::adapters::EntryRowsAdapter;
    use crate::structured_format::Format;

    #[test]
    fn builtin_entry_adapter_round_trips_through_the_contract() {
        let direct = EntryRowsAdapter::new(Format::Json).unwrap();
        let bridged = ContractAdapter::new(InternalContractAdapter::new(
            EntryRowsAdapter::new(Format::Json).unwrap(),
        ));
        let resource = Resource::for_test(0, "a.json");
        let bytes = br#"{"a":[1],"b":{"c":true},"d":null}"#;
        let limits = super::super::limits::Limits::default();
        let expected = direct.extract(&resource, bytes, &limits).unwrap();
        let actual = bridged.extract(&resource, bytes, &limits).unwrap();
        assert_eq!(actual, expected);
    }
}
