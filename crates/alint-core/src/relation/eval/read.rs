//! A relation's parts: the resources a selector picks, read and decoded
//! through the run's caches, and the rows of each, one cell per field.

use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;

use super::failure::{Failures, place};
use super::{Cell, Evaluator, FieldInfo, Provenance, RelationNote, RelationRows, Site, Skipped};
use crate::ByteSpan;
use crate::extract::{Extract, LinesOpts};
use crate::relation::adapter::{Adapter, AdapterError, AdapterNote, AdapterOutput};
use crate::relation::adapters::{EntryValuesAdapter, ExtractAdapter, entry_values};
use crate::relation::limits::{LimitExceeded, LimitKind};
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
        let mut resources = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            let index = u32::try_from(index).expect("a relation has fewer than 2^32 parts");
            if self
                .part(
                    site,
                    relation,
                    (index, part),
                    &mut out,
                    &mut resources,
                    failures,
                )
                .is_break()
            {
                break;
            }
        }
        // Each once: parts that read one file give it one group.
        let mut seen = std::collections::HashSet::new();
        resources.retain(|resource: &Resource| seen.insert(resource.clone()));
        out.resources = resources.into();
        out
    }

    /// One part's rows, added to `out`, and the resources it reads, to
    /// `resources`; `Break` once the run's cells, bytes or decoded rows ran
    /// out, so no later part builds a row or decodes a file.
    fn part(
        &self,
        site: &Site,
        relation: &RelationDef,
        (index, part): (u32, &Part),
        out: &mut RelationRows,
        resources_read: &mut Vec<Resource>,
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
        for resource in resources {
            let failure_at = || -> Option<Arc<Path>> { Some(Arc::from(resource.path().as_path())) };
            let limit_reached = |limit: &LimitExceeded| {
                let detail = super::limit_reached(limit);
                site.failure(AnalysisFailureKind::Evaluation, failure_at(), detail)
            };
            if let Err(limit) = self.take_cells(1) {
                failures.push_limit(limit_reached(&limit));
                return ControlFlow::Break(());
            }
            resources_read.push(resource.clone());
            // Read when a decode needs the bytes: a decode the run holds
            // needs none.
            let mut bytes = Bytes::Unread;
            let output = match self.decode(site, reader.adapter(), &resource, &mut bytes, failures)
            {
                Ok(Some(output)) => output,
                Ok(None) => continue,
                Err(Stop::Limit(limit)) => {
                    failures.push_limit(limit_reached(&limit));
                    return ControlFlow::Break(());
                }
                Err(Stop::Detail(error)) => {
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
                } => match self.decode(site, adapter, &resource, &mut bytes, failures) {
                    Ok(Some(output)) => Some(output),
                    Ok(None) => continue,
                    Err(Stop::Limit(limit)) => {
                        failures.push_limit(limit_reached(&limit));
                        return ControlFlow::Break(());
                    }
                    Err(Stop::Detail(error)) => {
                        failures.push(decode_failure(site, &reader, failure_at(), &error));
                        continue;
                    }
                },
                _ => None,
            };
            let rows = match Rows::new(self, site, relation, index, &resource) {
                Ok(rows) => rows,
                Err(limit) => {
                    failures.push_limit(limit_reached(&limit));
                    return ControlFlow::Break(());
                }
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
                Err(Stop::Limit(limit)) => {
                    failures.push_limit(limit_reached(&limit));
                    return ControlFlow::Break(());
                }
            }
        }
        ControlFlow::Continue(())
    }

    /// `resource` decoded through the run's cache, reading its bytes into
    /// `bytes` first when the cache does not hold the decode; `None` when
    /// they cannot be read (a failure says why). A decode this call makes
    /// takes what it holds from the run's bytes, once: each part, relation
    /// and rule that reads it later shares it. The run's decoded rows
    /// running out ([`LimitKind::RowsPerRun`]), or one decode's key paths
    /// passing the run's bytes, stops the relation, as its cells and bytes
    /// running out does; past the run's rows the cache decodes nothing more.
    fn decode(
        &self,
        site: &Site,
        adapter: &dyn Adapter,
        resource: &Resource,
        bytes: &mut Bytes,
        failures: &mut Failures,
    ) -> Result<Option<Arc<AdapterOutput>>, Stop<AdapterError>> {
        let (decoded, fresh) =
            if let Some(decoded) = self.decodes.cached(adapter, resource, &self.limits) {
                (decoded, false)
            } else {
                if matches!(bytes, Bytes::Unread) {
                    *bytes = self
                        .bytes(site, resource, failures)
                        .map_or(Bytes::Unreadable, Bytes::Read);
                }
                let Bytes::Read(read) = bytes else {
                    return Ok(None);
                };
                self.decodes
                    .decode_fresh(adapter, resource, read, &self.limits)
            };
        let output = decoded.map_err(|error| match error {
            // Run-wide: no later decode or row can be held either.
            AdapterError::Limit(limit)
                if matches!(limit.kind, LimitKind::RowsPerRun | LimitKind::BytesPerRun) =>
            {
                Stop::Limit(limit)
            }
            error => Stop::Detail(error),
        })?;
        if fresh {
            self.take_bytes(decode_bytes(&output))
                .map_err(Stop::Limit)?;
        }
        Ok(Some(output))
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
        let scope = match selector.compile(&name) {
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
                    // A dotenv or properties manifest's values are redacted
                    // by default (ADR-0019 decision 3), and its reader's
                    // message may quote one, as the engine's gate knows.
                    let redacts = predicate.redacts_by_default();
                    let reason = if redacts {
                        "it could not be read as a manifest, and the reason is not shown, since \
                         the file's values are redacted by default"
                    } else {
                        failure.message.as_str()
                    };
                    let mut failure = site.failure(
                        failure.kind,
                        Some(Arc::from(source)),
                        format!(
                            "{name}'s `scope_filter:` reads `{source}`: {reason}; fix `{source}`, \
                             or change the selector's `scope_filter:`",
                            source = source.display(),
                        ),
                    );
                    failure.sensitive |= redacts;
                    failures.push(failure);
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
                    adapter: ExtractAdapter::with_spans(extract),
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
            super::limit_reached(limit),
        ),
    }
}

