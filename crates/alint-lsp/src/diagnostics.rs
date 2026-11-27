//! Findings to LSP diagnostics: grouping, severities, UTF-16 positions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use lsp_types::{
    CodeDescription, Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location,
    NumberOrString, Position, Range, Url,
};

use alint_core::{Engine, Level, RuleResult, Violation};

use crate::render::{kind_description, rule_docs_url};
use crate::{Finding, FindingsByPath};

/// Normalize a filesystem path for use as a `HashMap`/`HashSet` key so
/// Windows drive-letter casing differences (`c:\repo` vs `C:\repo`, such as
/// VS Code's `file:///c%3A/...` vs `Url::from_file_path`'s `file:///C:/...`)
/// map to the same entry. Identity on non-Windows platforms.
#[cfg(windows)]
pub(crate) fn normalize_path_key(path: &Path) -> PathBuf {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    let mut units: Vec<u16> = path.as_os_str().encode_wide().collect();
    if units.len() >= 2
        && units[1] == u16::from(b':')
        && (u16::from(b'a')..=u16::from(b'z')).contains(&units[0])
    {
        units[0] -= 32;
        return PathBuf::from(OsString::from_wide(&units));
    }
    if units.len() >= 6
        && units[0] == u16::from(b'\\')
        && units[1] == u16::from(b'\\')
        && units[2] == u16::from(b'?')
        && units[3] == u16::from(b'\\')
        && units[5] == u16::from(b':')
        && (u16::from(b'a')..=u16::from(b'z')).contains(&units[4])
    {
        units[4] -= 32;
        return PathBuf::from(OsString::from_wide(&units));
    }
    path.to_path_buf()
}

/// Normalize a filesystem path for use as a `HashMap`/`HashSet` key.
#[cfg(not(windows))]
pub(crate) fn normalize_path_key(path: &Path) -> PathBuf {
    path.to_path_buf()
}

/// Logical document text for UTF-16 column conversion: the editor's buffer for
/// open documents (what was linted), else the BOM-decoded file on disk (cached).
#[derive(Debug, Default)]
pub(crate) struct TextSource {
    overlays: HashMap<PathBuf, String>,
    disk: HashMap<PathBuf, Option<String>>,
}

impl TextSource {
    pub(crate) fn with_overlays(overlays: HashMap<PathBuf, String>) -> Self {
        let overlays = overlays
            .into_iter()
            .map(|(path, text)| (normalize_path_key(&path), text))
            .collect();
        Self {
            overlays,
            disk: HashMap::new(),
        }
    }

    fn get(&mut self, abs: &Path) -> Option<&str> {
        let key = normalize_path_key(abs);
        if let Some(text) = self.overlays.get(&key) {
            return Some(without_utf8_bom(text));
        }
        self.disk
            .entry(key)
            .or_insert_with(|| {
                let bytes = alint_core::bounded_file::read_regular_capped(
                    abs,
                    alint_core::MAX_ANALYZE_BYTES,
                )
                .ok()?;
                Some(decode_disk_text(&bytes))
            })
            .as_deref()
    }
}

fn without_utf8_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

