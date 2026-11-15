//! Policy-test package path and fixture confinement validation.
//!
//! P1 validates package-owned inputs without copying or opening them for execution.
//! P2 performs descriptor-relative reads/copies after this preflight; lexical or
//! canonical validation here is never treated as a replacement for safe open/write.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use crate::treespec::{TreeNode, TreeSpec};

/// What an existing package path is expected to identify.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageEntryKind {
    File,
    Directory,
}

/// A canonical policy-test package root.
#[derive(Debug, Clone)]
pub struct PackageRoot {
    root: PathBuf,
}

/// Typed path/confinement failure suitable for structured harness diagnostics.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfinementError {
    #[error("invalid package path {path:?}: {reason}")]
    InvalidPath { path: String, reason: String },
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("policy-test source must be a regular non-symlink file: {0}")]
    InvalidPolicyTestSource(PathBuf),
    #[error("package path {path:?} resolves outside the policy-test package root")]
    Escape { path: String },
    #[error("package path {path:?} is not an existing {expected}")]
    WrongKind {
        path: String,
        expected: &'static str,
    },
    #[error("fixture entry {path} is unsupported: {reason}")]
    UnsupportedFixture { path: PathBuf, reason: String },
}

impl PackageRoot {
    /// Establish a package from a regular, non-symlink policy-test document.
    pub fn for_policy_test(source: &Path) -> Result<Self, ConfinementError> {
        let metadata =
            std::fs::symlink_metadata(source).map_err(|source_error| ConfinementError::Io {
                path: source.to_path_buf(),
                source: source_error,
            })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ConfinementError::InvalidPolicyTestSource(
                source.to_path_buf(),
            ));
        }
        let parent = source.parent().unwrap_or_else(|| Path::new("."));
        let root = parent
            .canonicalize()
            .map_err(|source_error| ConfinementError::Io {
                path: parent.to_path_buf(),
                source: source_error,
            })?;
        Ok(Self { root })
    }

    /// Canonical package root used only for confinement checks and safe display shortening.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.root
    }

    /// Resolve an existing package-relative file or directory and prove confinement.
    pub fn resolve_existing(
        &self,
        relative: &str,
        kind: PackageEntryKind,
    ) -> Result<PathBuf, ConfinementError> {
        validate_package_path(relative).map_err(|reason| ConfinementError::InvalidPath {
            path: relative.to_string(),
            reason,
        })?;
        let candidate = self.root.join(relative);
        let canonical = candidate
            .canonicalize()
            .map_err(|source| ConfinementError::Io {
                path: candidate,
                source,
            })?;
        if !canonical.starts_with(&self.root) {
            return Err(ConfinementError::Escape {
                path: relative.to_string(),
            });
        }
        let metadata = std::fs::metadata(&canonical).map_err(|source| ConfinementError::Io {
            path: canonical.clone(),
            source,
        })?;
        let valid = match kind {
            PackageEntryKind::File => metadata.is_file(),
            PackageEntryKind::Directory => metadata.is_dir(),
        };
        if !valid {
            return Err(ConfinementError::WrongKind {
                path: relative.to_string(),
                expected: match kind {
                    PackageEntryKind::File => "regular file",
                    PackageEntryKind::Directory => "directory",
                },
            });
        }
        Ok(canonical)
    }

    /// Resolve a snapshot target through existing package directories.
    ///
    /// Missing parent directories are allowed for update mode. Every existing
    /// component is checked without following symlinks before the lexical,
    /// already-validated remainder is appended.
    pub fn resolve_snapshot_target(&self, relative: &str) -> Result<PathBuf, ConfinementError> {
        validate_package_path(relative).map_err(|reason| ConfinementError::InvalidPath {
            path: relative.to_string(),
            reason,
        })?;
        let components: Vec<_> = Path::new(relative)
            .components()
            .map(|component| component.as_os_str().to_owned())
            .collect();
        let (file_name, parents) =
            components
                .split_last()
                .ok_or_else(|| ConfinementError::InvalidPath {
                    path: relative.to_string(),
                    reason: "must name a file".to_string(),
                })?;
        let mut current = self.root.clone();
        let mut missing = false;
        for component in parents {
            current.push(component);
            if missing {
                continue;
            }
            match std::fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(ConfinementError::InvalidPath {
                        path: relative.to_string(),
                        reason: "snapshot parent must not be a symlink".to_string(),
                    });
                }
                Ok(metadata) if !metadata.is_dir() => {
                    return Err(ConfinementError::WrongKind {
                        path: relative.to_string(),
                        expected: "directory parent",
                    });
                }
                Ok(_) => {
                    let canonical =
                        current
                            .canonicalize()
                            .map_err(|source| ConfinementError::Io {
                                path: current.clone(),
                                source,
                            })?;
                    if !canonical.starts_with(&self.root) {
                        return Err(ConfinementError::Escape {
                            path: relative.to_string(),
                        });
                    }
                    current = canonical;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => missing = true,
                Err(source) => {
                    return Err(ConfinementError::Io {
                        path: current,
                        source,
                    });
                }
            }
        }
        let target = current.join(file_name);
        match std::fs::symlink_metadata(&target) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ConfinementError::InvalidPath {
                    path: relative.to_string(),
                    reason: "snapshot target must not be a symlink".to_string(),
                });
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(ConfinementError::WrongKind {
                    path: relative.to_string(),
                    expected: "regular file",
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(ConfinementError::Io {
                    path: target,
                    source,
                });
            }
        }
        Ok(target)
    }

    /// Validate every entry in a directory fixture, including symlink targets and special files.
    pub fn validate_fixture(&self, relative: &str) -> Result<PathBuf, ConfinementError> {
        let fixture = self.resolve_existing(relative, PackageEntryKind::Directory)?;
        let metadata = std::fs::metadata(&fixture).map_err(|source| ConfinementError::Io {
            path: fixture.clone(),
            source,
        })?;
        validate_regular_metadata(&fixture, &metadata)?;
        validate_fixture_dir(&fixture, &fixture)?;
        Ok(fixture)
    }
}

