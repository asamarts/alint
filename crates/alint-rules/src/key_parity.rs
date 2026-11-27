//! `key_parity` — one reference file and a glob of peers must declare the same
//! key set. The first user of the internal relation layer (v0.21 R3).
//!
//! ```yaml
//! - id: locales-match-english
//!   kind: key_parity
//!   reference: locales/en.json
//!   peers: "locales/*.json"
//!   level: error
//! ```
//!
//! The formats are JSON, YAML, TOML, dotenv, INI, Java properties, and the
//! Android string-resource and .NET `.resx` XML dialects. Files are compared
//! only within one key model: the tree formats (JSON/YAML/TOML) together, the
//! flat formats (dotenv/properties) together, and INI and each XML dialect on
//! their own. A literal peer in another key model is a config error; a peer a
//! glob selects in another key model is an analysis failure.

use std::borrow::{Borrow, Cow};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::sync::Arc;

use alint_core::plugin_api::{Analyzer, AnalyzerError, AnalyzerInput, Finding, PointerSegment};
use alint_core::relation::{
    Adapter, AdapterOutput, AndroidStringsAdapter, ContractAnalyzer, DecodeCache, EntryRowsAdapter,
    LimitExceeded, LimitKind, Limits, Origin, Pointer, ReadCache, RelPath, Resource, ResourceRead,
    ResxAdapter, Row, RowState, Segment, Selector, TomlRowsAdapter,
};
use alint_core::template::{PathTokens, render_path, render_path_glob};
use alint_core::{
    AnalysisFailureKind, ByteSpan, Context, Error, Level, RelatedLocation, Result, Rule, RuleSpec,
    Scope, SourceRange, Violation,
};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum KeyFormat {
    Json,
    Yaml,
    Toml,
    Dotenv,
    Properties,
    Ini,
    AndroidStrings,
    Resx,
}

impl KeyFormat {
    fn adapter(
        self,
        strict_json: bool,
    ) -> Result<Box<dyn Adapter>, alint_core::relation::AdapterError> {
        match self {
            Self::Json => Ok(Box::new(EntryRowsAdapter::with_strict(
                alint_core::Format::Json,
                strict_json,
            )?)),
            Self::Yaml => Ok(Box::new(EntryRowsAdapter::new(alint_core::Format::Yaml)?)),
            Self::Dotenv => Ok(Box::new(EntryRowsAdapter::new(alint_core::Format::Dotenv)?)),
            Self::Properties => Ok(Box::new(EntryRowsAdapter::new(
                alint_core::Format::Properties,
            )?)),
            Self::Ini => Ok(Box::new(EntryRowsAdapter::new(alint_core::Format::Ini)?)),
            Self::Toml => Ok(Box::new(TomlRowsAdapter::new())),
            Self::AndroidStrings => Ok(Box::new(AndroidStringsAdapter::new())),
            Self::Resx => Ok(Box::new(ResxAdapter::new())),
        }
    }

    /// The `format:` spelling, for messages.
    fn name(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Toml => "toml",
            Self::Dotenv => "dotenv",
            Self::Properties => "properties",
            Self::Ini => "ini",
            Self::AndroidStrings => "android-strings",
            Self::Resx => "resx",
        }
    }

    fn model(self) -> KeyModel {
        match self {
            Self::Json | Self::Yaml | Self::Toml => KeyModel::Tree,
            Self::Dotenv | Self::Properties => KeyModel::Flat,
            Self::Ini => KeyModel::Ini,
            Self::AndroidStrings => KeyModel::AndroidStrings,
            Self::Resx => KeyModel::Resx,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum Direction {
    #[default]
    Both,
    Missing,
    Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum Plurals {
    #[default]
    Exact,
    Cldr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyModel {
    Tree,
    Flat,
    Ini,
    AndroidStrings,
    Resx,
}

impl KeyModel {
    /// Whether every key is one name (dotenv, properties, `.resx`), so a dot
    /// in it is part of the name and never a path separator.
    fn single_name(self) -> bool {
        matches!(self, Self::Flat | Self::Resx)
    }
}

fn present<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    match Option::<T>::deserialize(deserializer)? {
        Some(value) => Ok(Some(value)),
        None => Err(serde::de::Error::custom(
            "an optional key written as null; give it a value, or leave the key out",
        )),
    }
}

fn no_default(schema: &mut schemars::Schema) {
    schema.remove("default");
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend(
    "allOf" = [{
        "if": { "required": ["strict", "format"], "properties": { "format": { "type": "string" } } },
        "then": { "properties": { "format": { "const": "json" } } }
    }]
))]
struct Options {
    /// The authoritative key file.
    reference: String,
    /// Glob selecting the peer files. The reference is excluded automatically.
    /// A value without glob metacharacters is a literal path, read even when
    /// gitignored (`peers: .env`).
    peers: String,
    /// Key format. Inferred from the extension when omitted; `.env` and
    /// `.env.*` infer dotenv and `.resx` infers resx, while an Android `.xml`
    /// file needs `android-strings`.
    #[serde(default)]
    format: Option<KeyFormat>,
    /// Which differences to report.
    #[serde(default)]
    direction: Direction,
    /// Treat a CLDR plural group (nested object or i18next `_other` siblings)
    /// as one key. Exact per-locale category checking is not part of v0.21.
    #[serde(default)]
    plurals: Plurals,
    /// Compare below each file's single top-level key, as in Rails locale
    /// files (`en:`, `fr:`). JSON, YAML, and TOML only. A file with keys must
    /// have exactly one top-level key, an object, or it is an analysis failure.
    #[serde(default)]
    strip_root: bool,
    /// Keys to omit, written as they appear in findings. In dotenv,
    /// properties, and `.resx` files a pattern matches the key name, where
    /// `*` stops at a `.` and `**` crosses it; a name shown in bracket form
    /// (`['']`, `['[x']`) is written that way too. In every other format a
    /// pattern is a key path: `*` matches within one segment, a `**` segment
    /// matches any number of segments (at the end of a pattern, at least
    /// one), and `['a.b']` names a segment that contains a dot.
    #[serde(default)]
    ignore_keys: Vec<String>,
    /// Report an absent literal peer, or a `peers` glob that selects no file,
    /// as an informational note instead of an analysis failure. For a peer
    /// that exists only on developer machines, such as a gitignored `.env`.
    #[serde(default)]
    allow_missing_peers: bool,
    /// When `true` on JSON files, enforce strict RFC 8259 JSON parsing
    /// (rejecting comments, trailing commas, leading UTF-8 BOM, and empty
    /// files; ADR-0050 D7). Rejected at build time for non-JSON formats.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "bool", transform = no_default, extend("x-since" = "0.24"))]
    strict: Option<bool>,
}

crate::options_schema_for!(Options);

#[derive(Debug, Clone)]
struct KeyFile {
    format: KeyFormat,
    /// The file's decode, which holds each key path once: the file's keys
    /// borrow their paths from it ([`Self::keys`]) rather than copying them.
    decode: Arc<AdapterOutput>,
    /// The decode's leaf key rows, in key order, a repeated key once (its
    /// first row), each with its key's span.
    leaves: Vec<(usize, ByteSpan)>,
    text: Arc<str>,
    line_starts: Vec<usize>,
}

impl KeyFile {
    /// The key path of the decode's row `row`.
    fn path(&self, row: usize) -> &[Segment] {
        self.decode.rows[row]
            .pointer
            .as_ref()
            .map_or(&[], Pointer::segments)
    }

    /// Whether the file has no comparable keys once `strip_root` is applied.
    fn is_empty_for(&self, strip_root: bool) -> bool {
        self.leaves.is_empty()
            || (strip_root && self.leaves.iter().all(|&(row, _)| self.path(row).len() < 2))
    }

    /// The file's keys, each without its first `skip` segments, their paths
    /// borrowed from the decode.
    fn keys(&self, skip: usize) -> Keys<'_> {
        self.leaves
            .iter()
            .filter_map(|&(row, span)| {
                let path = self.path(row).get(skip..)?;
                (!path.is_empty()).then_some((Cow::Borrowed(path), span))
            })
            .collect()
    }

    /// What the file holds beside its decode, which the run's byte budget
    /// takes as the file is read.
    fn held_bytes(&self) -> usize {
        self.text.len()
            + self.line_starts.capacity() * std::mem::size_of::<usize>()
            + self.leaves.capacity() * std::mem::size_of::<(usize, ByteSpan)>()
    }

    #[cfg(test)]
    fn range(&self, key: &[Segment]) -> Option<SourceRange> {
        let (_, span) = self.leaves.iter().find(|(row, _)| self.path(*row) == key)?;
        self.range_at(*span)
    }

    /// The source range of a key span. Callers pass the span from the
    /// transformed key map, so a key renamed by `strip_root` or merged by
    /// `plurals: cldr` keeps the location of the key it came from.
    fn range_at(&self, span: ByteSpan) -> Option<SourceRange> {
        let (start_line, start_column) = self.line_column(span.start)?;
        let (end_line, end_column) = self.line_column(span.end)?;
        SourceRange::new(start_line, start_column, end_line, end_column)
    }

    fn line_column(&self, offset: usize) -> Option<(usize, usize)> {
        if offset > self.text.len() || !self.text.is_char_boundary(offset) {
            return None;
        }
        let line = self.line_starts.partition_point(|start| *start <= offset);
        let line = line.checked_sub(1)?;
        let start = *self.line_starts.get(line)?;
        let column = self.text.get(start..offset)?.chars().count() + 1;
        Some((line + 1, column))
    }
}

/// Byte offsets at which each physical line starts, treating LF, CRLF, and a
/// lone CR as one line ending. A leading BOM occupies no column.
fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![if text.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    }];
    let bytes = text.as_bytes();
    let mut index = starts[0];
    while index < bytes.len() {
        match bytes[index] {
            b'\n' => {
                starts.push(index + 1);
            }
            b'\r' => {
                if bytes.get(index + 1) == Some(&b'\n') {
                    index += 1;
                }
                starts.push(index + 1);
            }
            _ => {}
        }
        index += 1;
    }
    starts
}

