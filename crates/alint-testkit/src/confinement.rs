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
    // A write to `link/file` follows `link`, wherever it leads.
    for path in entries.keys() {
        let under_symlink = path.match_indices('/').any(|(end, _)| {
            entries
                .get(&path[..end])
                .is_some_and(|node| matches!(node, TreeNode::Symlink(_)))
        });
        if under_symlink {
            issues.push(TreeSpecIssue {
                path: path.clone(),
                reason: "entry must not sit under a symlink".to_string(),
            });
        }
    }
    if validate_symlink_targets {
        // A slash in a key makes the directories before it.
        let directories: BTreeSet<&str> = entries
            .keys()
            .flat_map(|path| path.match_indices('/').map(|(end, _)| &path[..end]))
            .collect();
        for (path, node) in &entries {
            if matches!(node, TreeNode::Symlink(_))
                && let Err(reason) = validate_tree_symlink(path, &entries, &directories)
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
        if alint_core::names_git_directory(component) {
            return Some("`.git` entries are runner-owned".to_string());
        }
    }
    None
}

/// The most symlinks one resolution follows before it counts as a cycle, as
/// Linux's `ELOOP` limit.
const MAX_TREE_SYMLINK_HOPS: usize = 40;

/// Prove that the symlink at `link_path` leads to an entry of the tree, as the
/// file system resolves it once the tree is written: each component in turn,
/// following a symlink it reaches before a later `..` steps up from where that
/// link leads. Read as text instead, `a/c/t/../../../x` (with `a/c/t -> ../../b`)
/// is `x`, while on disk it is two levels above the root.
fn validate_tree_symlink(
    link_path: &str,
    entries: &BTreeMap<String, &TreeNode>,
    directories: &BTreeSet<&str>,
) -> Result<(), String> {
    let Some(TreeNode::Symlink(link)) = entries.get(link_path) else {
        return Err("symlink target must exist in the inline tree".to_string());
    };
    let mut directory: Vec<&str> = link_path.split('/').collect();
    directory.pop();
    let tree = TreeLinks {
        entries,
        directories,
    };
    let mut hops = 0;
    let resolved = tree.resolve(directory, &link.target, &mut hops)?;
    if resolved.is_empty() {
        return Err("symlink must resolve to an in-tree file".to_string());
    }
    Ok(())
}

/// An inline tree's entries, and the directories a slash in a key makes, as
/// a symlink resolves against them.
struct TreeLinks<'t, 'a> {
    entries: &'t BTreeMap<String, &'a TreeNode>,
    directories: &'t BTreeSet<&'t str>,
}

impl<'a> TreeLinks<'_, 'a> {
    /// The entry `target` leads to from the tree directory `directory`, as its
    /// components, following every symlink entry it reaches.
    fn resolve(
        &self,
        directory: Vec<&'a str>,
        target: &'a str,
        hops: &mut usize,
    ) -> Result<Vec<&'a str>, String> {
        validate_tree_link_text(target)?;
        let mut resolved = directory;
        let mut at_file = false;
        for component in target.split('/') {
            if at_file {
                return Err("symlink target must not pass through a file".to_string());
            }
            match component {
                "" | "." => {}
                ".." => {
                    if resolved.pop().is_none() {
                        return Err("symlink resolves outside the inline tree".to_string());
                    }
                }
                name => {
                    resolved.push(name);
                    let path = resolved.join("/");
                    match self.entries.get(&path) {
                        Some(TreeNode::Symlink(link)) => {
                            *hops += 1;
                            if *hops > MAX_TREE_SYMLINK_HOPS {
                                return Err("symlink chain must not contain a cycle".to_string());
                            }
                            resolved.pop();
                            resolved = self.resolve(resolved, &link.target, hops)?;
                            at_file = !self.is_directory(&resolved.join("/"));
                        }
                        Some(TreeNode::File(_) | TreeNode::Exec(_)) => at_file = true,
                        Some(TreeNode::Dir(_)) => {}
                        None if self.directories.contains(path.as_str()) => {}
                        None => {
                            return Err("symlink target must exist in the inline tree".to_string());
                        }
                    }
                }
            }
        }
        Ok(resolved)
    }

    /// Whether `path` (`""` for the root) is a directory of the tree.
    fn is_directory(&self, path: &str) -> bool {
        path.is_empty()
            || matches!(self.entries.get(path), Some(TreeNode::Dir(_)))
            || (!self.entries.contains_key(path) && self.directories.contains(path))
    }
}

fn validate_tree_link_text(target: &str) -> Result<(), String> {
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
    Ok(())
}

/// Prove that every symlink in a test repository leads, as the file system
/// follows it, inside the repository (or would, once a missing target exists
/// there). The checks of `given.tree` and `given.fixture` read link text
/// before it is written; this one reads where each written link leads, so a
/// link they misjudge (a fixture link's text resolves from where the copy is,
/// not from where the fixture is) is refused before a rule, a fixer or Git
/// follows it. The runner checks again after each fix step: a fix that moves
/// a link (`relocate`) changes where its relative text leads.
pub(crate) fn validate_repository_symlinks(repository: &Path) -> Result<(), ConfinementError> {
    let root = repository
        .canonicalize()
        .map_err(|source| ConfinementError::Io {
            path: repository.to_path_buf(),
            source,
        })?;
    validate_repository_dir(&root, &root)
}

