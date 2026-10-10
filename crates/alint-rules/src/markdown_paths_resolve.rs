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

use alint_core::{
    Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation, eval_per_file,
};
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Whitelist of path-shape prefixes to validate. A backticked
    /// token must start with one of these to be considered a path
    /// candidate. No defaults - every project's layout differs and
    /// the user must declare which prefixes mark a path.
    #[schemars(length(min = 1))]
    prefixes: Vec<String>,

    /// When true (default), skip backticked tokens containing `{{ ... }}`,
    /// `${ ... }`, or `<...>` template-variable markers. These are
    /// placeholders, not real paths.
    #[serde(default = "default_ignore_template_vars")]
    ignore_template_vars: bool,
}

crate::options_schema_for!(Options);

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
    alint_core::rule_common_impl!();
    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        eval_per_file(self, ctx)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for MarkdownPathsResolveRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Ok(Vec::new()); // non-UTF-8 markdown is degenerate; skip
        };
        let mut violations = Vec::new();
        for cand in scan_markdown_paths(text, &self.prefixes) {
            if self.ignore_template_vars && has_template_vars(&cand.token) {
                continue;
            }
            if candidate_resolves(ctx, &cand.token) {
                continue;
            }
            let msg = self.message.clone().unwrap_or_else(|| {
                format!(
                    "backticked path `{}` doesn't resolve to a file or directory",
                    cand.token
                )
            });
            violations.push(
                Violation::new(msg)
                    .with_path(std::sync::Arc::<Path>::from(path))
                    .with_location(cand.line, cand.column)
                    // Several unresolved links can sit on one line, so
                    // line-content collapses them. Key on the link target.
                    .with_baseline_key(cand.token.clone()),
            );
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(_paths) = &spec.paths else {
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
            "markdown_paths_resolve requires a non-empty `prefixes` list - \
             declare which path shapes (e.g. [\"src/\", \"crates/\", \"docs/\"]) \
             count as path candidates in your codebase",
        ));
    }
    Ok(Box::new(MarkdownPathsResolveRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
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
    // Blockquote nesting (`>` count) the open fence lives in.
    let mut fence_depth: usize = 0;

    for (line_idx, line) in text.lines().enumerate() {
        let line_no = line_idx + 1;
        // Fences and indented code blocks can sit inside a `>` blockquote; judge
        // them on the content after the markers. A fence left open when its
        // blockquote ends closes with it (CommonMark: a fenced block never
        // lazily continues), so the rest of the file is still scanned.
        //
        // Inside an open fence only the markers of the fence's OWN container are
        // structure: a `>` beyond that depth is fence content (a quoted line in a
        // code sample), so `> ```` inside a top-level fence must not close it.
        let max_depth = if in_fenced { fence_depth } else { usize::MAX };
        let (depth, content) = strip_blockquote_markers(line, max_depth);
        if in_fenced && depth < fence_depth {
            in_fenced = false;
            fence_marker = None;
            fence_len = 0;
        }

        // Detect fenced-code-block boundaries. CommonMark allows
        // ``` and ~~~ with at least 3 markers; the closing fence
        // must use the same character and at least as many
        // markers. `info string` (e.g. ```yaml) follows the
        // opening fence; we don't care about its content.
        let trimmed = content.trim_start();
        if let Some((ch, n)) = detect_fence(trimmed) {
            if !in_fenced {
                in_fenced = true;
                fence_marker = Some(ch);
                fence_len = n;
                fence_depth = depth;
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
        if content.starts_with("    ") || content.starts_with('\t') {
            continue;
        }

        // Find inline backticks. A run of N backticks opens an
        // inline span that closes at the next run of EXACTLY N
        // backticks. Per CommonMark, longer runs nest the span so
        // it can contain shorter backtick sequences. Most paths
        // use single backticks, which is what we optimise for.
        //
        // Linear in the line length: every backtick run is indexed once, and
        // each run is linked to the next run of the SAME length (its only
        // possible closer). Re-searching the rest of the line for every
        // unmatched run was O(n^1.5) on a line of many distinct run lengths.
        let bytes = line.as_bytes();
        let runs = backtick_runs(bytes);
        let next_same = next_same_len_run(&runs);
        let mut k = 0;
        while k < runs.len() {
            let (run_start, run_len) = runs[k];
            let Some(j) = next_same[k] else {
                // Unmatched run -> per CommonMark it is literal text, not a
                // span; keep scanning after it (a later span on the line is
                // still a candidate).
                k += 1;
                continue;
            };
            let token_bytes = &bytes[run_start + run_len..runs[j].0];
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
            // Resume after the closing run.
            k = j + 1;
        }
    }
    out
}

/// Every maximal run of backticks in `bytes`, as `(start, len)`, in order.
fn backtick_runs(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
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
        runs.push((start, i - start));
    }
    runs
}

/// For each run, the index of the next run with exactly the same length (the
/// only run that can close it), or `None`. One backward pass.
fn next_same_len_run(runs: &[(usize, usize)]) -> Vec<Option<usize>> {
    let mut next = vec![None; runs.len()];
    let mut last_seen: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (k, &(_, len)) in runs.iter().enumerate().rev() {
        next[k] = last_seen.insert(len, k);
    }
    next
}

/// Strip up to `max_depth` leading blockquote markers (`>` after at most 3
/// spaces, plus one optional following space), returning the nesting depth
/// stripped and the content.
fn strip_blockquote_markers(line: &str, max_depth: usize) -> (usize, &str) {
    let mut depth = 0;
    let mut rest = line;
    while depth < max_depth {
        let after_indent = rest.trim_start_matches(' ');
        if rest.len() - after_indent.len() > 3 {
            break;
        }
        let Some(inner) = after_indent.strip_prefix('>') else {
            break;
        };
        rest = inner.strip_prefix(' ').unwrap_or(inner);
        depth += 1;
    }
    (depth, rest)
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
/// marker line could close a fence - `CommonMark` says the
/// closing fence cannot have an info string after the markers.
fn only_fence(s: &str, ch: char) -> bool {
    s.trim_end().chars().all(|c| c == ch)
}

/// A token is a candidate when it starts with a prefix -- directly, or after a
/// leading `./` (`./src/x` names the same path as `src/x`).
fn starts_with_any_prefix(s: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|p| {
        s.starts_with(p.as_str())
            || s.strip_prefix("./")
                .is_some_and(|rest| rest.starts_with(p.as_str()))
    })
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

/// Resolve either the complete inline-code span or, when the span is shaped
/// like a command invocation, its command path. Trying the complete span first
/// preserves real paths containing whitespace. The command fallback is
/// deliberately conservative: the remainder must begin with an option marker
/// or contain no path separator, and the first word itself must resolve.
fn candidate_resolves(ctx: &Context<'_>, token: &str) -> bool {
    if path_resolves(ctx, strip_path_decoration(token)) {
        return true;
    }

    let Some((split, _)) = token.char_indices().find(|(_, ch)| ch.is_whitespace()) else {
        return false;
    };
    let command = strip_path_decoration(&token[..split]);
    let arguments = token[split..].trim_start();
    looks_like_command_arguments(arguments) && path_resolves(ctx, command)
}

fn looks_like_command_arguments(arguments: &str) -> bool {
    !arguments.is_empty()
        && (arguments.starts_with('-') || (!arguments.contains('/') && !arguments.contains('\\')))
}

/// Does `lookup` resolve to a real file or directory in the
/// scanned tree? Glob characters in the lookup are matched
/// against the file index (any-of); plain paths use exact
/// lookup of either file or directory.
fn path_resolves(ctx: &Context<'_>, lookup: &str) -> bool {
    if lookup.is_empty() {
        return false;
    }
    // Normalize `.` / `..` segments lexically first: `./src/foo.c` and
    // `src/../src/foo.c` name `src/foo.c`, which is how the index keys it. A
    // `..` that climbs out of the repo (or an absolute path) never resolves.
    let Some(normalized) = crate::pathsafe::normalize_confined(Path::new(lookup)) else {
        return false;
    };
    let normalized = crate::slash(&normalized);
    let lookup = normalized.as_str();
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
    ctx.index.entries.iter().any(|e| &*e.path == p)
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
    fn command_argument_shape_is_conservative() {
        assert!(looks_like_command_arguments("--check"));
        assert!(looks_like_command_arguments("-q docs/guide.md"));
        assert!(looks_like_command_arguments("verify"));
        assert!(looks_like_command_arguments("verify tree"));
        assert!(!looks_like_command_arguments("docs/guide.md"));
        assert!(!looks_like_command_arguments("verify docs/guide.md"));
        assert!(!looks_like_command_arguments(r"docs\guide.md"));
        assert!(!looks_like_command_arguments(""));
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
        assert_eq!(cands, Vec::new());
    }

    fn findings(md: &str, prefixes: &str, files: &[&str]) -> Vec<String> {
        use crate::test_support::{ctx, index, spec_yaml};
        let rule = build(&spec_yaml(&format!(
            "id: t\nkind: markdown_paths_resolve\npaths: \"**/*.md\"\n\
             prefixes: {prefixes}\nlevel: error\n"
        )))
        .unwrap();
        let idx = index(files);
        rule.as_per_file()
            .unwrap()
            .evaluate_file(
                &ctx(Path::new("/fake"), &idx),
                Path::new("README.md"),
                md.as_bytes(),
            )
            .unwrap()
            .into_iter()
            .map(|v| v.baseline_key.unwrap_or_default().into_owned())
            .collect()
    }

    #[test]
    fn dot_and_dotdot_segments_are_normalized_before_lookup() {
        // FP regression: `./src/foo.c` / `src/../src/foo.c` name an existing file
        // but were looked up verbatim and reported as unresolved.
        const NONE: [String; 0] = [];
        let files = ["src/foo.c"];
        assert_eq!(
            findings("see `src/../src/foo.c`", "[\"src/\"]", &files),
            NONE
        );
        assert_eq!(findings("see `./src/foo.c`", "[\"./\"]", &files), NONE);
        // A `./`-led path is a candidate for a `src/` prefix too -- and a broken
        // one is still reported.
        assert_eq!(findings("see `./src/foo.c`", "[\"src/\"]", &files), NONE);
        assert_eq!(
            findings("see `./src/gone.c`", "[\"src/\"]", &files),
            vec!["./src/gone.c"]
        );
        // `..` that climbs out of the repo never resolves.
        assert_eq!(
            findings("see `src/../../src/foo.c`", "[\"src/\"]", &files),
            vec!["src/../../src/foo.c"]
        );
    }

    #[test]
    fn fences_inside_blockquotes_are_code() {
        // FP regression: a fence opened inside a `>` blockquote was not
        // recognized, so its sample paths were checked as factual claims.
        let pf = prefixes(&["src/"]);
        let md = "> ```sh\n> cat `src/sample.ts`\n> ```\n> after `src/real.ts`\n";
        let tokens: Vec<_> = scan_markdown_paths(md, &pf)
            .into_iter()
            .map(|c| c.token)
            .collect();
        assert_eq!(tokens, vec!["src/real.ts"]);
        // A fence left open when its blockquote ends closes with it (CommonMark),
        // so the rest of the file is still scanned.
        let md = "> ```\n> `src/sample.ts`\n\nplain `src/real.ts`\n";
        let tokens: Vec<_> = scan_markdown_paths(md, &pf)
            .into_iter()
            .map(|c| c.token)
            .collect();
        assert_eq!(tokens, vec!["src/real.ts"]);
    }

    #[test]
    fn a_quoted_fence_line_inside_a_top_level_fence_does_not_close_it() {
        // FP regression: blockquote markers were stripped even inside an open
        // fence, so a `> ```` sample line closed a top-level fence and the rest
        // of the code sample was scanned as prose.
        let pf = prefixes(&["src/"]);
        let md = "```md\n> ```\n`src/sample.ts`\n```\nafter `src/real.ts`\n";
        let tokens: Vec<_> = scan_markdown_paths(md, &pf)
            .into_iter()
            .map(|c| c.token)
            .collect();
        assert_eq!(tokens, vec!["src/real.ts"]);
        // A fence inside a blockquote still closes on its own quoted fence line.
        let md = "> ```\n> > ```\n> `src/sample.ts`\n> ```\n> `src/real.ts`\n";
        let tokens: Vec<_> = scan_markdown_paths(md, &pf)
            .into_iter()
            .map(|c| c.token)
            .collect();
        assert_eq!(tokens, vec!["src/real.ts"]);
    }

    #[test]
    fn an_unmatched_backtick_run_is_literal_not_the_end_of_the_line() {
        // FN regression: an opening run with no matching close abandoned the
        // rest of the line. Per CommonMark it is literal text; scanning goes on.
        let pf = prefixes(&["src/"]);
        let md = "a `` stray then `src/real.ts`";
        let cands = scan_markdown_paths(md, &pf);
        assert_eq!(cands.len(), 1, "{cands:?}");
        assert_eq!(cands[0].token, "src/real.ts");
    }

    #[test]
    fn many_distinct_unmatched_runs_scan_in_linear_time() {
        // Perf regression: each unmatched run re-searched the rest of the line
        // for a closer, so a long line of runs of lengths 1..N (all unmatched)
        // took O(n^1.5) -- tens of seconds on a few MB. Linked by length, the
        // scan is linear; this ~2 MB line must finish near-instantly.
        let pf = prefixes(&["src/"]);
        let mut line = String::new();
        for n in 2..=2000 {
            line.push_str(&"`".repeat(n));
            line.push('x');
        }
        line.push_str(" `src/real.ts`");
        let t = std::time::Instant::now();
        let cands = scan_markdown_paths(&line, &pf);
        assert!(
            t.elapsed() < std::time::Duration::from_secs(2),
            "took {:?}",
            t.elapsed()
        );
        // Every run of length >= 2 is unique (unmatched, literal); the trailing
        // single-backtick span is still found, at its byte position.
        assert_eq!(cands.len(), 1, "{cands:?}");
        assert_eq!(cands[0].token, "src/real.ts");
        // Pairing is by exact length, in order: `a``b`c`` -> run 0 closes at run
        // 2, run 1 at run 3.
        assert_eq!(
            next_same_len_run(&backtick_runs(b"`a``b`c``")),
            vec![Some(2), Some(3), None, None]
        );
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
