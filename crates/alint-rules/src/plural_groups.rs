//! CLDR plural groups (`plurals: cldr`), shared by `key_parity` and the
//! `constraint` kind's `set` (design `v0.23/selectors-and-constraints.md`
//! section 6, "CLDR plurals"; D11).
//!
//! A comparison of two key sets, a reference's and a peer's, merges plural
//! groups decided over both sets together:
//!
//! - **nested CLDR objects** (Rails YAML): a non-root object with a leaf
//!   named for a CLDR category in either set, `other` among its members in
//!   at least one, and, in each set that has the object, no member but
//!   category-named leaves. Its members merge into one key at the object's
//!   path;
//! - **i18next siblings:** `<base>_<category>` keys under one parent, with
//!   `<base>_other` in at least one set, a non-empty base, and `<base>`
//!   itself a key in neither. They merge into one key, `<base>`.
//!
//! Nested groups merge first, then i18next ones. A merged key sits at its
//! `other` member, or at its first member in key order when the set has no
//! `other`. Each key carries a location `V` (a byte span for `key_parity`, a
//! row for the `constraint` kind), which a merge takes from that member.
//!
//! [`pair`] decides and merges both sets at once, in time linear in both.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

use alint_core::relation::Segment;

/// A key set in key order: each key path, borrowed where it can be, and its
/// location.
pub(crate) type Keys<'a, V> = BTreeMap<Cow<'a, [Segment]>, V>;

/// The CLDR plural categories, in key order.
const CATEGORIES: [&str; 6] = ["few", "many", "one", "other", "two", "zero"];

pub(crate) fn cldr_category(name: &str) -> bool {
    CATEGORIES.contains(&name)
}

/// Both sides of one comparison, their plural groups merged: the groups are
/// decided over the two sets together, so a group that is partial in either
/// (a locale with fewer categories, or a missing `other`) merges the same
/// way in both, and a base that is a real key in either is never merged.
pub(crate) fn pair<'a, V: Copy>(
    reference: &Keys<'a, V>,
    peer: &Keys<'a, V>,
) -> (Keys<'a, V>, Keys<'a, V>) {
    let nested = nested_plural_groups(&[reference, peer]);
    let (reference, peer) = (
        merge_nested_groups(reference, &nested),
        merge_nested_groups(peer, &nested),
    );
    let i18next = i18next_plural_groups(&[&reference, &peer]);
    (
        merge_i18next_groups(&reference, &i18next),
        merge_i18next_groups(&peer, &i18next),
    )
}

/// The parents merged as nested CLDR plural objects (Rails YAML) in one
/// comparison: a non-root object with a category-named leaf in either file,
/// `other` among its members in at least one, and, in every file that has
/// it, no member but category-named leaves.
pub(crate) fn nested_plural_groups<'k, V>(files: &[&'k Keys<'_, V>]) -> BTreeSet<&'k [Segment]> {
    let mut candidates: BTreeMap<&[Segment], bool> = BTreeMap::new();
    for keys in files {
        for key in keys.keys() {
            if let Some((Segment::Key(name), parent)) = key.split_last()
                && !parent.is_empty()
                && cldr_category(name)
            {
                *candidates.entry(parent).or_default() |= &**name == "other";
            }
        }
    }
    candidates
        .into_iter()
        .filter(|(parent, has_other)| {
            *has_other && files.iter().all(|keys| is_plural_object(keys, parent))
        })
        .map(|(parent, _)| parent)
        .collect()
}

/// Whether every key below `parent` is a leaf child named for a CLDR category
/// (vacuously, when the file has no key below it). Keys that share a prefix
/// are contiguous in the map, so this visits only `parent`'s own keys and
/// stops at the first that disqualifies it.
fn is_plural_object<V>(keys: &Keys<'_, V>, parent: &[Segment]) -> bool {
    keys.range::<[Segment], _>((Bound::Included(parent), Bound::Unbounded))
        .take_while(|(key, _)| key.starts_with(parent))
        .all(|(key, _)| {
            key.len() == parent.len() + 1
                && matches!(&key[parent.len()], Segment::Key(name) if cldr_category(name))
        })
}

/// Replace each group's members with one key at its parent, located at the
/// file's `other` member, or at its first member when it has no `other`.
pub(crate) fn merge_nested_groups<'a, V: Copy>(
    keys: &Keys<'a, V>,
    groups: &BTreeSet<&[Segment]>,
) -> Keys<'a, V> {
    let mut out = Keys::new();
    let mut merged = Keys::new();
    for (key, at) in keys {
        match key.split_last() {
            Some((Segment::Key(name), parent)) if groups.contains(parent) => {
                let place = merged.entry(parent_key(key)).or_insert(*at);
                if &**name == "other" {
                    *place = *at;
                }
            }
            _ => {
                out.insert(key.clone(), *at);
            }
        }
    }
    out.extend(merged);
    out
}

