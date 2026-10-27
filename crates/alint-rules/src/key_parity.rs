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
//! The five row formats are JSON, YAML, dotenv, INI, and Java properties.
//! TOML and the Android/.resx dialects arrive with R9. Files are compared only
//! within one key model: the tree formats (JSON/YAML/TOML) together, the flat
//! formats (dotenv/properties) together, and INI on its own. A mismatch is a
//! configuration error.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use alint_core::relation::{
    Adapter, AndroidStringsAdapter, EntryRowsAdapter, Limits, Pointer, RelPath, Resource,
    ResourceRead, ResxAdapter, Segment, Selector, TomlRowsAdapter,
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
    peers: String,
    /// Key format. Inferred from the extension when omitted; `.env` and
    /// `.env.*` infer dotenv, while `.xml` and `.resx` need an explicit dialect.
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
    fn range(&self, key: &[Segment]) -> Option<SourceRange> {
        let span = self.keys.get(key)?;
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
    scope: Scope,
    direction: Direction,
    plurals: Plurals,
    strip_root: bool,
    ignore: Vec<String>,
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
        let mut peers: Vec<Resource> = selection
            .resources
            .into_iter()
            .filter(|resource| resource.path() != &self.reference)
            .collect();
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
        for (peer, file) in &files {
            let peer_keys = transform(file, self.plurals, strip, &self.ignore);
            record_key_trace(ctx, Path::new(peer.path().as_str()), &peer_keys);
            self.compare(
                &reference_file,
                &reference_keys,
                peer,
                file,
                &peer_keys,
                &mut findings,
            );
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
                let mut finding = if required {
                    Violation::new(format!("{}: reference file is missing", crate::slash(path)))
                        .with_path(path.to_path_buf())
                } else {
                    alint_core::missing_file_violation(path)
                };
                if required {
                    finding = finding.as_analysis_failure(AnalysisFailureKind::ResourceRead);
                }
                findings.push(finding);
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

    #[allow(clippy::too_many_arguments)]
    fn compare(
        &self,
        reference_file: &KeyFile,
        reference_keys: &BTreeMap<Vec<Segment>, ByteSpan>,
        peer: &Resource,
        peer_file: &KeyFile,
        peer_keys: &BTreeMap<Vec<Segment>, ByteSpan>,
        findings: &mut Vec<Violation>,
    ) {
        let peer_path = Path::new(peer.path().as_str());
        let reference_path = Path::new(self.reference.as_str());
        if matches!(self.direction, Direction::Both | Direction::Missing) {
            for key in reference_keys.keys() {
                if peer_keys.contains_key(key) {
                    continue;
                }
                let rendered = render_key(key);
                let mut finding =
                    Violation::new(self.message(&rendered, peer_path, reference_path, "missing"))
                        .with_path(peer_path.to_path_buf())
                        .with_baseline_key(format!(
                            "key-parity\u{0}{}\u{0}{rendered}\u{0}missing",
                            crate::slash(peer_path)
                        ));
                if let Some(range) = reference_file.range(key) {
                    finding = finding.with_related_location(
                        RelatedLocation::new(reference_path, "reference key").with_range(range),
                    );
                }
                findings.push(finding);
            }
        }
        if matches!(self.direction, Direction::Both | Direction::Extra) {
            for key in peer_keys.keys() {
                if reference_keys.contains_key(key) {
                    continue;
                }
                let rendered = render_key(key);
                let mut finding =
                    Violation::new(self.message(&rendered, peer_path, reference_path, "extra"))
                        .with_path(peer_path.to_path_buf())
                        .with_baseline_key(format!(
                            "key-parity\u{0}{}\u{0}{rendered}\u{0}extra",
                            crate::slash(peer_path)
                        ));
                if let Some(range) = peer_file.range(key) {
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

fn merge_plural_groups(
    keys: &BTreeMap<Vec<Segment>, ByteSpan>,
) -> BTreeMap<Vec<Segment>, ByteSpan> {
    let mut groups: BTreeMap<Vec<Segment>, Vec<(String, ByteSpan)>> = BTreeMap::new();
    let mut out = keys.clone();
    for (key, span) in keys {
        let Some(Segment::Key(last)) = key.last() else {
            continue;
        };
        if !cldr_category(last) {
            continue;
        }
        groups
            .entry(key[..key.len() - 1].to_vec())
            .or_default()
            .push((last.to_string(), *span));
    }
    for (parent, members) in groups {
        let has_other = members.iter().any(|(name, _)| name == "other");
        let all_categories = members.iter().all(|(name, _)| cldr_category(name));
        if !has_other || !all_categories {
            continue;
        }
        for (name, _) in &members {
            let mut key = parent.clone();
            key.push(Segment::Key(name.as_str().into()));
            out.remove(&key);
        }
        let span = members
            .iter()
            .find(|(name, _)| name == "other")
            .map_or(ByteSpan::new(0, 0), |(_, span)| *span);
        out.insert(parent, span);
    }
    out
}

fn merge_i18next_groups(
    keys: &BTreeMap<Vec<Segment>, ByteSpan>,
) -> BTreeMap<Vec<Segment>, ByteSpan> {
    type GroupKey = (Vec<Segment>, String);
    let mut groups: BTreeMap<GroupKey, Vec<(String, ByteSpan)>> = BTreeMap::new();
    let mut out = keys.clone();
    for (key, span) in keys {
        let Some(Segment::Key(last)) = key.last() else {
            continue;
        };
        let Some((base, category)) = last.rsplit_once('_') else {
            continue;
        };
        if !cldr_category(category) {
            continue;
        }
        groups
            .entry((key[..key.len() - 1].to_vec(), base.to_string()))
            .or_default()
            .push((category.to_string(), *span));
    }
    for ((parent, base), members) in groups {
        let has_other = members.iter().any(|(name, _)| name == "other");
        if !has_other {
            continue;
        }
        for (name, _) in &members {
            let mut key = parent.clone();
            key.push(Segment::Key(format!("{base}_{name}").into()));
            out.remove(&key);
        }
        let mut merged = parent;
        merged.push(Segment::Key(base.into()));
        let span = members
            .iter()
            .find(|(name, _)| name == "other")
            .map_or(ByteSpan::new(0, 0), |(_, span)| *span);
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
    let peers_scope = Scope::from_patterns(std::slice::from_ref(&opts.peers))?;
    let scope = Scope::from_patterns(&[reference.as_str().to_string(), opts.peers.clone()])?;
    Ok(Box::new(KeyParityRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        reference,
        reference_format,
        format_override: opts.format,
        peers: opts.peers,
        peers_scope,
        scope,
        direction: opts.direction,
        plurals: opts.plurals,
        strip_root: opts.strip_root,
        ignore,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};
    use alint_core::{FindingDisposition, Violation};

    fn run(yaml: &str, files: &[(&str, &[u8])]) -> Vec<Violation> {
        let (tmp, index) = tempdir_with_files(files);
        index.enable_relation_caches();
        let rule = build(&spec_yaml(yaml)).expect("rule builds");
        rule.evaluate(&ctx(tmp.path(), &index))
            .expect("rule evaluates")
    }

    fn messages(violations: &[Violation]) -> Vec<String> {
        violations.iter().map(|v| v.message.to_string()).collect()
    }

    #[test]
    fn format_inference_and_models() {
        assert_eq!(infer_format(Path::new("a.json")), Some(KeyFormat::Json));
        assert_eq!(infer_format(Path::new("a.yml")), Some(KeyFormat::Yaml));
        assert_eq!(
            infer_format(Path::new(".env.example")),
            Some(KeyFormat::Dotenv)
        );
        assert_eq!(
            infer_format(Path::new("a.properties")),
            Some(KeyFormat::Properties)
        );
        assert_eq!(infer_format(Path::new("a.ini")), Some(KeyFormat::Ini));
        assert_eq!(infer_format(Path::new("a.toml")), Some(KeyFormat::Toml));
        assert_eq!(infer_format(Path::new("a.xml")), None);
        assert_eq!(KeyFormat::Json.model(), KeyModel::Tree);
        assert_eq!(KeyFormat::Toml.model(), KeyModel::Tree);
        assert_eq!(KeyFormat::Dotenv.model(), KeyModel::Flat);
        assert_eq!(KeyFormat::AndroidStrings.model(), KeyModel::AndroidStrings);
        assert_eq!(KeyFormat::Resx.model(), KeyModel::Resx);
    }

    #[test]
    fn leaf_keys_drop_containers_and_array_members() {
        let mut keys = BTreeMap::new();
        for (parts, span) in [
            (vec![Segment::Key("nav".into())], ByteSpan::new(0, 1)),
            (
                vec![Segment::Key("nav".into()), Segment::Key("home".into())],
                ByteSpan::new(2, 3),
            ),
            (
                vec![
                    Segment::Key("list".into()),
                    Segment::Index(0),
                    Segment::Key("x".into()),
                ],
                ByteSpan::new(4, 5),
            ),
        ] {
            keys.insert(parts, span);
        }
        let keys = leaf_keys(keys);
        assert_eq!(
            keys.keys().map(|key| render_key(key)).collect::<Vec<_>>(),
            ["nav.home"]
        );
    }

    #[test]
    fn plural_groups_merge_nested_and_i18next_keys() {
        let mut keys = BTreeMap::new();
        for (parts, span) in [
            (
                vec![Segment::Key("item".into()), Segment::Key("one".into())],
                ByteSpan::new(0, 1),
            ),
            (
                vec![Segment::Key("item".into()), Segment::Key("other".into())],
                ByteSpan::new(2, 3),
            ),
            (vec![Segment::Key("msg_one".into())], ByteSpan::new(4, 5)),
            (vec![Segment::Key("msg_other".into())], ByteSpan::new(6, 7)),
        ] {
            keys.insert(parts, span);
        }
        let keys = merge_i18next_groups(&merge_plural_groups(&keys));
        assert_eq!(
            keys.keys().map(|key| render_key(key)).collect::<Vec<_>>(),
            ["item", "msg"]
        );
    }

    #[test]
    fn ranges_use_a_line_index_and_ignore_the_bom_column() {
        let text = "\u{feff}a\r\nbb\nç\rx";
        let file = KeyFile {
            keys: BTreeMap::from([
                (vec![Segment::Key("a".into())], ByteSpan::new(3, 4)),
                (vec![Segment::Key("bb".into())], ByteSpan::new(6, 8)),
                (vec![Segment::Key("ç".into())], ByteSpan::new(9, 11)),
                (vec![Segment::Key("x".into())], ByteSpan::new(12, 13)),
            ]),
            text: Arc::from(text),
            line_starts: line_starts(text),
        };
        let range = |key: &str| {
            file.range(&[Segment::Key(key.into())])
                .expect("range resolves")
        };
        assert_eq!((range("a").start_line(), range("a").start_column()), (1, 1));
        assert_eq!(
            (range("bb").start_line(), range("bb").start_column()),
            (2, 1)
        );
        assert_eq!((range("ç").start_line(), range("ç").start_column()), (3, 1));
        assert_eq!((range("x").start_line(), range("x").start_column()), (4, 1));
        assert_eq!(range("ç").end_column(), 2);
    }

    #[test]
    fn ignore_globs_respect_segment_boundaries() {
        assert!(key_glob_match("nav.*", "nav.home"));
        assert!(!key_glob_match("nav.*", "nav.home.x"));
        assert!(key_glob_match("nav.**", "nav.home.x"));
        assert!(key_glob_match("nav.**", "nav."));
    }

    #[test]
    fn build_rejects_an_unconfined_reference() {
        let spec = spec_yaml(
            "id: t\nkind: key_parity\nreference: ../en.json\npeers: '*.json'\nlevel: error\n",
        );
        let error = build(&spec).unwrap_err().to_string();
        assert!(error.contains("confined"), "{error}");
    }

    #[test]
    fn build_rejects_an_uninferable_reference_format() {
        let spec = spec_yaml(
            "id: t\nkind: key_parity\nreference: locales/en\npeers: 'locales/*'\nlevel: error\n",
        );
        let error = build(&spec).unwrap_err().to_string();
        assert!(error.contains("format"), "{error}");
    }

    #[test]
    fn reports_missing_and_extra_keys() {
        let findings = run(
            "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nlevel: error\n",
            &[
                (
                    "locales/en.json",
                    br#"{"nav":{"home":"Home","about":"About"}}"#,
                ),
                (
                    "locales/fr.json",
                    br#"{"nav":{"home":"Accueil","contact":"Contact"}}"#,
                ),
            ],
        );
        let messages = messages(&findings);
        assert!(
            messages
                .iter()
                .any(|m| m.contains("missing key `nav.about`")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("extra key `nav.contact`")),
            "{messages:?}"
        );
        assert!(
            findings
                .iter()
                .all(|v| v.disposition == FindingDisposition::Violation)
        );
    }

    #[test]
    fn direction_selects_one_side() {
        let findings = run(
            "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\ndirection: missing\nlevel: error\n",
            &[
                ("locales/en.json", br#"{"a":1}"#),
                ("locales/fr.json", br#"{"a":1,"b":2}"#),
            ],
        );
        assert!(findings.is_empty(), "{:?}", messages(&findings));
    }

    #[test]
    fn strip_root_compares_below_the_locale_key() {
        let findings = run(
            "id: t\nkind: key_parity\nreference: locales/en.yml\npeers: 'locales/*.yml'\nstrip_root: true\nlevel: error\n",
            &[
                (
                    "locales/en.yml",
                    b"en:\n  title: Title\n  nav:\n    home: Home\n",
                ),
                ("locales/fr.yml", b"fr:\n  title: Titre\n"),
            ],
        );
        let messages = messages(&findings);
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(
            messages[0].contains("missing key `nav.home`"),
            "{messages:?}"
        );
    }

    #[test]
    fn cldr_plural_groups_are_one_key() {
        let exact = run(
            "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nlevel: error\n",
            &[
                (
                    "locales/en.json",
                    br#"{"item":{"one":"one","other":"other"}}"#,
                ),
                (
                    "locales/fr.json",
                    br#"{"item":{"one":"un","other":"autre"}}"#,
                ),
            ],
        );
        assert!(exact.is_empty(), "{:?}", messages(&exact));
        let cldr = run(
            "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nplurals: cldr\nlevel: error\n",
            &[
                (
                    "locales/en.json",
                    br#"{"item":{"one":"one","other":"other"}}"#,
                ),
                (
                    "locales/fr.json",
                    br#"{"item":{"one":"un","other":"autre"}}"#,
                ),
            ],
        );
        assert!(cldr.is_empty(), "{:?}", messages(&cldr));
    }

    #[test]
    fn ignore_keys_omits_a_known_gap() {
        let findings = run(
            "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nignore_keys: ['nav.about']\nlevel: error\n",
            &[
                (
                    "locales/en.json",
                    br#"{"nav":{"home":"Home","about":"About"}}"#,
                ),
                ("locales/fr.json", br#"{"nav":{"home":"Accueil"}}"#),
            ],
        );
        assert!(findings.is_empty(), "{:?}", messages(&findings));
    }

    #[test]
    fn flat_and_ini_formats_compare() {
        let dotenv = run(
            "id: t\nkind: key_parity\nreference: .env.example\npeers: '.env'\nlevel: error\n",
            &[(".env.example", b"A=1\nB=2\n"), (".env", b"A=1\n")],
        );
        assert!(
            messages(&dotenv)
                .iter()
                .any(|m| m.contains("missing key `B`")),
            "{:?}",
            messages(&dotenv)
        );
        let ini = run(
            "id: t\nkind: key_parity\nreference: a.ini\npeers: 'b.ini'\nlevel: error\n",
            &[("a.ini", b"[s]\na=1\nb=2\n"), ("b.ini", b"[s]\na=1\n")],
        );
        assert!(
            messages(&ini)
                .iter()
                .any(|m| m.contains("missing key `s.b`")),
            "{:?}",
            messages(&ini)
        );
    }

    #[test]
    fn a_sensitive_peer_parse_failure_is_redacted() {
        let findings = run(
            "id: t\nkind: key_parity\nreference: .env.example\npeers: '.env'\nlevel: error\n",
            &[
                (".env.example", b"A=1\n"),
                (".env", b"A=\"top-secret-value\n"),
            ],
        );
        assert!(
            findings
                .iter()
                .any(|v| matches!(v.disposition, FindingDisposition::AnalysisFailure(_))),
            "{:?}",
            messages(&findings)
        );
        assert!(
            findings
                .iter()
                .all(|v| !v.message.contains("top-secret-value")),
            "{:?}",
            messages(&findings)
        );
    }

    #[test]
    fn toml_android_and_resx_formats_compare() {
        let toml = run(
            "id: t\nkind: key_parity\nreference: locales/en.toml\npeers: 'locales/*.toml'\nlevel: error\n",
            &[
                (
                    "locales/en.toml",
                    b"[nav]\nhome = \"Home\"\nabout = \"About\"\n",
                ),
                ("locales/fr.toml", b"[nav]\nhome = \"Accueil\"\n"),
            ],
        );
        assert!(
            messages(&toml)
                .iter()
                .any(|m| m.contains("missing key `nav.about`")),
            "{:?}",
            messages(&toml)
        );

        let android = run(
            "id: t\nkind: key_parity\nreference: values/strings.xml\npeers: 'values-*/*.xml'\nformat: android-strings\nlevel: error\n",
            &[
                ("values/strings.xml", b"<resources><string name=\"app\">App</string><string name=\"build\" translatable=\"false\">1</string></resources>"),
                ("values-fr/strings.xml", b"<resources><string name=\"app\">App</string></resources>"),
            ],
        );
        assert!(android.is_empty(), "{:?}", messages(&android));

        let resx = run(
            "id: t\nkind: key_parity\nreference: Strings.resx\npeers: 'Strings.*.resx'\nformat: resx\nlevel: error\n",
            &[
                ("Strings.resx", b"<root><data name=\"Greeting\"><value>Hello</value></data><data name=\"Icon\" type=\"Bitmap\"/></root>"),
                ("Strings.fr.resx", b"<root><data name=\"Greeting\"><value>Bonjour</value></data></root>"),
            ],
        );
        assert!(resx.is_empty(), "{:?}", messages(&resx));
    }

    #[test]
    fn missing_reference_parse_failure_and_empty_peers_fail_loudly() {
        let missing = run(
            "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nlevel: error\n",
            &[("locales/fr.json", br#"{"a":1}"#)],
        );
        assert!(
            missing
                .iter()
                .any(|v| matches!(v.disposition, FindingDisposition::AnalysisFailure(_))),
            "{:?}",
            messages(&missing)
        );
        let parse = run(
            "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nlevel: error\n",
            &[
                ("locales/en.json", br#"{"a":1}"#),
                ("locales/fr.json", b"{"),
            ],
        );
        assert!(
            parse
                .iter()
                .any(|v| matches!(v.disposition, FindingDisposition::AnalysisFailure(_))),
            "{:?}",
            messages(&parse)
        );
        let empty = run(
            "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nlevel: error\n",
            &[("locales/en.json", br#"{"a":1}"#)],
        );
        assert!(
            empty
                .iter()
                .any(|v| matches!(v.disposition, FindingDisposition::AnalysisFailure(_))),
            "{:?}",
            messages(&empty)
        );
    }

    #[test]
    fn the_rule_declares_the_relation_cache_and_full_index() {
        let spec = spec_yaml(
            "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nlevel: error\n",
        );
        let rule = build(&spec).unwrap();
        assert!(rule.uses_relations());
        assert!(rule.requires_full_index());
        assert!(rule.path_scope().is_some());
        assert!(!rule.supports_expect_matches());
    }
}

#[cfg(test)]
mod analyzer_pilot {
    use super::*;
    use alint_core::plugin_api::{Analyzer, AnalyzerError, AnalyzerInput, Finding, PointerSegment};
    use alint_core::relation::{ContractAnalyzer, Limits, Row, RowState};

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
                    .filter_map(|row| match &row.pointer {
                        Some(parts) => Some(
                            parts
                                .iter()
                                .map(|part| match part {
                                    PointerSegment::Key(key) => key.clone(),
                                    PointerSegment::Index(index) => format!("[{index}]"),
                                })
                                .collect::<Vec<_>>()
                                .join("."),
                        ),
                        None => None,
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
