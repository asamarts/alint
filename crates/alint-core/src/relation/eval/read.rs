//! A relation's parts: the resources a selector picks, read and decoded
//! through the run's caches, and the rows of each, one cell per field.

use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;

use super::failure::{Failures, place};
use super::{Cell, Evaluator, FieldInfo, Provenance, RelationNote, RelationRows, Site};
use crate::extract::{Extract, LinesOpts};
use crate::relation::adapter::{Adapter, AdapterError, AdapterNote, AdapterOutput};
use crate::relation::adapters::{EntryValuesAdapter, ExtractAdapter, entry_values};
use crate::relation::limits::LimitExceeded;
use crate::relation::path::RelPath;
use crate::relation::plan::{
    Entries, EntriesFormat, FieldDef, FieldSource, NamedRelation, Part, PathToken, Read, Reader,
    RelationDef, SelectorRef,
};
use crate::relation::pointer::{Pointer, Segment};
use crate::relation::resource::{Origin, Resource, ResourceId, ResourceRead};
use crate::relation::row::{Row, RowState};
use crate::relation::value::Value;
use crate::rule::AnalysisFailureKind;
use crate::scope::Scope;
use crate::scope_filter::{ManifestFailure, ManifestPredicate, ManifestSet, ScopeFilter};
use crate::structured_format::Format;
use crate::template::PathTokens;

/// A manifest-derived predicate's set, resolved by the evaluation.
pub(crate) type ManifestOutcome = Arc<Result<ManifestSet, ManifestFailure>>;

/// A manifest set the engine resolved for a rule, or one this run's
/// evaluations resolved.
enum SetRef<'a> {
    Engine(&'a ManifestSet),
    Memo(ManifestOutcome),
}

impl SetRef<'_> {
    fn contains_file(&self, file: &Path) -> bool {
        match self {
            Self::Engine(set) => set.contains_file(file),
            Self::Memo(outcome) => outcome
                .as_ref()
                .as_ref()
                .is_ok_and(|set| set.contains_file(file)),
        }
    }
}

