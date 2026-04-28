//! `markdown_paths_resolve` — backticked workspace paths in
//! markdown files must resolve to real files or directories.
//!
//! Targets the AGENTS.md / CLAUDE.md staleness problem:
//! agent-context files reference workspace paths in inline
//! backticks (`` `src/api/users.ts` ``), and those paths drift
//! as the codebase evolves. The v0.6 `agent-context-no-stale-paths`
//! rule surfaces *candidate* drift via a regex; this rule does
//! the precise check.
//!
//! Design doc: `docs/design/v0.7/markdown_paths_resolve.md`.

use std::path::Path;

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Options {
    /// Whitelist of path-shape prefixes to validate. A backticked
    /// token must start with one of these to be considered a path
    /// candidate. No defaults — every project's layout differs and
    /// the user must declare which prefixes mark a path.
    prefixes: Vec<String>,

    /// Skip backticked tokens containing template-variable
    /// markers (`{{ }}`, `${ }`, `<…>`). Default true.
    #[serde(default = "default_ignore_template_vars")]
    ignore_template_vars: bool,
}

fn default_ignore_template_vars() -> bool {
    true
}

#[derive(Debug)]
pub struct MarkdownPathsResolveRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    prefixes: Vec<String>,
    ignore_template_vars: bool,
}

impl Rule for MarkdownPathsResolveRule {
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
            let full = ctx.root.join(&entry.path);
            // Unreadable file: silently skip; a sibling rule can flag it.
            let Ok(bytes) = std::fs::read(&full) else {
                continue;
            };
            let Ok(text) = std::str::from_utf8(&bytes) else {
                continue; // non-UTF-8 markdown is degenerate; skip
            };
            for cand in scan_markdown_paths(text, &self.prefixes) {
                if self.ignore_template_vars && has_template_vars(&cand.token) {
                    continue;
                }
                let lookup = strip_path_decoration(&cand.token);
                if !path_resolves(ctx, lookup) {
                    let msg = self.message.clone().unwrap_or_else(|| {
                        format!(
                            "backticked path `{}` doesn't resolve to a file or directory",
                            cand.token
                        )
                    });
                    violations.push(
                        Violation::new(msg)
                            .with_path(&entry.path)
                            .with_location(cand.line, cand.column),
                    );
                }
            }
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "markdown_paths_resolve requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.prefixes.is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "markdown_paths_resolve requires a non-empty `prefixes` list — \
             declare which path shapes (e.g. [\"src/\", \"crates/\", \"docs/\"]) \
             count as path candidates in your codebase",
        ));
    }
    Ok(Box::new(MarkdownPathsResolveRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        prefixes: opts.prefixes,
        ignore_template_vars: opts.ignore_template_vars,
    }))
}

// ─── markdown scanner ──────────────────────────────────────────

/// One backticked path candidate found in a markdown source.
#[derive(Debug, PartialEq, Eq)]
struct Candidate {
    token: String,
    line: usize,
    column: usize,
}

