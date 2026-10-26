//! The internal relation layer: general-linter Phase 2 (ADR-0023), work item
//! R1 of `docs/design/v0.21/README.md`.
//!
//! Resources (index entries or literal paths), selectors that pick them,
//! adapters that turn a resource's bytes into rows, typed values that convert
//! to the plugin contract's flat form (`crate::plugin_api`), relations in
//! canonical order with provenance, absence states, cardinality, and limits. R2 puts the built-in formats on these types, and
//! `key_parity` (R3) and the shadowed `cross_file` `equals` port (R4) are
//! their first users.
//!
//! The engine's one internal trait set lives here (`Adapter` first): the
//! built-in formats implement it, and contract adapters reach it through a
//! bridge (R6a). The module is public only so `alint-rules` can build on it.
//! It is hidden from the docs, carries no semver promise until v1.0, and
//! nothing in it appears in any report or schema.

mod adapter;
mod adapters;
mod cache;
mod contract;
mod limits;
mod path;
mod pointer;
mod resource;
mod row;
mod value;

pub use adapter::{Adapter, AdapterError, AdapterNote, AdapterOutput, check_row_count};
pub use adapters::{
    AndroidStringsAdapter, EntryRowsAdapter, ExtractAdapter, ResxAdapter, TomlRowsAdapter,
};
pub use cache::{DecodeCache, ReadCache};
pub use contract::{ContractAdapter, InternalContractAdapter};
pub use limits::{LimitConfigError, LimitExceeded, LimitKind, Limits, RunBudget};
pub use path::RelPath;
pub use pointer::{Pointer, Segment};
pub use resource::{Origin, Resource, ResourceId, ResourceRead, Selection, Selector};
pub use row::{Cardinality, CardinalityError, Relation, Row, RowState};
pub use value::{ContractValueError, Number, Value};

#[cfg(test)]
mod tests {
    /// Invariant 6 of the v0.21 design: the layer stays out of the documented
    /// public API.
    #[test]
    fn the_module_is_hidden_from_the_docs() {
        let lib = include_str!("../lib.rs").replace("\r\n", "\n");
        assert!(
            lib.contains("#[doc(hidden)]\npub mod relation;"),
            "lib.rs must declare `#[doc(hidden)] pub mod relation;`"
        );
    }
}