/// Reject absolute, parent, Windows-prefixed, NUL, empty, and non-canonical package paths.
pub fn validate_package_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("must not be empty".to_string());
    }
    if path.contains('\0') {
        return Err("must not contain NUL".to_string());
    }
    if path.contains('\\') {
        return Err("must use forward slashes".to_string());
    }
    if path.starts_with('/') || path.starts_with("//") {
        return Err("must be relative".to_string());
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err("must not use a Windows drive prefix".to_string());
    }
    let parsed = Path::new(path);
    if parsed.is_absolute() {
        return Err("must be relative".to_string());
    }
    for segment in path.split('/') {
        match segment {
            "" => return Err("must not contain empty components".to_string()),
            "." => return Err("must not contain `.` components".to_string()),
            ".." => return Err("must not contain `..`".to_string()),
            _ => {}
        }
    }
    for component in parsed.components() {
        match component {
            Component::Normal(value) if value != OsStr::new("") => {}
            Component::ParentDir => return Err("must not contain `..`".to_string()),
            Component::CurDir => return Err("must not contain `.` components".to_string()),
            Component::RootDir | Component::Prefix(_) => return Err("must be relative".to_string()),
            Component::Normal(_) => return Err("must not contain empty components".to_string()),
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeSpecIssue {
    pub path: String,
    pub reason: String,
}

/// Validate the entry names in a declarative tree and, for a tree that will be
/// materialized, prove that every symlink resolves to an in-tree entry.
pub(crate) fn validate_tree_spec(
    tree: &TreeSpec,
    validate_symlink_targets: bool,
) -> Vec<TreeSpecIssue> {
    let mut issues = Vec::new();
    let mut entries = BTreeMap::new();
    collect_tree_entries(&tree.root, "", &mut entries, &mut issues);
    if validate_symlink_targets {
        for (path, node) in &entries {
            if matches!(node, TreeNode::Symlink(_))
                && let Err(reason) = validate_tree_symlink(path, &entries)
            {
                issues.push(TreeSpecIssue {
                    path: path.clone(),
                    reason,
                });
            }
        }
    }
    issues
}

fn collect_tree_entries<'a>(
    children: &'a BTreeMap<String, TreeNode>,
    prefix: &str,
    entries: &mut BTreeMap<String, &'a TreeNode>,
    issues: &mut Vec<TreeSpecIssue>,
) {
    for (name, node) in children {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if let Some(reason) = validate_tree_entry_name(name) {
            issues.push(TreeSpecIssue { path, reason });
            continue;
        }
        if entries.insert(path.clone(), node).is_some() {
            issues.push(TreeSpecIssue {
                path: path.clone(),
                reason: "entry path is defined more than once".to_string(),
            });
        }
        if let TreeNode::Dir(nested) = node {
            collect_tree_entries(nested, &path, entries, issues);
        }
    }
}

fn validate_tree_entry_name(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("entry name must not be empty".to_string());
    }
    if name.contains('\\') {
        return Some("entry paths must use forward slashes".to_string());
    }
    if name.contains('\0') {
        return Some("entry name must not contain NUL".to_string());
    }
    if name.len() >= 2 && name.as_bytes()[0].is_ascii_alphabetic() && name.as_bytes()[1] == b':' {
        return Some("entry name must not be a Windows drive prefix".to_string());
    }
    for component in name.split('/') {
        if component.is_empty() {
            return Some("entry path must not contain empty components".to_string());
        }
        if component == "." || component == ".." {
            return Some("entry path must not contain `.` or `..` components".to_string());
        }
        if names_git_directory(component) {
            return Some("`.git` entries are runner-owned".to_string());
        }
    }
    None
}