impl Evaluator<'_> {
    /// The rows of `parts`, in the order written.
    pub(super) fn parts(
        &self,
        site: &Site,
        named: &NamedRelation,
        parts: &[Part],
        failures: &mut Failures,
    ) -> RelationRows {
        let relation = &named.relation;
        let mut out = RelationRows {
            fields: relation
                .fields
                .iter()
                .map(|(name, field)| FieldInfo {
                    name: name.clone(),
                    sensitive: field.sensitive,
                })
                .collect(),
            ..RelationRows::default()
        };
        for part in parts {
            if self
                .part(site, relation, part, &mut out, failures)
                .is_break()
            {
                break;
            }
        }
        out
    }

    /// One part's rows, added to `out`; `Break` once the run's cells ran
    /// out, so no later part builds a row.
    fn part(
        &self,
        site: &Site,
        relation: &RelationDef,
        part: &Part,
        out: &mut RelationRows,
        failures: &mut Failures,
    ) -> ControlFlow<()> {
        let Some(resources) = self.select(site, &part.from, failures) else {
            return ControlFlow::Continue(());
        };
        let bound: Vec<String> = bound_queries(relation);
        let reader = match PartReader::new(&part.reader, &bound) {
            Ok(reader) => reader,
            Err(message) => {
                failures.push(site.failure(AnalysisFailureKind::Evaluation, None, message));
                return ControlFlow::Continue(());
            }
        };
        let take_cells = |cells| self.take_cells(cells);
        for resource in resources {
            let failure_at = || -> Option<Arc<Path>> { Some(Arc::from(resource.path().as_path())) };
            let cells_exhausted = |limit: &LimitExceeded| {
                let detail = super::cells_exhausted(limit);
                site.failure(AnalysisFailureKind::Evaluation, failure_at(), detail)
            };
            if let Err(limit) = self.take_cells(1) {
                failures.push(cells_exhausted(&limit));
                return ControlFlow::Break(());
            }
            out.resources.push(resource.clone());
            let Some(bytes) = self.bytes(site, &resource, failures) else {
                continue;
            };
            let decoded = self
                .decodes
                .decode(reader.adapter(), &resource, &bytes, &self.limits);
            let output = match decoded {
                Ok(output) => output,
                Err(error) => {
                    failures.push(decode_failure(site, &reader, failure_at(), &error));
                    continue;
                }
            };
            // A `read:` of a structured file decodes its `bound:` paths once
            // more, as an `entries:` part with no `at:` would.
            let bound_rows = match &reader {
                PartReader::Read {
                    bound: Some(adapter),
                    ..
                } => match self
                    .decodes
                    .decode(adapter, &resource, &bytes, &self.limits)
                {
                    Ok(output) => Some(output),
                    Err(error) => {
                        failures.push(decode_failure(site, &reader, failure_at(), &error));
                        continue;
                    }
                },
                _ => None,
            };
            let rows = Rows {
                hide_keys: site.keys_sensitive,
                relation,
                resource: &resource,
                tokens: PathTokens::from_path(resource.path().as_path()),
                take_cells: &take_cells,
            };
            let result = match &reader {
                PartReader::Entries(_) => rows.entries(&output, out),
                PartReader::Read {
                    read,
                    optional,
                    bound,
                    ..
                } => rows.read(
                    read,
                    *optional,
                    bound
                        .as_ref()
                        .map(|_| bound_rows.as_deref().unwrap_or(&output)),
                    &output,
                    out,
                ),
            };
            match result {
                Ok(()) => {}
                Err(Stop::Detail(detail)) => {
                    let kind = AnalysisFailureKind::Evaluation;
                    failures.push(site.failure(kind, failure_at(), detail));
                }
                Err(Stop::Cells(limit)) => {
                    failures.push(cells_exhausted(&limit));
                    return ControlFlow::Break(());
                }
            }
        }
        ControlFlow::Continue(())
    }

    /// The resources `from` picks, in path order, as a rule's `paths:` and
    /// `scope_filter:` would pick them; `None` when they cannot be known.
    fn select(
        &self,
        site: &Site,
        from: &SelectorRef,
        failures: &mut Failures,
    ) -> Option<Vec<Resource>> {
        let (selector, name) = match from {
            SelectorRef::Named(reached) => {
                let named = self.plan.selector(reached.id);
                (&named.selector, format!("selector `{}`", named.name))
            }
            SelectorRef::Inline(selector) => (selector.as_ref(), "the selector".to_string()),
        };
        let compiled = Scope::from_paths_spec(&selector.paths).and_then(|scope| {
            match &selector.scope_filter {
                Some(spec) => {
                    Ok(scope.with_scope_filter(ScopeFilter::from_spec(&name, spec.clone())?))
                }
                None => Ok(scope),
            }
        });
        let scope = match compiled {
            Ok(scope) => scope,
            Err(error) => {
                failures.push(site.failure(
                    AnalysisFailureKind::Evaluation,
                    None,
                    format!("{name} does not compile: {error}"),
                ));
                return None;
            }
        };
        if scope
            .scope_filter()
            .and_then(ScopeFilter::changed_since)
            .is_some()
        {
            // The loader refuses it (design section 4); a plan built some
            // other way gets no rows that depend on the diff.
            failures.push(site.failure(
                AnalysisFailureKind::Evaluation,
                None,
                format!(
                    "{name} uses `changed_since`, so what the relation reads would depend on \
                     the diff; drop it from the selector"
                ),
            ));
            return None;
        }
        let mut sets = Vec::new();
        for predicate in scope
            .scope_filter()
            .map_or(&[][..], ScopeFilter::manifest_predicates)
        {
            match self.manifest_set(predicate) {
                Ok(set) => sets.push((predicate, set)),
                Err(failure) => {
                    let source = predicate.source();
                    failures.push(site.failure(
                        failure.kind,
                        Some(Arc::from(source)),
                        format!(
                            "{name}'s `scope_filter:` reads `{source}`: {}; fix `{source}`, or \
                             change the selector's `scope_filter:`",
                            failure.message,
                            source = source.display(),
                        ),
                    ));
                    return None;
                }
            }
        }
        let mut resources = Vec::new();
        for (position, entry) in self.index.entries.iter().enumerate() {
            if entry.is_dir
                || !scope.matches_without_manifest(&entry.path, self.index)
                || !sets
                    .iter()
                    .all(|(predicate, set)| predicate.keeps(set.contains_file(&entry.path)))
            {
                continue;
            }
            match (
                u32::try_from(position).ok(),
                RelPath::from_index_path(&entry.path),
            ) {
                (Some(position), Some(path)) => resources.push(Resource::from_parts(
                    Origin::Index(ResourceId::from_index(position)),
                    path,
                )),
                _ => failures.push(site.failure(
                    AnalysisFailureKind::ResourceRead,
                    Some(Arc::clone(&entry.path)),
                    "the file's path is not valid UTF-8, so a relation cannot read it; rename \
                     it, or leave it out of the selector",
                )),
            }
        }
        resources.sort_by(|a, b| a.path().cmp(b.path()));
        Some(resources)
    }

    /// The set a manifest-derived predicate keeps or drops: the one the
    /// engine resolved for a rule's scope, or one resolved here, once per run.
    fn manifest_set<'s>(
        &'s self,
        predicate: &ManifestPredicate,
    ) -> Result<SetRef<'s>, ManifestFailure> {
        let key = predicate.cache_key();
        if let Some(failure) = self.index.manifest_failure(key) {
            return Err(failure.clone());
        }
        if let Some(set) = self.index.manifest_paths(key) {
            return Ok(SetRef::Engine(set));
        }
        let outcome = {
            let mut sets = self.memo.manifests.lock().expect("manifest set lock");
            Arc::clone(sets.entry(key.to_string()).or_insert_with(|| {
                Arc::new(
                    crate::scope_filter::read_manifest_confined(self.root, predicate.source())
                        .and_then(|text| predicate.resolve_set(&text))
                        .map(ManifestSet::from_paths),
                )
            }))
        };
        match outcome.as_ref() {
            Ok(_) => Ok(SetRef::Memo(outcome)),
            Err(failure) => Err(failure.clone()),
        }
    }

    /// `resource`'s bytes, read as `cross_file` reads a file its glob
    /// matched: as a literal path, which the read cache resolves to keep it
    /// inside the root and out of a Git directory, and opens only when it is
    /// a regular file (a FIFO would block the read, and a device never ends).
    /// The walk's word on a file is not taken: it may have been swapped since
    /// for a symlink into `.git` or a FIFO.
    fn bytes(
        &self,
        site: &Site,
        resource: &Resource,
        failures: &mut Failures,
    ) -> Option<Arc<[u8]>> {
        let relative = resource.path().as_path();
        let literal = Resource::literal(resource.path().clone());
        let read = Arc::unwrap_or_clone(self.reads.read_unretained(self.root, &literal, 0));
        let detail = match read {
            ResourceRead::Present(bytes) => return Some(bytes),
            ResourceRead::Missing => "file vanished after repository discovery".to_string(),
            ResourceRead::TooLarge(length) => format!(
                "file is too large to analyze ({length} bytes; {} MiB cap)",
                crate::walker::MAX_ANALYZE_BYTES / (1024 * 1024)
            ),
            ResourceRead::Unreadable(error) => format!("could not read file: {error}"),
        };
        failures.push(site.failure(
            AnalysisFailureKind::ResourceRead,
            Some(Arc::from(relative)),
            detail,
        ));
        None
    }
}