fn validate_repository_dir(root: &Path, directory: &Path) -> Result<(), ConfinementError> {
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
        let file_type = entry.file_type().map_err(|source| ConfinementError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_symlink() {
            if !symlink_leads_inside(root, directory, &path) {
                return Err(ConfinementError::UnsupportedFixture {
                    path: path.strip_prefix(root).unwrap_or(&path).to_path_buf(),
                    reason: "symlink leads outside the test repository".to_string(),
                });
            }
        } else if file_type.is_dir() {
            validate_repository_dir(root, &path)?;
        }
    }
    Ok(())
}

/// Whether the symlink `link` in `directory` leads inside the canonical
/// `root`: where it resolves, or, when its target is missing, where the
/// target would be.
fn symlink_leads_inside(root: &Path, directory: &Path, link: &Path) -> bool {
    match link.canonicalize() {
        Ok(target) => target.starts_with(root),
        Err(_) => std::fs::read_link(link)
            .is_ok_and(|text| alint_core::resolved_within_root(&directory.join(text), root)),
    }
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
        if alint_core::names_git_directory(&entry.file_name().to_string_lossy()) {
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
            "git~2",
            "git~1.",
            "GIT~1 ",
            "git~1. .",
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
            "git~0",
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

    /// A symlink resolves as the file system resolves it, through the
    /// symlinks it reaches, not as text: with `a/c/t -> ../../b`,
    /// `a/c/t/../../../x` is two levels above the root, not `x`. And no entry
    /// sits under a symlink, where a write would follow it.
    #[test]
    fn inline_symlinks_resolve_through_the_symlinks_they_reach() {
        let escape = TreeSpec::from_yaml(
            "a: { c: { t: { \"$symlink\": \"../../b\" } } }\n\
             b: {}\n\
             rev522-escape-target: {}\n\
             s: { \"$symlink\": \"a/c/t/../../../rev522-escape-target\" }\n\
             \"s/pwned.txt\": \"written outside the sandbox\\n\"\n",
        )
        .unwrap();
        let issues = validate_tree_spec(&escape, true);
        assert!(
            issues.contains(&TreeSpecIssue {
                path: "s".to_string(),
                reason: "symlink resolves outside the inline tree".to_string(),
            }),
            "{issues:#?}"
        );
        assert!(
            issues.contains(&TreeSpecIssue {
                path: "s/pwned.txt".to_string(),
                reason: "entry must not sit under a symlink".to_string(),
            }),
            "{issues:#?}"
        );
        // `expect_tree` gets the second check too.
        assert_eq!(validate_tree_spec(&escape, false).len(), 1);

        let valid = TreeSpec::from_yaml(
            "a: { c: { t: { \"$symlink\": \"../../b\" } } }\n\
             b: { f: x }\n\
             through-link: { \"$symlink\": \"a/c/t/f\" }\n\
             up-from-link: { \"$symlink\": \"a/c/t/../b/f\" }\n\
             implied/dir/f: x\n\
             to-implied: { \"$symlink\": \"implied/dir\" }\n\
             nested: { link: { \"$symlink\": \"../implied/dir/f\" } }\n",
        )
        .unwrap();
        assert_eq!(validate_tree_spec(&valid, true), Vec::new());

        let invalid = TreeSpec::from_yaml(
            "f: x\n\
             l: { \"$symlink\": \"f\" }\n\
             through-file: { \"$symlink\": \"f/../f\" }\n\
             through-file-link: { \"$symlink\": \"l/../f\" }\n\
             root: { \"$symlink\": \"b/..\" }\n\
             b: {}\n",
        )
        .unwrap();
        let issues = validate_tree_spec(&invalid, true);
        for (path, reason) in [
            ("through-file", "must not pass through a file"),
            ("through-file-link", "must not pass through a file"),
            ("root", "must resolve to an in-tree file"),
        ] {
            assert!(
                issues
                    .iter()
                    .any(|issue| issue.path == path && issue.reason.contains(reason)),
                "{path}: {issues:#?}"
            );
        }
    }

    /// A written link is checked where it leads: one that leaves the
    /// repository, or whose missing target would be outside it, is refused,
    /// wherever its text came from.
    #[cfg(unix)]
    #[test]
    fn written_repository_symlinks_must_lead_inside_it() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let repository = temp.path().join("repository");
        std::fs::create_dir_all(repository.join("d")).unwrap();
        std::fs::write(repository.join("f"), "x").unwrap();
        std::fs::write(temp.path().join("outside"), "x").unwrap();
        symlink("../f", repository.join("d/in")).unwrap();
        symlink("d", repository.join("dir-link")).unwrap();
        assert!(validate_repository_symlinks(&repository).is_ok());

        // A missing target in the repository is there once it exists.
        symlink("missing", repository.join("dangling")).unwrap();
        assert!(validate_repository_symlinks(&repository).is_ok());
        std::fs::remove_file(repository.join("dangling")).unwrap();

        for (link, target) in [("d/out", "../../outside"), ("d/missing", "../../missing")] {
            symlink(target, repository.join(link)).unwrap();
            let error = validate_repository_symlinks(&repository).unwrap_err();
            assert!(
                matches!(&error, ConfinementError::UnsupportedFixture { path, .. } if path == Path::new(link)),
                "{error:?}"
            );
            std::fs::remove_file(repository.join(link)).unwrap();
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
