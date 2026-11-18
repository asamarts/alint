//! The structural pointer inside a resource: one internal type for what
//! `structured_fix::PathSeg` and `extract::EntryPathSegment` each describe.

use std::fmt::Write as _;

use crate::extract::{EntryPathSegment, StructuredEntryRow};
use crate::structured_fix::PathSeg;

/// One step of a [`Pointer`]: an object key or an array index.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Segment {
    Key(Box<str>),
    Index(usize),
}

/// The location of a value inside its resource, as a list of segments. Two
/// pointers are the same key only when their segment lists are equal, so
/// `{"nav.home": 1}` and `{"nav": {"home": 1}}` are different keys.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pointer(Vec<Segment>);

impl Pointer {
    #[must_use]
    pub fn root() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn from_segments(segments: Vec<Segment>) -> Self {
        Self(segments)
    }

    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.0
    }

    #[must_use]
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// This pointer extended by an object key.
    #[must_use]
    pub fn key(&self, key: &str) -> Self {
        let mut segments = self.0.clone();
        segments.push(Segment::Key(key.into()));
        Self(segments)
    }

    /// This pointer extended by an array index.
    #[must_use]
    pub fn index(&self, index: usize) -> Self {
        let mut segments = self.0.clone();
        segments.push(Segment::Index(index));
        Self(segments)
    }

    /// The one rendering messages, key globs, and baseline keys use: keys
    /// joined by `.`, indices as `[n]`, and bracket notation (`nav['a.b']`) for
    /// a key that is empty or contains `.`, `[`, `]`, or a quote. Inside
    /// brackets `\` and `'` are escaped, so distinct pointers never render the
    /// same.
    #[must_use]
    pub fn render(&self) -> String {
        Self::render_segments(&self.0)
    }

    /// [`render`](Self::render) of the pointer `segments` make, without
    /// building it.
    #[must_use]
    pub fn render_segments(segments: &[Segment]) -> String {
        let mut out = String::new();
        for segment in segments {
            match segment {
                Segment::Index(index) => {
                    let _ = write!(out, "[{index}]");
                }
                Segment::Key(key) if needs_brackets(key) => {
                    out.push_str("['");
                    for c in key.chars() {
                        if matches!(c, '\\' | '\'') {
                            out.push('\\');
                        }
                        out.push(c);
                    }
                    out.push_str("']");
                }
                Segment::Key(key) => {
                    if !out.is_empty() {
                        out.push('.');
                    }
                    out.push_str(key);
                }
            }
        }
        out
    }

    /// The bytes the pointer holds beyond its own size: its segments and
    /// their keys' text.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.0.iter().fold(
            self.0.len() * std::mem::size_of::<Segment>(),
            |bytes, segment| match segment {
                Segment::Key(key) => bytes.saturating_add(key.len()),
                Segment::Index(_) => bytes,
            },
        )
    }

    /// The `structured_fix` form, for span resolution.
    #[must_use]
    pub fn to_path_segs(&self) -> Vec<PathSeg> {
        self.0
            .iter()
            .map(|segment| match segment {
                Segment::Key(key) => PathSeg::Key(key.to_string()),
                Segment::Index(index) => PathSeg::Index(*index),
            })
            .collect()
    }
}

fn needs_brackets(key: &str) -> bool {
    key.is_empty() || key.contains(['.', '[', ']', '\'', '"'])
}

impl From<&[PathSeg]> for Pointer {
    fn from(path: &[PathSeg]) -> Self {
        Self(
            path.iter()
                .map(|segment| match segment {
                    PathSeg::Key(key) => Segment::Key(key.as_str().into()),
                    PathSeg::Index(index) => Segment::Index(*index),
                })
                .collect(),
        )
    }
}

impl From<&[EntryPathSegment]> for Pointer {
    fn from(path: &[EntryPathSegment]) -> Self {
        Self(
            path.iter()
                .map(|segment| match segment {
                    EntryPathSegment::Key(key) => Segment::Key(key.as_str().into()),
                    EntryPathSegment::Index(index) => Segment::Index(*index),
                })
                .collect(),
        )
    }
}

impl From<&StructuredEntryRow> for Pointer {
    /// The entry's own location: its parent path plus its key.
    fn from(row: &StructuredEntryRow) -> Self {
        Self::from(row.parent.as_slice()).key(&row.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::ByteSpan;
    use proptest::prelude::*;

    #[test]
    fn renders_dots_indices_and_brackets() {
        let pointer = Pointer::root().key("nav").key("home");
        assert_eq!(pointer.render(), "nav.home");
        assert_eq!(Pointer::root().key("steps").index(2).render(), "steps[2]");
        assert_eq!(Pointer::root().key("nav").key("a.b").render(), "nav['a.b']");
        assert_eq!(Pointer::root().key("a.b").key("c").render(), "['a.b'].c");
        assert_eq!(Pointer::root().key("").render(), "['']");
        assert_eq!(Pointer::root().key("it's").render(), r"['it\'s']");
        assert_eq!(Pointer::root().render(), "");
        assert_eq!(
            Pointer::render_segments(pointer.segments()),
            pointer.render()
        );
        assert_ne!(
            Pointer::root().key("nav.home").render(),
            Pointer::root().key("nav").key("home").render()
        );
        assert_ne!(
            Pointer::root().key("0").render(),
            Pointer::root().index(0).render()
        );
    }

    #[test]
    fn converts_from_both_public_pointer_types() {
        let fix = [PathSeg::Key("a".into()), PathSeg::Index(1)];
        let entry = [
            EntryPathSegment::Key("a".into()),
            EntryPathSegment::Index(1),
        ];
        let expected = Pointer::root().key("a").index(1);
        assert_eq!(Pointer::from(&fix[..]), expected);
        assert_eq!(Pointer::from(&entry[..]), expected);
        assert_eq!(expected.to_path_segs(), fix);

        let row = StructuredEntryRow {
            parent: vec![EntryPathSegment::Key("nav".into())],
            key: "home".into(),
            key_span: ByteSpan::new(0, 4),
            value_span: None,
        };
        assert_eq!(Pointer::from(&row), Pointer::root().key("nav").key("home"));
    }

    fn segment() -> impl Strategy<Value = Segment> {
        prop_oneof![
            "[a.'\\[\\]\"\\\\ ]{0,4}".prop_map(|key| Segment::Key(key.into())),
            (0usize..3).prop_map(Segment::Index),
        ]
    }

    proptest! {
        #[test]
        fn rendering_is_injective(
            a in proptest::collection::vec(segment(), 0..4),
            b in proptest::collection::vec(segment(), 0..4),
        ) {
            let (a, b) = (Pointer::from_segments(a), Pointer::from_segments(b));
            prop_assert_eq!(a == b, a.render() == b.render(), "{:?} vs {:?}", a, b);
        }
    }
}
