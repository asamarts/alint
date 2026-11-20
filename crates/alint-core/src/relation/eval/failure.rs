//! A relation's analysis failures, and the phrases their messages share.

use std::borrow::Cow;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

use super::{Cell, Pointer, Value};
use crate::relation::Segment;
use crate::rule::{AnalysisFailureKind, Violation};

/// Why a relation has no rows: one problem in one resource, or in the
/// relation as a whole (a join limit). Every message starts with the relation
/// and the source that declared it (`relation `deps` (.alint.yml): ...`),
/// names the key path where there is one, and says what to write instead; the
/// resource is [`Self::resource`], the path a report shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationFailure {
    pub kind: AnalysisFailureKind,
    pub resource: Option<Arc<Path>>,
    /// Shared by every relation that reads this one and fails with it.
    pub message: Arc<str>,
    /// The message holds nothing of the relation's configuration or what it
    /// selected, because its configuration reads the environment, nor what
    /// a file whose values are redacted by default says; report it redacted
    /// ([`Self::to_violation`]).
    pub sensitive: bool,
}

impl RelationFailure {
    /// The analysis failure a rule reports for this one, as rule `rule_id`:
    /// redacted as a rule's own analysis failure is when it is sensitive.
    #[must_use]
    pub fn to_violation(&self, rule_id: &str) -> Violation {
        // Each value and key path in it is escaped already; this escapes a
        // path, or a parser's words about the file, that is not.
        let mut violation = Violation::new(escaped(&self.message).into_owned());
        if let Some(resource) = &self.resource {
            violation = violation.with_path(Arc::clone(resource));
        }
        let mut violation = violation.as_analysis_failure(self.kind);
        if self.sensitive {
            violation.redact_message(rule_id);
        }
        violation
    }
}

impl std::fmt::Display for RelationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.resource {
            Some(resource) => write!(
                f,
                "{}: {}",
                escaped(&resource.display().to_string()),
                escaped(&self.message)
            ),
            None => f.write_str(&escaped(&self.message)),
        }
    }
}

/// The failures of one evaluation, at most one per resource: the first
/// problem a resource shows is reported, and the others wait for it to be
/// fixed, so a file with a million bad rows gives one failure, not a million.
///
/// A relation that fails only with the failures of one relation it reads
/// shares that relation's list, so a chain of joins over a failing relation
/// holds its failures once; [`Self::bytes`] is what any other list holds.
#[derive(Debug, Default)]
pub(super) struct Failures {
    list: Vec<RelationFailure>,
    /// The failures of the one relation read, while they are all there is:
    /// then `list` is empty.
    shared: Option<Arc<[RelationFailure]>>,
    seen: std::collections::HashSet<Option<Arc<Path>>>,
    /// What `list` holds: each failure's size, and the text of each one
    /// this relation wrote (a failure read keeps sharing its text).
    bytes: usize,
    /// The failure saying the run's cells or bytes ran out, if one does: the
    /// relation built nothing past it.
    limit: Option<RelationFailure>,
}

impl Failures {
    /// A failure of this relation's own.
    pub(super) fn push(&mut self, failure: RelationFailure) {
        if self.seen.insert(failure.resource.clone()) {
            self.unshare();
            self.bytes = self.bytes.saturating_add(failure.message.len());
            self.add(failure);
        }
    }

    /// A failure saying the run's cells or bytes ran out
    /// ([`Self::into_limit`]).
    pub(super) fn push_limit(&mut self, failure: RelationFailure) {
        self.limit.get_or_insert_with(|| failure.clone());
        self.push(failure);
    }

    /// The failures of a relation this one reads: shared while they are the
    /// only ones, copied (sharing their text) once there are others.
    pub(super) fn inherit(&mut self, theirs: &Arc<[RelationFailure]>) {
        if self.is_empty() {
            self.seen
                .extend(theirs.iter().map(|failure| failure.resource.clone()));
            self.shared = Some(Arc::clone(theirs));
            return;
        }
        for failure in theirs.iter() {
            if self.seen.insert(failure.resource.clone()) {
                self.unshare();
                self.add(failure.clone());
            }
        }
    }