#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct KeyParityRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    reference: RelPath,
    reference_format: KeyFormat,
    /// The reference's file name came from a path token and has no format
    /// yet: only a nested rule's preparation build, from placeholder tokens,
    /// sets this, and each iteration builds the rule again from its own path
    /// (#504). `reference_format` is then a stand-in.
    format_pending: bool,
    format_override: Option<KeyFormat>,
    peers: String,
    /// Selects the files `peers` matches, under its literal directories only.
    peers_selector: Selector,
    /// The peer a `peers` value without glob metacharacters names. It is read
    /// from disk even when the index leaves it out, so a gitignored `.env`
    /// can be a peer of `.env.example`.
    literal_peer: Option<RelPath>,
    scope: Scope,
    direction: Direction,
    plurals: Plurals,
    strip_root: bool,
    ignore: Vec<KeyPattern>,
    allow_missing_peers: bool,
    strict: bool,
}

/// The key-parity comparison as a pure contract analyzer over one reference
/// and one peer. `KeyParityRule` uses this for diff detection, then attaches
/// its own messages, spans, and baseline keys.
///
/// Keys are compared by their injective rendering ([`Pointer::render`]), the
/// rule's key identity, so `nav['a.b']` and `nav.a.b` stay distinct (messages
/// and baseline keys use [`display_key`]). A key row's pointer is one segment
/// that holds it ([`identity_pointer`]). The analyzer knows the peer's path,
/// so a peer with no rows (an empty file, or one whose every key
/// `ignore_keys` removed) still reports every reference key as missing.
struct KeySetAnalyzer {
    reference: String,
    peer: String,
    direction: Direction,
    /// Remaining findings budget for this comparison; diffs past this count are
    /// summarized in one `truncated\0<count>` sentinel finding instead of
    /// allocating unbounded findings.
    max_findings: usize,
}

impl Analyzer for KeySetAnalyzer {
    fn id(&self) -> &'static str {
        "key-parity-keys"
    }

    fn analyze(&self, input: AnalyzerInput<'_>) -> Result<Vec<Finding>, AnalyzerError> {
        // Preserve the canonical `[Segment]` order produced by `key_rows` so
        // truncation under `FindingsPerRule` always keeps the first keys in
        // segment order rather than `render_key` string order (where bracketed
        // keys starting with `[` sort after uppercase ASCII).
        let mut reference_order = Vec::new();
        let mut reference_set = HashSet::new();
        let mut peer_order = Vec::new();
        let mut peer_set = HashSet::new();
        for row in input.rows {
            let Some(pointer) = &row.pointer else {
                return Err(AnalyzerError::InvalidInput(format!(
                    "a key row of {} has no key path",
                    row.resource.path
                )));
            };
            let [PointerSegment::Key(key)] = pointer.as_slice() else {
                return Err(AnalyzerError::InvalidInput(format!(
                    "a key row of {} does not hold one key identity",
                    row.resource.path
                )));
            };
            let key = key.as_str();
            if row.resource.path == self.reference {
                if reference_set.insert(key) {
                    reference_order.push(key);
                }
            } else if row.resource.path == self.peer {
                if peer_set.insert(key) {
                    peer_order.push(key);
                }
            } else {
                return Err(AnalyzerError::InvalidInput(format!(
                    "a key row of {} belongs to neither compared file",
                    row.resource.path
                )));
            }
        }
        let mut findings = Vec::new();
        let mut suppressed = 0usize;
        let max = self.max_findings.min(input.max_findings);
        let mut emit = |key: &str, direction: &str| {
            if findings.len() >= max {
                suppressed = suppressed.saturating_add(1);
                return;
            }
            findings.push(Finding {
                path: Some(self.peer.clone()),
                message: format!("{direction}\u{0}{key}"),
                line: None,
                column: None,
                related: Vec::new(),
                baseline_key: None,
                sensitive: false,
                analysis_failure: None,
                failure_kind: None,
                not_fixable: false,
            });
        };
        if matches!(self.direction, Direction::Both | Direction::Missing) {
            for key in reference_order {
                if !peer_set.contains(key) {
                    emit(key, "missing");
                }
            }
        }
        if matches!(self.direction, Direction::Both | Direction::Extra) {
            for key in peer_order {
                if !reference_set.contains(key) {
                    emit(key, "extra");
                }
            }
        }
        if suppressed > 0 {
            findings.push(Finding {
                path: Some(self.peer.clone()),
                message: format!("truncated\u{0}{suppressed}"),
                line: None,
                column: None,
                related: Vec::new(),
                baseline_key: None,
                sensitive: false,
                analysis_failure: None,
                failure_kind: None,
                not_fixable: false,
            });
        }
        Ok(findings)
    }
}

/// Contract key rows for one compared file. Every key becomes a row; a file
/// with more keys than a row ordinal can number is an error, never a
/// silently shortened key set.
fn key_rows<K: Borrow<[Segment]>>(
    resource: &Resource,
    keys: &BTreeMap<K, ByteSpan>,
) -> std::result::Result<Vec<Row>, String> {
    keys.iter()
        .enumerate()
        .map(|(ordinal, (key, span))| {
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| format!("{} has more keys than a row can number", resource.path()))?;
            let mut row = Row::new(
                resource.clone(),
                Some(identity_pointer(key.borrow())),
                ordinal,
                RowState::Key,
            );
            row.span = Some(*span);
            Ok(row)
        })
        .collect()
}

/// A key row's pointer: one segment, the key's identity ([`render_key`]),
/// which the analyzer compares as it is. A row that held the key path itself
/// would have the contract copy every segment of it into a string of its
/// own, W x D strings for each file W keys wide at depth D, in every
/// comparison.
fn identity_pointer(key: &[Segment]) -> Pointer {
    Pointer::from_segments(vec![Segment::Key(render_key(key).into())])
}

/// What a comparison's rows hold: each row and its key's identity.
fn rows_bytes(rows: &[Row]) -> usize {
    rows.iter().fold(std::mem::size_of_val(rows), |bytes, row| {
        bytes.saturating_add(row.pointer.as_ref().map_or(0, Pointer::heap_bytes))
    })
}

