//! Built-in row adapters (v0.21 R2).

mod entry;
pub(crate) mod entry_values;
mod extract;
pub(crate) mod records;
mod toml;
mod xml;

pub use entry::EntryRowsAdapter;
pub(crate) use entry_values::{EntryValuesAdapter, entry_table, entry_table_with_arrays};
pub use extract::ExtractAdapter;
pub(crate) use extract::{locate, same_reading, table_bytes, walks};
pub(crate) use records::RecordsAdapter;
pub use toml::TomlRowsAdapter;
pub use xml::{AndroidStringsAdapter, ResxAdapter};