/// The `bound:` paths `relation`'s fields read, each once.
fn bound_queries(relation: &RelationDef) -> Vec<String> {
    let mut queries: Vec<String> = Vec::new();
    for (_, field) in &relation.fields {
        if let FieldSource::Bound(query) = &field.source
            && !queries.contains(query)
        {
            queries.push(query.clone());
        }
    }
    queries
}

/// How one part decodes a resource.
enum PartReader {
    Entries(EntryValuesAdapter),
    Read {
        adapter: ExtractAdapter,
        read: Read,
        optional: bool,
        /// The `bound:` paths, for a structured `read:` whose fields have
        /// some.
        bound: Option<EntryValuesAdapter>,
    },
}

impl PartReader {
    fn new(reader: &Reader, bound: &[String]) -> Result<Self, String> {
        match reader {
            Reader::Entries(Entries { format, at, leaves }) => {
                EntryValuesAdapter::new(entries_format(*format), at, *leaves, bound)
                    .map(Self::Entries)
                    .map_err(|error| error.to_string())
            }
            Reader::Read { read, optional } => {
                let extract = match read {
                    Read::Json(query) => Extract::structured(Format::Json, query)?,
                    Read::Yaml(query) => Extract::structured(Format::Yaml, query)?,
                    Read::Toml(query) => Extract::structured(Format::Toml, query)?,
                    Read::Regex(pattern) => Extract::Regex(pattern.clone()),
                    Read::Lines { comment } => Extract::Lines(LinesOpts {
                        comment: comment.clone().unwrap_or_else(|| "#".to_string()),
                    }),
                    Read::WholeFile => Extract::WholeFile,
                };
                let bound = match (read_format(read), bound.is_empty()) {
                    (_, true) => None,
                    (Some(format), false) => Some(
                        EntryValuesAdapter::new(format, &[], false, bound)
                            .map_err(|error| error.to_string())?,
                    ),
                    (None, false) => {
                        return Err(format!(
                            "a `bound:` field reads a JSON, YAML or TOML document, but this \
                             part reads with `{}`; bind the field in a part that reads \
                             `entries:` or `read:` with `json:`, `yaml:` or `toml:`",
                            read_text(read)
                        ));
                    }
                };
                Ok(Self::Read {
                    adapter: ExtractAdapter::new(extract),
                    read: read.clone(),
                    optional: *optional,
                    bound,
                })
            }
        }
    }