/// Walk a markdown string, returning every backticked token that
/// starts with one of `prefixes`. Skips fenced code blocks
/// (```` ``` ```` / `~~~`) and 4-space-indented code blocks; those
/// contain code samples, not factual claims about the tree.
fn scan_markdown_paths(text: &str, prefixes: &[String]) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut in_fenced = false;
    let mut fence_marker: Option<char> = None;
    let mut fence_len: usize = 0;

    for (line_idx, line) in text.lines().enumerate() {
        let line_no = line_idx + 1;

        // Detect fenced-code-block boundaries. CommonMark allows
        // ``` and ~~~ with at least 3 markers; the closing fence
        // must use the same character and at least as many
        // markers. `info string` (e.g. ```yaml) follows the
        // opening fence; we don't care about its content.
        let trimmed = line.trim_start();
        if let Some((ch, n)) = detect_fence(trimmed) {
            if !in_fenced {
                in_fenced = true;
                fence_marker = Some(ch);
                fence_len = n;
            } else if fence_marker == Some(ch) && n >= fence_len && only_fence(trimmed, ch) {
                in_fenced = false;
                fence_marker = None;
                fence_len = 0;
            }
            continue;
        }
        if in_fenced {
            continue;
        }

        // Skip 4-space indented code blocks. Per CommonMark, only
        // applies when the indented line is NOT inside a list.
        // We're conservative — any 4-space-prefixed line is treated
        // as code unless it's a continuation of a list item, which
        // we don't track here. Acceptable: false-skip rate >
        // false-flag rate for our use.
        if line.starts_with("    ") || line.starts_with('\t') {
            continue;
        }

        // Find inline backticks. A run of N backticks opens an
        // inline span that closes at the next run of EXACTLY N
        // backticks. Per CommonMark, longer runs nest the span so
        // it can contain shorter backtick sequences. Most paths
        // use single backticks, which is what we optimise for.
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'`' {
                i += 1;
                continue;
            }
            let run_start = i;
            while i < bytes.len() && bytes[i] == b'`' {
                i += 1;
            }
            let run_len = i - run_start;
            // Find the matching closing run.
            let close_start = find_closing_run(&bytes[i..], run_len).map(|p| i + p);
            let Some(close) = close_start else {
                // Unmatched backticks → not a span; bail this line.
                break;
            };
            let token_bytes = &bytes[i..close];
            // Inline-code spans wrap their content with one space
            // padding when the content starts/ends with a backtick;
            // CommonMark trims one leading + one trailing space.
            let token = std::str::from_utf8(token_bytes).unwrap_or("").trim();
            if !token.is_empty() && starts_with_any_prefix(token, prefixes) {
                out.push(Candidate {
                    token: token.to_string(),
                    line: line_no,
                    column: run_start + 1, // 1-indexed; points at opening backtick
                });
            }
            i = close + run_len;
        }
    }
    out
}

/// If `s` starts with N+ backticks or tildes (N ≥ 3), return the
/// fence character and the run length. Otherwise None.
fn detect_fence(s: &str) -> Option<(char, usize)> {
    let mut chars = s.chars();
    let ch = chars.next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let n = 1 + chars.take_while(|&c| c == ch).count();
    if n >= 3 { Some((ch, n)) } else { None }
}

/// True if `s` consists only of `ch`-characters (allowing
/// trailing whitespace). Used to decide if an opening-fence
/// marker line could close a fence — `CommonMark` says the
/// closing fence cannot have an info string after the markers.
fn only_fence(s: &str, ch: char) -> bool {
    s.trim_end().chars().all(|c| c == ch)
}

/// Find the position (relative to `bytes` start) of the next run
/// of exactly `len` backticks. Returns None if not found in
/// `bytes`.
fn find_closing_run(bytes: &[u8], len: usize) -> Option<usize> {
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] == b'`' {
            i += 1;
        }
        if i - start == len {
            return Some(start);
        }
    }
    None
}

fn starts_with_any_prefix(s: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|p| s.starts_with(p))
}

/// True if `s` contains a template-variable marker
/// (`{{ … }}` / `${ … }` / `<…>`).
fn has_template_vars(s: &str) -> bool {
    s.contains("{{") || s.contains("${") || (s.contains('<') && s.contains('>'))
}

/// Strip trailing punctuation, trailing slashes, and
/// `:line` / `#L<n>` location suffixes that aren't part of
/// the path-on-disk we want to look up.
fn strip_path_decoration(s: &str) -> &str {
    // Strip a `#L<n>` GitHub-style anchor first (everything from
    // `#` to end), then a `:N` line-number suffix, then trailing
    // punctuation, then trailing slash.
    let hash = s.find('#').unwrap_or(s.len());
    let s = &s[..hash];
    let colon_loc = s
        .rfind(':')
        .filter(|&i| s[i + 1..].chars().all(|c| c.is_ascii_digit()) && i + 1 < s.len());
    let s = match colon_loc {
        Some(i) => &s[..i],
        None => s,
    };
    let s = s.trim_end_matches(|c: char| ".,:;?!".contains(c));
    s.trim_end_matches('/')
}