/// What a table of `len` keys holds beyond the decode its paths are
/// borrowed from: an entry each, and about half again for the tree's nodes.
fn table_bytes(len: usize) -> usize {
    len.saturating_mul(3 * std::mem::size_of::<(Cow<'static, [Segment]>, ByteSpan)>() / 2)
}

impl Rule for KeyParityRule {
    alint_core::rule_common_impl!();

    fn uses_relations(&self) -> bool {
        true
    }

    fn requires_full_index(&self) -> bool {
        true
    }

    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }

    fn supports_expect_matches(&self) -> bool {
        false
    }

    #[allow(clippy::too_many_lines)]
    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        if self.format_pending {
            return Err(Error::rule_config(
                &self.id,
                no_reference_format(&self.reference),
            ));
        }
        // The run's caches, or fresh ones when the engine did not enable them
        // (a rule evaluated directly, as the unit tests do).
        let mut local = None;
        let (read_cache, decode_cache) = if let (Some(reads), Some(decodes)) =
            (ctx.index.read_cache(), ctx.index.decode_cache())
        {
            (reads, decodes)
        } else {
            let (reads, decodes) = local.insert((
                ReadCache::new(),
                DecodeCache::with_run_limits(&ctx.index.relation_limits()),
            ));
            (&*reads, &*decodes)
        };
        let limits = *decode_cache.limits();
        let mut findings = Vec::new();

        let reference = Resource::literal(self.reference.clone());
        let Some(reference_file) = self.read_file(
            ctx,
            read_cache,
            decode_cache,
            &limits,
            &reference,
            self.reference_format,
            true,
            &mut findings,
        ) else {
            return Ok(findings);
        };

        let mut peers: Vec<Resource> = if let Some(path) = &self.literal_peer {
            vec![Resource::literal(path.clone())]
        } else {
            let selection = self.peers_selector.select(ctx.index);
            for path in selection.unrepresentable {
                findings.push(
                    Violation::new(format!(
                        "{}: path cannot be represented as a repository key-parity path",
                        crate::slash(&path)
                    ))
                    .with_path(path.as_ref().to_path_buf())
                    .as_analysis_failure(AnalysisFailureKind::ResourceRead),
                );
            }
            selection.resources
        };
        peers.retain(|resource| resource.path() != &self.reference);
        peers.sort_by(|a, b| a.path().cmp(b.path()));
        if peers.is_empty() {
            let finding = Violation::new(format!(
                "{}: `peers` glob {:?} matched no files other than the reference",
                crate::slash(Path::new(self.reference.as_str())),
                self.peers_scope_label()
            ))
            .with_path(Path::new(self.reference.as_str()).to_path_buf());
            findings.push(if self.allow_missing_peers {
                finding.as_note()
            } else {
                finding.as_analysis_failure(AnalysisFailureKind::Evaluation)
            });
            return Ok(findings);
        }

        let mut files: Vec<(Resource, KeyFile)> = Vec::new();
        for peer in &peers {
            let peer_path = Path::new(peer.path().as_str());
            // An explicit `format` reads every file, but an extension that
            // implies another key model still refuses the comparison.
            let inferred = infer_format(peer_path);
            let Some(format) = self.format_override.or(inferred) else {
                findings.push(
                    Violation::new(format!(
                        "{}: cannot infer a key format; set `format` explicitly",
                        crate::slash(peer_path)
                    ))
                    .with_path(peer_path.to_path_buf())
                    .as_analysis_failure(AnalysisFailureKind::ResourceParse),
                );
                continue;
            };
            let implied = inferred.unwrap_or(format);
            if implied.model() != self.reference_format.model() {
                findings.push(
                    Violation::new(format!(
                        "{}: format {} has a different key model than the reference {} ({})",
                        crate::slash(peer_path),
                        implied.name(),
                        self.reference.as_str(),
                        self.reference_format.name(),
                    ))
                    .with_path(peer_path.to_path_buf())
                    .as_analysis_failure(AnalysisFailureKind::ResourceParse),
                );
                continue;
            }
            if let Some(file) = self.read_file(
                ctx,
                read_cache,
                decode_cache,
                &limits,
                peer,
                format,
                false,
                &mut findings,
            ) {
                files.push((peer.clone(), file));
            }
        }
        if self.strip_root {
            // Every file with keys must have one root to strip; a file
            // without one fails rather than silently changing what the
            // others compare. A file with no keys reports every key missing.
            let reference_path = Path::new(self.reference.as_str());
            if let Some(reason) = strip_root_blocker(&reference_file) {
                findings.push(evaluation_failure(reference_path, &reason));
                return Ok(findings);
            }
            files.retain(|(peer, file)| match strip_root_blocker(file) {
                None => true,
                Some(reason) => {
                    findings.push(evaluation_failure(Path::new(peer.path().as_str()), &reason));
                    false
                }
            });
        }
        if files.is_empty() {
            return Ok(findings);
        }

        let reference_path = Path::new(self.reference.as_str());
        let reference_base = base_keys(&reference_file, self.strip_root);
        // Plural groups depend on both files of a comparison, so only exact
        // keys can be shared across peers. The trace shows the reference's
        // own grouping.
        let (shared, reference_trace) = match self.plurals {
            Plurals::Exact => {
                let keys = self.without_ignored(reference_base.clone());
                let rows = match key_rows(&reference, &keys) {
                    Ok(rows) => rows,
                    Err(message) => {
                        findings.push(evaluation_failure(reference_path, &message));
                        return Ok(findings);
                    }
                };
                (Some((keys.clone(), rows)), keys)
            }
            Plurals::Cldr => (None, self.pair_keys(&reference_base, &reference_base).0),
        };
        // What the comparisons keep of the reference for every peer takes
        // the run's bytes. What one comparison builds of a peer (its key
        // table, both files' rows and the analyzer's copy of them) is let go
        // before the next, and is a small multiple of what the two files'
        // decodes hold, which the run's bytes took as they were built.
        let kept = table_bytes(reference_base.len())
            + table_bytes(reference_trace.len())
            + shared
                .as_ref()
                .map_or(0, |(keys, rows)| table_bytes(keys.len()) + rows_bytes(rows));
        if let Err(limit) = decode_cache.take_bytes(kept) {
            findings.push(limit_failure(reference_path, &limit));
            return Ok(findings);
        }
        record_key_trace(
            ctx,
            reference_path,
            self.reference_format.model(),
            &reference_trace,
        );
        let finding_cap = limits.get(LimitKind::FindingsPerRule);
        let mut budget = FindingBudget::new(finding_cap);
        for (peer, file) in &files {
            if budget.breached() {
                if self.plurals == Plurals::Exact
                    && file.is_empty_for(self.strip_root)
                    && matches!(self.direction, Direction::Both | Direction::Missing)
                {
                    budget.suppress(reference_trace.len(), false);
                    continue;
                }
                budget.suppress(0, true);
                break;
            }
            let peer_base = base_keys(file, self.strip_root);
            let paired;
            let (reference_keys, reference_rows, peer_keys) = if let Some((keys, rows)) = &shared {
                (keys, rows, self.without_ignored(peer_base))
            } else {
                let (reference_keys, peer_keys) = self.pair_keys(&reference_base, &peer_base);
                let rows = match key_rows(&reference, &reference_keys) {
                    Ok(rows) => rows,
                    Err(message) => {
                        findings.push(evaluation_failure(reference_path, &message));
                        return Ok(findings);
                    }
                };
                paired = (reference_keys, rows);
                (&paired.0, &paired.1, peer_keys)
            };
            record_key_trace(
                ctx,
                Path::new(peer.path().as_str()),
                self.reference_format.model(),
                &peer_keys,
            );
            let compared = Compared {
                file: &reference_file,
                keys: reference_keys,
                rows: reference_rows,
            };
            self.compare(
                &limits,
                &compared,
                peer,
                file,
                &peer_keys,
                &mut budget,
                &mut findings,
            );
        }
        if let Some(failure) = budget.truncation_failure(
            reference_path,
            limits.is_redacted(LimitKind::FindingsPerRule),
        ) {
            findings.push(failure);
        }
        Ok(findings)
    }
}

impl KeyParityRule {
    fn peers_scope_label(&self) -> &str {
        &self.peers
    }

