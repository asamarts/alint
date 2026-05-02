//! `commented_out_code` — heuristic detector for blocks of
//! commented-out source code (as opposed to prose comments,
//! license headers, doc comments, or ASCII banners).
//!
//! Targets the "agent left dead code behind" pattern: agents
//! tend to comment-rather-than-delete during iteration, and
//! the leftovers accumulate. Existing primitives can ban
//! specific phrasings but can't catch the generic
//! "block-of-code-shaped-comments" pattern.
//!
//! Design doc: `docs/design/v0.7/commented_out_code.md`.
//!
//! ## Heuristic
//!
//! For each consecutive run of comment lines (≥ `min_lines`),
//! count the fraction of non-whitespace characters that are
//! **structural punctuation strongly biased toward code**:
//!
//! ```text
//!   strong_chars = ( ) { } [ ] ; = < > & | ^
//!   raw_density  = count(strong_chars) / non-whitespace-char-count
//! ```
//!
//! Backticks and quotes are deliberately excluded — backticks
//! show up constantly in rustdoc / `TSDoc` prose to delimit code
//! references (`` `foo` matches `bar` ``), and double quotes
//! appear in normal English. Including either inflates the
//! score on legitimate prose comments.
//!
//! Then normalise so the user-facing `threshold` field has a
//! useful midpoint at `0.5`:
//!
//! ```text
//!   density = min(raw_density / 0.20, 1.0)
//! ```
//!
//! At `raw_density = 0.20` (i.e. one-fifth of non-whitespace
//! chars are strong-code chars), the normalised density is
//! `1.0`. Real code blocks comfortably exceed this; English
//! prose is well below it because everyday writing rarely
//! uses brackets, semicolons, or assignment operators.
//!
//! Density ≥ `threshold` (default 0.5) marks the block as
//! code-shaped. Doc-comment markers (`///`, `/** */`) and
//! the file's first `skip_leading_lines` lines (license
//! headers) are excluded by construction.
//!
//! The score deliberately does NOT use identifier-token
//! density: English prose is dominated by 3+-letter words
//! that look identifier-shaped, so identifier counts can't
//! discriminate code from explanation. Punctuation can.

use std::path::Path;

use alint_core::{
    Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, ScopeFilter, Violation,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Options {
    /// `auto` (default) infers the comment-marker set from
    /// each file's extension. Explicit override useful for
    /// embedded DSLs or cases where the extension lies.
    #[serde(default)]
    language: Language,
    /// Minimum consecutive comment-line count for a block to
    /// be considered. 1-2 line comments are almost always
    /// prose; 3+ starts looking like dead code. Default 3.
    #[serde(default = "default_min_lines")]
    min_lines: usize,
    /// Token-density floor (0.0-1.0). Higher = stricter (only
    /// the most code-shaped blocks fire). Default 0.5.
    #[serde(default = "default_threshold")]
    threshold: f64,
    /// Skip the first N lines of any file. Defaults to 30 to
    /// pass over license headers without false-positive
    /// flagging them as commented-out code.
    #[serde(default = "default_skip_leading_lines")]
    skip_leading_lines: usize,
}

fn default_min_lines() -> usize {
    3
}
fn default_threshold() -> f64 {
    0.5
}
fn default_skip_leading_lines() -> usize {
    30
}

#[derive(Debug, Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Language {
    #[default]
    Auto,
    Rust,
    Typescript,
    Javascript,
    Python,
    Go,
    Java,
    C,
    Cpp,
    Ruby,
    Shell,
}

