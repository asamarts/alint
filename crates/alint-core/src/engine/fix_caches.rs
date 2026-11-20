//! What a writing fix pass lets go of in its relation caches once a rule's
//! fixes land, by what they changed on disk before the pass ends.
//!
//! The pass reads and decodes each file once, and evaluates the relations
//! the rules read once, as `check` does; a rule after a fix must still read
//! the tree as it is. Only a fix that applied counts, and only in a pass
//! that writes: `--dry-run` and `--diff` change nothing on disk.

use std::path::Path;

use crate::config::FixSpec;
use crate::report::{FixItem, FixStatus};
use crate::walker::FileIndex;

use super::RuleEntry;

/// Where in a fix pass the relations the rules read are evaluated: before
/// the rule at this index, or `None` when no rule reads one.
///
/// Before the first rule, as `check` evaluates them, so the run's rows and
/// bytes go to the relations first and to the rules after, as in `check`: a
/// rule that decodes or holds through the caches before them (`key_parity`)
/// would otherwise leave them less than `check` leaves them, and the
/// relations could fail, or a finding fail to be placed, where `check`
/// succeeds. A pass that `writes`, and has a rule before the first that
/// reads a relation whose fix may remove or move a file or run a command,
/// evaluates them just before that rule instead: such a fix lets go of what
/// they read, and they would be read twice.
pub(super) fn relations_at(entries: &[RuleEntry], writes: bool) -> Option<usize> {
    let first = entries
        .iter()
        .position(|entry| entry.rule.relation_inputs().is_some())?;
    let moves = |entry: &RuleEntry| {
        entry.rule.fixer().is_some()
            && !entry
                .spec
                .as_deref()
                .and_then(|spec| spec.fix.as_ref())
                .is_some_and(FixSpec::keeps_paths_in_place)
    };
    Some(if writes && entries[..first].iter().any(moves) {
        first
    } else {
        0
    })
}

/// What a rule's applied fixes changed on disk before their pass ends.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Landed<'a> {
    /// No file the pass's index lists, nor its bytes: a content edit waits
    /// in the compose buffer until the pass ends (and no rule reads that
    /// buffer), a mode change changes no bytes, and a create makes a file
    /// where there was none ([`FixSpec::keeps_paths_in_place`]). A create
    /// never replaces a file, and the relations select from the index, which
    /// does not list the new one; only a read of its path that found no
    /// file is untrue now.
    InPlace,
    /// These files, with those under them, were removed or moved.
    Moved(Vec<&'a Path>),
    /// Anything may have changed: a command ran, `git_untrack` changed what
    /// Git tracks, or the engine cannot say which files a fix changed.
    Anything,
}

impl<'a> Landed<'a> {
    /// What `items`, the findings of a rule whose `fix:` is `fix`, changed:
    /// `None` when none of its fixes applied.
    pub(super) fn of(fix: Option<&FixSpec>, items: &'a [FixItem]) -> Option<Self> {
        let mut applied = items
            .iter()
            .filter(|item| matches!(item.status, FixStatus::Applied(_)))
            .peekable();
        applied.peek()?;
        Some(match fix {
            // Each removes or moves the file of its finding.
            Some(
                FixSpec::FileRemove { .. } | FixSpec::FileRename { .. } | FixSpec::Relocate { .. },
            ) => applied
                .map(|item| item.violation.path.as_deref())
                .collect::<Option<Vec<_>>>()
                .map_or(Self::Anything, Self::Moved),
            Some(fix) if fix.keeps_paths_in_place() => Self::InPlace,
            _ => Self::Anything,
        })
    }