    fn adapter(&self) -> &dyn Adapter {
        match self {
            Self::Entries(adapter) => adapter,
            Self::Read { adapter, .. } => adapter,
        }
    }

    fn format(&self) -> Option<Format> {
        match self {
            Self::Entries(adapter) => Some(adapter.format()),
            Self::Read { read, .. } => read_format(read),
        }
    }
}

const fn entries_format(format: EntriesFormat) -> Format {
    match format {
        EntriesFormat::Json => Format::Json,
        EntriesFormat::Yaml => Format::Yaml,
        EntriesFormat::Toml => Format::Toml,
    }
}

const fn read_format(read: &Read) -> Option<Format> {
    match read {
        Read::Json(_) => Some(Format::Json),
        Read::Yaml(_) => Some(Format::Yaml),
        Read::Toml(_) => Some(Format::Toml),
        Read::Regex(_) | Read::Lines { .. } | Read::WholeFile => None,
    }
}

/// A `read:` as a message names it: `read.json: $.name`.
fn read_text(read: &Read) -> String {
    match read {
        Read::Json(query) => format!("read.json: {query}"),
        Read::Yaml(query) => format!("read.yaml: {query}"),
        Read::Toml(query) => format!("read.toml: {query}"),
        Read::Regex(pattern) => format!("read.regex: {pattern}"),
        Read::Lines { .. } => "read.lines".to_string(),
        Read::WholeFile => "read.whole_file".to_string(),
    }
}

fn decode_failure(
    site: &Site,
    reader: &PartReader,
    resource: Option<Arc<Path>>,
    error: &AdapterError,
) -> super::RelationFailure {
    match error {
        AdapterError::Parse(message) => {
            let detail = match reader.format() {
                Some(format) => {
                    format!("could not parse the file as {}: {message}", format.label())
                }
                None => format!("could not read values from the file: {message}"),
            };
            site.failure(AnalysisFailureKind::ResourceParse, resource, detail)
        }
        AdapterError::Limit(limit) => site.failure(
            AnalysisFailureKind::Evaluation,
            resource,
            format!(
                "{limit}, so the relation is not evaluated rather than cut short; narrow what \
                 it reads (its selector, or its `at:` paths)"
            ),
        ),
    }
}