/// Whether a file system could open the entry `name` as `.git`, the
/// runner's own repository directory. Beyond `.git` in any case, Git's own
/// checks name three more spellings: Windows drops trailing dots and spaces
/// (`.git.`, `.git `) and reads `name:stream` as a stream of `name`
/// (`.git::$INDEX_ALLOCATION`), and its short name for `.git` is `GIT~1`;
/// macOS's HFS+ ignores some invisible code points (`.g\u{200c}it`).
fn names_git_directory(name: &str) -> bool {
    const HFS_IGNORED: [std::ops::RangeInclusive<char>; 4] = [
        '\u{200c}'..='\u{200f}',
        '\u{202a}'..='\u{202e}',
        '\u{206a}'..='\u{206f}',
        '\u{feff}'..='\u{feff}',
    ];
    let visible: String = name
        .chars()
        .filter(|c| !HFS_IGNORED.iter().any(|range| range.contains(c)))
        .collect();
    let file = visible.split(':').next().unwrap_or_default();
    file.trim_end_matches(['.', ' '])
        .eq_ignore_ascii_case(".git")
        || file.eq_ignore_ascii_case("git~1")
}

fn validate_tree_symlink(
    link_path: &str,
    entries: &BTreeMap<String, &TreeNode>,
) -> Result<(), String> {
    let mut current = link_path.to_string();
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(current.clone()) {
            return Err("symlink chain must not contain a cycle".to_string());
        }
        let Some(node) = entries.get(&current) else {
            return Err("symlink target must exist in the inline tree".to_string());
        };
        match node {
            TreeNode::File(_) | TreeNode::Exec(_) | TreeNode::Dir(_) => return Ok(()),
            TreeNode::Symlink(link) => {
                current = normalize_tree_link_target(&current, &link.target)?;
            }
        }
    }
}

fn normalize_tree_link_target(link_path: &str, target: &str) -> Result<String, String> {
    if target.is_empty() {
        return Err("symlink target must not be empty".to_string());
    }
    if target.contains('\0') {
        return Err("symlink target must not contain NUL".to_string());
    }
    if target.contains('\\') {
        return Err("symlink target must use forward slashes".to_string());
    }
    if target.starts_with('/') {
        return Err("absolute symlinks are forbidden".to_string());
    }
    let bytes = target.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err("Windows-prefixed symlinks are forbidden".to_string());
    }

    let mut components: Vec<&str> = link_path.split('/').collect();
    components.pop();
    for component in target.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err("symlink resolves outside the inline tree".to_string());
                }
            }
            value => components.push(value),
        }
    }
    if components.is_empty() {
        return Err("symlink must resolve to an in-tree file".to_string());
    }
    Ok(components.join("/"))
}