pub(crate) fn decode_disk_text(bytes: &[u8]) -> String {
    if let Some(text) = bytes.strip_prefix(b"\xef\xbb\xbf") {
        return String::from_utf8_lossy(text).into_owned();
    }
    if let Some(text) = bytes.strip_prefix(b"\xff\xfe") {
        return decode_utf16(text, false);
    }
    if let Some(text) = bytes.strip_prefix(b"\xfe\xff") {
        return decode_utf16(text, true);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn decode_utf16(bytes: &[u8], big_endian: bool) -> String {
    let units = (0..bytes.len() / 2).map(|index| {
        let offset = index * 2;
        let pair = [bytes[offset], bytes[offset + 1]];
        if big_endian {
            u16::from_be_bytes(pair)
        } else {
            u16::from_le_bytes(pair)
        }
    });
    let mut text: String = char::decode_utf16(units)
        .map(|scalar| scalar.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
    if !bytes.len().is_multiple_of(2) {
        text.push(char::REPLACEMENT_CHARACTER);
    }
    text
}

/// What [`group_findings`] needs from a session.
pub(crate) struct GroupCtx<'a> {
    pub(crate) root: &'a Path,
    pub(crate) engine: &'a Engine,
    pub(crate) config_path: &'a Path,
    pub(crate) kinds: &'a HashMap<String, String>,
}

/// Group rule-result violations into per-file findings keyed by absolute
/// path. Path-less findings (existence / tree-level rules) are anchored
/// to the config file so they're still visible in the editor. Each
/// finding is tagged `per_file` so the change hot path can preserve
/// cross-file findings. Columns are converted to UTF-16 against the
/// document text from `texts`.
pub(crate) fn group_findings(
    ctx: &GroupCtx<'_>,
    results: &[RuleResult],
    texts: &mut TextSource,
) -> FindingsByPath {
    let mut by_path = FindingsByPath::new();
    for result in results {
        let severity = severity_of(result.level);
        let policy_url = result.policy_url.as_ref().map(ToString::to_string);
        let per_file = ctx.engine.is_per_file(&result.rule_id);
        let kind = ctx.kinds.get(result.rule_id.as_ref());
        let description = kind.and_then(|k| kind_description(k));
        let docs_url = kind.and_then(|k| rule_docs_url(k));
        if let Some(severity) = severity {
            for violation in &result.violations {
                // Findings whose selector path came from protected configuration
                // are anchored to the config; exposing a URI would reveal it.
                let path_sensitive = violation.path_is_sensitive();
                let abs = normalize_path_key(&if path_sensitive {
                    ctx.config_path.to_path_buf()
                } else {
                    violation
                        .path
                        .as_deref()
                        .map_or_else(|| ctx.config_path.to_path_buf(), |rel| ctx.root.join(rel))
                });
                let text = if !path_sensitive
                    && violation.path.is_some()
                    && (violation.column.is_some() || violation.range.is_some())
                {
                    texts.get(&abs)
                } else {
                    None
                };
                let range = if path_sensitive {
                    finding_range(None, None, None)
                } else {
                    violation_range(violation, text)
                };
                let related_information =
                    related_information(ctx.root, &abs, range, violation, texts);
                by_path.entry(abs).or_default().push(Finding {
                    range,
                    severity,
                    rule_id: result.rule_id.to_string(),
                    message: violation.message.to_string(),
                    line: (!path_sensitive).then_some(violation.line).flatten(),
                    column: (!path_sensitive).then_some(violation.column).flatten(),
                    has_path: !path_sensitive && violation.path.is_some(),
                    baseline_key: violation.baseline_key.as_deref().map(str::to_string),
                    policy_url: policy_url.clone(),
                    fixable: result.is_fixable,
                    sensitive: violation.is_sensitive() || path_sensitive,
                    per_file,
                    is_pattern_hint: false,
                    description,
                    docs_url: docs_url.clone(),
                    related_information,
                });
            }
        }
        for failure in &result.analysis_failures {
            let path_sensitive = failure.path_is_sensitive();
            let abs = normalize_path_key(&if path_sensitive {
                ctx.config_path.to_path_buf()
            } else {
                failure
                    .path
                    .as_deref()
                    .map_or_else(|| ctx.config_path.to_path_buf(), |rel| ctx.root.join(rel))
            });
            let text = if !path_sensitive && failure.path.is_some() && failure.column.is_some() {
                texts.get(&abs)
            } else {
                None
            };
            by_path.entry(abs).or_default().push(Finding {
                range: if path_sensitive {
                    finding_range(None, None, None)
                } else {
                    finding_range(failure.line, failure.column, text)
                },
                severity: DiagnosticSeverity::ERROR,
                rule_id: result.rule_id.to_string(),
                message: format!(
                    "analysis failure ({}): {}",
                    failure.kind.as_str(),
                    failure.message
                ),
                line: (!path_sensitive).then_some(failure.line).flatten(),
                column: (!path_sensitive).then_some(failure.column).flatten(),
                has_path: !path_sensitive && failure.path.is_some(),
                baseline_key: None,
                policy_url: None,
                fixable: false,
                sensitive: false,
                per_file,
                is_pattern_hint: false,
                description: None,
                docs_url: None,
                related_information: Vec::new(),
            });
        }
    }
    by_path
}

/// Load-time pattern hints ([`alint_core::pattern_hints`]) as warnings on
/// the config file, each at the line that declares its rule or fact when the
/// file declares it (a nested `require:` rule points at its parent), else at
/// the top.
pub(crate) fn hint_findings(
    hints: &[alint_core::pattern_hints::ConfigHint],
    config_path: &Path,
    texts: &mut TextSource,
) -> Vec<Finding> {
    let text = texts.get(config_path);
    hints
        .iter()
        .map(|hint| {
            let declared = hint.id.split(".require[").next().unwrap_or(&hint.id);
            let line = text.and_then(|text| declaring_line(text, declared));
            Finding {
                range: finding_range(line, None, None),
                severity: DiagnosticSeverity::WARNING,
                rule_id: hint.id.clone(),
                message: hint.to_string(),
                line,
                column: None,
                has_path: false,
                baseline_key: None,
                policy_url: None,
                fixable: false,
                sensitive: false,
                per_file: false,
                is_pattern_hint: true,
                description: None,
                docs_url: None,
                related_information: Vec::new(),
            }
        })
        .collect()
}

/// The 1-indexed line of `id: <id>` (a rule or fact) in a config's text.
fn declaring_line(text: &str, id: &str) -> Option<usize> {
    alint_core::physical_lines(text)
        .enumerate()
        .find_map(|(index, (_, raw))| {
            let line = raw
                .trim_start()
                .trim_start_matches('-')
                .trim_start()
                .trim_start_matches('{')
                .trim_start();
            let rest = line.strip_prefix("id:")?.trim_start();
            let value = if let Some(quoted) = rest.strip_prefix('"') {
                quoted.split_once('"')?.0
            } else if let Some(quoted) = rest.strip_prefix('\'') {
                quoted.split_once('\'')?.0
            } else {
                // Stop at flow delimiter `,` / `}` or inline `#` comment.
                let before_flow = rest.split([',', '}']).next().unwrap_or(rest);
                let before_comment = before_flow
                    .split_once(" #")
                    .or_else(|| before_flow.split_once("\t#"))
                    .map_or(before_flow, |(head, _)| head);
                before_comment.trim()
            };
            (value == id).then_some(index + 1)
        })
}

pub(crate) fn severity_of(level: Level) -> Option<DiagnosticSeverity> {
    match level {
        Level::Error => Some(DiagnosticSeverity::ERROR),
        Level::Warning => Some(DiagnosticSeverity::WARNING),
        Level::Info => Some(DiagnosticSeverity::INFORMATION),
        Level::Off => None,
    }
}

pub(crate) fn severity_label(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::ERROR => "error",
        DiagnosticSeverity::WARNING => "warning",
        _ => "info",
    }
}

/// alint coordinates are 1-indexed Unicode scalar positions; LSP coordinates
/// are 0-indexed UTF-16 positions. A primary range is preserved when present;
/// legacy point findings retain their one-character marker.
pub(crate) fn violation_range(violation: &Violation, text: Option<&str>) -> Range {
    violation.range.as_ref().map_or_else(
        || finding_range(violation.line, violation.column, text),
        |range| source_range(range, text),
    )
}

fn source_range(range: &alint_core::SourceRange, text: Option<&str>) -> Range {
    Range::new(
        source_position(range.start_line(), range.start_column(), text),
        source_position(range.end_line(), range.end_column(), text),
    )
}

fn physical_line(text: &str, line_index: usize) -> Option<&str> {
    alint_core::physical_lines(text)
        .nth(line_index)
        .map(|(_, line)| line)
}

fn source_position(line: usize, column: usize, text: Option<&str>) -> Position {
    let line_idx = line.saturating_sub(1);
    let character = text
        .and_then(|text| physical_line(text, line_idx))
        .map_or_else(
            || u32::try_from(column.saturating_sub(1)).unwrap_or(u32::MAX),
            |line_text| utf16_column(line_text, column).0,
        );
    Position::new(u32::try_from(line_idx).unwrap_or(u32::MAX), character)
}

fn finding_range(line: Option<usize>, column: Option<usize>, text: Option<&str>) -> Range {
    let line_idx = line.map_or(0, |line| line.saturating_sub(1));
    let line = u32::try_from(line_idx).unwrap_or(0);
    let (col, width) = match column {
        None => (0, 1),
        Some(column) => match text.and_then(|text| physical_line(text, line_idx)) {
            Some(line_text) => utf16_column(line_text, column),
            None => (u32::try_from(column.saturating_sub(1)).unwrap_or(0), 1),
        },
    };
    Range::new(
        Position::new(line, col),
        Position::new(line, col.saturating_add(width)),
    )
}

pub(crate) fn related_information(
    root: &Path,
    primary_abs: &Path,
    primary_range: Range,
    violation: &Violation,
    texts: &mut TextSource,
) -> Vec<DiagnosticRelatedInformation> {
    if violation.path_is_sensitive() {
        return Vec::new();
    }
    let mut information = Vec::with_capacity(
        violation.related_locations.len() + usize::from(violation.related_locations_omitted > 0),
    );
    for related in &violation.related_locations {
        let abs = root.join(&related.path);
        let Ok(uri) = Url::from_file_path(&abs) else {
            continue;
        };
        let range = related.range.as_ref().map_or_else(
            || finding_range(None, None, None),
            |range| source_range(range, texts.get(&abs)),
        );
        information.push(DiagnosticRelatedInformation {
            location: Location::new(uri, range),
            message: related.label.clone(),
        });
    }
    if violation.related_locations_omitted > 0
        && let Ok(uri) = Url::from_file_path(primary_abs)
    {
        information.push(DiagnosticRelatedInformation {
            location: Location::new(uri, primary_range),
            message: format!(
                "and {} more related locations",
                violation.related_locations_omitted
            ),
        });
    }
    information
}

/// Convert a 1-based `char` column within `line_text` to a 0-based
/// UTF-16 offset, plus the UTF-16 width of the character there (1 past
/// the end of the line).
pub(crate) fn utf16_column(line_text: &str, column: usize) -> (u32, u32) {
    let skip = column.saturating_sub(1);
    let mut chars = line_text.chars();
    let mut units: usize = 0;
    let mut consumed = 0usize;
    for c in chars.by_ref().take(skip) {
        units += c.len_utf16();
        consumed += 1;
    }
    // A column past the end of the line: one unit per missing char.
    units += skip - consumed;
    let width = chars.next().map_or(1, char::len_utf16);
    (
        u32::try_from(units).unwrap_or(u32::MAX),
        u32::try_from(width).unwrap_or(1),
    )
}

pub(crate) fn finding_to_diagnostic(
    f: &Finding,
    related_information_supported: bool,
) -> Diagnostic {
    let code_description = f
        .policy_url
        .as_deref()
        .and_then(|u| Url::parse(u).ok())
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .map(|href| CodeDescription { href });
    Diagnostic {
        range: f.range,
        severity: Some(f.severity),
        code: Some(NumberOrString::String(f.rule_id.clone())),
        code_description,
        source: Some("alint".to_string()),
        message: f.message.clone(),
        related_information: related_information_supported.then(|| f.related_information.clone()),
        ..Diagnostic::default()
    }
}

/// True when `pos` falls within the half-open `range`. Zero-width ranges
/// (such as EOF missing-newline or BOM markers emitted by `editorconfig_conforms`
/// and `constraint` rules) match when `pos` is exactly at the marker position.
pub(crate) fn range_contains(range: Range, pos: Position) -> bool {
    let start = (range.start.line, range.start.character);
    let end = (range.end.line, range.end.character);
    let pos = (pos.line, pos.character);
    if start == end {
        pos == start
    } else {
        start <= pos && pos < end
    }
}

/// True when two half-open ranges intersect. A zero-width range represents an
/// editor cursor or a zero-width diagnostic marker (BOM, missing final newline)
/// and overlaps a non-empty range when inside it, or another zero-width range
/// at the exact same position.
pub(crate) fn ranges_overlap(a: Range, b: Range) -> bool {
    let a_start = (a.start.line, a.start.character);
    let a_end = (a.end.line, a.end.character);
    let b_start = (b.start.line, b.start.character);
    let b_end = (b.end.line, b.end.character);
    match (a_start == a_end, b_start == b_end) {
        (true, true) => a_start == b_start,
        (true, false) => b_start <= a_start && a_start < b_end,
        (false, true) => a_start <= b_start && b_start < a_end,
        (false, false) => a_start < b_end && b_start < a_end,
    }
}

/// A range that covers any whole document. LSP clients clamp positions
/// past EOF, so this replaces the full file regardless of its length —
/// sidestepping UTF-16 column counting for a full-document edit.
pub(crate) fn whole_document() -> Range {
    Range::new(Position::new(0, 0), Position::new(u32::MAX, u32::MAX))
}

/// Convert a byte offset into `text` (valid UTF-8) to an LSP [`Position`]:
/// 0-indexed line, and a character column counted in UTF-16 code units (the LSP
/// default position encoding). A leading UTF-8 BOM (`\u{feff}` at byte 0) is
/// zero-width, matching [`alint_core::SourceRange::from_byte_range`] and
/// [`TextSource`]. A non-BMP scalar (e.g. an emoji) is two UTF-16
/// units, so a byte or `char` count would misplace the edit. LF, CRLF, and lone
/// CR end physical lines; an offset between the two bytes of CRLF maps to the
/// following line's start, matching [`alint_core::SourceRange`]. A range that
/// spans several lines is handled by converting its endpoints independently.
/// An offset at or past the end of `text` clamps to the final position.
pub(crate) fn byte_offset_to_position(text: &str, byte_offset: usize) -> Position {
    let mut line: u32 = 0;
    let mut character: u32 = 0;
    let mut previous_was_cr = false;
    for (idx, ch) in text.char_indices() {
        if idx >= byte_offset {
            return Position::new(line, character);
        }
        match ch {
            '\r' => {
                line = line.saturating_add(1);
                character = 0;
                previous_was_cr = true;
            }
            '\n' if previous_was_cr => previous_was_cr = false,
            '\n' => {
                line = line.saturating_add(1);
                character = 0;
            }
            _ => {
                if idx != 0 || ch != '\u{feff}' {
                    character =
                        character.saturating_add(u32::try_from(ch.len_utf16()).unwrap_or(0));
                }
                previous_was_cr = false;
            }
        }
    }
    Position::new(line, character)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declaring_line_handles_flow_mappings_comments_and_cr_endings() {
        let yaml = "version: 1\rrules:\r  - { id: flow_rule, kind: file_exists }\r  - id: inline_comment # note\r    kind: file_exists\r";
        assert_eq!(declaring_line(yaml, "flow_rule"), Some(3));
        assert_eq!(declaring_line(yaml, "inline_comment"), Some(4));
        assert_eq!(declaring_line(yaml, "missing"), None);
    }

    #[cfg(unix)]
    #[test]
    fn text_source_refuses_fifo_and_dev_zero_without_blocking() {
        use std::os::unix::fs::symlink;
        use std::time::Duration;

        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("hang.fifo");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs");
        assert!(status.success(), "mkfifo failed: {status}");

        let dev_zero_link = dir.path().join("zero.txt");
        symlink("/dev/zero", &dev_zero_link).unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        let fifo_clone = fifo.clone();
        let zero_clone = dev_zero_link.clone();
        std::thread::spawn(move || {
            let mut texts = TextSource::default();
            let a = texts.get(&fifo_clone).is_none();
            let b = texts.get(&zero_clone).is_none();
            let _ = tx.send((a, b));
        });
        let (fifo_none, zero_none) = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("TextSource::get must reject FIFO without hanging");
        assert!(fifo_none, "FIFO must be rejected");
        assert!(zero_none, "/dev/zero symlink must be rejected");
    }
}