/// One resource's rows: the cells of each, by field.
struct Rows<'r> {
    /// Key paths are hidden from messages ([`super::failure::key_path`]).
    hide_keys: bool,
    relation: &'r RelationDef,
    resource: &'r Resource,
    tokens: PathTokens,
    /// Takes a row's cells from the run's count before the row is built.
    take_cells: &'r dyn Fn(usize) -> Result<(), LimitExceeded>,
}

/// Why a resource's rows stop.
enum Stop {
    /// A problem in this resource: an analysis failure for it, and the next
    /// resource is read.
    Detail(String),
    /// The run's cells ran out: the relation builds no more rows.
    Cells(LimitExceeded),
}

impl From<String> for Stop {
    fn from(detail: String) -> Self {
        Self::Detail(detail)
    }
}

impl Rows<'_> {
    fn token(&self, token: PathToken) -> Value {
        let tokens = &self.tokens;
        let text = match token {
            PathToken::Path => &tokens.path,
            PathToken::Dir => &tokens.dir,
            PathToken::Basename => &tokens.basename,
            PathToken::Stem => &tokens.stem,
            PathToken::Ext => &tokens.ext,
            PathToken::ParentName => &tokens.parent_name,
        };
        Value::String(text.as_str().into())
    }

    fn sensitive(&self, name: &str) -> bool {
        self.relation
            .field(name)
            .is_some_and(|field| field.sensitive)
    }

    /// Each `bound:` field's cell, by field position (`None` for other
    /// fields), from the decode's rows labelled with its path.
    fn bound_cells(&self, output: &AdapterOutput) -> Result<Vec<Option<Cell>>, String> {
        let mut cells = Vec::with_capacity(self.relation.fields.len());
        for (name, field) in &self.relation.fields {
            let FieldSource::Bound(query) = &field.source else {
                cells.push(None);
                continue;
            };
            let label = format!("{}{query}", entry_values::BOUND_LABEL);
            let matches: Vec<&Row> = output
                .rows
                .iter()
                .filter(|row| row.label.as_deref() == Some(label.as_str()))
                .collect();
            let row = match matches.as_slice() {
                [] => {
                    return Err(format!(
                        "`{name}` (`bound: {query}`) matches nothing; a bound path matches exactly \
                         once in each resource the relation reads, so fix the path, or narrow \
                         the selector to the files that have it"
                    ));
                }
                [one] => *one,
                [first, second, ..] => {
                    return Err(format!(
                        "`{name}` (`bound: {query}`) matches more than once ({} and {}); a \
                         bound path matches exactly once in each resource the relation reads, \
                         so make it select one value",
                        place(first.pointer.as_ref(), self.hide_keys),
                        place(second.pointer.as_ref(), self.hide_keys),
                    ));
                }
            };
            let value = match &row.state {
                RowState::Value(value) => value.clone(),
                RowState::Error(kind) if kind.as_ref() == entry_values::ARRAY_NUMBER => {
                    return Err(format!(
                        "`{name}` (`bound: {query}`) selects {kind} {}, whose lexeme alint does \
                         not keep (`1.10` would read as `1.1`); bind an object member instead",
                        place(row.pointer.as_ref(), self.hide_keys)
                    ));
                }
                RowState::Error(kind) => {
                    return Err(format!(
                        "`{name}` (`bound: {query}`) selects {kind} {}; a field holds a scalar \
                         (null, a boolean, a number or a string), so bind a path that selects one",
                        place(row.pointer.as_ref(), self.hide_keys)
                    ));
                }
                RowState::Key | RowState::Missing => {
                    unreachable!("a bound row holds a value or an error")
                }
            };
            cells.push(Some(Cell {
                value: Some(value),
                at: Arc::new(Provenance {
                    resource: self.resource.clone(),
                    pointer: row.pointer.clone(),
                }),
                span: row.value_span.or(row.span),
                path: None,
            }));
        }
        Ok(cells)
    }

    /// Fails when an `at:` path selected something other than an object
    /// (the adapter labels that row with the path).
    fn at_selects_objects(&self, output: &AdapterOutput) -> Result<(), String> {
        if let Some((row, query)) = output.rows.iter().find_map(|row| {
            row.label
                .as_deref()
                .and_then(|label| label.strip_prefix(entry_values::AT_LABEL))
                .map(|query| (row, query))
        }) {
            let kind = match &row.state {
                RowState::Error(kind) => kind.as_ref(),
                _ => "not an object",
            };
            let advice = if kind == entry_values::ARRAY_TABLE {
                "the members of a table in an array of tables are not entries, so select a \
                 table outside one"
            } else {
                "`at:` selects objects, whose members become rows, so select an object (read an \
                 array's or a scalar's values with `read:`)"
            };
            return Err(format!(
                "`at: {query}` selects {kind} {}; {advice}",
                place(row.pointer.as_ref(), self.hide_keys)
            ));
        }
        Ok(())
    }

    /// Take the cells of one row before it is built.
    fn take_row(&self) -> Result<(), Stop> {
        (self.take_cells)(self.relation.fields.len().max(1)).map_err(Stop::Cells)
    }

    /// An `entries:` part's rows in one resource.
    fn entries(&self, output: &AdapterOutput, out: &mut RelationRows) -> Result<(), Stop> {
        self.at_selects_objects(output)?;
        let bound = self.bound_cells(output)?;
        for row in output.rows.iter().filter(|row| row.label.is_none()) {
            self.take_row()?;
            let pointer = row.pointer.clone().unwrap_or_default();
            let provenance = Arc::new(Provenance {
                resource: self.resource.clone(),
                pointer: Some(pointer.clone()),
            });
            let mut cells = Vec::with_capacity(self.relation.fields.len());
            for (position, (name, field)) in self.relation.fields.iter().enumerate() {
                let cell = |value: Value, span| Cell {
                    value: Some(value),
                    at: Arc::clone(&provenance),
                    span,
                    path: None,
                };
                cells.push(match &field.source {
                    FieldSource::Path(token) => cell(self.token(*token), None),
                    FieldSource::EntryKey => match pointer.segments().last() {
                        Some(Segment::Key(key)) => cell(Value::String(key.clone()), row.span),
                        _ => return Err(not_a_member(name, &pointer, self.hide_keys).into()),
                    },
                    FieldSource::EntryValue => match &row.state {
                        RowState::Value(value) => cell(value.clone(), row.value_span.or(row.span)),
                        RowState::Error(kind) => {
                            return Err(format!(
                                "`{name}` binds `entry.value`, which is {kind} {}; a field \
                                 holds a scalar (null, a boolean, a number or a string), so \
                                 select the members below it with `at:`, or bind only \
                                 `entry.key`",
                                place(Some(&pointer), self.hide_keys)
                            )
                            .into());
                        }
                        RowState::Key | RowState::Missing => {
                            unreachable!("an entry row holds a value or an error")
                        }
                    },
                    FieldSource::EntryParent => {
                        let segments = pointer.segments();
                        match segments
                            .len()
                            .checked_sub(2)
                            .map(|holder| &segments[holder])
                        {
                            None => cell(Value::Null, row.span),
                            Some(Segment::Key(key)) => cell(Value::String(key.clone()), row.span),
                            Some(Segment::Index(_)) => {
                                return Err(format!(
                                    "`{name}` binds `entry.parent`, but the entry {} is a \
                                     member of an array's element, which has no key; select \
                                     objects outside arrays with `at:`",
                                    place(Some(&pointer), self.hide_keys)
                                )
                                .into());
                            }
                        }
                    }
                    FieldSource::EntryRoot => match pointer.segments().first() {
                        Some(Segment::Key(key)) => cell(Value::String(key.clone()), row.span),
                        _ => {
                            return Err(format!(
                                "`{name}` binds `entry.root`, but the entry {} starts in \
                                 an array, whose elements have no key; select objects outside \
                                 arrays with `at:`",
                                place(Some(&pointer), self.hide_keys)
                            )
                            .into());
                        }
                    },
                    FieldSource::EntryPath => Cell {
                        path: Some(pointer.clone()),
                        ..cell(Value::String(pointer.render().into()), row.span)
                    },
                    FieldSource::Bound(_) => bound[position].clone().expect("a bound field's cell"),
                    FieldSource::Value | FieldSource::Side(..) => {
                        return Err(wrong_source(name, field, "an `entries:` row").into());
                    }
                });
            }
            out.rows.push(super::FieldRow { cells });
        }
        Ok(())
    }

    /// A `read:` part's rows in one resource: one per string it extracted,
    /// or one missing row when it extracted nothing (unless `optional`).
    fn read(
        &self,
        read: &Read,
        optional: bool,
        bound_output: Option<&AdapterOutput>,
        output: &AdapterOutput,
        out: &mut RelationRows,
    ) -> Result<(), Stop> {
        if !output.non_strings.is_empty() {
            return Err(format!(
                "`{}` matched non-string value(s) ({}); `read:` rows hold strings, so select \
                 string values, or read typed values with `entries:`",
                read_text(read),
                non_string_summary(output)
            )
            .into());
        }
        let value_sensitive = self
            .relation
            .fields
            .iter()
            .any(|(name, field)| field.source == FieldSource::Value && self.sensitive(name));
        for note in &output.notes {
            let AdapterNote::NonLiteral { value } = note;
            out.notes.push(RelationNote {
                resource: self.resource.clone(),
                message: if value_sensitive {
                    "skipped a non-literal value".to_string()
                } else {
                    format!("skipped non-literal value {value:?}")
                },
            });
        }
        let bound = match bound_output {
            Some(output) => self.bound_cells(output)?,
            None => vec![None; self.relation.fields.len()],
        };
        let provenance = Arc::new(Provenance {
            resource: self.resource.clone(),
            pointer: None,
        });
        let values: Vec<Option<Value>> = if output.rows.is_empty() {
            if optional || !output.notes.is_empty() {
                Vec::new()
            } else {
                vec![None]
            }
        } else {
            output.rows.iter().map(|row| row.value().cloned()).collect()
        };
        for value in values {
            self.take_row()?;
            let mut cells = Vec::with_capacity(self.relation.fields.len());
            for (position, (name, field)) in self.relation.fields.iter().enumerate() {
                let (value, span) = match &field.source {
                    FieldSource::Path(token) => (Some(self.token(*token)), None),
                    FieldSource::Value => (value.clone(), None),
                    FieldSource::Bound(_) => {
                        cells.push(bound[position].clone().expect("a bound field's cell"));
                        continue;
                    }
                    _ => return Err(wrong_source(name, field, "a `read:` row").into()),
                };
                cells.push(Cell {
                    value,
                    at: Arc::clone(&provenance),
                    span,
                    path: None,
                });
            }
            out.rows.push(super::FieldRow { cells });
        }
        Ok(())
    }
}

