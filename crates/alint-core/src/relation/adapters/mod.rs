//! Built-in row adapters (v0.21 R2).

mod entry;
mod extract;
mod toml;
mod xml;

pub use entry::EntryRowsAdapter;
pub use extract::ExtractAdapter;
pub use toml::TomlRowsAdapter;
pub use xml::{AndroidStringsAdapter, ResxAdapter};
