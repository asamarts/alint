//! Resources (repository files a relation reads) and the selectors that pick
//! them from the index.

use std::path::Path;
use std::sync::Arc;

use super::path::RelPath;
use crate::scope::Scope;
use crate::walker::FileIndex;

/// A file's position in [`FileIndex::entries`]: stable for one run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceId(u32);

impl ResourceId {
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// Rebuild an id from the plugin contract's numeric index.
    #[must_use]
    pub(crate) fn from_index(index: u32) -> Self {
        Self(index)
    }
}

/// How a resource was named.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    /// An entry of the walked index, picked by a selector.
    Index(ResourceId),
    /// A literal path from the config, read from disk even when the index
    /// leaves it out (an ignored file), as `cross_file` reads its source and
    /// target files and `file_exists` checks literal paths.
    Literal,
}

/// A confined repository path and how it was named.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Resource {
    origin: Origin,
    path: RelPath,
}

impl Resource {
    /// A literal path from the config.
    #[must_use]
    pub fn literal(path: RelPath) -> Self {
        Self {
            origin: Origin::Literal,
            path,
        }
    }

    #[must_use]
    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// Rebuild a resource from the plugin contract's origin fields.
    #[must_use]
    pub(crate) fn from_parts(origin: Origin, path: RelPath) -> Self {
        Self { origin, path }
    }

    #[must_use]
    pub fn path(&self) -> &RelPath {
        &self.path
    }

    /// An index resource outside an index, for tests and fixtures.
    #[cfg(test)]
    pub(crate) fn for_test(id: u32, path: &str) -> Self {
        Self {
            origin: Origin::Index(ResourceId(id)),
            path: RelPath::parse(path).expect("a valid test path"),
        }
    }
}

/// What reading a resource found. A literal path may not exist; that is its
/// own outcome, distinct from an empty file and from a read error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceRead {
    Present(Arc<[u8]>),
    Missing,
    /// The file exceeds the bounded whole-file analysis cap.
    TooLarge(u64),
    Unreadable(String),
}

/// Picks resources from the index: a compiled scope, including any
/// `scope_filter`. It decides which rows a relation holds; whether a rule runs
/// under `--changed` is its dispatch class's concern.
#[derive(Debug)]
pub struct Selector {
    scope: Scope,
}

/// What a selector picked, in canonical (segment-wise path) order, plus every
/// matching file whose path a relation cannot represent. The caller reports
/// those; a silently dropped file would be a pass that never looked at it.
#[derive(Debug, Default)]
pub struct Selection {
    pub resources: Vec<Resource>,
    pub unrepresentable: Vec<Arc<Path>>,
}

impl Selector {
    #[must_use]
    pub fn new(scope: Scope) -> Self {
        Self { scope }
    }

    #[must_use]
    pub fn select(&self, index: &FileIndex) -> Selection {
        let mut selection = Selection::default();
        for (position, entry) in index.entries.iter().enumerate() {
            if entry.is_dir || !self.scope.matches(&entry.path, index) {
                continue;
            }
            let id = u32::try_from(position).ok().map(ResourceId);
            match (id, RelPath::from_index_path(&entry.path)) {
                (Some(id), Some(path)) => selection.resources.push(Resource {
                    origin: Origin::Index(id),
                    path,
                }),
                _ => selection.unrepresentable.push(Arc::clone(&entry.path)),
            }
        }
        selection.resources.sort_by(|a, b| a.path.cmp(&b.path));
        selection
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walker::FileEntry;

    fn index(paths: &[(&str, bool)]) -> FileIndex {
        FileIndex::from_entries(
            paths
                .iter()
                .map(|(path, is_dir)| FileEntry {
                    path: Arc::from(Path::new(path)),
                    is_dir: *is_dir,
                    size: 1,
                })
                .collect(),
        )
    }

    #[test]
    fn selects_matching_files_in_index_order() {
        let index = index(&[
            ("locales", true),
            ("locales/fr.json", false),
            ("locales/en.json", false),
            ("locales/De.json", false),
            ("README.md", false),
        ]);
        let scope = Scope::from_patterns(&["locales/*.json".to_string()]).unwrap();
        let selection = Selector::new(scope).select(&index);
        let paths: Vec<&str> = selection
            .resources
            .iter()
            .map(|r| r.path().as_str())
            .collect();
        assert_eq!(
            paths,
            ["locales/De.json", "locales/en.json", "locales/fr.json"]
        );
        assert_eq!(selection.unrepresentable, Vec::<Arc<Path>>::new());
        assert_eq!(
            selection.resources[1].origin(),
            Origin::Index(ResourceId(2))
        );
    }

    #[test]
    fn literal_resources_are_named_by_path_alone() {
        let resource = Resource::literal(RelPath::parse(".env").unwrap());
        assert_eq!(resource.origin(), Origin::Literal);
        assert_eq!(resource.path().as_str(), ".env");
        assert_ne!(
            ResourceRead::Missing,
            ResourceRead::Present(Arc::from(&b""[..]))
        );
    }
}
