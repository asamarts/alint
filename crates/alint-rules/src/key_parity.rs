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

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use alint_core::plugin_api::{Analyzer, AnalyzerError, AnalyzerInput, Finding, PointerSegment};
use alint_core::relation::{
    Adapter, AndroidStringsAdapter, ContractAnalyzer, EntryRowsAdapter, Limits, Origin, Pointer,
    RelPath, Resource, ResourceRead, ResxAdapter, Row, RowState, Segment, Selector,
    TomlRowsAdapter,
};
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
    fn adapter(self) -> Result<Box<dyn Adapter>, alint_core::relation::AdapterError> {
        match self {
            Self::Json => Ok(Box::new(EntryRowsAdapter::new(alint_core::Format::Json)?)),
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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
    /// When every compared file has one top-level key (Rails' `en:`), compare
    /// below it.
    #[serde(default)]
    strip_root: bool,
    /// Key-path globs to omit: `*` within a segment, `**` across segments.
    #[serde(default)]
    ignore_keys: Vec<String>,
}

crate::options_schema_for!(Options);

#[derive(Debug, Clone)]
struct KeyFile {
    keys: BTreeMap<Vec<Segment>, ByteSpan>,
    text: Arc<str>,
    line_starts: Vec<usize>,
}

impl KeyFile {
    #[cfg(test)]
    fn range(&self, key: &[Segment]) -> Option<SourceRange> {
        self.range_at(*self.keys.get(key)?)
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
pub struct KeyParityRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    reference: RelPath,
    reference_format: KeyFormat,
    format_override: Option<KeyFormat>,
    peers: String,
    peers_scope: Scope,
    /// The peer a `peers` value without glob metacharacters names. It is read
    /// from disk even when the index leaves it out, so a gitignored `.env`
    /// can be a peer of `.env.example`.
    literal_peer: Option<RelPath>,
    scope: Scope,
    direction: Direction,
    plurals: Plurals,
    strip_root: bool,
    ignore: Vec<String>,
}

/// The key-parity comparison as a pure contract analyzer over one reference
/// and one peer. `KeyParityRule` uses this for diff detection, then attaches
/// its own messages, spans, and baseline keys.
///
/// Keys are compared by their injective rendering ([`Pointer::render`]), the
/// same one the rule uses for messages and baseline keys, so `nav['a.b']` and
/// `nav.a.b` stay distinct. The analyzer knows the peer's path, so a peer with
/// no rows (an empty file, or one whose every key `ignore_keys` removed) still
/// reports every reference key as missing.
struct KeySetAnalyzer {
    reference: String,
    peer: String,
    direction: Direction,
}

impl Analyzer for KeySetAnalyzer {
    fn id(&self) -> &'static str {
        "key-parity-keys"
    }

    fn analyze(&self, input: AnalyzerInput<'_>) -> Result<Vec<Finding>, AnalyzerError> {
        let mut reference = BTreeSet::new();
        let mut peer = BTreeSet::new();
        for row in input.rows {
            let Some(pointer) = &row.pointer else {
                return Err(AnalyzerError::InvalidInput(format!(
                    "a key row of {} has no key path",
                    row.resource.path
                )));
            };
            let key = render_contract_pointer(pointer)?;
            if row.resource.path == self.reference {
                reference.insert(key);
            } else if row.resource.path == self.peer {
                peer.insert(key);
            } else {
                return Err(AnalyzerError::InvalidInput(format!(
                    "a key row of {} belongs to neither compared file",
                    row.resource.path
                )));
            }
        }
        let mut findings = Vec::new();
        let mut emit = |key: &str, direction: &str| {
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
            for key in reference.difference(&peer) {
                emit(key, "missing");
            }
        }
        if matches!(self.direction, Direction::Both | Direction::Extra) {
            for key in peer.difference(&reference) {
                emit(key, "extra");
            }
        }
        Ok(findings)
    }
}

/// Render a contract key path exactly as [`render_key`] renders the internal
/// one, so analyzer findings and rule-side keys compare as equal strings.
fn render_contract_pointer(pointer: &[PointerSegment]) -> Result<String, AnalyzerError> {
    let mut segments = Vec::with_capacity(pointer.len());
    for part in pointer {
        segments.push(match part {
            PointerSegment::Key(key) => Segment::Key(key.as_str().into()),
            PointerSegment::Index(index) => {
                Segment::Index(usize::try_from(*index).map_err(|_| {
                    AnalyzerError::InvalidInput(format!("key index {index} does not fit usize"))
                })?)
            }
        });
    }
    Ok(render_key(&segments))
}

/// Contract key rows for one compared file. Every key becomes a row; a file
/// with more keys than a row ordinal can number is an error, never a
/// silently shortened key set.
fn key_rows(
    resource: &Resource,
    keys: &BTreeMap<Vec<Segment>, ByteSpan>,
) -> std::result::Result<Vec<Row>, String> {
    keys.iter()
        .enumerate()
        .map(|(ordinal, (segments, span))| {
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| format!("{} has more keys than a row can number", resource.path()))?;
            let mut row = Row::new(
                resource.clone(),
                Some(Pointer::from_segments(segments.clone())),
                ordinal,
                RowState::Key,
            );
            row.span = Some(*span);
            Ok(row)
        })
        .collect()
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
        let Some(read_cache) = ctx.index.read_cache() else {
            return Err(Error::internal(
                "key_parity requires the relation read cache",
            ));
        };
        let Some(decode_cache) = ctx.index.decode_cache() else {
            return Err(Error::internal(
                "key_parity requires the relation decode cache",
            ));
        };
        let limits = Limits::default();
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
            let selection = Selector::new(self.peers_scope.clone()).select(ctx.index);
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
            findings.push(
                Violation::new(format!(
                    "{}: `peers` glob {:?} matched no files other than the reference",
                    crate::slash(Path::new(self.reference.as_str())),
                    self.peers_scope_label()
                ))
                .with_path(Path::new(self.reference.as_str()).to_path_buf())
                .as_analysis_failure(AnalysisFailureKind::Evaluation),
            );
            return Ok(findings);
        }

        let mut files: Vec<(Resource, KeyFile)> = Vec::new();
        for peer in &peers {
            let Some(format) = self.format_for(peer.path()) else {
                findings.push(
                    Violation::new(format!(
                        "{}: cannot infer a key format; set `format` explicitly",
                        crate::slash(Path::new(peer.path().as_str()))
                    ))
                    .with_path(Path::new(peer.path().as_str()).to_path_buf())
                    .as_analysis_failure(AnalysisFailureKind::ResourceParse),
                );
                continue;
            };
            if format.model() != self.reference_format.model() {
                findings.push(
                    Violation::new(format!(
                        "{}: format {format:?} has a different key model than the reference {}",
                        crate::slash(Path::new(peer.path().as_str())),
                        self.reference.as_str()
                    ))
                    .with_path(Path::new(peer.path().as_str()).to_path_buf())
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
        if files.is_empty() {
            return Ok(findings);
        }

        let strip = self.strip_root
            && self.reference_format.model() == KeyModel::Tree
            && std::iter::once(&reference_file)
                .chain(files.iter().map(|(_, file)| file))
                .all(has_single_root);
        let reference_keys = transform(&reference_file, self.plurals, strip, &self.ignore);
        record_key_trace(ctx, Path::new(self.reference.as_str()), &reference_keys);
        let reference_rows = match key_rows(&reference, &reference_keys) {
            Ok(rows) => rows,
            Err(message) => {
                findings.push(evaluation_failure(
                    Path::new(self.reference.as_str()),
                    &message,
                ));
                return Ok(findings);
            }
        };
        let compared = Compared {
            file: &reference_file,
            keys: &reference_keys,
            rows: &reference_rows,
        };
        for (peer, file) in &files {
            let peer_keys = transform(file, self.plurals, strip, &self.ignore);
            record_key_trace(ctx, Path::new(peer.path().as_str()), &peer_keys);
            self.compare(&limits, &compared, peer, file, &peer_keys, &mut findings);
        }
        Ok(findings)
    }
}

impl KeyParityRule {
    fn peers_scope_label(&self) -> &str {
        &self.peers
    }

    fn format_for(&self, path: &RelPath) -> Option<KeyFormat> {
        self.format_override
            .or_else(|| infer_format(Path::new(path.as_str())))
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
        let adapter = match format.adapter() {
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
        let mut keys = BTreeMap::new();
        for row in &output.rows {
            if !row.is_key() {
                continue;
            }
            let Some(pointer) = row.pointer.as_ref() else {
                continue;
            };
            if pointer
                .segments()
                .iter()
                .any(|segment| matches!(segment, Segment::Index(_)))
            {
                continue;
            }
            let Some(span) = row.span else {
                continue;
            };
            keys.entry(pointer.segments().to_vec()).or_insert(span);
        }
        let keys = leaf_keys(keys);
        let Ok(text) = std::str::from_utf8(&bytes) else {
            findings.push(analysis_failure(
                path,
                "the entry adapter produced a document that is not valid UTF-8",
            ));
            return None;
        };
        Some(KeyFile {
            keys,
            text: Arc::from(text),
            line_starts: line_starts(text),
        })
    }

    fn compare(
        &self,
        limits: &Limits,
        reference: &Compared<'_>,
        peer: &Resource,
        peer_file: &KeyFile,
        peer_keys: &BTreeMap<Vec<Segment>, ByteSpan>,
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
        })
        .analyze("key-parity", &rows, limits)
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
        let mut missing = BTreeSet::new();
        let mut extra = BTreeSet::new();
        for finding in diffs {
            match finding.message.split_once('\u{0}') {
                Some(("missing", key)) => {
                    missing.insert(key.to_string());
                }
                Some(("extra", key)) => {
                    extra.insert(key.to_string());
                }
                _ => {
                    findings.push(evaluation_failure(
                        peer_path,
                        "the key comparison returned an unrecognized finding",
                    ));
                    return;
                }
            }
        }
        if matches!(self.direction, Direction::Both | Direction::Missing) {
            for (key, span) in reference.keys {
                let rendered = render_key(key);
                if !missing.contains(&rendered) {
                    continue;
                }
                let mut finding =
                    Violation::new(self.message(&rendered, peer_path, reference_path, "missing"))
                        .with_path(peer_path.to_path_buf())
                        .with_baseline_key(format!(
                            "key-parity\u{0}{}\u{0}{rendered}\u{0}missing",
                            crate::slash(peer_path)
                        ));
                if let Some(range) = reference.file.range_at(*span) {
                    finding = finding.with_related_location(
                        RelatedLocation::new(reference_path, "reference key").with_range(range),
                    );
                }
                findings.push(finding);
            }
        }
        if matches!(self.direction, Direction::Both | Direction::Extra) {
            for (key, span) in peer_keys {
                let rendered = render_key(key);
                if !extra.contains(&rendered) {
                    continue;
                }
                let mut finding =
                    Violation::new(self.message(&rendered, peer_path, reference_path, "extra"))
                        .with_path(peer_path.to_path_buf())
                        .with_baseline_key(format!(
                            "key-parity\u{0}{}\u{0}{rendered}\u{0}extra",
                            crate::slash(peer_path)
                        ));
                if let Some(range) = peer_file.range_at(*span) {
                    finding = finding.with_range(range);
                }
                findings.push(finding);
            }
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

fn record_key_trace(ctx: &Context<'_>, path: &Path, keys: &BTreeMap<Vec<Segment>, ByteSpan>) {
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
        let rendered = render_key(key);
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
    keys: &'a BTreeMap<Vec<Segment>, ByteSpan>,
    rows: &'a [Row],
}

/// An analysis failure for a comparison that could not complete: never an
/// empty result that passes (invariant 2).
fn evaluation_failure(path: &Path, message: &str) -> Violation {
    Violation::new(format!("{}: {message}", crate::slash(path)))
        .with_path(path.to_path_buf())
        .with_not_fixable()
        .as_analysis_failure(AnalysisFailureKind::Evaluation)
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

fn leaf_keys(mut keys: BTreeMap<Vec<Segment>, ByteSpan>) -> BTreeMap<Vec<Segment>, ByteSpan> {
    keys.retain(|key, _| {
        !key.iter()
            .any(|segment| matches!(segment, Segment::Index(_)))
    });
    let ordered: Vec<Vec<Segment>> = keys.keys().cloned().collect();
    let mut non_leaf = BTreeSet::new();
    for pair in ordered.windows(2) {
        let [parent, child] = pair else {
            continue;
        };
        if child.len() > parent.len() && child.starts_with(parent) {
            non_leaf.insert(parent.clone());
        }
    }
    for key in non_leaf {
        keys.remove(&key);
    }
    keys
}

fn has_single_root(file: &KeyFile) -> bool {
    let mut firsts = file.keys.keys().filter_map(|key| key.first());
    let Some(first) = firsts.next() else {
        return false;
    };
    firsts.all(|other| other == first)
}

fn transform(
    file: &KeyFile,
    plurals: Plurals,
    strip_root: bool,
    ignore: &[String],
) -> BTreeMap<Vec<Segment>, ByteSpan> {
    let mut keys = file.keys.clone();
    if strip_root {
        let stripped: BTreeMap<_, _> = keys
            .into_iter()
            .map(|(mut key, span)| {
                if !key.is_empty() {
                    key.remove(0);
                }
                (key, span)
            })
            .filter(|(key, _)| !key.is_empty())
            .collect();
        keys = stripped;
    }
    if plurals == Plurals::Cldr {
        keys = merge_plural_groups(&keys);
        keys = merge_i18next_groups(&keys);
    }
    keys.retain(|key, _| !key.is_empty());
    if !ignore.is_empty() {
        keys.retain(|key, _| {
            let rendered = render_key(key);
            !ignore
                .iter()
                .any(|pattern| key_glob_match(pattern, &rendered))
        });
    }
    keys
}

fn cldr_category(name: &str) -> bool {
    matches!(name, "zero" | "one" | "two" | "few" | "many" | "other")
}

/// Merge each nested CLDR plural group into one key: an object whose children
/// are all CLDR categories and include `other` (Rails YAML). An object with any
/// other child, or with a container child, is not a plural group and keeps its
/// keys.
fn merge_plural_groups(
    keys: &BTreeMap<Vec<Segment>, ByteSpan>,
) -> BTreeMap<Vec<Segment>, ByteSpan> {
    // Every object that has a child which cannot belong to a plural group:
    // a leaf whose name is not a category, or any container.
    let mut not_groups: BTreeSet<&[Segment]> = BTreeSet::new();
    let mut groups: BTreeMap<&[Segment], Vec<(&str, ByteSpan)>> = BTreeMap::new();
    for (key, span) in keys {
        for depth in 0..key.len().saturating_sub(1) {
            not_groups.insert(&key[..depth]);
        }
        let Some((last, parent)) = key.split_last() else {
            continue;
        };
        match last {
            Segment::Key(name) if cldr_category(name) => {
                groups.entry(parent).or_default().push((name, *span));
            }
            _ => {
                not_groups.insert(parent);
            }
        }
    }
    let mut out = keys.clone();
    for (parent, members) in groups {
        if parent.is_empty()
            || not_groups.contains(parent)
            || !members.iter().any(|(name, _)| *name == "other")
        {
            continue;
        }
        let mut span = ByteSpan::new(0, 0);
        for (name, member_span) in &members {
            let mut key = parent.to_vec();
            key.push(Segment::Key((*name).into()));
            out.remove(&key);
            if *name == "other" {
                span = *member_span;
            }
        }
        out.insert(parent.to_vec(), span);
    }
    out
}

/// Merge i18next plural siblings (`<base>_<category>`, including
/// `<base>_other`) into one `<base>` key. A group whose base is empty, or whose
/// base is already a key of its own, is left as it is rather than merged into
/// a name that would hide a real key.
fn merge_i18next_groups(
    keys: &BTreeMap<Vec<Segment>, ByteSpan>,
) -> BTreeMap<Vec<Segment>, ByteSpan> {
    type GroupKey = (Vec<Segment>, String);
    let mut groups: BTreeMap<GroupKey, Vec<(String, ByteSpan)>> = BTreeMap::new();
    for (key, span) in keys {
        let Some((Segment::Key(last), parent)) = key.split_last() else {
            continue;
        };
        let Some((base, category)) = last.rsplit_once('_') else {
            continue;
        };
        if base.is_empty() || !cldr_category(category) {
            continue;
        }
        groups
            .entry((parent.to_vec(), base.to_string()))
            .or_default()
            .push((category.to_string(), *span));
    }
    let mut out = keys.clone();
    for ((parent, base), members) in groups {
        let mut merged = parent.clone();
        merged.push(Segment::Key(base.as_str().into()));
        if keys.contains_key(&merged) || !members.iter().any(|(name, _)| name == "other") {
            continue;
        }
        let mut span = ByteSpan::new(0, 0);
        for (name, member_span) in &members {
            let mut key = parent.clone();
            key.push(Segment::Key(format!("{base}_{name}").into()));
            out.remove(&key);
            if name == "other" {
                span = *member_span;
            }
        }
        out.insert(merged, span);
    }
    out
}

/// Match a key-path glob: `*` stays within one segment (does not cross `.`),
/// `**` crosses segments. Every other character is literal. The matcher is
/// iterative so a long config pattern cannot overflow the stack.
fn key_glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let mut next = vec![false; text.len() + 1];
    next[text.len()] = true;
    let mut next2 = vec![false; text.len() + 1];
    for i in (0..pattern.len()).rev() {
        let mut current = vec![false; text.len() + 1];
        for j in (0..=text.len()).rev() {
            current[j] = match pattern[i] {
                '*' if pattern.get(i + 1) == Some(&'*') => {
                    let zero = if i + 2 <= pattern.len() {
                        next2[j]
                    } else {
                        false
                    };
                    zero || (j < text.len() && current[j + 1])
                }
                '*' => next[j] || (j < text.len() && text[j] != '.' && current[j + 1]),
                literal => j < text.len() && text[j] == literal && next[j + 1],
            };
        }
        next2 = next;
        next = current;
    }
    next[0]
}

fn render_key(key: &[Segment]) -> String {
    Pointer::from_segments(key.to_vec()).render()
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
    alint_core::reject_scope_filter_on_cross_file(spec, "key_parity")?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|error| Error::rule_config(&spec.id, format!("invalid options: {error}")))?;
    let reference = RelPath::parse(opts.reference.trim()).ok_or_else(|| {
        Error::rule_config(&spec.id, "`reference` must be a confined repository path")
    })?;
    if opts.peers.trim().is_empty() {
        return Err(Error::rule_config(&spec.id, "`peers` must not be empty"));
    }
    if opts.peers.trim() == reference.as_str() {
        return Err(Error::rule_config(
            &spec.id,
            "`peers` must select at least one file other than the reference",
        ));
    }
    let reference_format = match opts.format {
        Some(format) => format,
        None => infer_format(Path::new(reference.as_str())).ok_or_else(|| {
            Error::rule_config(
                &spec.id,
                "cannot infer the key format from `reference`; set `format` explicitly",
            )
        })?,
    };
    if let Some(format) = opts.format
        && let Some(inferred) = infer_format(Path::new(reference.as_str()))
        && inferred.model() != format.model()
    {
        return Err(Error::rule_config(
            &spec.id,
            format!(
                "explicit `format` has a different key model than the reference extension ({inferred:?})"
            ),
        ));
    }
    if opts.ignore_keys.iter().any(|pattern| pattern.len() > 1024) {
        return Err(Error::rule_config(
            &spec.id,
            "`ignore_keys` patterns must be at most 1024 bytes",
        ));
    }
    let ignore = opts.ignore_keys.clone();
    let literal_peer = literal_peer(spec, &opts, &reference, reference_format)?;
    let peers_scope = Scope::from_patterns(std::slice::from_ref(&opts.peers))?;
    // The reference is a literal path: escape it, so a name such as
    // `locales/[id].json` matches its own `--changed` edit.
    let scope = Scope::from_patterns(&[
        alint_core::template::glob_escape(reference.as_str()),
        opts.peers.clone(),
    ])?;
    Ok(KeyParityRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        reference,
        reference_format,
        format_override: opts.format,
        peers: opts.peers,
        peers_scope,
        literal_peer,
        scope,
        direction: opts.direction,
        plurals: opts.plurals,
        strip_root: opts.strip_root,
        ignore,
    })
}

/// A `peers` value with no glob metacharacters, as a confined path other than
/// the reference. Without an explicit `format` its extension picks its format,
/// so a key-model mismatch is a config error here rather than an analysis
/// failure when it is read.
fn literal_peer(
    spec: &RuleSpec,
    opts: &Options,
    reference: &RelPath,
    reference_format: KeyFormat,
) -> Result<Option<RelPath>> {
    let Some(path) = alint_core::template::literal_glob_path(opts.peers.trim()) else {
        return Ok(None);
    };
    let path = RelPath::parse(&path).ok_or_else(|| {
        Error::rule_config(
            &spec.id,
            "a literal `peers` path must be a confined repository path",
        )
    })?;
    if &path == reference {
        return Err(Error::rule_config(
            &spec.id,
            "`peers` must select at least one file other than the reference",
        ));
    }
    if opts.format.is_none() {
        let format = infer_format(Path::new(path.as_str())).ok_or_else(|| {
            Error::rule_config(
                &spec.id,
                "cannot infer the key format of the literal `peers` path; set `format` explicitly",
            )
        })?;
        if format.model() != reference_format.model() {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "the literal `peers` path ({format:?}) has a different key model than the reference ({reference_format:?})"
                ),
            ));
        }
    }
    Ok(Some(path))
}