fn validate_fixture_dir(root: &Path, directory: &Path) -> Result<(), ConfinementError> {
    let entries = std::fs::read_dir(directory).map_err(|source| ConfinementError::Io {
        path: directory.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| ConfinementError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if names_git_directory(&entry.file_name().to_string_lossy()) {
            return Err(ConfinementError::UnsupportedFixture {
                path,
                reason: "`.git` entries are runner-owned".to_string(),
            });
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|source| ConfinementError::Io {
            path: path.clone(),
            source,
        })?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            validate_fixture_symlink(root, &path)?;
        } else if file_type.is_dir() {
            validate_regular_metadata(&path, &metadata)?;
            validate_fixture_dir(root, &path)?;
        } else if file_type.is_file() {
            validate_regular_metadata(&path, &metadata)?;
        } else {
            return Err(ConfinementError::UnsupportedFixture {
                path,
                reason: "device nodes, sockets, and FIFOs are forbidden".to_string(),
            });
        }
    }
    Ok(())
}

fn validate_fixture_symlink(root: &Path, path: &Path) -> Result<(), ConfinementError> {
    let target = std::fs::read_link(path).map_err(|source| ConfinementError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if target.is_absolute() {
        return Err(ConfinementError::UnsupportedFixture {
            path: path.to_path_buf(),
            reason: "absolute symlinks are forbidden".to_string(),
        });
    }
    let canonical = path.canonicalize().map_err(|source| {
        let reason = match source.kind() {
            std::io::ErrorKind::NotFound => "dangling symlinks are forbidden",
            _ => "cyclic or unreadable symlinks are forbidden",
        };
        ConfinementError::UnsupportedFixture {
            path: path.to_path_buf(),
            reason: reason.to_string(),
        }
    })?;
    if !canonical.starts_with(root) {
        return Err(ConfinementError::UnsupportedFixture {
            path: path.to_path_buf(),
            reason: "symlink resolves outside the fixture".to_string(),
        });
    }
    let target_metadata = std::fs::metadata(&canonical).map_err(|source| ConfinementError::Io {
        path: canonical.clone(),
        source,
    })?;
    if !target_metadata.is_file() {
        return Err(ConfinementError::UnsupportedFixture {
            path: path.to_path_buf(),
            reason: "only symlinks to regular files are supported".to_string(),
        });
    }
    validate_regular_metadata(path, &target_metadata)
}

#[cfg(unix)]
fn validate_regular_metadata(
    path: &Path,
    metadata: &std::fs::Metadata,
) -> Result<(), ConfinementError> {
    use std::os::unix::fs::PermissionsExt as _;
    if metadata.permissions().mode() & 0o6000 != 0 {
        return Err(ConfinementError::UnsupportedFixture {
            path: path.to_path_buf(),
            reason: "setuid/setgid bits are forbidden".to_string(),
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_regular_metadata(
    _path: &Path,
    _metadata: &std::fs::Metadata,
) -> Result<(), ConfinementError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spelling_of_git_is_runner_owned() {
        for name in [
            ".git",
            ".GIT",
            ".git.",
            ".git ",
            ".git. .",
            ".git::$INDEX_ALLOCATION",
            ".git:stream",
            "GIT~1",
            "git~1",
            ".g\u{200c}it",
            "\u{feff}.git",
            "sub/.git.",
        ] {
            assert_eq!(
                validate_tree_entry_name(name).as_deref(),
                Some("`.git` entries are runner-owned"),
                "{name:?}"
            );
        }
        for name in [
            ".github",
            ".gitignore",
            "git",
            ".git-blame",
            "git~2",
            "a.git",
            ".gi t",
        ] {
            assert_eq!(validate_tree_entry_name(name), None, "{name:?}");
        }
    }

    #[test]
    fn lexical_package_paths_reject_escape_and_platform_prefixes() {
        for invalid in [
            "",
            "/absolute",
            "../escape",
            "a/../../escape",
            "a//b",
            "./a",
            "a/./b",
            "a/.",
            r"C:\\repo",
            r"\\server\\share",
            "nul\0byte",
        ] {
            assert!(
                validate_package_path(invalid).is_err(),
                "{invalid:?} should be rejected"
            );
        }
        for valid in [
            "fixture",
            "fixtures/repo",
            ".config.yml",
            "snapshots/a.json",
        ] {
            assert!(
                validate_package_path(valid).is_ok(),
                "{valid:?} should be accepted"
            );
        }
    }

    #[test]
    fn inline_tree_paths_and_symlinks_are_confined_before_materialization() {
        let valid = TreeSpec::from_yaml(
            "target: x\nflat/path.txt: y\ndirectory: {}\ndir:\n  link: { \"$symlink\": \"../target\" }\ndirectory-link: { \"$symlink\": \"directory\" }\n",
        )
        .unwrap();
        assert_eq!(validate_tree_spec(&valid, true), Vec::new());

        let invalid = TreeSpec::from_yaml(
            "\"../outside\": x\n\
             \"C:outside\": x\n\
             .GIT: {}\n\
             directory: {}\n\
             escape: { \"$symlink\": \"../../outside\" }\n\
             dangling: { \"$symlink\": \"missing\" }\n\
             directory-link: { \"$symlink\": \"directory\" }\n\
             loop-a: { \"$symlink\": \"loop-b\" }\n\
             loop-b: { \"$symlink\": \"loop-a\" }\n",
        )
        .unwrap();
        let issues = validate_tree_spec(&invalid, true);
        for expected in [
            "components",
            "Windows drive prefix",
            "runner-owned",
            "outside the inline tree",
            "must exist in the inline tree",
            "cycle",
        ] {
            assert!(
                issues.iter().any(|issue| issue.reason.contains(expected)),
                "missing {expected:?} in {issues:#?}"
            );
        }
    }

    #[test]
    fn existing_paths_are_canonical_and_confined() {
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("package");
        std::fs::create_dir_all(package.join("fixtures/repo")).unwrap();
        std::fs::write(package.join("case.yml"), "x").unwrap();
        std::fs::write(package.join("config.yml"), "version: 1\nrules: []\n").unwrap();
        let root = PackageRoot::for_policy_test(&package.join("case.yml")).unwrap();
        assert!(
            root.resolve_existing("fixtures/repo", PackageEntryKind::Directory)
                .is_ok()
        );
        assert!(
            root.resolve_existing("config.yml", PackageEntryKind::File)
                .is_ok()
        );
    }

    #[cfg(unix)]
    #[test]
    fn fixture_allows_in_root_file_links_and_rejects_escapes_and_git() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("package");
        let fixture = package.join("fixtures/repo");
        std::fs::create_dir_all(&fixture).unwrap();
        std::fs::write(package.join("case.yml"), "x").unwrap();
        std::fs::write(fixture.join("target"), "x").unwrap();
        symlink("target", fixture.join("link")).unwrap();
        let root = PackageRoot::for_policy_test(&package.join("case.yml")).unwrap();
        assert!(root.validate_fixture("fixtures/repo").is_ok());

        symlink("../../case.yml", fixture.join("escape")).unwrap();
        assert!(matches!(
            root.validate_fixture("fixtures/repo"),
            Err(ConfinementError::UnsupportedFixture { .. })
        ));
        std::fs::remove_file(fixture.join("escape")).unwrap();
        std::fs::create_dir(fixture.join(".GIT")).unwrap();
        assert!(matches!(
            root.validate_fixture("fixtures/repo"),
            Err(ConfinementError::UnsupportedFixture { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn policy_test_and_snapshot_symlinks_are_rejected() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("package");
        std::fs::create_dir_all(package.join("snapshots")).unwrap();
        std::fs::write(package.join("real.yml"), "x").unwrap();
        symlink("real.yml", package.join("case.yml")).unwrap();
        assert!(matches!(
            PackageRoot::for_policy_test(&package.join("case.yml")),
            Err(ConfinementError::InvalidPolicyTestSource(_))
        ));
        let root = PackageRoot::for_policy_test(&package.join("real.yml")).unwrap();
        std::fs::write(package.join("target"), "x").unwrap();
        symlink("../target", package.join("snapshots/result.json")).unwrap();
        assert!(
            root.resolve_snapshot_target("snapshots/result.json")
                .is_err()
        );
        std::fs::remove_file(package.join("snapshots/result.json")).unwrap();
        std::fs::create_dir(package.join("snapshots/result.json")).unwrap();
        assert!(matches!(
            root.resolve_snapshot_target("snapshots/result.json"),
            Err(ConfinementError::WrongKind { .. })
        ));
    }
}
