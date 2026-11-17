//! Built-in row adapters (v0.21 R2).

mod entry;
pub(crate) mod entry_values;
mod extract;
mod toml;
mod xml;

pub use entry::EntryRowsAdapter;
pub(crate) use entry_values::EntryValuesAdapter;
pub use extract::ExtractAdapter;
pub use toml::TomlRowsAdapter;
pub use xml::{AndroidStringsAdapter, ResxAdapter};
