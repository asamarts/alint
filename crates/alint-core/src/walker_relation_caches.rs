//! What the relation caches of an index let go of after a fix changes files,
//! so the rules after it read the tree as it is, while what no fix touched is
//! read and decoded once a pass.

use std::path::Path;

use super::FileIndex;

impl FileIndex {
    /// Let go of what the relation caches hold that a fix may have made
    /// untrue: every read that found no file, since a fix may have created
    /// one there; and the reads and decodes of `moved`, files a fix removed
    /// or moved, with those under them. The relation memo goes when
    /// `forget_memo` asks (its rows may come from a file a fix removed or
    /// moved), or when it read a `scope_filter:` manifest from disk itself (a
    /// create may have made that file). What else they hold stays: the rows
    /// and bytes it took are not taken again.
    pub(crate) fn forget_relation_reads(&self, moved: &[&Path], forget_memo: bool) {
        {
            let mut memo = self.relation_memo.lock().expect("relation memo lock");
            if forget_memo || memo.as_ref().is_some_and(|memo| memo.read_manifests()) {
                *memo = None;
            }
        }
        if let Some(cache) = self.reads.get() {
            for path in moved {
                cache.forget_under(path);
            }
            cache.forget_missing();
        }
        if let Some(cache) = self.decodes.get() {
            for path in moved {
                cache.forget_under(path);
            }
        }
    }

    /// Whether the walk followed no symlink, so a file a fix removes or
    /// moves was reached at its own path and at no other the index lists.
    /// `false` for an index built from entries, which cannot say.
    pub(crate) fn followed_no_symlink(&self) -> bool {
        self.followed_symlinks.as_ref().is_some_and(Vec::is_empty)
    }
}