    /// Copy the shared failures into a list of this relation's own, so that
    /// another can join them.
    fn unshare(&mut self) {
        if let Some(shared) = self.shared.take() {
            for failure in shared.iter() {
                self.add(failure.clone());
            }
        }
    }

    fn add(&mut self, failure: RelationFailure) {
        self.bytes = self
            .bytes
            .saturating_add(std::mem::size_of::<RelationFailure>());
        self.list.push(failure);
    }

    pub(super) fn is_empty(&self) -> bool {
        self.list.is_empty() && self.shared.is_none()
    }

    /// The bytes the list holds beyond those it shares: what keeping it takes
    /// from the run's
    /// [`LimitKind::BytesPerRun`](crate::relation::LimitKind::BytesPerRun).
    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }

    /// The failure saying the run's cells or bytes ran out, alone: what the
    /// relation keeps when the run has no bytes left for its list.
    pub(super) fn into_limit(self) -> Option<RelationFailure> {
        self.limit
    }

    pub(super) fn into_shared(self) -> Arc<[RelationFailure]> {
        match self.shared {
            Some(shared) => shared,
            None => self.list.into(),
        }
    }
}

/// The type a comparison message names: `null`, `a boolean`, `a number`, `a
/// string`.
#[must_use]
pub fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) | Value::Path(_) => "a string",
        Value::Bytes(_) => "bytes",
        Value::List(_) => "a list",
        Value::Record(_) => "a record",
    }
}

/// The characters of one key a message shows, as it shows a value's
/// ([`render_value`](super::render_value)): past them, the key is cut and its
/// length named.
const KEY_CHARS: usize = 64;

/// The characters of a key path a message shows: the segment that would
/// take it past them is left out, with every later one, and the path's
/// segments are counted instead. The first segment is always shown.
const KEY_PATH_CHARS: usize = 256;

/// A key path as a message shows it: `` `nav.home` ``, or a stand-in when
/// `hidden`, because a field that holds keys (`entry.key`, `entry.parent`,
/// `entry.root` or `entry.path`) is sensitive in the relation or one it
/// reads, so a key path could be that field's value.
///
/// It is the file's text, so it is cut as a value is: each key at
/// [`KEY_CHARS`] characters (`` `['LLL…'... (1000000 bytes)]` ``) and the
/// path at [`KEY_PATH_CHARS`] (`` `a.b`... (5000 segments) ``), so a message
/// that repeats it stays short however long the key; and a character a
/// string's `Debug` form escapes (a control, a bidi control or a zero-width
/// character) is written escaped, as `\n` or `\u{202e}`, so the key can
/// neither add a line to a report nor reorder what a reader sees.
pub(super) fn key_path(pointer: &Pointer, hidden: bool) -> String {
    if hidden {
        return "a key path not shown (a field holding keys is sensitive)".to_string();
    }
    let segments = pointer.segments();
    let mut out = String::from("`");
    let mut chars = 0;
    for (index, segment) in segments.iter().enumerate() {
        let shown = segment_shown(segment, index == 0);
        let count = shown.chars().count();
        if index > 0 && chars + count > KEY_PATH_CHARS {
            write!(out, "`... ({} segments)", segments.len())
                .expect("writing to a String cannot fail");
            return out;
        }
        chars += count;
        out.push_str(&shown);
    }
    out.push('`');
    out
}