/// Does `lookup` resolve to a real file or directory in the
/// scanned tree? Glob characters in the lookup are matched
/// against the file index (any-of); plain paths use exact
/// lookup of either file or directory.
fn path_resolves(ctx: &Context<'_>, lookup: &str) -> bool {
    if lookup.is_empty() {
        return false;
    }
    if lookup.contains('*') || lookup.contains('?') || lookup.contains('[') {
        // Glob — match against the index. Build a globset on the
        // fly; cheap for one pattern.
        let Ok(glob) = globset::Glob::new(lookup) else {
            return false;
        };
        let matcher = glob.compile_matcher();
        return ctx.index.entries.iter().any(|e| matcher.is_match(&e.path));
    }
    let p = Path::new(lookup);
    ctx.index.entries.iter().any(|e| e.path == p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefixes(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn finds_inline_backtick_with_matching_prefix() {
        let pf = prefixes(&["src/", "docs/"]);
        let cands = scan_markdown_paths("see `src/foo.ts` and `npm` and `docs/x.md`", &pf);
        assert_eq!(cands.len(), 2);
        assert_eq!(cands[0].token, "src/foo.ts");
        assert_eq!(cands[1].token, "docs/x.md");
    }

    #[test]
    fn skips_fenced_code_blocks() {
        let pf = prefixes(&["src/"]);
        let md = "before\n\
                  ```yaml\n\
                  example: `src/should-not-fire.ts`\n\
                  ```\n\
                  after `src/should-fire.ts`";
        let cands = scan_markdown_paths(md, &pf);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].token, "src/should-fire.ts");
    }

    #[test]
    fn skips_indented_code_blocks() {
        let pf = prefixes(&["src/"]);
        let md = "normal `src/a.ts` line\n\
                  \n\
                  \x20\x20\x20\x20indented `src/should-not-fire.ts`\n";
        let cands = scan_markdown_paths(md, &pf);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].token, "src/a.ts");
    }

    #[test]
    fn handles_tilde_fences() {
        let pf = prefixes(&["src/"]);
        let md = "before `src/yes.ts`\n~~~\nin code: `src/no.ts`\n~~~\nafter `src/yes2.ts`";
        let tokens: Vec<_> = scan_markdown_paths(md, &pf)
            .into_iter()
            .map(|c| c.token)
            .collect();
        assert_eq!(tokens, vec!["src/yes.ts", "src/yes2.ts"]);
    }

    #[test]
    fn line_and_column_are_correct() {
        let pf = prefixes(&["src/"]);
        let md = "first line\nsecond `src/foo.ts` here";
        let cands = scan_markdown_paths(md, &pf);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].line, 2);
        // "second " is 7 chars + 1 for the opening backtick at col 8
        assert_eq!(cands[0].column, 8);
    }

    #[test]
    fn template_vars_detected() {
        assert!(has_template_vars("src/{{user_id}}.json"));
        assert!(has_template_vars("src/${name}.ts"));
        assert!(has_template_vars("src/<placeholder>.ts"));
        assert!(!has_template_vars("src/concrete.ts"));
        assert!(!has_template_vars("src/foo[0].ts")); // brackets without angle
    }

    #[test]
    fn path_decoration_stripped() {
        assert_eq!(strip_path_decoration("src/foo.ts"), "src/foo.ts");
        assert_eq!(strip_path_decoration("src/foo.ts."), "src/foo.ts");
        assert_eq!(strip_path_decoration("src/foo.ts,"), "src/foo.ts");
        assert_eq!(strip_path_decoration("src/foo.ts:42"), "src/foo.ts");
        assert_eq!(strip_path_decoration("src/foo.ts#L42"), "src/foo.ts");
        assert_eq!(strip_path_decoration("src/foo.ts:42#L1"), "src/foo.ts");
        assert_eq!(strip_path_decoration("src/foo/"), "src/foo");
    }

    #[test]
    fn prefix_matching() {
        let pf = prefixes(&["src/", "crates/"]);
        assert!(starts_with_any_prefix("src/foo.ts", &pf));
        assert!(starts_with_any_prefix("crates/alint", &pf));
        assert!(!starts_with_any_prefix("docs/x.md", &pf));
        assert!(!starts_with_any_prefix("README.md", &pf));
    }

    #[test]
    fn unmatched_backticks_do_not_explode() {
        let pf = prefixes(&["src/"]);
        let cands = scan_markdown_paths("`src/foo.ts unmatched", &pf);
        assert!(cands.is_empty());
    }

    #[test]
    fn double_backticks_can_contain_single() {
        let pf = prefixes(&["src/"]);
        let md = "double `` ` `` then `src/foo.ts`";
        let cands = scan_markdown_paths(md, &pf);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].token, "src/foo.ts");
    }
}