    /// Both sides of one comparison. CLDR plural groups are decided over the
    /// two files together, so a group that is partial in either file (a
    /// locale with fewer categories, or a missing `other`) merges the same
    /// way in both and a base that is a real key in either is never merged.
    fn pair_keys<'a>(&self, reference: &Keys<'a>, peer: &Keys<'a>) -> (Keys<'a>, Keys<'a>) {
        if self.plurals != Plurals::Cldr {
            return (
                self.without_ignored(reference.clone()),
                self.without_ignored(peer.clone()),
            );
        }
        let (reference, peer) = crate::plural_groups::pair(reference, peer);
        (self.without_ignored(reference), self.without_ignored(peer))
    }

    fn without_ignored<'a>(&self, mut keys: Keys<'a>) -> Keys<'a> {
        if !self.ignore.is_empty() {
            keys.retain(|key, _| !self.ignore.iter().any(|pattern| pattern.matches(key)));
        }
        keys
    }

    #[allow(clippy::too_many_arguments)]
    fn read_file(
        &self,
        ctx: &Context<'_>,
        read_cache: &alint_core::relation::ReadCache,
        decode_cache: &alint_core::relation::DecodeCache,
        limits: &Limits,
        resource: &Resource,
        format: KeyFormat,
        required: bool,
        findings: &mut Vec<Violation>,
    ) -> Option<KeyFile> {
        let path = Path::new(resource.path().as_str());
        let read = read_cache.read(ctx.root, resource, 0);
        let bytes = match read.as_ref() {
            ResourceRead::Present(bytes) => Arc::clone(bytes),
            ResourceRead::Missing => {
                let role = if required {
                    Some("reference")
                } else if resource.origin() == Origin::Literal {
                    Some("peer")
                } else {
                    None
                };
                findings.push(match role {
                    Some("peer") if self.allow_missing_peers => Violation::new(format!(
                        "{}: peer file is not present, so nothing was compared",
                        crate::slash(path)
                    ))
                    .with_path(path.to_path_buf())
                    .as_note(),
                    Some(role) => {
                        Violation::new(format!("{}: {role} file is missing", crate::slash(path)))
                            .with_path(path.to_path_buf())
                            .as_analysis_failure(AnalysisFailureKind::ResourceRead)
                    }
                    None => alint_core::missing_file_violation(path),
                });
                return None;
            }
            ResourceRead::TooLarge(size) => {
                findings.push(alint_core::too_large_file_violation(path, *size));
                return None;
            }
            ResourceRead::Unreadable(message) => {
                findings.push(
                    Violation::new(format!(
                        "{}: could not read file: {message}",
                        crate::slash(path)
                    ))
                    .with_path(path.to_path_buf())
                    .as_analysis_failure(AnalysisFailureKind::ResourceRead),
                );
                return None;
            }
        };
        let adapter = match format.adapter(self.strict && format == KeyFormat::Json) {
            Ok(adapter) => adapter,
            Err(error) => {
                record_parse_failure(ctx);
                findings.push(analysis_failure(path, &error.to_string()));
                return None;
            }
        };
        let redacts = adapter.redacts_by_default();
        let output = match decode_cache.decode(adapter.as_ref(), resource, &bytes, limits) {
            Ok(output) => output,
            Err(error) => {
                record_parse_failure(ctx);
                let mut finding = analysis_failure(path, &error.to_string());
                if redacts {
                    finding.redact_message(&self.id);
                }
                findings.push(finding);
                return None;
            }
        };
        let leaves = leaf_rows(&output);
        let Ok(text) = std::str::from_utf8(&bytes) else {
            findings.push(analysis_failure(
                path,
                "the entry adapter produced a document that is not valid UTF-8",
            ));
            return None;
        };
        let file = KeyFile {
            format,
            decode: output,
            leaves,
            text: Arc::from(text),
            line_starts: line_starts(text),
        };
        // The decode's key paths took the run's bytes as they were built;
        // what the file holds beside them, for the length of the
        // comparisons, takes them too.
        if let Err(limit) = decode_cache.take_bytes(file.held_bytes()) {
            findings.push(limit_failure(path, &limit));
            return None;
        }
        Some(file)
    }

    #[allow(clippy::too_many_arguments)]
    fn compare(
        &self,
        limits: &Limits,
        reference: &Compared<'_>,
        peer: &Resource,
        peer_file: &KeyFile,
        peer_keys: &Keys<'_>,
        budget: &mut FindingBudget,
        findings: &mut Vec<Violation>,
    ) {
        let peer_path = Path::new(peer.path().as_str());
        let reference_path = Path::new(self.reference.as_str());
        let mut rows = reference.rows.to_vec();
        match key_rows(peer, peer_keys) {
            Ok(peer_rows) => rows.extend(peer_rows),
            Err(message) => {
                findings.push(evaluation_failure(peer_path, &message));
                return;
            }
        }
        let diffs = match ContractAnalyzer::new(KeySetAnalyzer {
            reference: self.reference.as_str().to_string(),
            peer: peer.path().as_str().to_string(),
            direction: self.direction,
            max_findings: budget.remaining(),
        })
        .analyze("key-parity", &[&self.reference, peer.path()], &rows, limits)
        {
            Ok(diffs) => diffs,
            Err(error) => {
                findings.push(evaluation_failure(
                    peer_path,
                    &format!("the key comparison failed: {error}"),
                ));
                return;
            }
        };
        let (missing, extra, truncated) = match classify(&diffs) {
            Ok(sets) => sets,
            Err(message) => {
                findings.push(evaluation_failure(peer_path, message));
                return;
            }
        };
        // Sets hold key identities; findings show each key as its format
        // writes it (ADR-0025).
        let model = self.reference_format.model();
        if matches!(self.direction, Direction::Both | Direction::Missing) {
            for (key, span) in reference.keys {
                if !missing.contains(&render_key(key)) {
                    continue;
                }
                if budget.remaining() == 0 {
                    budget.suppress(1, false);
                    continue;
                }
                let shown = display_key(model, key);
                let mut finding =
                    Violation::new(self.message(&shown, peer_path, reference_path, "missing"))
                        .with_path(peer_path.to_path_buf())
                        .with_baseline_key(format!(
                            "key-parity\u{0}{}\u{0}{shown}\u{0}missing",
                            crate::slash(peer_path)
                        ));
                if let Some(range) = reference.file.range_at(*span) {
                    finding = finding.with_related_location(
                        RelatedLocation::new(reference_path, "reference key").with_range(range),
                    );
                }
                findings.push(finding);
                budget.record_emitted();
            }
        }
        if matches!(self.direction, Direction::Both | Direction::Extra) {
            for (key, span) in peer_keys {
                if !extra.contains(&render_key(key)) {
                    continue;
                }
                if budget.remaining() == 0 {
                    budget.suppress(1, false);
                    continue;
                }
                let shown = display_key(model, key);
                let mut finding =
                    Violation::new(self.message(&shown, peer_path, reference_path, "extra"))
                        .with_path(peer_path.to_path_buf())
                        .with_baseline_key(format!(
                            "key-parity\u{0}{}\u{0}{shown}\u{0}extra",
                            crate::slash(peer_path)
                        ));
                if let Some(range) = peer_file.range_at(*span) {
                    finding = finding.with_range(range);
                }
                findings.push(finding);
                budget.record_emitted();
            }
        }
        if truncated > 0 {
            budget.suppress(truncated, false);
        }
    }

    fn message(&self, key: &str, peer: &Path, reference: &Path, direction: &str) -> String {
        if let Some(template) = self.message.as_deref() {
            let key = key.to_string();
            let peer = crate::slash(peer);
            let reference = crate::slash(reference);
            let direction = direction.to_string();
            return alint_core::template::render_message(template, |namespace, name| {
                match (namespace, name) {
                    ("ctx", "key") => Some(key.clone()),
                    ("ctx", "peer") => Some(peer.clone()),
                    ("ctx", "reference") => Some(reference.clone()),
                    ("ctx", "direction") => Some(direction.clone()),
                    _ => None,
                }
            });
        }
        match direction {
            "missing" => format!(
                "{}: missing key `{key}` (present in {})",
                crate::slash(peer),
                crate::slash(reference)
            ),
            _ => format!(
                "{}: extra key `{key}` (absent from {})",
                crate::slash(peer),
                crate::slash(reference)
            ),
        }
    }
}

fn record_parse_failure(ctx: &Context<'_>) {
    if ctx.trace.is_some() {
        ctx.record_trace_extraction(alint_core::TraceExtractionObservation {
            inputs: 1,
            parse_failures: 1,
            ..alint_core::TraceExtractionObservation::default()
        });
    }
}

fn record_key_trace(ctx: &Context<'_>, path: &Path, model: KeyModel, keys: &Keys<'_>) {
    if ctx.trace.is_none() {
        return;
    }
    ctx.record_trace_extraction(alint_core::TraceExtractionObservation {
        inputs: 1,
        values: u64::try_from(keys.len()).expect("key count fits u64"),
        ..alint_core::TraceExtractionObservation::default()
    });
    if ctx.trace_preview_limit().is_none() {
        return;
    }
    let sensitivity = ctx.trace_evidence_sensitivity(false);
    for (occurrence, key) in keys.keys().enumerate() {
        let rendered = display_key(model, key);
        ctx.record_trace_preview(alint_core::TracePreviewRow::new(
            path,
            alint_core::TracePathOrigin::RepositorySource,
            None,
            Some(rendered.clone()),
            u64::try_from(occurrence).expect("preview index fits u64"),
            alint_core::TraceProvenance::Structured,
            sensitivity,
            alint_core::TraceValueType::String,
            &rendered,
        ));
    }
}