/// What a decode holds: its rows, their values, labels and key paths, and
/// its notes and non-string kinds.
fn decode_bytes(output: &AdapterOutput) -> usize {
    let rows = output.rows.iter().fold(
        output.rows.capacity() * std::mem::size_of::<Row>(),
        |bytes, row| {
            let state = match &row.state {
                RowState::Value(value) => value.heap_bytes(),
                RowState::Error(kind) => kind.len(),
                RowState::Key | RowState::Missing => 0,
            };
            bytes
                .saturating_add(row.pointer.as_ref().map_or(0, Pointer::heap_bytes))
                .saturating_add(row.label.as_ref().map_or(0, |label| label.len()))
                .saturating_add(row.original.as_ref().map_or(0, Value::heap_bytes))
                .saturating_add(state)
        },
    );
    let notes = output.notes.iter().fold(
        output.notes.capacity() * std::mem::size_of::<AdapterNote>(),
        |bytes, note| {
            let AdapterNote::NonLiteral { value } = note;
            bytes.saturating_add(value.len())
        },
    );
    rows.saturating_add(notes).saturating_add(
        output.non_strings.capacity() * std::mem::size_of::<crate::extract::NonStringValueKind>(),
    )
}

/// One resource's rows: the cells of each, by field.
struct Rows<'r> {
    /// Takes a row's cells and what it holds beyond them from the run.
    evaluator: &'r Evaluator<'r>,
    /// Key paths are hidden from messages ([`super::failure::key_path`]).
    hide_keys: bool,
    /// The relation's configuration reads the environment, so a message
    /// names nothing it selects ([`Site`]).
    hide_config: bool,
    relation: &'r RelationDef,
    /// The part's index in the relation.
    part: u32,
    resource: &'r Resource,
    /// The path tokens the relation's fields bind, by [`token_slot`]: built
    /// once for the resource and shared by its rows.
    tokens: [Option<Value>; 6],
}

/// A resource's bytes, read when a decode first needs them.
enum Bytes {
    Unread,
    Read(Arc<[u8]>),
    /// The read failed, and a failure says why.
    Unreadable,
}

/// Why a resource's rows stop.
enum Stop<D = String> {
    /// A problem in this resource: an analysis failure for it, and the next
    /// resource is read.
    Detail(D),
    /// The run's cells, bytes or decoded rows ran out: the relation builds
    /// no more rows.
    Limit(LimitExceeded),
}

impl From<String> for Stop {
    fn from(detail: String) -> Self {
        Self::Detail(detail)
    }
}

/// A path token's slot in [`Rows::tokens`].
const fn token_slot(token: PathToken) -> usize {
    match token {
        PathToken::Path => 0,
        PathToken::Dir => 1,
        PathToken::Basename => 2,
        PathToken::Stem => 3,
        PathToken::Ext => 4,
        PathToken::ParentName => 5,
    }
}