impl Language {
    /// Resolve a language to its concrete value (never `Auto`)
    /// based on a file extension.
    fn resolve(self, path: &Path) -> Self {
        if self != Self::Auto {
            return self;
        }
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "rs" => Self::Rust,
            "ts" | "tsx" => Self::Typescript,
            "js" | "jsx" | "mjs" | "cjs" => Self::Javascript,
            "py" => Self::Python,
            "go" => Self::Go,
            "java" | "kt" | "kts" | "scala" => Self::Java,
            "c" | "h" => Self::C,
            "cc" | "cpp" | "cxx" | "hpp" | "hh" => Self::Cpp,
            "rb" => Self::Ruby,
            "sh" | "bash" | "zsh" | "fish" => Self::Shell,
            _ => Self::Auto, // unknown — caller skips
        }
    }

    /// The set of line-comment markers for this language.
    /// Returned in priority order; the longest-match wins.
    fn line_markers(self) -> &'static [&'static str] {
        match self {
            // Doc-comment markers (`///`, `//!`) are ALSO line comments — we
            // identify them separately below to skip rather than score.
            Self::Rust
            | Self::Typescript
            | Self::Javascript
            | Self::Go
            | Self::Java
            | Self::C
            | Self::Cpp => &["//"],
            Self::Python | Self::Shell | Self::Ruby => &["#"],
            Self::Auto => &[],
        }
    }

    /// Inner-line markers that indicate a DOC comment, not a
    /// regular line comment. Blocks made entirely of these
    /// are excluded from density scoring.
    fn doc_line_markers(self) -> &'static [&'static str] {
        // `TSDoc` / JSDoc / Javadoc live in `/** */` block comments,
        // not line comments — they fall through to the empty default.
        match self {
            Self::Rust => &["///", "//!"],
            _ => &[],
        }
    }

    /// Block-comment delimiters: (open, close).
    fn block_delim(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Rust
            | Self::Typescript
            | Self::Javascript
            | Self::Go
            | Self::Java
            | Self::C
            | Self::Cpp => Some(("/*", "*/")),
            _ => None,
        }
    }

    /// Block-comment delimiters that mark a DOC block (Javadoc
    /// / `TSDoc` / rustdoc inner block). Skipped, not scored.
    fn doc_block_delim(self) -> Option<(&'static str, &'static str)> {
        match self {
            // /** … */ is Javadoc / `TSDoc` / rustdoc-inner.
            Self::Rust | Self::Typescript | Self::Javascript | Self::Java | Self::Cpp => {
                Some(("/**", "*/"))
            }
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct CommentedOutCodeRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    scope_filter: Option<ScopeFilter>,
    language: Language,
    min_lines: usize,
    threshold: f64,
    skip_leading_lines: usize,
}

impl Rule for CommentedOutCodeRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }
    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path) {
                continue;
            }
            if let Some(filter) = &self.scope_filter
                && !filter.matches(&entry.path, ctx.index)
            {
                continue;
            }
            let full = ctx.root.join(&entry.path);
            let Ok(bytes) = std::fs::read(&full) else {
                continue;
            };
            violations.extend(self.evaluate_file(ctx, &entry.path, &bytes)?);
        }
        Ok(violations)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }

    fn scope_filter(&self) -> Option<&ScopeFilter> {
        self.scope_filter.as_ref()
    }
}

impl PerFileRule for CommentedOutCodeRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        let lang = self.language.resolve(path);
        if lang == Language::Auto {
            return Ok(Vec::new()); // unknown extension — skip silently
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Ok(Vec::new());
        };
        let mut violations = Vec::new();
        for block in find_comment_blocks(text, lang) {
            if block.lines.len() < self.min_lines {
                continue;
            }
            if block.start_line <= self.skip_leading_lines {
                continue;
            }
            if block.is_doc_comment {
                continue;
            }
            let density = score_density(&block.content);
            if density >= self.threshold {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "block of {} commented-out lines (density {:.2}); remove or convert to runtime-checked branch",
                        block.lines.len(),
                        density,
                    )
                });
                violations.push(
                    Violation::new(msg)
                        .with_path(std::sync::Arc::<Path>::from(path))
                        .with_location(block.start_line, 1),
                );
            }
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "commented_out_code requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.min_lines < 2 {
        return Err(Error::rule_config(
            &spec.id,
            "commented_out_code `min_lines` must be ≥ 2",
        ));
    }
    if !(0.0..=1.0).contains(&opts.threshold) {
        return Err(Error::rule_config(
            &spec.id,
            "commented_out_code `threshold` must be between 0.0 and 1.0",
        ));
    }
    Ok(Box::new(CommentedOutCodeRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        scope_filter: spec.parse_scope_filter()?,
        language: opts.language,
        min_lines: opts.min_lines,
        threshold: opts.threshold,
        skip_leading_lines: opts.skip_leading_lines,
    }))
}

// ─── block detection ───────────────────────────────────────────

#[derive(Debug)]
struct CommentBlock {
    start_line: usize,
    lines: Vec<String>,
    /// Concatenated comment content with markers stripped.
    /// This is what the density scorer sees.
    content: String,
    /// True if every comment marker in the block is a
    /// doc-comment marker (e.g. `///`, `/** */`).
    is_doc_comment: bool,
}