/// The reference side of a comparison, built once per evaluation.
struct Compared<'a> {
    file: &'a KeyFile,
    keys: &'a Keys<'a>,
    rows: &'a [Row],
}

/// Per-rule finding budget enforcing [`LimitKind::FindingsPerRule`] across
/// peers without allocating unbounded findings.
struct FindingBudget {
    cap: usize,
    emitted: usize,
    suppressed: usize,
    lower_bound: bool,
}

impl FindingBudget {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            emitted: 0,
            suppressed: 0,
            lower_bound: false,
        }
    }

    fn remaining(&self) -> usize {
        self.cap.saturating_sub(self.emitted)
    }

    fn breached(&self) -> bool {
        self.suppressed > 0
    }

    fn record_emitted(&mut self) {
        self.emitted = self.emitted.saturating_add(1);
    }

    fn suppress(&mut self, count: usize, lower_bound: bool) {
        self.suppressed = self.suppressed.saturating_add(count);
        self.lower_bound |= lower_bound;
    }

    fn truncation_failure(&self, reference_path: &Path, redacted: bool) -> Option<Violation> {
        if self.suppressed == 0 {
            return None;
        }
        let message = if redacted {
            "relation limit `findings_per_rule` (<redacted>) reached: remaining findings were \
             omitted to bound run memory and output"
                .to_string()
        } else {
            let omitted = if self.lower_bound {
                format!("at least {}", self.suppressed)
            } else {
                self.suppressed.to_string()
            };
            let noun = if self.emitted == 1 {
                "finding"
            } else {
                "findings"
            };
            format!(
                "relation limit `findings_per_rule` ({}) reached: emitted {} {noun} and \
                 suppressed {omitted} more; remaining findings were omitted to bound run memory \
                 and output",
                self.cap, self.emitted
            )
        };
        Some(evaluation_failure(reference_path, &message))
    }
}

/// Split the analyzer's findings into missing and extra key identities plus
/// any count truncated past the rule's finding cap. A finding of any other
/// shape is an error, never a silently dropped key.
fn classify(
    diffs: &[Violation],
) -> std::result::Result<(BTreeSet<String>, BTreeSet<String>, usize), &'static str> {
    let mut missing = BTreeSet::new();
    let mut extra = BTreeSet::new();
    let mut truncated = 0usize;
    for finding in diffs {
        match finding.message.split_once('\u{0}') {
            Some(("missing", key)) => {
                missing.insert(key.to_string());
            }
            Some(("extra", key)) => {
                extra.insert(key.to_string());
            }
            Some(("truncated", count)) => {
                let parsed = count
                    .parse::<usize>()
                    .map_err(|_| "the key comparison returned an unrecognized finding")?;
                truncated = truncated.saturating_add(parsed);
            }
            _ => return Err("the key comparison returned an unrecognized finding"),
        }
    }
    Ok((missing, extra, truncated))
}

/// An analysis failure for a comparison that could not complete: never an
/// empty result that passes (invariant 2).
fn evaluation_failure(path: &Path, message: &str) -> Violation {
    Violation::new(format!("{}: {message}", crate::slash(path)))
        .with_path(path.to_path_buf())
        .with_not_fixable()
        .as_analysis_failure(AnalysisFailureKind::Evaluation)
}

/// The failure of a comparison the run's byte budget refused: the keys are
/// not compared at all rather than in part (invariant 2).
fn limit_failure(path: &Path, limit: &LimitExceeded) -> Violation {
    evaluation_failure(
        path,
        &format!(
            "{limit}, so the keys are not compared rather than compared in part; a run holds \
             the key paths of each file it decodes and the keys it compares, so compare fewer \
             or smaller files"
        ),
    )
}

fn analysis_failure(path: &Path, message: &str) -> Violation {
    Violation::new(format!(
        "{}: key extraction failed: {message}",
        crate::slash(path)
    ))
    .with_path(path.to_path_buf())
    .with_not_fixable()
    .as_analysis_failure(AnalysisFailureKind::ResourceParse)
}

fn infer_format(path: &Path) -> Option<KeyFormat> {
    let name = path.file_name()?.to_str()?;
    if name == ".env" || name.starts_with(".env.") {
        return Some(KeyFormat::Dotenv);
    }
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "json" => Some(KeyFormat::Json),
        "yaml" | "yml" => Some(KeyFormat::Yaml),
        "toml" => Some(KeyFormat::Toml),
        "properties" => Some(KeyFormat::Properties),
        "ini" => Some(KeyFormat::Ini),
        "resx" => Some(KeyFormat::Resx),
        _ => None,
    }
}

/// The keys with no key below them, array members dropped. A key's
/// descendants follow it in key order, so a key with one is followed by one.
fn leaf_keys<K: Ord + Borrow<[Segment]>, V>(mut keys: BTreeMap<K, V>) -> BTreeMap<K, V> {
    keys.retain(|key, _| {
        !key.borrow()
            .iter()
            .any(|segment| matches!(segment, Segment::Index(_)))
    });
    let mut ordered = keys.keys().map(Borrow::<[Segment]>::borrow).peekable();
    let mut leaf = Vec::with_capacity(keys.len());
    while let Some(parent) = ordered.next() {
        leaf.push(
            !ordered
                .peek()
                .is_some_and(|child| child.len() > parent.len() && child.starts_with(parent)),
        );
    }
    // `retain` visits the keys in order, as `leaf` lists them.
    let mut leaf = leaf.into_iter();
    keys.retain(|_, _| leaf.next().unwrap_or(true));
    keys
}

/// A file's keys in key order. Each borrows its path from the file's decode
/// (or is part of one there) and is copied only to merge an i18next plural
/// group, whose key no file holds.
type Keys<'a> = crate::plural_groups::Keys<'a, ByteSpan>;

/// A decode's leaf key rows, in key order, a repeated key once (its first
/// row), each with its key's span. The keys are compared by the paths the
/// decode holds, never copies of them.
fn leaf_rows(output: &AdapterOutput) -> Vec<(usize, ByteSpan)> {
    let mut keys = BTreeMap::new();
    for (index, row) in output.rows.iter().enumerate() {
        if !row.is_key() {
            continue;
        }
        let Some(pointer) = row.pointer.as_ref() else {
            continue;
        };
        // Array elements are values, not keys, and the document root has
        // no name to compare.
        if pointer.segments().is_empty()
            || pointer
                .segments()
                .iter()
                .any(|segment| matches!(segment, Segment::Index(_)))
        {
            continue;
        }
        let Some(span) = row.span else {
            continue;
        };
        keys.entry(pointer.segments()).or_insert((index, span));
    }
    leaf_keys(keys).into_values().collect()
}

fn root_shape(file: &KeyFile) -> alint_core::extract::StripRootShape {
    let structured = match file.format {
        KeyFormat::Json => alint_core::Format::Json,
        KeyFormat::Yaml => alint_core::Format::Yaml,
        KeyFormat::Toml => alint_core::Format::Toml,
        _ => return alint_core::extract::StripRootShape::SingleRootValue,
    };
    alint_core::extract::strip_root_shape(structured, &file.text)
}