/// Every path token of `resource`, from its index path, which is `/`-separated
/// on every host, so a token is too.
pub(super) fn resource_tokens(resource: &Resource) -> PathTokens {
    PathTokens::from_path(resource.path().as_path())
}

pub(super) fn token_text(paths: &PathTokens, token: PathToken) -> &str {
    match token {
        PathToken::Path => &paths.path,
        PathToken::Dir => &paths.dir,
        PathToken::Basename => &paths.basename,
        PathToken::Stem => &paths.stem,
        PathToken::Ext => &paths.ext,
        PathToken::ParentName => &paths.parent_name,
    }
}

/// The text a field bound to path token `token` holds in every row of
/// `resource`, before its transforms: what a `set` grouped by a path token
/// gives a resource without rows, as its rows would have it.
#[must_use]
pub fn path_token(resource: &Resource, token: PathToken) -> String {
    token_text(&resource_tokens(resource), token).to_string()
}

/// The cells a row's `entry.*` fields share, by source ([`KEY`] to
/// [`VALUE`]): each is built once for the row, and every field bound to it
/// copies it, sharing its value.
type Derived = [Option<Cell>; 5];
const KEY: usize = 0;
const PARENT: usize = 1;
const ROOT: usize = 2;
const PATH: usize = 3;
const VALUE: usize = 4;

impl<'r> Rows<'r> {
    /// `resource`'s rows for `relation`, its path tokens built and their
    /// bytes taken from the run.
    fn new(
        evaluator: &'r Evaluator<'r>,
        site: &Site,
        relation: &'r RelationDef,
        part: u32,
        resource: &'r Resource,
    ) -> Result<Self, LimitExceeded> {
        let mut tokens: [Option<Value>; 6] = Default::default();
        let mut paths = None;
        for (_, field) in &relation.fields {
            let FieldSource::Path(token) = field.source else {
                continue;
            };
            let slot = &mut tokens[token_slot(token)];
            if slot.is_some() {
                continue;
            }
            let paths = paths.get_or_insert_with(|| resource_tokens(resource));
            let text = token_text(paths, token);
            evaluator.take_bytes(text.len())?;
            *slot = Some(Value::String(text.into()));
        }
        Ok(Self {
            evaluator,
            hide_keys: site.keys_sensitive,
            hide_config: site.sensitive,
            relation,
            part,
            resource,
            tokens,
        })
    }

    fn token(&self, token: PathToken) -> Value {
        self.tokens[token_slot(token)]
            .clone()
            .expect("a bound path token is built with the rows")
    }

    /// Take `bytes` from the run for what a row keeps beyond its cells.
    fn take_bytes(&self, bytes: usize) -> Result<(), Stop> {
        self.evaluator.take_bytes(bytes).map_err(Stop::Limit)
    }

    /// The provenance of the row the part's adapter emitted `ordinal`th, at
    /// `pointer`, its key path's bytes taken; `empty` when it is an entry
    /// whose value is null, an empty object or an empty array.
    fn provenance(
        &self,
        pointer: Option<Pointer>,
        ordinal: u32,
        empty: bool,
    ) -> Result<Arc<Provenance>, Stop> {
        self.take_bytes(pointer.as_ref().map_or(0, Pointer::heap_bytes))?;
        Ok(Arc::new(Provenance {
            resource: self.resource.clone(),
            pointer,
            part: self.part,
            ordinal,
            empty,
        }))
    }

    fn sensitive(&self, name: &str) -> bool {
        self.relation
            .field(name)
            .is_some_and(|field| field.sensitive)
    }