    /// Let go of what `index`'s relation caches hold that this made untrue.
    /// `moved` says whether a fix earlier in the pass removed or moved a
    /// file, or may have: a create may then have put one back where the
    /// index lists it, after a relation found it gone.
    pub(super) fn let_go(&self, index: &FileIndex, moved: &mut bool) {
        match self {
            Self::InPlace => index.forget_relation_reads(&[], *moved),
            // Without a symlink, the file was reached at its own path alone.
            Self::Moved(paths) if index.followed_no_symlink() => {
                index.forget_relation_reads(paths, true);
                *moved = true;
            }
            Self::Moved(_) | Self::Anything => {
                index.clear_relation_caches();
                *moved = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::extract::Extract;
    use crate::relation::plan::Plan;
    use crate::relation::{ExtractAdapter, Limits, RelPath, Resource, ResourceRead};
    use crate::rule::Violation;

    fn fix(yaml: &str) -> FixSpec {
        crate::yaml::from_str(yaml).unwrap()
    }

    fn item(path: Option<&str>, applied: bool) -> FixItem {
        let violation = Violation::new("finding");
        FixItem {
            violation: match path {
                Some(path) => violation.with_path(Path::new(path)),
                None => violation,
            },
            status: if applied {
                FixStatus::Applied("done".into())
            } else {
                FixStatus::Unfixable
            },
        }
    }

    #[test]
    fn what_landed_is_what_the_fix_op_changes_on_disk() {
        let applied = [item(Some("a.json"), false), item(Some("b.json"), true)];
        let create = fix("file_create:\n  content: x\n");
        let remove = fix("file_remove: {}\n");
        assert_eq!(
            Landed::of(Some(&create), &applied[..1]),
            None,
            "nothing applied"
        );
        assert_eq!(Landed::of(Some(&create), &applied), Some(Landed::InPlace));
        for in_place in [
            "file_append_final_newline: {}\n",
            "sync_from: {}\n",
            "chmod: {}\n",
            "dir_create: {}\n",
        ] {
            assert_eq!(
                Landed::of(Some(&fix(in_place)), &applied),
                Some(Landed::InPlace),
                "{in_place}"
            );
        }
        for moves in ["file_remove: {}\n", "file_rename: {}\n"] {
            assert_eq!(
                Landed::of(Some(&fix(moves)), &applied),
                Some(Landed::Moved(vec![Path::new("b.json")])),
                "{moves}"
            );
        }
        // A move whose finding names no file, a command, `git_untrack`, and
        // a rule built without its spec: the engine cannot say what changed.
        let pathless = [item(None, true)];
        assert_eq!(Landed::of(Some(&remove), &pathless), Some(Landed::Anything));
        for anything in ["command:\n  run: [x]\n", "git_untrack: {}\n"] {
            assert_eq!(
                Landed::of(Some(&fix(anything)), &applied),
                Some(Landed::Anything),
                "{anything}"
            );
        }
        assert_eq!(Landed::of(None, &applied), Some(Landed::Anything));
    }

    /// An index of `a.json`, `d/b.json` and `c.json`, each read and decoded
    /// through its caches, with a read of `gone.json` that found no file,
    /// and its relation memo set.
    fn read_index(dir: &Path, plan: &Arc<Plan>) -> FileIndex {
        for (path, body) in [("a.json", "a"), ("d/b.json", "b"), ("c.json", "c")] {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        let index = crate::walker::walk(dir, &crate::walker::WalkOptions::test_default()).unwrap();
        index.enable_relation_caches();
        read_all(dir, &index);
        index.relation_memo(plan).unwrap();
        index
    }

    /// Read and decode every file of [`read_index`] through `index`'s caches:
    /// how many reads and decodes they have made so far.
    fn read_all(dir: &Path, index: &FileIndex) -> (usize, usize) {
        let (reads, decodes) = (index.read_cache().unwrap(), index.decode_cache().unwrap());
        let adapter = ExtractAdapter::new(Extract::Regex("(.*)".to_string()));
        for path in ["a.json", "d/b.json", "c.json", "gone.json"] {
            let resource = Resource::literal(RelPath::parse(path).unwrap());
            if let ResourceRead::Present(bytes) = reads.read(dir, &resource, 0).as_ref() {
                decodes
                    .decode(&adapter, &resource, bytes, &Limits::default())
                    .unwrap();
            }
        }
        (reads.reads(), decodes.decodes())
    }

    fn plan() -> Arc<Plan> {
        Arc::new(Plan {
            sources: Vec::new(),
            selectors: Vec::new(),
            relations: Vec::new(),
            rules: Vec::new(),
        })
    }

    /// A create keeps every decode and the memo; a later read of a path
    /// that found no file reads it again.
    #[test]
    fn a_create_lets_go_of_the_reads_that_found_no_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let plan = plan();
        let index = read_index(dir.path(), &plan);
        let mut moved = false;
        Landed::InPlace.let_go(&index, &mut moved);
        assert!(index.relation_memo_is_set());
        assert_eq!(read_all(dir.path(), &index), (5, 3));
        assert!(!moved);
        // After a move in the pass, a create may put back a file a relation
        // found gone: the memo goes too.
        Landed::InPlace.let_go(&index, &mut true);
        assert!(!index.relation_memo_is_set());
        assert_eq!(read_all(dir.path(), &index), (6, 3));
    }

    /// A removal or move lets go of the moved file's reads and decodes, and
    /// of those under it, and of the memo; the other files' stay.
    #[test]
    fn a_move_lets_go_of_the_moved_files_and_the_memo() {
        let dir = tempfile::tempdir().unwrap();
        let plan = plan();
        let index = read_index(dir.path(), &plan);
        let mut moved = false;
        Landed::Moved(vec![Path::new("d")]).let_go(&index, &mut moved);
        assert!(!index.relation_memo_is_set());
        assert!(moved);
        // `d/b.json` and `gone.json` are read again, `d/b.json` decoded again.
        assert_eq!(read_all(dir.path(), &index), (6, 4));
    }

    /// The walk followed a symlink, so a removed or moved file may be
    /// reached at another path the index lists too: a move lets go of all.
    #[cfg(unix)]
    #[test]
    fn a_move_where_the_walk_followed_a_symlink_lets_go_of_all() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("e")).unwrap();
        std::os::unix::fs::symlink("e", dir.path().join("link")).unwrap();
        let plan = plan();
        let index = read_index(dir.path(), &plan);
        assert!(!index.followed_no_symlink());
        let mut moved = false;
        Landed::Moved(vec![Path::new("d")]).let_go(&index, &mut moved);
        assert!(!index.relation_memo_is_set());
        assert_eq!(read_all(dir.path(), &index), (8, 6));
    }

    /// A command, or a fix the engine cannot classify, lets go of all.
    #[test]
    fn anything_lets_go_of_every_read_decode_and_the_memo() {
        let dir = tempfile::tempdir().unwrap();
        let plan = plan();
        let index = read_index(dir.path(), &plan);
        let mut moved = false;
        Landed::Anything.let_go(&index, &mut moved);
        assert!(!index.relation_memo_is_set());
        assert!(moved);
        assert_eq!(read_all(dir.path(), &index), (8, 6));
    }
}
