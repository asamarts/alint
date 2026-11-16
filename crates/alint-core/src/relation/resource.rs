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
    /// A directory every path the scope matches lies under, so only the
    /// entries at and below it are tested.
    within: Option<std::path::PathBuf>,
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
        Self {
            scope,
            within: None,
        }
    }

    /// A selector for the files one glob `pattern` matches, compiled as
    /// [`Scope::from_patterns`] compiles it. It tests only the entries under
    /// the directories the pattern starts with, when it starts with literal
    /// names (`locales/*.json`, or a nested rule's `{dir}/*.json` once
    /// rendered), and selects what testing every entry would, in the same
    /// order. A rule that selects once per iteration of a nested `require:`
    /// block then reads its own directory rather than the whole tree.
    ///
    /// # Errors
    ///
    /// An invalid glob, as [`Scope::from_patterns`] reports it.
    pub fn for_glob(pattern: &str) -> crate::error::Result<Self> {
        Ok(Self {
            scope: Scope::from_patterns(&[pattern.to_owned()])?,
            within: crate::scope::literal_dir_prefix(pattern).map(std::path::PathBuf::from),
        })
    }

    /// The positions in [`FileIndex::entries`] this selector tests, in index
    /// order: every entry, or those at and below `within` when the
    /// index keeps them together.
    fn candidates(&self, index: &FileIndex) -> std::ops::Range<usize> {
        self.within
            .as_deref()
            .and_then(|dir| index.subtree(dir))
            .unwrap_or(0..index.entries.len())
    }

    #[must_use]
    pub fn select(&self, index: &FileIndex) -> Selection {
        let mut selection = Selection::default();
        for position in self.candidates(index) {
            let entry = &index.entries[position];
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

    /// The entries the narrowing table below selects from, in the walk's
    /// order (`Path`'s, component by component).
    fn narrowing_index(sorted: bool) -> FileIndex {
        let mut paths = vec![
            ("en.json", false),
            ("locales", true),
            ("locales/en.json", false),
            ("locales/fr.json", false),
            ("locales/sub", true),
            ("locales/sub/de.json", false),
            ("locales-x", true),
            ("locales-x/en.json", false),
            ("locales.json", false),
            ("Locales", true),
            ("Locales/en.json", false),
            ("app", true),
            ("app/[slug]", true),
            ("app/[slug]/page.json", false),
            ("app/s", true),
            ("app/s/page.json", false),
            ("a\\b", true),
            ("a\\b/x.json", false),
            ("z/locales/en.json", false),
        ];
        paths.sort_by(|a, b| Path::new(a.0).cmp(Path::new(b.0)));
        if !sorted {
            paths.reverse();
        }
        index(&paths)
    }

    fn selected(selection: &Selection) -> Vec<(String, Origin)> {
        selection
            .resources
            .iter()
            .map(|r| (r.path().as_str().to_owned(), r.origin()))
            .collect()
    }

    /// A selector for one glob tests only the entries under the glob's literal
    /// directories, and selects exactly what testing every entry selects, in
    /// the same order: escaped glob characters, a prefix that is a file, a
    /// name that differs only in case, a sibling that shares a string prefix,
    /// and an index in another order, which it scans whole (the audit of #507:
    /// a nested `key_parity` scanned the whole index once per iteration).
    #[test]
    fn a_glob_selector_tests_only_its_literal_directories_and_selects_the_same() {
        let backslash_separates = std::path::is_separator('\\');
        for (pattern, prefix) in [
            ("locales/*.json", Some("locales")),
            ("locales/**/*.json", Some("locales")),
            ("locales/**", Some("locales")),
            ("locales/sub/*.json", Some("locales/sub")),
            ("locales-x/*.json", Some("locales-x")),
            ("Locales/*.json", Some("Locales")),
            ("LOCALES/*.json", Some("LOCALES")),
            // Where `\\` separates, a class in a segment ends the prefix.
            (
                "app/[[]slug[]]/*.json",
                Some(if backslash_separates {
                    "app"
                } else {
                    "app/[slug]"
                }),
            ),
            (
                "app/[s]/*.json",
                Some(if backslash_separates { "app" } else { "app/s" }),
            ),
            ("app/[sl]*/*.json", Some("app")),
            ("en.json/*.json", Some("en.json")),
            ("missing/*.json", Some("missing")),
            ("locales/../locales/*.json", Some("locales")),
            ("**/*.json", None),
            ("*/en.json", None),
            ("{locales,app}/*.json", None),
            ("./locales/*.json", None),
            ("/locales/*.json", None),
            ("!locales/*.json", None),
            ("en.json", None),
        ] {
            assert_eq!(
                crate::scope::literal_dir_prefix(pattern).as_deref(),
                prefix,
                "{pattern}"
            );
            let full = Selector::new(Scope::from_patterns(&[pattern.to_owned()]).unwrap());
            let narrowed = Selector::for_glob(pattern).unwrap();
            for sorted in [true, false] {
                let index = narrowing_index(sorted);
                let (all, some) = (full.select(&index), narrowed.select(&index));
                assert_eq!(
                    selected(&some),
                    selected(&all),
                    "{pattern} sorted: {sorted}"
                );
                assert_eq!(some.unrepresentable, all.unrepresentable, "{pattern}");
                // Only entries at or below the prefix are tested, when the
                // index keeps them together.
                let tested = narrowed.candidates(&index);
                for (position, entry) in index.entries.iter().enumerate() {
                    let below = prefix.is_some_and(|dir| entry.path.starts_with(dir));
                    if sorted && prefix.is_some() {
                        assert_eq!(tested.contains(&position), below, "{pattern} {position}");
                    } else {
                        assert!(tested.contains(&position), "{pattern} {position}");
                    }
                }
            }
        }
        // A name with a backslash, where it is not a separator.
        if !backslash_separates {
            let narrowed = Selector::for_glob("a\\\\b/*.json").unwrap();
            let index = narrowing_index(true);
            assert_eq!(narrowed.candidates(&index).len(), 2);
            assert_eq!(selected(&narrowed.select(&index)).len(), 1);
        }
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