/// Why `strip_root` cannot strip this file, if it cannot: a non-empty file
/// must have a top-level object with at most one root key, and that key must
/// be an object (an empty object such as `fr: {}`, `{"fr": {}}`, or `[fr]` is
/// valid and has zero keys below it). Top-level non-object documents (`null`,
/// arrays, scalars) are rejected instead of silently passing as empty locales.
fn strip_root_blocker(file: &KeyFile) -> Option<String> {
    use alint_core::extract::StripRootShape;

    if file.leaves.is_empty() {
        return match root_shape(file) {
            StripRootShape::EmptyDocument
            | StripRootShape::EmptyObject
            | StripRootShape::Object
            | StripRootShape::SingleRootEmptyObject => None,
            StripRootShape::TopLevelNull => {
                Some("`strip_root` needs a top-level object, but the file is null".to_string())
            }
            StripRootShape::TopLevelArray => {
                Some("`strip_root` needs a top-level object, but the file is an array".to_string())
            }
            StripRootShape::TopLevelValue => {
                Some("`strip_root` needs a top-level object, but the file is a value".to_string())
            }
            StripRootShape::SingleRootNull => Some(
                "`strip_root` needs the top-level key to be an object, but it is null".to_string(),
            ),
            StripRootShape::SingleRootArray => Some(
                "`strip_root` needs the top-level key to be an object, but it is an array"
                    .to_string(),
            ),
            StripRootShape::SingleRootValue => Some(
                "`strip_root` needs the top-level key to be an object, but it is a value"
                    .to_string(),
            ),
        };
    }

    let mut paths = file.leaves.iter().map(|(row, _)| file.path(*row));
    let roots: BTreeSet<&Segment> = paths.clone().filter_map(<[Segment]>::first).collect();
    if roots.len() > 1 {
        return Some(format!(
            "`strip_root` needs one top-level key, but the file has {}",
            roots.len()
        ));
    }
    if !paths.any(|key| key.len() < 2) {
        return None;
    }
    match root_shape(file) {
        StripRootShape::SingleRootEmptyObject => None,
        StripRootShape::SingleRootNull => {
            Some("`strip_root` needs the top-level key to be an object, but it is null".to_string())
        }
        StripRootShape::SingleRootArray => Some(
            "`strip_root` needs the top-level key to be an object, but it is an array".to_string(),
        ),
        _ => Some(
            "`strip_root` needs the top-level key to be an object, but it is a value".to_string(),
        ),
    }
}

/// A file's keys before pairing: below its one root when `strip_root` is set
/// (`strip_root_blocker` has accepted the file).
fn base_keys(file: &KeyFile, strip_root: bool) -> Keys<'_> {
    file.keys(usize::from(strip_root))
}

/// One parsed atom in a key-name glob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GlobAtom {
    /// A literal character (including `\*` escaped inside `['...']`).
    Lit(char),
    /// Unescaped `*`: any run of characters in a tree segment, or any run
    /// without `.` in a single-name key.
    Star,
    /// Unescaped `**` inside a single-name key: any run of characters across `.`.
    DoubleStar,
}

fn parse_bare_glob(pattern: &str) -> Vec<GlobAtom> {
    let mut out = Vec::with_capacity(pattern.len());
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '*' {
            if chars.next_if_eq(&'*').is_some() {
                out.push(GlobAtom::DoubleStar);
            } else {
                out.push(GlobAtom::Star);
            }
        } else {
            out.push(GlobAtom::Lit(c));
        }
    }
    out
}

/// Match a single-name key against a parsed glob: `*` stops at a `.`, `**`
/// crosses it, and every [`GlobAtom::Lit`] matches literally (including `\*`).
/// Iterative so a long config pattern cannot overflow the stack.
fn key_glob_match(pattern: &[GlobAtom], text: &str) -> bool {
    let text: Vec<char> = text.chars().collect();
    if pattern.iter().all(|atom| matches!(atom, GlobAtom::Lit(_))) {
        return pattern.len() == text.len()
            && pattern
                .iter()
                .zip(&text)
                .all(|(atom, ch)| *atom == GlobAtom::Lit(*ch));
    }
    let n = text.len();
    let mut next = vec![false; n + 1];
    next[n] = true;
    let mut current = vec![false; n + 1];
    for atom in pattern.iter().rev() {
        for j in (0..=n).rev() {
            current[j] = match *atom {
                GlobAtom::DoubleStar => next[j] || (j < n && current[j + 1]),
                GlobAtom::Star => next[j] || (j < n && text[j] != '.' && current[j + 1]),
                GlobAtom::Lit(lit) => j < n && text[j] == lit && next[j + 1],
            };
        }
        std::mem::swap(&mut next, &mut current);
    }
    next[0]
}

/// A key's identity: the injective [`Pointer::render`] form the comparison
/// uses, so `nav['a.b']` and `nav.a.b` stay distinct.
fn render_key(key: &[Segment]) -> String {
    Pointer::render_segments(key)
}

/// A key as findings, baseline keys, and traces show it (ADR-0025). A key of
/// a single-name format is its name as written (`app.title`); every other key
/// is its identity. A name that is empty or starts with `[` keeps the bracket
/// form, so the display stays injective: a plain name never starts with `[`
/// and a bracket form always does.
fn display_key(model: KeyModel, key: &[Segment]) -> String {
    if model.single_name()
        && let [Segment::Key(name)] = key
        && !name.is_empty()
        && !name.starts_with('[')
    {
        return name.to_string();
    }
    render_key(key)
}

/// One `ignore_keys` pattern, parsed for the rule's key model.
#[derive(Debug, Clone, PartialEq, Eq)]
enum KeyPattern {
    /// A glob over a single-name key ([`key_glob_match`]).
    Name(Vec<GlobAtom>),
    /// A key path, one entry per segment.
    Path(Vec<PatternSegment>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PatternSegment {
    /// `**`: any number of segments; at the end of a longer pattern, at
    /// least one (`nav.**` matches below `nav`, not `nav` itself).
    Any,
    /// One segment whose name matches this glob, where unescaped `*` is any run
    /// of characters and `\*` inside `['...']` is literal `*`.
    Name(Vec<GlobAtom>),
}

impl KeyPattern {
    fn parse(pattern: &str, model: KeyModel, rendered: bool) -> std::result::Result<Self, String> {
        if !model.single_name() {
            return parse_path_pattern(pattern).map(Self::Path);
        }
        // A name findings show in bracket form (`['']`, `['[x']`) is written
        // that way here too. When a nested rule rendered path tokens via
        // `render_path_key_pattern` (e.g. `['v1.0'].TOKEN` or `app['v1.0'].TOKEN`
        // for a dotted directory name in a flat format), rejoin the segments
        // with literal `.` so the rendered single-name glob still parses and
        // matches cleanly.
        if pattern.starts_with("['") || (rendered && pattern.contains("['")) {
            let segments = parse_path_pattern(pattern)?;
            if !rendered {
                return match segments.as_slice() {
                    [PatternSegment::Name(atoms)] => Ok(Self::Name(atoms.clone())),
                    _ => Err("a bracketed name must be one `['...']`".to_string()),
                };
            }
            let mut joined = Vec::new();
            for (idx, seg) in segments.into_iter().enumerate() {
                if idx > 0 {
                    joined.push(GlobAtom::Lit('.'));
                }
                match seg {
                    PatternSegment::Any => joined.push(GlobAtom::DoubleStar),
                    PatternSegment::Name(atoms) => joined.extend(atoms),
                }
            }
            return Ok(Self::Name(joined));
        }
        Ok(Self::Name(parse_bare_glob(pattern)))
    }

    fn matches(&self, key: &[Segment]) -> bool {
        match (self, key) {
            (Self::Name(glob), [Segment::Key(name)]) => key_glob_match(glob, name),
            (Self::Name(_), _) => false,
            (Self::Path(pattern), _) => path_pattern_match(pattern, key),
        }
    }
}

/// Parse a key-path pattern in the notation findings use: names joined by
/// `.`, and `['name']` for a segment with a dot, bracket, quote, or nothing in
/// it (`\` escapes a quote, backslash, or `*`). An index `[n]` is refused:
/// array elements are values, so no key contains one.
fn parse_path_pattern(pattern: &str) -> std::result::Result<Vec<PatternSegment>, String> {
    let mut segments = Vec::new();
    let mut chars = pattern.chars().peekable();
    loop {
        if chars.next_if_eq(&'[').is_some() {
            if chars.next_if_eq(&'\'').is_some() {
                let mut atoms = Vec::new();
                loop {
                    match chars.next() {
                        Some('\\') => {
                            let escaped = chars.next().ok_or("a `\\` escape ends the pattern")?;
                            atoms.push(GlobAtom::Lit(escaped));
                        }
                        Some('\'') => break,
                        Some('*') => {
                            if chars.next_if_eq(&'*').is_some() {
                                atoms.push(GlobAtom::DoubleStar);
                            } else {
                                atoms.push(GlobAtom::Star);
                            }
                        }
                        Some(c) => atoms.push(GlobAtom::Lit(c)),
                        None => return Err("a `['` name is not closed".to_string()),
                    }
                }
                if chars.next() != Some(']') {
                    return Err("a `['name'` must be followed by `]`".to_string());
                }
                segments.push(PatternSegment::Name(atoms));
            } else if chars.next_if(char::is_ascii_digit).is_some() {
                return Err(
                    "an index `[n]` never matches: array elements are values, not keys".to_string(),
                );
            } else {
                return Err("`[` must start a `['name']` segment".to_string());
            }
        } else {
            let mut name = String::new();
            while let Some(c) = chars.next_if(|c| *c != '.' && *c != '[') {
                name.push(c);
            }
            if name.is_empty() {
                return Err("a segment is empty; write `['']` for an empty key".to_string());
            }
            segments.push(if name == "**" {
                PatternSegment::Any
            } else {
                PatternSegment::Name(parse_bare_glob(&name))
            });
        }
        match chars.peek() {
            None => return Ok(segments),
            Some('.') => {
                chars.next();
                if chars.peek().is_none() {
                    return Err("the pattern ends with `.`".to_string());
                }
            }
            Some('[') => {}
            Some(c) => return Err(format!("expected `.` or `[` after `]`, found {c:?}")),
        }
    }
}

/// Whether a key path matches a parsed pattern. Iterative over the pattern,
/// so neither a long pattern nor a deep key can exhaust the stack.
fn path_pattern_match(pattern: &[PatternSegment], key: &[Segment]) -> bool {
    // A leading name must match the first segment, which settles most
    // patterns before the table.
    if let Some(PatternSegment::Name(glob)) = pattern.first()
        && !matches!(key.first(), Some(Segment::Key(name)) if wildcard_match(glob, name))
    {
        return false;
    }
    // `next[j]`: the rest of the pattern after the current part matches `key[j..]`.
    let mut next = vec![false; key.len() + 1];
    next[key.len()] = true;
    let mut current = vec![false; key.len() + 1];
    for (i, part) in pattern.iter().enumerate().rev() {
        let trailing = i + 1 == pattern.len() && i > 0;
        for j in (0..=key.len()).rev() {
            current[j] = match part {
                PatternSegment::Any if trailing => j < key.len(),
                PatternSegment::Any => next[j] || (j < key.len() && current[j + 1]),
                PatternSegment::Name(glob) => {
                    j < key.len()
                        && next[j + 1]
                        && matches!(&key[j], Segment::Key(name) if wildcard_match(glob, name))
                }
            };
        }
        std::mem::swap(&mut next, &mut current);
    }
    next[0]
}

/// Match one segment name against a parsed glob where unescaped `*` / `**` is
/// any run of characters and [`GlobAtom::Lit`] matches literally (including `\*`).
fn wildcard_match(pattern: &[GlobAtom], text: &str) -> bool {
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some(GlobAtom::Star | GlobAtom::DoubleStar) => {
                star = Some((p, t));
                p += 1;
            }
            Some(GlobAtom::Lit(lit)) if *lit == text[t] => {
                p += 1;
                t += 1;
            }
            _ => {
                if let Some((star_p, star_t)) = star {
                    p = star_p + 1;
                    t = star_t + 1;
                    star = Some((star_p, star_t + 1));
                } else {
                    return false;
                }
            }
        }
    }
    pattern[p..]
        .iter()
        .all(|atom| matches!(atom, GlobAtom::Star | GlobAtom::DoubleStar))
}