    /// Each `bound:` field's cell, by field position (`None` for other
    /// fields), from the decode's rows labelled with its path: built once
    /// for the resource and shared by its rows.
    fn bound_cells(&self, output: &AdapterOutput) -> Result<Vec<Option<Cell>>, Stop> {
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
                    )
                    .into());
                }
                [one] => *one,
                [first, second, ..] => {
                    return Err(format!(
                        "`{name}` (`bound: {query}`) matches more than once ({} and {}); a \
                         bound path matches exactly once in each resource the relation reads, \
                         so make it select one value",
                        place(first.pointer.as_ref(), self.hide_keys),
                        place(second.pointer.as_ref(), self.hide_keys),
                    )
                    .into());
                }
            };
            let value = match &row.state {
                RowState::Value(value) => value.clone(),
                RowState::Error(kind) if kind.as_ref() == entry_values::ARRAY_NUMBER => {
                    return Err(format!(
                        "`{name}` (`bound: {query}`) selects {kind} {}, whose lexeme alint does \
                         not keep (`1.10` would read as `1.1`); bind an object member instead",
                        place(row.pointer.as_ref(), self.hide_keys)
                    )
                    .into());
                }
                RowState::Error(kind) => {
                    return Err(format!(
                        "`{name}` (`bound: {query}`) selects {kind} {}; a field holds a scalar \
                         (null, a boolean, a number or a string), so bind a path that selects one",
                        place(row.pointer.as_ref(), self.hide_keys)
                    )
                    .into());
                }
                RowState::Key | RowState::Missing => {
                    unreachable!("a bound row holds a value or an error")
                }
            };
            cells.push(Some(Cell {
                value: Some(value),
                at: self.provenance(row.pointer.clone(), row.ordinal, false)?,
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
        self.evaluator
            .take_cells(self.relation.fields.len().max(1))
            .map_err(Stop::Limit)
    }

    /// An `entries:` part's rows in one resource.
    fn entries(&self, output: &AdapterOutput, out: &mut RelationRows) -> Result<(), Stop> {
        self.at_selects_objects(output)?;
        let bound = self.bound_cells(output)?;
        for row in output.rows.iter().filter(|row| row.label.is_none()) {
            self.take_row()?;
            let provenance = self.provenance(
                Some(row.pointer.clone().unwrap_or_default()),
                row.ordinal,
                entry_values::holds_nothing(&row.state),
            )?;
            let pointer = provenance
                .pointer
                .as_ref()
                .expect("an entry row's key path");
            let mut derived = Derived::default();
            let mut cells = Vec::with_capacity(self.relation.fields.len());
            for (position, (name, field)) in self.relation.fields.iter().enumerate() {
                let cell = |value: Value, span| Cell {
                    value: Some(value),
                    at: Arc::clone(&provenance),
                    span,
                    path: None,
                };
                let (slot, source) = match &field.source {
                    FieldSource::Path(token) => {
                        cells.push(cell(self.token(*token), None));
                        continue;
                    }
                    FieldSource::Bound(_) => {
                        cells.push(bound[position].clone().expect("a bound field's cell"));
                        continue;
                    }
                    FieldSource::Value | FieldSource::Side(..) => {
                        return Err(wrong_source(name, field, "an `entries:` row").into());
                    }
                    FieldSource::EntryKey => (KEY, &field.source),
                    FieldSource::EntryParent => (PARENT, &field.source),
                    FieldSource::EntryRoot => (ROOT, &field.source),
                    FieldSource::EntryPath => (PATH, &field.source),
                    FieldSource::EntryValue => (VALUE, &field.source),
                };
                if derived[slot].is_none() {
                    derived[slot] = Some(self.entry_cell(name, source, row, pointer, &cell)?);
                }
                cells.push(derived[slot].clone().expect("the cell was just built"));
            }
            out.rows.push(super::FieldRow {
                cells,
                at: provenance,
            });
        }
        Ok(())
    }

    /// The cell of an `entry.*` source for one entry row at `pointer`, its
    /// new text's bytes taken from the run. `cell` makes a cell of a value
    /// and span at the row's provenance.
    fn entry_cell(
        &self,
        name: &str,
        source: &FieldSource,
        row: &Row,
        pointer: &Pointer,
        cell: &dyn Fn(Value, Option<ByteSpan>) -> Cell,
    ) -> Result<Cell, Stop> {
        let key = |key: &str| -> Result<Value, Stop> {
            self.take_bytes(key.len())?;
            Ok(Value::String(key.into()))
        };
        Ok(match source {
            FieldSource::EntryKey => match pointer.segments().last() {
                Some(Segment::Key(name)) => cell(key(name)?, row.span),
                _ => return Err(not_a_member(name, pointer, self.hide_keys).into()),
            },
            FieldSource::EntryValue => match &row.state {
                // Shared with the decode, which took its bytes.
                RowState::Value(value) => cell(value.clone(), row.value_span.or(row.span)),
                RowState::Error(kind) => {
                    return Err(format!(
                        "`{name}` binds `entry.value`, which is {kind} {}; a field holds a \
                         scalar (null, a boolean, a number or a string), so select the members \
                         below it with `at:`, or bind only `entry.key`",
                        place(Some(pointer), self.hide_keys)
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
                    Some(Segment::Key(parent)) => cell(key(parent)?, row.span),
                    Some(Segment::Index(_)) => {
                        return Err(format!(
                            "`{name}` binds `entry.parent`, but the entry {} is a member of an \
                             array's element, which has no key; select objects outside arrays \
                             with `at:`",
                            place(Some(pointer), self.hide_keys)
                        )
                        .into());
                    }
                }
            }
            FieldSource::EntryRoot => match pointer.segments().first() {
                Some(Segment::Key(root)) => cell(key(root)?, row.span),
                _ => {
                    return Err(format!(
                        "`{name}` binds `entry.root`, but the entry {} starts in an array, whose \
                         elements have no key; select objects outside arrays with `at:`",
                        place(Some(pointer), self.hide_keys)
                    )
                    .into());
                }
            },
            FieldSource::EntryPath => {
                let text = pointer.render();
                self.take_bytes(text.len() + super::shared_pointer_bytes(pointer))?;
                Cell {
                    path: Some(Arc::new(pointer.clone())),
                    ..cell(Value::String(text.into()), row.span)
                }
            }
            FieldSource::Path(_)
            | FieldSource::Bound(_)
            | FieldSource::Value
            | FieldSource::Side(..) => unreachable!("an `entry.*` source"),
        })
    }

    /// A `read:` part's rows in one resource: one per string it extracted,
    /// or one missing row when it extracted nothing (unless `optional`, or
    /// it skipped a value or matched one that is not a string, which a note
    /// says instead). Each row is its own, for its ordinal.
    fn read(
        &self,
        read: &Read,
        optional: bool,
        bound_output: Option<&AdapterOutput>,
        output: &AdapterOutput,
        out: &mut RelationRows,
    ) -> Result<(), Stop> {
        if !output.non_strings.is_empty() {
            let kinds = non_string_kinds(output);
            let summary = kinds
                .iter()
                .map(|(kind, count)| format!("{}: {count}", kind.as_str()))
                .collect::<Vec<_>>()
                .join(", ");
            let read = if self.hide_config {
                "`read:`".to_string()
            } else {
                format!("`{}`", read_text(read))
            };
            let message = format!(
                "{read} matched non-string value(s) ({summary}); `read:` rows hold strings, so \
                 select string values, or read typed values with `entries:`"
            );
            let skipped = Skipped::NonString {
                part: self.part,
                kinds,
            };
            self.take_bytes(note_bytes(&skipped, &message))?;
            out.notes.push(RelationNote {
                resource: self.resource.clone(),
                skipped,
                message,
            });
        }
        let value_sensitive = self
            .relation
            .fields
            .iter()
            .any(|(name, field)| field.source == FieldSource::Value && self.sensitive(name));
        for note in &output.notes {
            let AdapterNote::NonLiteral { value } = note;
            let message = if value_sensitive {
                "skipped a non-literal value".to_string()
            } else {
                format!("skipped non-literal value {value:?}")
            };
            self.take_bytes(note_bytes(&Skipped::NonLiteral, &message))?;
            out.notes.push(RelationNote {
                resource: self.resource.clone(),
                skipped: Skipped::NonLiteral,
                message,
            });
        }
        let bound = match bound_output {
            Some(output) => self.bound_cells(output)?,
            None => vec![None; self.relation.fields.len()],
        };
        // Shared with the decode, which took their bytes.
        let values: Vec<(Option<Value>, u32, Option<ByteSpan>)> = if output.rows.is_empty() {
            if optional || !output.notes.is_empty() || !output.non_strings.is_empty() {
                Vec::new()
            } else {
                vec![(None, 0, None)]
            }
        } else {
            output
                .rows
                .iter()
                .map(|row| (row.value().cloned(), row.ordinal, row.span))
                .collect()
        };
        for (value, ordinal, value_span) in values {
            self.take_row()?;
            let provenance = self.provenance(None, ordinal, false)?;
            let mut cells = Vec::with_capacity(self.relation.fields.len());
            for (position, (name, field)) in self.relation.fields.iter().enumerate() {
                let (value, span) = match &field.source {
                    FieldSource::Path(token) => (Some(self.token(*token)), None),
                    FieldSource::Value => (value.clone(), value_span),
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
            out.rows.push(super::FieldRow {
                cells,
                at: provenance,
            });
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

/// What a note with `message` holds.
pub(super) fn note_bytes(skipped: &Skipped, message: &str) -> usize {
    let kinds = match skipped {
        Skipped::NonLiteral => 0,
        Skipped::NonString { kinds, .. } => {
            kinds.len() * std::mem::size_of::<(crate::extract::NonStringValueKind, usize)>()
        }
    };
    std::mem::size_of::<RelationNote>() + message.len() + kinds
}

/// Each kind of non-string match and how many times, in a fixed order.
fn non_string_kinds(output: &AdapterOutput) -> Vec<(crate::extract::NonStringValueKind, usize)> {
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
        (count > 0).then_some((kind, count))
    })
    .collect()
}
