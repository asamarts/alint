//! A relation's analysis failures, and the phrases their messages share.

use std::path::Path;
use std::sync::Arc;

use super::{Cell, Pointer, Value};
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
    /// selected, because its configuration reads the environment; report it
    /// redacted ([`Self::to_violation`]).
    pub sensitive: bool,
}

impl RelationFailure {
    /// The analysis failure a rule reports for this one, as rule `rule_id`:
    /// redacted as a rule's own analysis failure is when it is sensitive.
    #[must_use]
    pub fn to_violation(&self, rule_id: &str) -> Violation {
        let mut violation = Violation::new(self.message.to_string());
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
            Some(resource) => write!(f, "{}: {}", resource.display(), self.message),
            None => f.write_str(&self.message),
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
pub(super) fn type_name(value: &Value) -> &'static str {
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

/// A key path as a message shows it: `` `nav.home` ``, or a stand-in when
/// `hidden`, because a field that holds keys (`entry.key`, `entry.parent`,
/// `entry.root` or `entry.path`) is sensitive in the relation or one it
/// reads, so a key path could be that field's value.
pub(super) fn key_path(pointer: &Pointer, hidden: bool) -> String {
    if hidden {
        "a key path not shown (a field holding keys is sensitive)".to_string()
    } else {
        format!("`{}`", pointer.render())
    }
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
pub(super) fn located(cell: &Cell, hidden: bool) -> String {
    let resource = cell.at.resource.path().as_str();
    match &cell.at.pointer {
        Some(pointer) if !pointer.segments().is_empty() => {
            format!("{} in {resource}", key_path(pointer, hidden))
        }
        _ => resource.to_string(),
    }
}