pub static META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::FullIndex { path_scope: true },
    paths: alint_core::PathsSupport::Forbidden,
    scope_filter: alint_core::ScopeFilterSupport::Rejected {
        reason: std::borrow::Cow::Borrowed("scope_filter is not supported by this kind"),
    },
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("reference"),
            selector_type: alint_core::SelectorType::LiteralPath,
            rerootable: false,
        },
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("peers"),
            selector_type: alint_core::SelectorType::Glob,
            rerootable: true,
        },
    ]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[
        alint_core::Format::Json,
        alint_core::Format::Yaml,
        alint_core::Format::Toml,
        alint_core::Format::Dotenv,
        alint_core::Format::Properties,
        alint_core::Format::Ini,
    ]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[]),
    expect_matches: false,
    trace: alint_core::TraceMeta::EXTRACTION,
    nested_rules: false,
};

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    Ok(Box::new(build_rule(spec)?))
}

fn build_rule(spec: &RuleSpec) -> Result<KeyParityRule> {
    build_rule_with(spec, std::path::is_separator('\\'))
}

fn validate_key_parity_format_flags(
    rule_id: &str,
    opts: &Options,
    format_pending: bool,
    reference_format: KeyFormat,
    model_format: Option<KeyFormat>,
) -> Result<()> {
    if opts.strip_root
        && let Some(format) = model_format
        && format.model() != KeyModel::Tree
    {
        return Err(Error::rule_config(
            rule_id,
            format!(
                "`strip_root` applies only to JSON, YAML, and TOML files, not {}",
                format.name()
            ),
        ));
    }
    if opts.strict.is_some() {
        if let Some(format) = opts.format
            && format != KeyFormat::Json
        {
            return Err(Error::rule_config(
                rule_id,
                format!("`strict` applies only to JSON files, not {}", format.name()),
            ));
        }
        if !format_pending && reference_format != KeyFormat::Json {
            return Err(Error::rule_config(
                rule_id,
                format!(
                    "`strict` applies only to JSON files, not {}",
                    reference_format.name()
                ),
            ));
        }
        if let Some(format) = model_format
            && format != KeyFormat::Json
        {
            return Err(Error::rule_config(
                rule_id,
                format!("`strict` applies only to JSON files, not {}", format.name()),
            ));
        }
    }
    Ok(())
}

/// [`build_rule`], reading `\` in `reference` and a literal `peers` path as a
/// separator when `backslash_separates` ([`config_path`]).
fn build_rule_with(spec: &RuleSpec, backslash_separates: bool) -> Result<KeyParityRule> {
    alint_core::reject_scope_filter_on_cross_file(spec, "key_parity")?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|error| Error::rule_config(&spec.id, format!("invalid options: {error}")))?;
    let reference = config_path(opts.reference.trim(), backslash_separates).ok_or_else(|| {
        Error::rule_config(&spec.id, "`reference` must be a confined repository path")
    })?;
    crate::pathsafe::reject_git_directory_read(&spec.id, "reference", reference.as_str())?;
    let reference_written = as_written(&opts.reference, false, backslash_separates);
    if opts.peers.trim().is_empty() {
        return Err(Error::rule_config(&spec.id, "`peers` must not be empty"));
    }
    if opts.peers.trim() != opts.peers {
        return Err(Error::rule_config(
            &spec.id,
            "`peers` must not start or end with whitespace",
        ));
    }
    if opts.peers == reference.as_str() {
        return Err(Error::rule_config(
            &spec.id,
            "`peers` must select at least one file other than the reference",
        ));
    }
    // A nested rule is first built from placeholder path tokens. When a token
    // decides the reference's format, the format is known only once an
    // iteration renders the name, and that iteration's build infers it.
    let format_pending = opts.format.is_none()
        && infer_format(Path::new(reference.as_str())).is_none()
        && format_from_token(&reference_written);
    let reference_format = match opts.format {
        Some(format) => format,
        None if format_pending => KeyFormat::Json,
        None => infer_format(Path::new(reference.as_str()))
            .ok_or_else(|| Error::rule_config(&spec.id, no_reference_format(&reference)))?,
    };
    if let Some(format) = opts.format
        && let Some(inferred) = infer_format(Path::new(reference.as_str()))
        && inferred.model() != format.model()
    {
        return Err(Error::rule_config(
            &spec.id,
            format!(
                "explicit `format` {} has a different key model than the reference extension ({})",
                format.name(),
                inferred.name()
            ),
        ));
    }
    // With the format pending, a literal peer whose name no token decides
    // stands in for it: every iteration whose reference has a different key
    // model fails the peer's model check.
    let model_format = if format_pending {
        literal_peer_format(&opts.peers)
    } else {
        Some(reference_format)
    };
    validate_key_parity_format_flags(
        &spec.id,
        &opts,
        format_pending,
        reference_format,
        model_format,
    )?;
    let ignore = ignore_patterns(spec, &opts.ignore_keys, model_format)?;
    let known_format = (!format_pending).then_some(reference_format);
    let literal_peer = literal_peer(
        spec,
        &opts,
        (&reference, &reference_written),
        known_format,
        backslash_separates,
    )?;
    let peers_selector = Selector::for_glob(&opts.peers)?;
    // The reference is a literal path: escape it, so a name such as
    // `locales/[id].json` matches its own `--changed` edit. Git cannot report
    // an edit to a gitignored literal peer such as `.env`, so a rule with a
    // literal peer runs whenever anything changed.
    let scope = if literal_peer.is_some() {
        Scope::match_all()
    } else {
        Scope::from_patterns(&[
            alint_core::template::glob_escape(reference.as_str()),
            opts.peers.clone(),
        ])?
    };
    Ok(KeyParityRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        reference,
        reference_format,
        format_pending,
        format_override: opts.format,
        peers: opts.peers,
        peers_selector,
        literal_peer,
        scope,
        direction: opts.direction,
        plurals: opts.plurals,
        strip_root: opts.strip_root,
        ignore,
        allow_missing_peers: opts.allow_missing_peers,
        strict: opts.strict.unwrap_or(false),
    })
}