#[cfg(test)]
#[path = "key_parity_tests.rs"]
mod tests;

#[cfg(test)]
mod analyzer_pilot {
    use super::*;
    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};
    use alint_core::Format;
    use alint_core::plugin_api::{Analyzer, AnalyzerError, AnalyzerInput, Finding};
    use alint_core::relation::{
        Adapter, ContractAnalyzer, EntryRowsAdapter, Limits, Row, RowState,
    };

    /// The key-parity shape as a pure contract analyzer: the first resource is
    /// the reference, every other resource is a peer, and pointer segments are
    /// the keys. This is the R6a pilot, not a replacement for the rule.
    struct ParityPilot {
        reference: String,
    }

    impl Analyzer for ParityPilot {
        fn id(&self) -> &'static str {
            "key-parity-pilot"
        }

        fn analyze(&self, input: AnalyzerInput<'_>) -> Result<Vec<Finding>, AnalyzerError> {
            let keys = |path: &str| -> BTreeSet<String> {
                input
                    .rows
                    .iter()
                    .filter(|row| row.resource.path == path)
                    .filter_map(|row| {
                        row.pointer
                            .as_ref()
                            .map(|parts| render_contract_pointer(parts).unwrap())
                    })
                    .collect()
            };
            let reference = keys(&self.reference);
            let mut findings = Vec::new();
            for path in input
                .rows
                .iter()
                .map(|row| row.resource.path.as_str())
                .collect::<BTreeSet<_>>()
            {
                if path == self.reference {
                    continue;
                }
                let peer = keys(path);
                for key in reference.difference(&peer) {
                    findings.push(Finding {
                        path: Some(path.into()),
                        message: format!(
                            "{path}: missing key `{key}` (present in {})",
                            self.reference
                        ),
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
                for key in peer.difference(&reference) {
                    findings.push(Finding {
                        path: Some(path.into()),
                        message: format!(
                            "{path}: extra key `{key}` (absent from {})",
                            self.reference
                        ),
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
            }
            Ok(findings)
        }
    }

    fn key_row(path: &str, key: &str) -> Row {
        Row::new(
            Resource::literal(RelPath::parse(path).unwrap()),
            Some(Pointer::root().key(key)),
            0,
            RowState::Key,
        )
    }

    fn parsed_rows(path: &str, text: &str) -> Vec<Row> {
        EntryRowsAdapter::new(Format::Json)
            .unwrap()
            .extract(
                &Resource::literal(RelPath::parse(path).unwrap()),
                text.as_bytes(),
                &Limits::default(),
            )
            .unwrap()
            .rows
    }

    #[test]
    fn pilot_matches_the_real_rule_on_parsed_key_rows() {
        // Top-level dotted keys need bracket notation: the oracle must render
        // them exactly as the rule does, or this comparison fails.
        let (tmp, index) = tempdir_with_files(&[
            ("en.json", br#"{"a":1,"b":2,"x.y":3}"#),
            ("fr.json", br#"{"a":1,"c":3,"x.z":4}"#),
        ]);
        index.enable_relation_caches();
        let rule = build(&spec_yaml(
            "id: t\nkind: key_parity\nreference: en.json\npeers: '*.json'\nlevel: error\n",
        ))
        .unwrap();
        let expected: Vec<String> = rule
            .evaluate(&ctx(tmp.path(), &index))
            .unwrap()
            .iter()
            .map(|finding| finding.message.to_string())
            .collect();
        assert!(
            expected.iter().any(|m| m.contains("missing key `['x.y']`")),
            "{expected:?}"
        );
        let mut rows = parsed_rows("en.json", r#"{"a":1,"b":2,"x.y":3}"#);
        rows.extend(parsed_rows("fr.json", r#"{"a":1,"c":3,"x.z":4}"#));
        let actual: Vec<String> = ContractAnalyzer::new(ParityPilot {
            reference: "en.json".into(),
        })
        .analyze("key-parity-pilot", &rows, &Limits::default())
        .unwrap()
        .iter()
        .map(|finding| finding.message.to_string())
        .collect();
        let mut expected = expected;
        let mut actual = actual;
        expected.sort();
        actual.sort();
        assert_eq!(actual, expected);
    }

    #[test]
    fn key_parity_shape_runs_through_contract_analyzer() {
        let rows = [
            key_row("en.json", "a"),
            key_row("en.json", "b"),
            key_row("fr.json", "a"),
            key_row("fr.json", "c"),
        ];
        let findings = ContractAnalyzer::new(ParityPilot {
            reference: "en.json".into(),
        })
        .analyze("key-parity-pilot", &rows, &Limits::default())
        .unwrap();
        let messages: Vec<String> = findings.iter().map(|f| f.message.to_string()).collect();
        assert!(messages.iter().any(|m| m.contains("missing key `b`")));
        assert!(messages.iter().any(|m| m.contains("extra key `c`")));
        assert_eq!(findings.len(), 2);
    }
}
