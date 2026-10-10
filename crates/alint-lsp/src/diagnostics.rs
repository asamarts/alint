//! Findings to LSP diagnostics: grouping, severities, UTF-16 positions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::{
    CodeDescription, Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range, Url,
};

use alint_core::{Engine, Level, RuleResult, Violation};

use crate::render::{kind_description, rule_docs_url};
use crate::{Finding, FindingsByPath};

/// Document text for UTF-16 column conversion: the editor's buffer for
/// open documents (what was linted), else the file on disk (cached).
#[derive(Debug, Default)]
pub(crate) struct TextSource {
    overlays: HashMap<PathBuf, String>,
    disk: HashMap<PathBuf, Option<String>>,
}

impl TextSource {
    pub(crate) fn with_overlays(overlays: HashMap<PathBuf, String>) -> Self {
        Self {
            overlays,
            disk: HashMap::new(),
        }
    }

    fn get(&mut self, abs: &Path) -> Option<&str> {
        if self.overlays.contains_key(abs) {
            return self.overlays.get(abs).map(String::as_str);
        }
        self.disk
            .entry(abs.to_path_buf())
            .or_insert_with(|| {
                let size = std::fs::metadata(abs).ok()?.len();
                let bytes = alint_core::read_capped_or_skip(abs, size)?;
                Some(String::from_utf8_lossy(&bytes).into_owned())
            })
            .as_deref()
    }
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
        let Some(severity) = severity_of(result.level) else {
            continue;
        };
        let policy_url = result.policy_url.as_ref().map(ToString::to_string);
        let per_file = ctx.engine.is_per_file(&result.rule_id);
        let kind = ctx.kinds.get(result.rule_id.as_ref());
        let description = kind.and_then(|k| kind_description(k));
        let docs_url = kind.and_then(|k| rule_docs_url(k));
        for violation in &result.violations {
            // Anchor path-less (tree/file-level) findings to the config
            // file so a "missing required file" still shows somewhere.
            let abs = match &violation.path {
                Some(rel) => ctx.root.join(rel.as_ref()),
                None => ctx.config_path.to_path_buf(),
            };
            let text = if violation.path.is_some() && violation.column.is_some() {
                texts.get(&abs)
            } else {
                None
            };
            let range = violation_range(violation, text);
            by_path.entry(abs).or_default().push(Finding {
                range,
                severity,
                rule_id: result.rule_id.to_string(),
                message: violation.message.to_string(),
                line: violation.line,
                column: violation.column,
                policy_url: policy_url.clone(),
                fixable: result.is_fixable,
                per_file,
                description,
                docs_url: docs_url.clone(),
            });
        }
    }
    by_path
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

/// alint line/column are 1-indexed and optional, and the column counts
/// Unicode scalar values (`char`s); LSP positions are 0-indexed with the
/// character offset in UTF-16 code units (the protocol default). With
/// the document `text`, the column is converted exactly (an emoji
/// before the marker counts as two units) and the range spans the
/// marked character's full UTF-16 width. Without text it falls back to
/// the raw column. File- and tree-level findings (no line) anchor at
/// the start of the file. The range is one character wide so the editor
/// has something to attach the marker (and hover) to.
pub(crate) fn violation_range(violation: &Violation, text: Option<&str>) -> Range {
    let line_idx = violation.line.map_or(0, |l| l.saturating_sub(1));
    let line = u32::try_from(line_idx).unwrap_or(0);
    let (col, width) = match violation.column {
        None => (0, 1),
        Some(column) => match text.and_then(|t| t.split('\n').nth(line_idx)) {
            Some(line_text) => utf16_column(line_text, column),
            None => (u32::try_from(column.saturating_sub(1)).unwrap_or(0), 1),
        },
    };
    Range::new(
        Position::new(line, col),
        Position::new(line, col.saturating_add(width)),
    )
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

pub(crate) fn finding_to_diagnostic(f: &Finding) -> Diagnostic {
    let code_description = f
        .policy_url
        .as_deref()
        .and_then(|u| Url::parse(u).ok())
        .map(|href| CodeDescription { href });
    Diagnostic {
        range: f.range,
        severity: Some(f.severity),
        code: Some(NumberOrString::String(f.rule_id.clone())),
        code_description,
        source: Some("alint".to_string()),
        message: f.message.clone(),
        ..Diagnostic::default()
    }
}

/// True when `pos` falls within `range` (inclusive of both ends so a
/// hover on the single-character marker registers).
pub(crate) fn range_contains(range: Range, pos: Position) -> bool {
    let after_start = (pos.line, pos.character) >= (range.start.line, range.start.character);
    let before_end = (pos.line, pos.character) <= (range.end.line, range.end.character);
    after_start && before_end
}

/// True when two ranges intersect (the code-action selection vs. a
/// finding's marker).
pub(crate) fn ranges_overlap(a: Range, b: Range) -> bool {
    let a_start = (a.start.line, a.start.character);
    let a_end = (a.end.line, a.end.character);
    let b_start = (b.start.line, b.start.character);
    let b_end = (b.end.line, b.end.character);
    a_start <= b_end && b_start <= a_end
}

/// A range that covers any whole document. LSP clients clamp positions
/// past EOF, so this replaces the full file regardless of its length —
/// sidestepping UTF-16 column counting for a full-document edit.
pub(crate) fn whole_document() -> Range {
    Range::new(Position::new(0, 0), Position::new(u32::MAX, u32::MAX))
}

/// Convert a byte offset into `text` (valid UTF-8) to an LSP [`Position`]:
/// 0-indexed line, and a character column counted in UTF-16 code units (the LSP
/// default position encoding). A non-BMP scalar (e.g. an emoji) is two UTF-16
/// units, so a byte or `char` count would misplace the edit. `'\n'` ends a line and
/// resets the column; a lone `'\r'` counts as an ordinary character. A range that
/// spans several lines (a multi-line `replace` pattern, e.g. `(?s)foo.bar`) is
/// handled -- start and end are converted independently. An offset at or past the
/// end of `text` clamps to the final position.
pub(crate) fn byte_offset_to_position(text: &str, byte_offset: usize) -> Position {
    let mut line: u32 = 0;
    let mut character: u32 = 0;
    for (idx, ch) in text.char_indices() {
        if idx >= byte_offset {
            return Position::new(line, character);
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += u32::try_from(ch.len_utf16()).unwrap_or(0);
        }
    }
    Position::new(line, character)
}