fn find_comment_blocks(text: &str, lang: Language) -> Vec<CommentBlock> {
    let mut blocks = Vec::new();
    let line_markers = lang.line_markers();
    let doc_line_markers = lang.doc_line_markers();
    let block_delim = lang.block_delim();
    let doc_block_delim = lang.doc_block_delim();

    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();

        // Block-comment open (`/* … */`) — consume until close.
        if let Some((open, close)) = block_delim {
            if trimmed.starts_with(open) {
                let is_doc = doc_block_delim.is_some_and(|(d_open, _)| trimmed.starts_with(d_open));
                let start_line = i + 1;
                let mut block_lines = Vec::new();
                let mut block_content = String::new();
                let mut closed = false;
                let mut j = i;
                while j < lines.len() {
                    let l = lines[j];
                    block_lines.push(l.to_string());
                    let stripped = strip_block_comment_markers(l, open, close);
                    block_content.push_str(&stripped);
                    block_content.push('\n');
                    if l.contains(close) && (j > i || trimmed.matches(close).count() > 0) {
                        closed = true;
                        j += 1;
                        break;
                    }
                    j += 1;
                }
                if closed {
                    blocks.push(CommentBlock {
                        start_line,
                        lines: block_lines,
                        content: block_content,
                        is_doc_comment: is_doc,
                    });
                }
                i = j;
                continue;
            }
        }

        // Line-comment run (consecutive `//` / `#` lines).
        if line_markers.iter().any(|m| trimmed.starts_with(*m)) {
            let start_line = i + 1;
            let mut block_lines = Vec::new();
            let mut block_content = String::new();
            let mut all_doc = !doc_line_markers.is_empty();
            let mut j = i;
            while j < lines.len() {
                let l = lines[j];
                let lt = l.trim_start();
                let Some(m) = line_markers.iter().find(|mk| lt.starts_with(*mk)).copied() else {
                    break;
                };
                let is_doc_line = doc_line_markers.iter().any(|d| {
                    lt.starts_with(d)
                        && (lt.len() == d.len()
                            || !lt[d.len()..].starts_with(m.chars().next().unwrap_or(' ')))
                });
                if !is_doc_line {
                    all_doc = false;
                }
                block_lines.push(l.to_string());
                block_content.push_str(strip_line_marker(lt, m));
                block_content.push('\n');
                j += 1;
            }
            blocks.push(CommentBlock {
                start_line,
                lines: block_lines,
                content: block_content,
                is_doc_comment: all_doc,
            });
            i = j;
            continue;
        }

        i += 1;
    }
    blocks
}

fn strip_line_marker<'a>(line: &'a str, marker: &str) -> &'a str {
    let after = line.strip_prefix(marker).unwrap_or(line);
    after.strip_prefix(' ').unwrap_or(after)
}

fn strip_block_comment_markers(line: &str, open: &str, close: &str) -> String {
    let mut s = line.trim().to_string();
    if let Some(rest) = s.strip_prefix(open) {
        s = rest.to_string();
    }
    if let Some(rest) = s.strip_suffix(close) {
        s = rest.to_string();
    }
    // Trim leading ` * ` (Javadoc / rustdoc continuation).
    let trimmed = s.trim_start();
    if let Some(rest) = trimmed.strip_prefix("* ") {
        return rest.to_string();
    }
    if trimmed == "*" {
        return String::new();
    }
    s
}

// ─── density scoring ───────────────────────────────────────────

/// Characters strongly biased toward code over English prose.
/// Brackets and assignment / comparison operators show up
/// constantly in code and almost never in normal writing.
/// Backticks and quotes are NOT included — backticks delimit
/// code references in rustdoc / `TSDoc` prose
/// (`` `foo` matches `bar` ``), double quotes appear in normal
/// English. Either would inflate the score on legitimate prose
/// comments.
const STRONG_CODE_CHARS: &[char] = &[
    '(', ')', '{', '}', '[', ']', ';', '=', '<', '>', '&', '|', '^',
];

/// `raw_density / SATURATION_POINT` is clamped to 1.0, so this
/// is the raw-density value that maps to a normalised density
/// of 1.0. 0.20 was chosen empirically by sampling: typical
/// Rust / TS / Python code blocks sit at 0.18-0.30; pure
/// English prose sits below 0.05.
const SATURATION_POINT: f64 = 0.20;

/// Punctuation-density score in [0.0, 1.0]. See module-level
/// rustdoc for the design rationale — the short version is
/// "count brackets / semicolons / assignment operators, ignore
/// identifier tokens (prose has identifier-shaped words too)."
///
/// Pre-pass: any run of 5+ identical characters gets dropped
/// before scoring, so ASCII-art separators
/// (`============================================`, `----`,
/// `####`) don't inflate the structural-char count and
/// flag a banner comment as "looks like code."
fn score_density(content: &str) -> f64 {
    let collapsed = drop_long_runs(content);
    let nonws_count = collapsed.chars().filter(|c| !c.is_whitespace()).count();
    if nonws_count == 0 {
        return 0.0;
    }
    let strong_count = collapsed
        .chars()
        .filter(|c| STRONG_CODE_CHARS.contains(c))
        .count();
    #[allow(clippy::cast_precision_loss)]
    let raw = strong_count as f64 / nonws_count as f64;
    (raw / SATURATION_POINT).min(1.0)
}

