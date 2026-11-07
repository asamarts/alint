//! Repository paths as relations compare them (ADR-0023, decision V21-2).

use std::ffi::OsString;
use std::fmt;
use std::path::{Component, Path};
use std::sync::Arc;

/// A repository path as relations compare it: root-relative, `/`-separated,
/// with `.` and empty segments dropped and `..` resolved. Equality is byte for
/// byte on every operating system; case folding is an explicit operation
/// ([`RelPath::eq_ignore_case`]), never a property of the host file system.
/// Paths order segment by segment, as the walker orders its index, so `a/b`
/// sorts before `a-c/x` (a whole-string byte order would put `-` first).
///
/// A walked path whose name is not UTF-8 keeps the name as the operating
/// system gave it ([`RelPath::from_os_path`]): it reads the right file and
/// compares exactly, and only its text ([`as_str`](RelPath::as_str),
/// `Display`) is lossy, as `Path::display` is.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct RelPath(Repr);

#[derive(Clone, PartialEq, Eq, Hash)]
enum Repr {
    Text(Arc<str>),
    /// Never valid UTF-8, so it never equals a `Text` path.
    Os(Arc<OsPath>),
}

#[derive(PartialEq, Eq, Hash)]
struct OsPath {
    /// The segments as the operating system gave them, joined with `/`.
    path: OsString,
    /// `path` with each invalid sequence replaced by U+FFFD.
    text: Box<str>,
}

impl Ord for RelPath {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let separator = |byte: &u8| *byte == b'/';
        self.bytes()
            .split(separator)
            .cmp(other.bytes().split(separator))
    }
}

impl PartialOrd for RelPath {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl RelPath {
    /// A walked index path: its components joined with `/`, whatever the host
    /// separator. `None` for an absolute, root-escaping, empty, or non-UTF-8
    /// path.
    #[must_use]
    pub fn from_index_path(path: &Path) -> Option<Self> {
        let mut segments: Vec<&str> = Vec::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    segments.pop()?;
                }
                Component::Normal(name) => segments.push(name.to_str()?),
                Component::RootDir | Component::Prefix(_) => return None,
            }
        }
        Self::join(&segments)
    }

    /// [`from_index_path`](Self::from_index_path) that also keeps a path with
    /// a name that is not UTF-8. `None` only for an absolute, root-escaping, or
    /// empty path. Selectors and the plugin contract keep using
    /// `from_index_path`, because a plugin receives paths as strings.
    #[must_use]
    pub fn from_os_path(path: &Path) -> Option<Self> {
        let mut segments = Vec::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    segments.pop()?;
                }
                Component::Normal(name) => segments.push(name),
                Component::RootDir | Component::Prefix(_) => return None,
            }
        }
        if let Some(text) = segments
            .iter()
            .map(|name| name.to_str())
            .collect::<Option<Vec<&str>>>()
        {
            return Self::join(&text);
        }
        let mut joined = OsString::new();
        for (position, name) in segments.iter().enumerate() {
            if position > 0 {
                joined.push("/");
            }
            joined.push(name);
        }
        let text = joined.to_string_lossy().into();
        Some(Self(Repr::Os(Arc::new(OsPath { path: joined, text }))))
    }

    /// A path written in a file. It splits on `/` only, so a backslash stays
    /// part of a name on every host, and resolves `.`, `..`, repeated and
    /// trailing slashes. `None` for an absolute path (a leading `/`), a path
    /// that escapes the root, or one that resolves to nothing.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text.starts_with('/') {
            return None;
        }
        let mut segments: Vec<&str> = Vec::new();
        for segment in text.split('/') {
            match segment {
                "" | "." => {}
                ".." => {
                    segments.pop()?;
                }
                name => segments.push(name),
            }
        }
        Self::join(&segments)
    }

    /// The bytes the order compares: the text, or the operating system's
    /// encoding of a path that is not UTF-8.
    fn bytes(&self) -> &[u8] {
        match &self.0 {
            Repr::Text(text) => text.as_bytes(),
            Repr::Os(os) => os.path.as_encoded_bytes(),
        }
    }

    fn join(segments: &[&str]) -> Option<Self> {
        if segments.is_empty() {
            return None;
        }
        Some(Self(Repr::Text(Arc::from(segments.join("/")))))
    }

    /// The path as text: exact for a UTF-8 path, lossy for one that is not.
    /// Compare `RelPath`s, not their text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match &self.0 {
            Repr::Text(text) => text,
            Repr::Os(os) => &os.text,
        }
    }

    /// The path to read, relative to the repository root. Exact on every host.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        match &self.0 {
            Repr::Text(text) => Path::new(&**text),
            Repr::Os(os) => Path::new(&os.path),
        }
    }

    /// Case-insensitive equality, for a comparison that opts into it. Uses
    /// Unicode lowercasing, the same on every host. A path that is not UTF-8
    /// equals only itself.
    #[must_use]
    pub fn eq_ignore_case(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Repr::Text(a), Repr::Text(b)) => a == b || a.to_lowercase() == b.to_lowercase(),
            _ => self == other,
        }
    }
}