/// One segment of a key path as [`key_path`] shows it: as
/// [`Pointer::render`] writes it, or, for a key it cuts or that holds a
/// character a message escapes, in brackets, `\` and `'` escaped as
/// [`Pointer::render`] escapes them there.
fn segment_shown(segment: &Segment, first: bool) -> String {
    let key = match segment {
        Segment::Key(key) => key,
        Segment::Index(_) => return Pointer::render_segments(std::slice::from_ref(segment)),
    };
    let (kept, cut) = match key.char_indices().nth(KEY_CHARS) {
        Some((end, _)) => (&key[..end], true),
        None => (&key[..], false),
    };
    if !cut && !kept.chars().any(escaped_in_message) {
        let rendered = Pointer::render_segments(std::slice::from_ref(segment));
        return if first || rendered.starts_with('[') {
            rendered
        } else {
            format!(".{rendered}")
        };
    }
    let mut out = String::from("['");
    for c in kept.chars() {
        if matches!(c, '\\' | '\'') {
            out.push('\\');
            out.push(c);
        } else if escaped_in_message(c) {
            out.extend(c.escape_debug());
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    if cut {
        write!(out, "... ({} bytes)", key.len()).expect("writing to a String cannot fail");
    }
    out.push(']');
    out
}

/// Whether a message writes `c` escaped, as a string's `Debug` form does,
/// which is how a message quotes a value ([`render_value`](super::render_value)):
/// a control character (`\n`), a bidi control (`\u{202e}`), a zero-width
/// character and the others that form escapes. Quotes and `\` are not: text
/// shown unquoted keeps them as written.
fn escaped_in_message(c: char) -> bool {
    !matches!(c, '"' | '\'' | '\\') && c.escape_debug().nth(1).is_some()
}

/// `text` (a path, or words about a file) as a message shows it unquoted:
/// each character [`escaped_in_message`] names escaped, the others as they
/// are. Text it escaped already shows the same.
fn escaped(text: &str) -> Cow<'_, str> {
    if !text.chars().any(escaped_in_message) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if escaped_in_message(c) {
            out.extend(c.escape_debug());
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// Where a key path sits: ``at `nav.home` ``, or `at the document root`.
pub(super) fn place(pointer: Option<&Pointer>, hidden: bool) -> String {
    match pointer {
        Some(pointer) if !pointer.segments().is_empty() => {
            format!("at {}", key_path(pointer, hidden))
        }
        _ => "at the document root".to_string(),
    }
}

/// Where a cell sits within its resource, after a space: ``at `nav.home` ``
/// or `at the document root`; nothing for a `read:` value, which has no key
/// path.
pub(super) fn at(cell: &Cell, hidden: bool) -> String {
    match &cell.at.pointer {
        Some(pointer) => format!(" {}", place(Some(pointer), hidden)),
        None => String::new(),
    }
}

/// Where a cell sits, its resource included, for a message that names two
/// rows: `` `nav.home` in locales/en.json ``, or the resource alone.
#[must_use]
pub fn located(cell: &Cell, hidden: bool) -> String {
    let resource = escaped(cell.at.resource.path().as_str());
    match &cell.at.pointer {
        Some(pointer) if !pointer.segments().is_empty() => {
            format!("{} in {resource}", key_path(pointer, hidden))
        }
        _ => resource.into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(keys: &[&str]) -> Pointer {
        Pointer::from_segments(keys.iter().map(|key| Segment::Key((*key).into())).collect())
    }

    #[test]
    fn a_key_path_renders_as_a_pointer_does() {
        let mut pointer = keys(&["nav", "a.b", ""]);
        pointer = pointer.index(2).key("home");
        assert_eq!(key_path(&pointer, false), "`nav['a.b'][''][2].home`");
        assert_eq!(key_path(&pointer, false), format!("`{}`", pointer.render()));
        assert_eq!(
            key_path(&pointer, true),
            "a key path not shown (a field holding keys is sensitive)"
        );
        assert_eq!(place(Some(&Pointer::root()), false), "at the document root");
    }

    /// A key is cut as a value is, and a path at [`KEY_PATH_CHARS`]: a
    /// message that repeats a 1 MB key stays short.
    #[test]
    fn a_long_key_and_a_long_path_are_cut() {
        let long = "L".repeat(1 << 20);
        let shown = key_path(&keys(&[&long, "b"]), false);
        assert_eq!(
            shown,
            format!("`['{}'... ({} bytes)].b`", "L".repeat(KEY_CHARS), 1 << 20)
        );
        // At the bound, a key is shown as it is.
        let bound = "é".repeat(KEY_CHARS);
        assert_eq!(key_path(&keys(&[&bound]), false), format!("`{bound}`"));
        // 10,000 segments: those that fit, then the count.
        let deep: Vec<String> = (0..10_000).map(|index| format!("k{index}")).collect();
        let deep: Vec<&str> = deep.iter().map(String::as_str).collect();
        let shown = key_path(&keys(&deep), false);
        assert!(shown.ends_with("`... (10000 segments)"), "{shown}");
        assert!(shown.starts_with("`k0.k1.k2."), "{shown}");
        assert!(shown.chars().count() <= KEY_PATH_CHARS + 32, "{shown}");
        // A first segment is shown however long, and a later one that
        // would pass the bound is counted instead.
        let cut = format!("['{}'... ({} bytes)]", "L".repeat(KEY_CHARS), 1 << 20);
        let shown = key_path(&keys(&[&long, &long, &long, &long]), false);
        assert_eq!(shown, format!("`{cut}{cut}`... (4 segments)"));
    }

    /// A key's control, bidi and zero-width characters are escaped as a
    /// quoted value's are, inside brackets, where `\` and `'` are escaped
    /// too, so the key adds no line and reorders nothing.
    #[test]
    fn a_key_path_escapes_what_a_quoted_value_escapes() {
        let pointer = keys(&["a\n  related: x", "b\u{202e}c", "d\u{200b}", "it's\\"]);
        let shown = key_path(&pointer, false);
        assert_eq!(
            shown,
            "`['a\\n  related: x']['b\\u{202e}c']['d\\u{200b}']['it\\'s\\\\']`"
        );
        assert!(!shown.chars().any(char::is_control));
        assert!(!shown.contains('\u{202e}'));
        for c in [
            '\n', '\u{1b}', '\u{202e}', '\u{2066}', '\u{200b}', '\u{feff}',
        ] {
            assert!(escaped_in_message(c), "{c:?}");
            assert_eq!(
                format!("{:?}", c.to_string()),
                format!("\"{}\"", c.escape_debug()),
                "escaped as a string's Debug form escapes it"
            );
        }
        for c in ['a', 'é', '"', '\'', '\\', ' '] {
            assert!(!escaped_in_message(c), "{c:?}");
        }
    }

    /// A path in a message, alone or after a key path, and the text of a
    /// relation's analysis failure (a parser's words about a file, a path
    /// another module wrote into it) are escaped as a value is; text escaped
    /// already shows the same.
    #[test]
    fn paths_and_failure_text_are_escaped_in_messages() {
        let name = "d/a\u{1b}[31m\u{202e}.json";
        let shown = "d/a\\u{1b}[31m\\u{202e}.json";
        let cell = |pointer: Option<Pointer>| Cell {
            value: None,
            at: Arc::new(super::super::Provenance {
                resource: crate::relation::Resource::literal(
                    crate::relation::RelPath::from_index_path(Path::new(name)).unwrap(),
                ),
                pointer,
                part: 0,
                ordinal: 0,
                empty: false,
            }),
            span: None,
            path: None,
        };
        assert_eq!(located(&cell(None), false), shown);
        assert_eq!(
            located(&cell(Some(keys(&["k"]))), false),
            format!("`k` in {shown}")
        );
        let failure = RelationFailure {
            kind: AnalysisFailureKind::ResourceParse,
            resource: Some(Arc::from(Path::new(name))),
            message: format!("could not parse {name}: a\nb").into(),
            sensitive: false,
        };
        assert_eq!(
            failure.to_violation("r").message,
            format!("could not parse {shown}: a\\nb")
        );
        assert_eq!(
            failure.to_string(),
            format!("{shown}: could not parse {shown}: a\\nb")
        );
        assert_eq!(escaped(shown), shown, "escaped once");
    }
}