/// The `ignore_keys` patterns, parsed for the reference's key model. With no
/// model known yet (`None`: the format is pending and no literal peer stands
/// in), a pattern must parse under some key model; the iteration's build
/// parses it under the one its format has.
fn ignore_patterns(
    spec: &RuleSpec,
    patterns: &[String],
    reference_format: Option<KeyFormat>,
) -> Result<Vec<KeyPattern>> {
    if patterns.iter().any(|pattern| pattern.len() > 1024) {
        return Err(Error::rule_config(
            &spec.id,
            "`ignore_keys` patterns must be at most 1024 bytes",
        ));
    }
    let models: &[KeyModel] = match reference_format {
        Some(format) => &[format.model()],
        None => &[KeyModel::Tree, KeyModel::Flat],
    };
    patterns
        .iter()
        .map(|pattern| {
            let mut first_error = None;
            for &model in models {
                match KeyPattern::parse(pattern, model, spec.rendered) {
                    Ok(parsed) => return Ok(parsed),
                    Err(error) => {
                        first_error.get_or_insert(error);
                    }
                }
            }
            Err(Error::rule_config(
                &spec.id,
                format!(
                    "`ignore_keys` pattern {pattern:?}: {}",
                    first_error.unwrap_or_default()
                ),
            ))
        })
        .collect()
}

/// A `peers` value with no glob metacharacters, as a confined path other than
/// the reference (given with what [`as_written`] made of it). Its extension
/// implies a key model (without an explicit `format`, it picks the format), so
/// a key-model mismatch with a known reference format is a config error here
/// rather than an analysis failure when it is read. With the reference format
/// pending, each iteration's build makes that check.
fn literal_peer(
    spec: &RuleSpec,
    opts: &Options,
    reference: (&RelPath, &[String]),
    reference_format: Option<KeyFormat>,
    backslash_separates: bool,
) -> Result<Option<RelPath>> {
    let Some(path) = alint_core::template::literal_glob_path(&opts.peers) else {
        return Ok(None);
    };
    let path = config_path(&path, backslash_separates).ok_or_else(|| {
        Error::rule_config(
            &spec.id,
            "a literal `peers` path must be a confined repository path",
        )
    })?;
    crate::pathsafe::reject_git_directory_read(&spec.id, "peers", path.as_str())?;
    let written = as_written(&opts.peers, true, backslash_separates);
    if same_in_every_iteration((&path, &written), reference) {
        return Err(Error::rule_config(
            &spec.id,
            "`peers` must select at least one file other than the reference",
        ));
    }
    let inferred = infer_format(Path::new(path.as_str()));
    let from_token = format_from_token(&written);
    if opts.format.is_none() && inferred.is_none() && !from_token {
        return Err(Error::rule_config(
            &spec.id,
            "cannot infer the key format of the literal `peers` path; set `format` explicitly",
        ));
    }
    if let Some(format) = inferred
        && let Some(reference_format) = reference_format
        && format.model() != reference_format.model()
    {
        return Err(Error::rule_config(
            &spec.id,
            format!(
                "the literal `peers` path ({}) has a different key model than the reference ({})",
                format.name(),
                reference_format.name()
            ),
        ));
    }
    Ok(Some(path))
}

/// The file names [`infer_format`] knows, for an error message.
const FORMAT_NAMES: &str = "`.env`, `.env.*`, `*.json`, `*.yaml`, `*.yml`, `*.toml`, \
                            `*.properties`, `*.ini` or `*.resx`";

fn no_reference_format(reference: &RelPath) -> String {
    format!(
        "cannot infer the key format of `reference` {:?}: the name is not {FORMAT_NAMES}; \
         set `format`, or, in a nested rule, select only files with such a name",
        reference.as_str()
    )
}

/// A `reference` or literal `peers` path as the index spells it. Path tokens
/// render with the host's separator (`{path}` is `apps\web\en.json` on
/// Windows) and the index joins names with `/`, so where `\` is a separator
/// (`backslash_separates`, as on Windows) it splits names as `/` does; elsewhere
/// it is part of a name.
fn config_path(text: &str, backslash_separates: bool) -> Option<RelPath> {
    if backslash_separates {
        RelPath::parse(&text.replace('\\', "/"))
    } else {
        RelPath::parse(text)
    }
}

/// Marks each path token in what [`as_written`] returns: the token's name
/// between two of these, a private-use character that a path in a
/// configuration has no reason to hold.
const MARK: char = '\u{E000}';

fn marked(name: &str) -> String {
    format!("{MARK}{name}{MARK}")
}

/// What the configuration wrote for `rendered`, the value of `reference` (or
/// of `peers`, a glob, when `glob`), in a nested rule's preparation build:
/// each template that renders it with the placeholder tokens
/// ([`alint_core::template::templates_rendering`]), rendered again with each
/// token marked by [`MARK`], as the path [`config_path`] reads (for `peers`,
/// the literal path it names). Unlike a placeholder, a mark cannot come from
/// text the configuration wrote, so `data/_stem_` has no token. Empty outside
/// a preparation, where no part of a value comes from a token.
fn as_written(rendered: &str, glob: bool, backslash_separates: bool) -> Vec<String> {
    let tokens = PathTokens {
        path: marked("path"),
        dir: marked("dir"),
        basename: marked("basename"),
        stem: marked("stem"),
        ext: marked("ext"),
        parent_name: marked("parent_name"),
    };
    alint_core::template::templates_rendering(rendered)
        .iter()
        .filter_map(|template| {
            let path = if glob {
                alint_core::template::literal_glob_path(&render_path_glob(template, &tokens))?
            } else {
                render_path(template, &tokens).trim().to_owned()
            };
            config_path(&path, backslash_separates).map(|path| path.as_str().to_owned())
        })
        .collect()
}

/// Whether a path token decides the format [`infer_format`] gives the file
/// name of a path [`as_written`] marked, for any of `written`: the extension
/// comes from a token, the name has no literal extension, or a token can make
/// the name start with `.env.`. Each iteration's build sees the real name. A
/// name such as `strings-{stem}.xml` has the same unsupported extension in
/// every iteration.
fn format_from_token(written: &[String]) -> bool {
    written.iter().any(|written| {
        let path = Path::new(written);
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return false;
        };
        let Some(first) = name.find(MARK) else {
            return false;
        };
        match path.extension().and_then(|ext| ext.to_str()) {
            None => true,
            Some(ext) if ext.contains(MARK) => true,
            Some(_) => ".env.".starts_with(&name[..first]),
        }
    })
}

/// Whether a literal peer names the reference in every iteration, each given
/// as a path with what [`as_written`] made of it. Outside a preparation build
/// the paths are compared. In one, every template written for the peer must
/// name what every template written for the reference names, `{path}` read as
/// `{dir}/{basename}`: when the configuration also spells a placeholder in
/// another string, a load error is only for what holds whichever it wrote.
fn same_in_every_iteration(peer: (&RelPath, &[String]), reference: (&RelPath, &[String])) -> bool {
    if peer.1.is_empty() || reference.1.is_empty() {
        return peer.0 == reference.0;
    }
    let path = marked("path");
    let spelled_out = format!("{}/{}", marked("dir"), marked("basename"));
    let normal = |written: &String| written.replace(&path, &spelled_out);
    peer.1.iter().all(|peer| {
        reference
            .1
            .iter()
            .all(|reference| normal(peer) == normal(reference))
    })
}

/// The format a literal `peers` path's name implies. As for the reference, a
/// literal extension decides it at load (`{dir}/{stem}.json` is JSON); a name
/// whose format a token decides implies none until an iteration renders it.
fn literal_peer_format(peers: &str) -> Option<KeyFormat> {
    infer_format(Path::new(&alint_core::template::literal_glob_path(peers)?))
}

#[cfg(test)]
#[path = "key_parity_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "key_parity_pilot_tests.rs"]
mod analyzer_pilot;

#[cfg(test)]
#[path = "key_parity_budget_tests.rs"]
mod budget_tests;
