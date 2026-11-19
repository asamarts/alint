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
//! [`Probe`] makes the same decisions on demand, for a peer far smaller than
//! the reference: what a peer holds once merged, and whether the merged
//! reference holds a key, cost time linear in the peer, however large the
//! reference. A test checks the two against each other.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
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

/// One side of a [`Probe`]'s comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Reference,
    Peer,
}

/// One comparison of a reference's keys with a peer's, its plural groups
/// decided on demand as [`pair`] decides them, each decision once. What the
/// peer holds once merged ([`Probe::merged`]) and whether the merged
/// reference holds a key ([`Probe::find`]) look up only the keys around the
/// one asked about, so a comparison with a peer of a few keys costs a few
/// lookups however many keys the reference has.
pub(crate) struct Probe<'k, 'a, V> {
    reference: &'k Keys<'a, V>,
    peer: &'k Keys<'a, V>,
    /// Whether each parent asked about is a merged nested group.
    nested: HashMap<Vec<Segment>, bool>,
    /// Whether each `(parent, base)` asked about is a merged i18next group.
    i18next: HashMap<(Vec<Segment>, Box<str>), bool>,
}

impl<'k, 'a, V: Copy> Probe<'k, 'a, V> {
    pub(crate) fn new(reference: &'k Keys<'a, V>, peer: &'k Keys<'a, V>) -> Self {
        Self {
            reference,
            peer,
            nested: HashMap::new(),
            i18next: HashMap::new(),
        }
    }

    fn keys(&self, side: Side) -> &'k Keys<'a, V> {
        match side {
            Side::Reference => self.reference,
            Side::Peer => self.peer,
        }
    }

    /// Whether `parent` is a merged nested group: [`nested_plural_groups`]
    /// of the two sets holds it.
    fn nested(&mut self, parent: &[Segment]) -> bool {
        if parent.is_empty() {
            return false;
        }
        if let Some(&decided) = self.nested.get(parent) {
            return decided;
        }
        let other = child(parent, "other");
        let decided = (self.reference.contains_key(other.as_slice())
            || self.peer.contains_key(other.as_slice()))
            && is_plural_object(self.reference, parent)
            && is_plural_object(self.peer, parent);
        self.nested.insert(parent.to_vec(), decided);
        decided
    }

    /// Where `side` holds `key` once nested groups merge: its own place, the
    /// place of the group it names, or `None` (absent, or merged into its
    /// parent).
    fn after_nested(&mut self, side: Side, key: &[Segment]) -> Option<V> {
        let keys = self.keys(side);
        if let Some(&at) = keys.get(key) {
            // A merged parent has only category-named leaves (in each set
            // that has it), so a key of another name is never merged.
            let merged = match key.split_last() {
                Some((Segment::Key(name), parent)) => cldr_category(name) && self.nested(parent),
                _ => false,
            };
            return (!merged).then_some(at);
        }
        if !self.nested(key) {
            return None;
        }
        // A merged group's keys below it are all its members.
        let mut first = None;
        for (member, &at) in keys
            .range::<[Segment], _>((Bound::Excluded(key), Bound::Unbounded))
            .take_while(|(member, _)| member.starts_with(key))
        {
            if matches!(member.last(), Some(Segment::Key(name)) if &**name == "other") {
                return Some(at);
            }
            first.get_or_insert(at);
        }
        first
    }

    /// Whether `(parent, base)` is a merged i18next group:
    /// [`i18next_plural_groups`] of the two sets, nested groups merged,
    /// holds it.
    fn i18next(&mut self, parent: &[Segment], base: &str) -> bool {
        if base.is_empty() {
            return false;
        }
        let memo = (parent.to_vec(), Box::<str>::from(base));
        if let Some(&decided) = self.i18next.get(&memo) {
            return decided;
        }
        let other = child(parent, &format!("{base}_other"));
        let merged = child(parent, base);
        let decided = (self.after_nested(Side::Reference, &other).is_some()
            || self.after_nested(Side::Peer, &other).is_some())
            && self.after_nested(Side::Reference, &merged).is_none()
            && self.after_nested(Side::Peer, &merged).is_none();
        self.i18next.insert(memo, decided);
        decided
    }