/// The key of the object that holds `key`, borrowed when `key` is.
fn parent_key<'a>(key: &Cow<'a, [Segment]>) -> Cow<'a, [Segment]> {
    match key {
        Cow::Borrowed(path) => {
            let path: &'a [Segment] = path;
            Cow::Borrowed(&path[..path.len().saturating_sub(1)])
        }
        Cow::Owned(path) => Cow::Owned(path[..path.len().saturating_sub(1)].to_vec()),
    }
}

/// The `(parent, base)` i18next plural groups (`<base>_<category>` siblings)
/// merged in one comparison: `<base>_other` in at least one file, a non-empty
/// base, and `<base>` itself a key in neither, so a merge never hides a key.
pub(crate) fn i18next_plural_groups<'k, V>(
    files: &[&'k Keys<'_, V>],
) -> BTreeSet<(&'k [Segment], &'k str)> {
    let mut candidates: BTreeMap<(&[Segment], &str), bool> = BTreeMap::new();
    for keys in files {
        for key in keys.keys() {
            if let Some((parent, base, category)) = i18next_member(key) {
                *candidates.entry((parent, base)).or_default() |= category == "other";
            }
        }
    }
    candidates
        .into_iter()
        .filter(|((parent, base), has_other)| {
            *has_other && {
                let merged = child(parent, base);
                files
                    .iter()
                    .all(|keys| !keys.contains_key(merged.as_slice()))
            }
        })
        .map(|(group, _)| group)
        .collect()
}

/// A key's i18next plural parts: its parent, base, and CLDR category suffix.
fn i18next_member(key: &[Segment]) -> Option<(&[Segment], &str, &str)> {
    let (Segment::Key(last), parent) = key.split_last()? else {
        return None;
    };
    let (base, category) = last.rsplit_once('_')?;
    (!base.is_empty() && cldr_category(category)).then_some((parent, base, category))
}

/// Replace each i18next group's members with one `<base>` key, located like
/// [`merge_nested_groups`] does. No file holds that key's path, so it is
/// built, once per group.
pub(crate) fn merge_i18next_groups<'a, V: Copy>(
    keys: &Keys<'a, V>,
    groups: &BTreeSet<(&[Segment], &str)>,
) -> Keys<'a, V> {
    let mut out = Keys::new();
    let mut merged: BTreeMap<(&[Segment], &str), V> = BTreeMap::new();
    for (key, at) in keys {
        if let Some((parent, base, category)) = i18next_member(key)
            && groups.contains(&(parent, base))
        {
            let place = merged.entry((parent, base)).or_insert(*at);
            if category == "other" {
                *place = *at;
            }
        } else {
            out.insert(key.clone(), *at);
        }
    }
    out.extend(
        merged
            .into_iter()
            .map(|((parent, base), at)| (Cow::Owned(child(parent, base)), at)),
    );
    out
}

/// `parent`'s child key `name`.
fn child(parent: &[Segment], name: &str) -> Vec<Segment> {
    let mut path = Vec::with_capacity(parent.len() + 1);
    path.extend_from_slice(parent);
    path.push(Segment::Key(name.into()));
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(text: &str) -> Vec<Segment> {
        text.split('.')
            .map(|part| Segment::Key(part.into()))
            .collect()
    }

    fn keys(paths: &[&str]) -> Keys<'static, usize> {
        paths
            .iter()
            .enumerate()
            .map(|(index, text)| (Cow::Owned(path(text)), index))
            .collect()
    }

    fn shown(keys: &Keys<'_, usize>) -> Vec<(String, usize)> {
        keys.iter()
            .map(|(key, at)| {
                let parts: Vec<&str> = key
                    .iter()
                    .map(|segment| match segment {
                        Segment::Key(name) => &**name,
                        Segment::Index(_) => "#",
                    })
                    .collect();
                (parts.join("."), *at)
            })
            .collect()
    }

    #[test]
    fn a_pair_merges_groups_decided_over_both_sets() {
        // `item` is a plural object in both; `msg_*` merges, `other` in the
        // reference only; `count_one` stays, since `count` is a key.
        let reference = keys(&["item.one", "item.other", "msg_one", "msg_other", "count"]);
        let peer = keys(&["item.few", "msg_few", "count_one"]);
        let (reference, peer) = pair(&reference, &peer);
        assert_eq!(
            shown(&reference),
            [("count".into(), 4), ("item".into(), 1), ("msg".into(), 3)]
        );
        assert_eq!(
            shown(&peer),
            [
                ("count_one".into(), 2),
                ("item".into(), 0),
                ("msg".into(), 1)
            ]
        );
    }

}