/// Strip runs of 5+ identical characters. Used to defang
/// ASCII-art separators / banners (`==========`, `----`,
/// `####`) before density scoring — those are layout, not
/// code structure, and inflate the strong-char count.
fn drop_long_runs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut buf: Vec<char> = Vec::new();
    let mut prev: Option<char> = None;
    for ch in s.chars() {
        if Some(ch) == prev {
            buf.push(ch);
        } else {
            if buf.len() < 5 {
                out.extend(buf.iter());
            }
            buf.clear();
            buf.push(ch);
            prev = Some(ch);
        }
    }
    if buf.len() < 5 {
        out.extend(buf.iter());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_high_for_code_low_for_prose() {
        // Real code: high density.
        let code = "let x = compute(y, z); if x > 0 { return x; }";
        let d_code = score_density(code);
        assert!(d_code > 0.5, "code density {d_code} should be > 0.5");

        // Prose: low density.
        let prose = "This module parses RFC 9535 JSONPath expressions and resolves them.";
        let d_prose = score_density(prose);
        assert!(d_prose < 0.5, "prose density {d_prose} should be < 0.5");
    }

    #[test]
    fn line_block_in_rust_detected_with_markers_stripped() {
        let src = "fn main() {}\n// let x = compute(y);\n// if x > 0 { return x; }\n// log(\"unused\");\nfn other() {}";
        let blocks = find_comment_blocks(src, Language::Rust);
        assert_eq!(blocks.len(), 1);
        let b = &blocks[0];
        assert_eq!(b.lines.len(), 3);
        assert_eq!(b.start_line, 2);
        assert!(b.content.contains("let x = compute(y);"));
        assert!(!b.is_doc_comment);
    }

    #[test]
    fn rust_doc_line_comments_marked_as_doc() {
        let src = "/// Documents the next item.\n/// More docs.\n/// Even more.\nfn foo() {}";
        let blocks = find_comment_blocks(src, Language::Rust);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].is_doc_comment, "/// block must be marked as doc");
    }

    #[test]
    fn block_comment_javadoc_marked_as_doc() {
        let src = "/**\n * Documented.\n * @param x foo\n */\nfunction bar() {}";
        let blocks = find_comment_blocks(src, Language::Typescript);
        assert!(!blocks.is_empty());
        assert!(blocks[0].is_doc_comment, "/** … */ must be marked as doc");
    }

    #[test]
    fn python_hash_block_detected() {
        let src = "x = 1\n# old = compute(x)\n# if old > 0:\n#    print(old)\nprint(x)";
        let blocks = find_comment_blocks(src, Language::Python);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].content.contains("old = compute(x)"));
    }

    #[test]
    fn end_to_end_threshold_filters_prose() {
        // A 3-line // block of prose: should NOT score above default.
        let prose_src = "fn foo() {}\n// This is a normal explanatory comment\n// describing what foo does.\n// Multiple lines of prose.";
        let blocks = find_comment_blocks(prose_src, Language::Rust);
        assert_eq!(blocks.len(), 1);
        let d = score_density(&blocks[0].content);
        assert!(d < 0.5, "prose comment density {d} should be < 0.5");

        // A 3-line // block of code: should score above default.
        let code_src = "fn foo() {}\n// let x = compute(y);\n// if x > 0 { return x; }\n// log_metric(\"path-a\", x);";
        let blocks = find_comment_blocks(code_src, Language::Rust);
        assert_eq!(blocks.len(), 1);
        let d = score_density(&blocks[0].content);
        assert!(d >= 0.5, "code comment density {d} should be >= 0.5");
    }

    #[test]
    fn banner_separators_dont_score_as_code() {
        // Common pattern: ASCII-art banner around a section title.
        let banner = "// ============================================\n\
                      // Section Title\n\
                      // ============================================";
        let blocks = find_comment_blocks(banner, Language::Rust);
        assert_eq!(blocks.len(), 1);
        let d = score_density(&blocks[0].content);
        assert!(d < 0.5, "banner density {d} should be < 0.5");
    }

    #[test]
    fn drop_long_runs_strips_banners() {
        assert_eq!(drop_long_runs("foo ============= bar"), "foo  bar");
        assert_eq!(drop_long_runs("a==b"), "a==b"); // run of 2, kept
        assert_eq!(drop_long_runs("a===b"), "a===b"); // run of 3, kept
        assert_eq!(drop_long_runs("a====b"), "a====b"); // run of 4, kept
        assert_eq!(drop_long_runs("a=====b"), "ab"); // run of 5, dropped
    }

    #[test]
    fn language_extension_resolution() {
        let path = Path::new("foo.rs");
        assert_eq!(Language::Auto.resolve(path), Language::Rust);
        let path = Path::new("foo.py");
        assert_eq!(Language::Auto.resolve(path), Language::Python);
        let path = Path::new("foo.tsx");
        assert_eq!(Language::Auto.resolve(path), Language::Typescript);
        let path = Path::new("unknown");
        assert_eq!(Language::Auto.resolve(path), Language::Auto);
    }
}