    /// Where `side`, both kinds of group merged, holds `key`: what [`pair`]
    /// gives that side holds `key` there.
    pub(crate) fn find(&mut self, side: Side, key: &[Segment]) -> Option<V> {
        if let Some((Segment::Key(last), parent)) = key.split_last()
            && self.i18next(parent, last)
        {
            let mut first = None;
            for category in CATEGORIES {
                let member = child(parent, &format!("{last}_{category}"));
                if let Some(at) = self.after_nested(side, &member) {
                    if category == "other" {
                        return Some(at);
                    }
                    first.get_or_insert(at);
                }
            }
            return first;
        }
        let at = self.after_nested(side, key)?;
        match i18next_member(key) {
            Some((parent, base, _)) if self.i18next(parent, base) => None,
            _ => Some(at),
        }
    }

    /// What `side` holds, both kinds of group merged: what [`pair`] gives
    /// that side. Each decision it takes is about a key of `side`.
    pub(crate) fn merged(&mut self, side: Side) -> Keys<'a, V> {
        let keys = self.keys(side);
        let mut nested = BTreeSet::new();
        for key in keys.keys() {
            if let Some((Segment::Key(name), parent)) = key.split_last()
                && cldr_category(name)
                && self.nested(parent)
            {
                nested.insert(parent);
            }
        }
        let after = merge_nested_groups(keys, &nested);
        let mut groups = BTreeSet::new();
        for key in after.keys() {
            if let Some((parent, base, _)) = i18next_member(key)
                && self.i18next(parent, base)
            {
                groups.insert((parent, base));
            }
        }
        merge_i18next_groups(&after, &groups)
    }
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

    /// A small deterministic generator, so the cases are the same each run.
    struct Cases(u64);

    impl Cases {
        fn next(&mut self, below: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            usize::try_from(self.0 % below as u64).expect("below fits")
        }

        /// A key path from a small alphabet that forms plural groups often.
        fn key(&mut self) -> String {
            const NAMES: [&str; 12] = [
                "a",
                "b",
                "one",
                "other",
                "few",
                "zero",
                "a_one",
                "a_other",
                "b_few",
                "x",
                "x_other",
                "one_other",
            ];
            let depth = 1 + self.next(3);
            (0..depth)
                .map(|_| NAMES[self.next(NAMES.len())])
                .collect::<Vec<_>>()
                .join(".")
        }

        fn keys(&mut self) -> Keys<'static, usize> {
            let count = self.next(9);
            (0..count)
                .map(|index| (Cow::Owned(path(&self.key())), index))
                .collect()
        }
    }

    /// The probe decides as [`pair`] does: the merged peer it builds, and
    /// every key it finds or does not find on either side, over 5,000
    /// random pairs.
    #[test]
    fn a_probe_decides_as_a_pair_does() {
        let mut cases = Cases(0x9e37_79b9_7f4a_7c15);
        for _ in 0..5_000 {
            let (reference, peer) = (cases.keys(), cases.keys());
            let (want_reference, want_peer) = pair(&reference, &peer);
            let mut probe = Probe::new(&reference, &peer);
            assert_eq!(
                shown(&probe.merged(Side::Peer)),
                shown(&want_peer),
                "{:?} / {:?}",
                shown(&reference),
                shown(&peer)
            );
            assert_eq!(
                shown(&probe.merged(Side::Reference)),
                shown(&want_reference)
            );
            let asked: Vec<Vec<Segment>> = want_reference
                .keys()
                .chain(want_peer.keys())
                .chain(reference.keys())
                .chain(peer.keys())
                .map(|key| key.to_vec())
                .chain((0..4).map(|_| path(&cases.key())))
                .collect();
            for key in &asked {
                for (side, want) in [(Side::Reference, &want_reference), (Side::Peer, &want_peer)] {
                    assert_eq!(
                        probe.find(side, key),
                        want.get(key.as_slice()).copied(),
                        "{side:?} {key:?} in {:?} / {:?}",
                        shown(&reference),
                        shown(&peer)
                    );
                }
            }
        }
    }
}