fn not_a_member(name: &str, pointer: &Pointer, hide_keys: bool) -> String {
    format!(
        "`{name}` binds `entry.key`, but the entry {} is not an object member; select objects \
         with `at:`",
        place(Some(pointer), hide_keys)
    )
}

fn wrong_source(name: &str, field: &FieldDef, row: &str) -> String {
    let source = match &field.source {
        FieldSource::Value => "value",
        FieldSource::Side(..) => "a join side's field",
        FieldSource::Bound(_) => "bound:",
        FieldSource::Path(_) => "a path token",
        _ => "an `entry.*` source",
    };
    format!(
        "`{name}` binds {source}, which {row} does not have; bind a source the part's reader \
         gives"
    )
}

/// `null: 1, number: 2`, counting each kind of non-string match.
fn non_string_summary(output: &AdapterOutput) -> String {
    use crate::extract::NonStringValueKind as Kind;
    [
        Kind::Null,
        Kind::Boolean,
        Kind::Number,
        Kind::Array,
        Kind::Object,
    ]
    .into_iter()
    .filter_map(|kind| {
        let count = output
            .non_strings
            .iter()
            .filter(|held| **held == kind)
            .count();
        (count > 0).then(|| format!("{}: {count}", kind.as_str()))
    })
    .collect::<Vec<_>>()
    .join(", ")
}
