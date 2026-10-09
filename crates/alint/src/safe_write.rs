//! Write-side confinement for files the CLI creates from repo-derived
//! paths (the `baseline:` key, the default baseline, `init`'s config).
//!
//! Repository content is untrusted: a `baseline: ../elsewhere` key or a
//! committed symlink at the default baseline path must not let
//! `alint baseline` overwrite a file outside the repository. Writes go
//! through [`write_in_root`], which
//!
//! 1. rejects a target that lexically escapes `root` (`..`, absolute),
//! 2. rejects a target whose (existing) parent directory canonically
//!    resolves outside `root` (a symlinked directory),
//! 3. refuses to write through a symlink (dangling or not) or onto a
//!    non-regular file, and
//! 4. writes a temp file in the same directory and renames it into
//!    place, so the final step never follows a link.

use std::io::Write as _;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context as _, Result, bail};

/// Lexically normalise `path` (resolve `.` / `..` without touching the
/// filesystem). `..` above the start is kept so containment fails.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Fail unless `target` stays inside `root` both lexically and after
/// resolving symlinks in its parent directory, and is not itself a
/// symlink or non-regular file. `what` names the file in errors.
pub(crate) fn check_target(root: &Path, target: &Path, what: &str) -> Result<()> {
    let root_abs = lexical_normalize(
        &std::path::absolute(root).with_context(|| format!("resolving {}", root.display()))?,
    );
    let target_abs = lexical_normalize(
        &std::path::absolute(target).with_context(|| format!("resolving {}", target.display()))?,
    );
    if !target_abs.starts_with(&root_abs) {
        bail!(
            "refusing to write {what} {}: it is outside the repository root {} \
             (pass an explicit `--output` path to write elsewhere)",
            target.display(),
            root.display()
        );
    }
    let parent = target_abs
        .parent()
        .with_context(|| format!("{what} path {} has no parent", target.display()))?;
    let parent_real = parent
        .canonicalize()
        .with_context(|| format!("resolving the directory of {what} {}", target.display()))?;
    let root_real = root_abs
        .canonicalize()
        .with_context(|| format!("resolving {}", root.display()))?;
    if !parent_real.starts_with(&root_real) {
        bail!(
            "refusing to write {what} {}: its directory resolves (through a symlink) \
             outside the repository root {}",
            target.display(),
            root.display()
        );
    }
    refuse_symlink_or_special(target, what)
}

/// Refuse a `target` that is a symlink (even dangling) or exists but is
/// not a regular file.
pub(crate) fn refuse_symlink_or_special(target: &Path, what: &str) -> Result<()> {
    match std::fs::symlink_metadata(target) {
        Ok(meta) if meta.file_type().is_symlink() => bail!(
            "refusing to write {what} {}: it is a symlink; replace it with a regular file",
            target.display()
        ),
        Ok(meta) if !meta.is_file() => bail!(
            "refusing to write {what} {}: it exists and is not a regular file",
            target.display()
        ),
        _ => Ok(()),
    }
}

/// Write `contents` to `target` without following a symlink at the
/// final path: create a fresh temp file beside it (`create_new`, which
/// never follows links) and rename it over `target`.
pub(crate) fn write_replacing(target: &Path, contents: &[u8], what: &str) -> Result<()> {
    refuse_symlink_or_special(target, what)?;
    let dir = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .with_context(|| format!("{what} path {} has no file name", target.display()))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".alint-tmp-{}", std::process::id()));
    let tmp = dir.join(tmp_name);
    let result = (|| -> Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .with_context(|| format!("creating {}", tmp.display()))?;
        file.write_all(contents)
            .with_context(|| format!("writing {}", tmp.display()))?;
        file.sync_all().ok();
        std::fs::rename(&tmp, target)
            .with_context(|| format!("writing {what} {}", target.display()))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Validate an output path: a repo-derived one (`explicit == false`) is
/// confined to `root` ([`check_target`]); an explicit command-line path
/// may point anywhere but still never at a symlink.
pub(crate) fn check_output(root: &Path, target: &Path, explicit: bool, what: &str) -> Result<()> {
    if explicit {
        refuse_symlink_or_special(target, what)
    } else {
        check_target(root, target, what)
    }
}

/// [`check_output`] + [`write_replacing`]: the confined write.
pub(crate) fn write_output(
    root: &Path,
    target: &Path,
    contents: &[u8],
    explicit: bool,
    what: &str,
) -> Result<()> {
    check_output(root, target, explicit, what)?;
    write_replacing(target, contents, what)
}

/// Create `target` only if nothing (not even a dangling symlink) exists
/// there. `create_new` maps to `O_CREAT | O_EXCL`, which fails on any
/// existing path, links included, so this never writes through a link.
pub(crate) fn create_new(target: &Path, contents: &[u8], what: &str) -> Result<()> {
    refuse_symlink_or_special(target, what)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .with_context(|| format!("creating {what} {}", target.display()))?;
    file.write_all(contents)
        .with_context(|| format!("writing {what} {}", target.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_normalize_resolves_dots_and_keeps_escapes() {
        assert_eq!(
            lexical_normalize(Path::new("/r/a/../b/./c")),
            PathBuf::from("/r/b/c")
        );
        assert_eq!(lexical_normalize(Path::new("../x")), PathBuf::from("../x"));
    }

    #[test]
    fn check_target_rejects_a_lexical_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        let err = check_target(&root, &root.join("../out.json"), "baseline").unwrap_err();
        assert!(err.to_string().contains("outside the repository"), "{err}");
        check_target(&root, &root.join("ok.json"), "baseline").unwrap();
    }

    #[test]
    fn write_replacing_overwrites_a_regular_file_and_leaves_no_temp() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("b.json");
        std::fs::write(&target, "old").unwrap();
        write_replacing(&target, b"new", "baseline").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn create_new_refuses_a_dangling_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let link = tmp.path().join(".alint.yml");
        std::os::unix::fs::symlink(tmp.path().join("elsewhere.yml"), &link).unwrap();
        assert!(create_new(&link, b"x", "config").is_err());
        assert!(!tmp.path().join("elsewhere.yml").exists());
    }
}