impl fmt::Debug for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_normalizes_like_manifest_scope() {
        let cases = [
            ("docs/README.md", Some("docs/README.md")),
            ("./docs/../docs/README.md", Some("docs/README.md")),
            ("docs//api/", Some("docs/api")),
            ("a/./b/../c", Some("a/c")),
            ("a\\b", Some("a\\b")),
            ("/etc/passwd", None),
            ("../outside", None),
            ("a/../..", None),
            ("", None),
            (".", None),
            ("a/..", None),
        ];
        for (input, want) in cases {
            assert_eq!(
                RelPath::parse(input).as_ref().map(RelPath::as_str),
                want,
                "{input:?}"
            );
        }
    }

    #[test]
    fn index_paths_join_components_with_forward_slashes() {
        let path = Path::new("crates").join("core").join("lib.rs");
        assert_eq!(
            RelPath::from_index_path(&path).unwrap().as_str(),
            "crates/core/lib.rs"
        );
        assert!(RelPath::from_index_path(Path::new("/abs")).is_none());
        assert!(RelPath::from_index_path(Path::new("../up")).is_none());
        assert!(RelPath::from_index_path(Path::new("")).is_none());
    }

    #[test]
    fn paths_order_like_the_walked_index() {
        let parse = |p: &str| RelPath::parse(p).unwrap();
        assert!(parse("a/b") < parse("a-c/x"));
        assert!(parse("a") < parse("a/b"));
        assert!(parse("B") < parse("a"));
        let mut paths = [parse("a-c/x"), parse("a/b/c"), parse("a/b"), parse("a.d")];
        paths.sort();
        let ordered: Vec<&str> = paths.iter().map(RelPath::as_str).collect();
        assert_eq!(ordered, ["a/b", "a/b/c", "a-c/x", "a.d"]);
        // The same order std gives the walker's `Path`s.
        let mut std_paths: Vec<&Path> = ["a-c/x", "a/b/c", "a/b", "a.d"].map(Path::new).to_vec();
        std_paths.sort();
        assert_eq!(std_paths, ["a/b", "a/b/c", "a-c/x", "a.d"].map(Path::new));
    }

    #[test]
    fn comparison_is_byte_exact_unless_folding_is_asked_for() {
        let upper = RelPath::parse("Docs/README.md").unwrap();
        let lower = RelPath::parse("docs/readme.md").unwrap();
        assert_ne!(upper, lower);
        assert!(upper.eq_ignore_case(&lower));
        assert!(!upper.eq_ignore_case(&RelPath::parse("docs/other.md").unwrap()));
    }

    #[test]
    fn os_paths_match_index_paths_when_utf8() {
        for path in ["a/b.txt", "./a/../b", "crates/core/lib.rs"] {
            let path = Path::new(path);
            assert_eq!(
                RelPath::from_os_path(path),
                RelPath::from_index_path(path),
                "{path:?}"
            );
        }
        for path in ["/abs", "../up", "", "a/.."] {
            assert!(RelPath::from_os_path(Path::new(path)).is_none(), "{path:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn os_paths_keep_names_that_are_not_utf8() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let name = |bytes: &[u8]| Path::new(OsStr::from_bytes(bytes)).to_path_buf();
        let one = Path::new("dir").join(name(b"bad\xff.json"));
        let two = Path::new("dir").join(name(b"bad\xfe.json"));
        assert!(RelPath::from_index_path(&one).is_none());
        let (one_rel, two_rel) = (
            RelPath::from_os_path(&one).unwrap(),
            RelPath::from_os_path(&two).unwrap(),
        );
        assert_eq!(one_rel.as_path(), one.as_path());
        assert_eq!(one_rel.as_str(), "dir/bad\u{fffd}.json");
        // Both render alike, yet stay distinct paths.
        assert_eq!(one_rel.as_str(), two_rel.as_str());
        assert_ne!(one_rel, two_rel);
        assert!(!one_rel.eq_ignore_case(&two_rel));
        assert!(one_rel.eq_ignore_case(&one_rel.clone()));
        // Segment-wise order, like the walker's: `dir/…` before `dir-x`, and
        // the raw bytes decide between the two names.
        let dash = RelPath::parse("dir-x").unwrap();
        assert!(one_rel < dash);
        assert!(two_rel < one_rel);
        assert_eq!(format!("{one_rel:?}"), "\"dir/bad\u{fffd}.json\"");
    }

    proptest::proptest! {
        #[test]
        fn parse_is_idempotent(text in "[a-c./\\\\]{0,24}") {
            if let Some(path) = RelPath::parse(&text) {
                proptest::prop_assert_eq!(RelPath::parse(path.as_str()), Some(path.clone()));
                proptest::prop_assert!(!path.as_str().starts_with('/'));
                proptest::prop_assert!(!path.as_str().ends_with('/'));
                proptest::prop_assert!(!path.as_str().split('/').any(|s| s.is_empty() || s == "." || s == ".."));
            }
        }
    }
}
