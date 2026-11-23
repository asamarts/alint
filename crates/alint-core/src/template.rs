//! String substitution for path templates and message templates.
//!
//! Two variants, distinguished by delimiter style:
//!
//! - **Path templates** — single braces, fixed token set derived from a
//!   matched file's relative path. Example: `"{dir}/{stem}.h"`.
//! - **Message templates** — double braces, namespaced lookups for rule
//!   messages and similar user-facing strings. Example:
//!   `"{{ctx.primary}} has no matching header at {{ctx.partner}}"`.
//!
//! Both are intentionally small and self-contained: no regex dependency,
//! no dynamic parser. Unknown tokens are preserved literally so a typo
//! surfaces in output rather than silently blanking out.

use std::path::Path;

use crate::config::PathsSpec;

/// Token values derived from a relative path. Consumed by
/// [`render_path`] and by cross-file rules to resolve partner paths.
#[derive(Debug, Clone)]
pub struct PathTokens {
    pub path: String,
    pub dir: String,
    pub basename: String,
    pub stem: String,
    pub ext: String,
    pub parent_name: String,
}

/// Which path tokens a set of templates refers to. Per-entry callers derive
/// only these: each token is a fresh allocation, and most templates use one or
/// two of the six.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenSet(u8);

impl TokenSet {
    const PATH: u8 = 1;
    const DIR: u8 = 1 << 1;
    const BASENAME: u8 = 1 << 2;
    const STEM: u8 = 1 << 3;
    const EXT: u8 = 1 << 4;
    const PARENT_NAME: u8 = 1 << 5;

    /// Every token.
    pub const ALL: Self = Self(0b11_1111);
    /// No token.
    pub const NONE: Self = Self(0);

    /// The tokens `template` contains (by literal `{name}` occurrence, so an
    /// unknown or escaped token can only over-include, never miss one).
    #[must_use]
    pub fn of(template: &str) -> Self {
        if !template.contains('{') {
            return Self::NONE;
        }
        let mut bits = 0;
        for (name, bit) in [
            ("{path}", Self::PATH),
            ("{dir}", Self::DIR),
            ("{basename}", Self::BASENAME),
            ("{stem}", Self::STEM),
            ("{ext}", Self::EXT),
            ("{parent_name}", Self::PARENT_NAME),
        ] {
            if template.contains(name) {
                bits |= bit;
            }
        }
        Self(bits)
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    const fn has(self, bit: u8) -> bool {
        self.0 & bit != 0
    }
}

impl PathTokens {
    /// Derive tokens from a relative path. Missing components (e.g. a path
    /// with no parent, or no extension) resolve to the empty string.
    pub fn from_path(rel: &Path) -> Self {
        Self::from_path_with(rel, TokenSet::ALL)
    }

    /// [`Self::from_path`] deriving only the tokens in `needed`; the others are
    /// empty. Rendering a template whose tokens are all in `needed` gives the
    /// same result as with every token derived. A path containing a glob
    /// metacharacter derives every token, so [`unescape_token_values`] (which
    /// consults all six) also behaves exactly as with [`Self::from_path`].
    pub fn from_path_with(rel: &Path, needed: TokenSet) -> Self {
        let needed = if needs_glob_escape(&rel.to_string_lossy()) {
            TokenSet::ALL
        } else {
            needed
        };
        let derive = |bit, f: &dyn Fn() -> String| {
            if needed.has(bit) { f() } else { String::new() }
        };
        Self {
            path: derive(TokenSet::PATH, &|| rel.display().to_string()),
            dir: derive(TokenSet::DIR, &|| {
                rel.parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            }),
            basename: derive(TokenSet::BASENAME, &|| {
                rel.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string()
            }),
            stem: derive(TokenSet::STEM, &|| {
                rel.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string()
            }),
            ext: derive(TokenSet::EXT, &|| {
                rel.extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string()
            }),
            parent_name: derive(TokenSet::PARENT_NAME, &|| {
                rel.parent()
                    .and_then(|p| p.file_name())
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string()
            }),
        }
    }

    /// Tokens for building a nested `require:` rule before any entry is
    /// iterated: the parent's load-time check and the engine's preparation
    /// traversal. Each token renders as its name between underscores (`{stem}`
    /// as `_stem_`). No token renders empty or like another, each value is as
    /// wide as the token it stands for, and the value is ASCII word characters,
    /// so a regex, glob or `JSONPath` that is valid for an ordinary name is valid
    /// for it too. Inside a [`Preparation`], [`templates_rendering`] finds the
    /// templates a value was rendered from, so a builder can tell a token from
    /// text the configuration wrote, and [`unrender_placeholders`] turns the
    /// values back into tokens for an error message. A check that depends on
    /// the value a token will take, such as a format inferred from an
    /// extension, belongs to each iteration's own build.
    ///
    /// For alint's own crates, which prepare nested rules with it; hidden from
    /// the documentation like [`Preparation`], outside which it means nothing.
    #[doc(hidden)]
    #[must_use]
    pub fn placeholder() -> Self {
        let token = |name: &str| format!("_{name}_");
        Self {
            path: token("path"),
            dir: token("dir"),
            basename: token("basename"),
            stem: token("stem"),
            ext: token("ext"),
            parent_name: token("parent_name"),
        }
    }
}

std::thread_local! {
    /// The templates of each [`Preparation`] open on this thread, outermost
    /// first. Only the outermost one's are kept: see [`Preparation`].
    static PREPARATIONS: std::cell::RefCell<Vec<Vec<String>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// A nested rule's preparation build on this thread, open until the guard
/// drops. While one is open, [`templates_rendering`] finds the templates a
/// value was rendered from with [`PathTokens::placeholder`], so a builder
/// tells a token from text the configuration wrote, even text spelled like a
/// placeholder (`data/_stem_`). Outside one, every rendered value comes from a
/// real path (an iteration's build) or from the configuration (a top-level
/// rule), so no part of it is taken for a token.
///
/// Only the outermost open preparation's templates count. They are those of a
/// nested rule that a top-level rule declares, as the configuration wrote
/// them, and they hold every string of the rules nested inside it. A deeper
/// rule's own templates are renderings of those (its parent was built from
/// the placeholders), which cannot tell a token from its spelling, so a
/// caller keeps its guard open while it prepares the rules nested inside.
///
/// For alint's own crates, which prepare nested rules with it. A thread-local
/// rather than a field or an argument: the build runs on the thread that
/// opens the scope, and a field on the sealed `RuleSpec` (or a parameter of
/// the registry's build functions) would add public API for state that only
/// alint's own builders read while one nested rule is prepared.
#[doc(hidden)]
#[derive(Debug)]
#[must_use = "the preparation ends when the guard drops"]
pub struct Preparation {
    /// Keeps the guard on the thread whose stack it pushed.
    _not_send: std::marker::PhantomData<*const ()>,
}

impl Preparation {
    /// Open a preparation of a nested rule whose spec renders `templates`
    /// ([`NestedRuleSpec::templates`](crate::NestedRuleSpec::templates)).
    pub fn enter<'t>(templates: impl IntoIterator<Item = &'t str>) -> Self {
        PREPARATIONS.with(|open| {
            let mut open = open.borrow_mut();
            let kept = if open.is_empty() {
                templates.into_iter().map(str::to_owned).collect()
            } else {
                Vec::new()
            };
            open.push(kept);
        });
        Self {
            _not_send: std::marker::PhantomData,
        }
    }
}

impl Drop for Preparation {
    fn drop(&mut self) {
        PREPARATIONS.with(|open| {
            open.borrow_mut().pop();
        });
    }
}

/// Inside a [`Preparation`], the templates of the outermost open one that
/// render as `rendered` with [`PathTokens::placeholder`], each once. A glob
/// option renders the same way, since no placeholder needs escaping. Empty
/// outside one, where no value has a token, and for a value no template
/// renders. Several templates are found only when the configuration spells a
/// placeholder in one string and writes the token in another, as
/// `data/_stem_` and `data/{stem}`, so a caller deciding on them decides what
/// holds for each.
///
/// For alint's own crates; hidden like [`Preparation`].
#[doc(hidden)]
#[must_use]
pub fn templates_rendering(rendered: &str) -> Vec<String> {
    let tokens = PathTokens::placeholder();
    let mut found: Vec<String> = PREPARATIONS.with(|open| {
        open.borrow().first().map_or_else(Vec::new, |templates| {
            templates
                .iter()
                .filter(|template| render_path(template, &tokens) == rendered)
                .cloned()
                .collect()
        })
    });
    found.sort_unstable();
    found.dedup();
    found
}

/// `text`, an error message from a preparation build, with each value that
/// [`PathTokens::placeholder`] rendered from one of `templates` turned back
/// into that template (`"_dir_/[_stem_"` into `"{dir}/[{stem}"`). Only a whole
/// rendered value is replaced, as written or as `{:?}` escapes it, and only
/// where it does not run on into a word character, so text that merely
/// contains a placeholder's spelling stays as it is: a kind such as
/// `json_path_equals`, a rule id such as `pkg_dir_layout`, or a literal name.
/// A value and its template are equally wide, so a column a message points at
/// still lines up.
///
/// For alint's own crates; hidden like [`Preparation`].
#[doc(hidden)]
#[must_use]
pub fn unrender_placeholders<'t>(
    text: &str,
    templates: impl IntoIterator<Item = &'t str>,
) -> String {
    fn debug_escaped(s: &str) -> String {
        let quoted = format!("{s:?}");
        quoted[1..quoted.len() - 1].to_string()
    }
    let tokens = PathTokens::placeholder();
    let mut pairs: Vec<(String, String)> = Vec::new();
    for template in templates {
        if TokenSet::of(template) == TokenSet::NONE {
            continue;
        }
        let rendered = render_path(template, &tokens);
        if rendered == template {
            continue;
        }
        let escaped = debug_escaped(&rendered);
        if escaped != rendered {
            pairs.push((escaped, debug_escaped(template)));
        }
        pairs.push((rendered, template.to_string()));
    }
    // An empty value would match everywhere without moving the scan.
    pairs.retain(|(rendered, _)| !rendered.is_empty());
    // Longest first, so a whole value wins over a shorter one inside it.
    pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.cmp(b)));
    pairs.dedup();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = String::with_capacity(text.len());
    // The scan walks `text`'s characters once, so it ends whatever matches.
    let mut chars = text.char_indices();
    'scan: while let Some((at, ch)) = chars.next() {
        let rest = &text[at..];
        let before = text[..at].chars().next_back();
        for (rendered, template) in &pairs {
            let Some(after) = rest.strip_prefix(rendered.as_str()) else {
                continue;
            };
            let joins_before = rendered.starts_with(is_word) && before.is_some_and(is_word);
            let joins_after = rendered.ends_with(is_word) && after.starts_with(is_word);
            if !joins_before && !joins_after {
                out.push_str(template);
                // `ch` starts the match; skip the rest of it.
                for _ in 1..rendered.chars().count() {
                    chars.next();
                }
                continue 'scan;
            }
        }
        out.push(ch);
    }
    out
}

/// Substitute `{token}` placeholders in a path-shaped template. Unknown
/// tokens are preserved literally (so `"{unknown}"` renders as `"{unknown}"`).
///
/// Substitution is a single left-to-right scan into a fresh buffer: each known
/// `{token}` is replaced by its value, and that value is emitted as-is and
/// never re-scanned. A repeated `String::replace` pass (the prior approach)
/// re-substituted a token that appeared in an *earlier* substitution's value —
/// so a repo file literally named `a{ext}.c` (stem `a{ext}`) had its embedded
/// `{ext}` wrongly expanded by the later `{ext}` pass, yielding a bogus path
/// for the forbidding rules (L8). Unknown `{tokens}` are preserved verbatim.
///
/// An EMPTY token value that sits in its own leading path segment collapses
/// together with the `/` after it: a root-level file has `{dir}` = `""`, so the
/// documented `"{dir}/{stem}.h"` renders `"top.h"`, not the absolute-looking
/// `"/top.h"` (which never matches a repo-relative path).
///
/// The output is a LITERAL path: substituted values are not escaped. For a
/// template that is compiled as a glob, use [`render_path_glob`].
pub fn render_path(template: &str, t: &PathTokens) -> String {
    render_path_with(template, t, |v, out| out.push_str(v))
}

/// [`render_path`] for a template that is compiled as a **glob** (a nested
/// rule's `paths:`, an `iter.has_file` pattern). Each substituted token value
/// is glob-escaped (see [`glob_escape`]) so a real path containing glob
/// metacharacters -- a Next.js `app/[slug]` dir, a literal `pkgs/*`, an
/// unbalanced `{` -- matches only itself instead of being reinterpreted as a
/// pattern (a false positive / false negative / invalid-glob error). The
/// user-written template text around the tokens keeps its glob meaning.
pub fn render_path_glob(template: &str, t: &PathTokens) -> String {
    render_path_with(template, t, |v, out| {
        // A value landing at the very start of the pattern that begins with `!`
        // would be read as an EXCLUDE by `Scope::from_patterns`; wrap the `!` in
        // a single-arm alternation so it is a literal.
        let v = if out.is_empty()
            && let Some(rest) = v.strip_prefix('!')
        {
            out.push_str("{!}");
            rest
        } else {
            v
        };
        push_glob_escaped(out, v);
    })
}

/// True when `s` has a byte [`glob_escape`] would rewrite.
fn needs_glob_escape(s: &str) -> bool {
    s.bytes().any(|b| {
        matches!(b, b'?' | b'*' | b'[' | b']' | b'{' | b'}') || (b == b'\\' && !cfg!(windows))
    })
}

/// Append `v` glob-escaped, without allocating when it has nothing to escape.
fn push_glob_escaped(out: &mut String, v: &str) {
    if needs_glob_escape(v) {
        out.push_str(&glob_escape(v));
    } else {
        out.push_str(v);
    }
}

/// Escape `s` so it matches only itself when compiled as an alint glob
/// (`globset`, the dialect `Scope` uses): the metacharacters `* ? [ ] { }` are
/// wrapped in a one-character class (`[` -> `[[]`), and on non-Windows a `\`
/// (a legal filename byte there, and globset's escape character) is
/// backslash-escaped. On Windows `\` is the path separator, never part of a
/// name, so it is left as-is.
#[must_use]
pub fn glob_escape(s: &str) -> String {
    if !needs_glob_escape(s) {
        return s.to_owned();
    }
    let escaped = globset::escape(s);
    if cfg!(windows) {
        escaped
    } else {
        escaped.replace('\\', "\\\\")
    }
}

/// The single literal path an alint glob `pattern` matches, with its escapes
/// resolved -- or `None` when it has a real (unescaped) metacharacter (or a
/// leading `!` exclude), i.e. it may match more than one path.
///
/// This is the inverse of [`glob_escape`], and the ONE literal test the
/// O(1) "is this a literal path?" fast paths must use: a raw scan for
/// `* ? [ ] { }` misses that on non-Windows `\` is globset's escape character,
/// so a rendered `app/a\\b/page.tsx` (from a directory literally named `a\b`)
/// was looked up verbatim -- with the escape still in it -- and never found.
/// Recognized escapes: `\x` (non-Windows) and a one-character class `[x]`
/// (what `globset::escape` emits for a metacharacter). Anything else glob-shaped
/// returns `None`, so the caller falls back to real glob matching.
#[must_use]
pub fn literal_glob_path(pattern: &str) -> Option<String> {
    if pattern.starts_with('!') {
        return None;
    }
    if !pattern
        .bytes()
        .any(|b| matches!(b, b'\\' | b'[' | b'*' | b'?' | b']' | b'{' | b'}'))
    {
        return Some(pattern.to_owned());
    }
    let mut out = String::with_capacity(pattern.len());
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if !cfg!(windows) => out.push(chars.next()?),
            '[' => {
                // Only a single-char class `[x]` is literal (`[[]`, `[]]`, `[*]`).
                let x = chars.next()?;
                if matches!(x, '!' | '^') || chars.next()? != ']' {
                    return None;
                }
                out.push(x);
            }
            '*' | '?' | ']' | '{' | '}' => return None,
            c => out.push(c),
        }
    }
    Some(out)
}

fn render_path_with(
    template: &str,
    t: &PathTokens,
    mut push: impl FnMut(&str, &mut String),
) -> String {
    // Longest-first only matters if one token is a prefix of another; none is,
    // but the order is kept stable for clarity / future additions.
    let tokens: [(&str, &str); 6] = [
        ("{parent_name}", &t.parent_name),
        ("{basename}", &t.basename),
        ("{path}", &t.path),
        ("{stem}", &t.stem),
        ("{dir}", &t.dir),
        ("{ext}", &t.ext),
    ];
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let at_brace = &rest[open..];
        if let Some((tok, val)) = tokens.iter().find(|(tok, _)| at_brace.starts_with(tok)) {
            rest = &at_brace[tok.len()..];
            // An empty value that forms a whole leading segment (start of the
            // output or right after a `/`) collapses with its trailing `/`, so
            // `{dir}/x` for a root-level file is `x`, not `/x`.
            if val.is_empty() && (out.is_empty() || out.ends_with('/')) {
                if let Some(after) = rest.strip_prefix('/') {
                    rest = after;
                }
            } else {
                push(val, &mut out);
            }
        } else {
            // A `{` that doesn't begin a known token: emit it literally and
            // resume after it (preserves `{unknown}` verbatim).
            out.push('{');
            rest = &at_brace['{'.len_utf8()..];
        }
    }
    out.push_str(rest);
    out
}

/// [`render_path`] for a template compiled as a **Rust regex** (`pattern:`,
/// `shebang:`, `for_each_match.select`, `*_path_matches.matches`, etc.). Each
/// substituted token value is escaped with [`regex::escape`] so metacharacters
/// in a repository path (`.`, `(`, `[`, `+`, `?`, `*`, `^`, `$`, `\`) match
/// literally while the user-written template text around the tokens keeps its
/// regex meaning.
pub fn render_path_regex(template: &str, t: &PathTokens) -> String {
    render_path_with(template, t, |v, out| out.push_str(&regex::escape(v)))
}

/// Match a recognized `{token}` at the start of `s`, returning `(token, value)`.
fn match_path_token<'a>(s: &str, t: &'a PathTokens) -> Option<(&'static str, &'a str)> {
    let tokens: [(&str, &str); 6] = [
        ("{parent_name}", &t.parent_name),
        ("{basename}", &t.basename),
        ("{path}", &t.path),
        ("{stem}", &t.stem),
        ("{dir}", &t.dir),
        ("{ext}", &t.ext),
    ];
    tokens.into_iter().find(|(tok, _)| s.starts_with(tok))
}

/// Escape `value` for an RFC 9535 `JSONPath` string literal delimited by `quote`
/// (`'` or `"`).
fn push_jsonpath_escaped(value: &str, quote: char, out: &mut String) {
    use std::fmt::Write as _;
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            c if c == quote => {
                out.push('\\');
                out.push(quote);
            }
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
}

/// True when `s` is a safe ASCII RFC 9535 `member-name-shorthand` (`$.foo_1`).
fn is_jsonpath_member_shorthand(s: &str) -> bool {
    let mut bytes = s.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Consume a dot-member template run starting at `rest` (after `.` or `..`),
/// stopping before the next `JSONPath` delimiter while keeping recognized
/// `{token}` markers intact. Returns `(run, contains_token)`.
fn scan_jsonpath_dot_member<'a>(rest: &'a str, t: &PathTokens) -> (&'a str, bool) {
    let mut pos = 0usize;
    let mut has_token = false;
    while pos < rest.len() {
        let tail = &rest[pos..];
        if let Some((tok, _)) = match_path_token(tail, t) {
            has_token = true;
            pos += tok.len();
            continue;
        }
        let ch = tail.chars().next().expect("pos < len");
        if ch.is_ascii_whitespace()
            || matches!(
                ch,
                '.' | '['
                    | ']'
                    | '('
                    | ')'
                    | '\''
                    | '"'
                    | ','
                    | ';'
                    | '?'
                    | '@'
                    | '!'
                    | '='
                    | '<'
                    | '>'
                    | '&'
                    | '|'
                    | '*'
                    | ':'
            )
        {
            break;
        }
        pos += ch.len_utf8();
    }
    (&rest[..pos], has_token)
}

/// [`render_path`] for a template compiled as an RFC 9535 **`JSONPath`** query
/// (`json_path_*`, `yaml_path_*`, `toml_path_*`, `extract.json`, etc.).
///
/// - Inside `'...'` or `"..."` string literals, substituted values are escaped
///   for that quote delimiter (so quotes, backslashes, and controls cannot
///   break out of the string or alter a filter expression).
/// - In dot-member position (`.{basename}`, `$.pkg_{stem}`, `$..{basename}`),
///   if the rendered member is not a plain identifier, it is bracket-quoted
///   (`['...']`) so dots, hyphens, quotes, brackets, or wildcards in a path
///   match the literal key rather than becoming path operators.
/// - Outside quotes and outside dot-member position, a substituted token is
///   emitted as a single-quoted string literal (`'...'`).
pub fn render_path_jsonpath(template: &str, t: &PathTokens) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    let mut quote: Option<char> = None;
    let mut at_string_start = false;
    while !rest.is_empty() {
        if let Some(q) = quote {
            if rest.starts_with('\\') {
                let mut chars = rest.chars();
                let slash = chars.next().unwrap();
                out.push(slash);
                if let Some(next) = chars.next() {
                    out.push(next);
                    rest = &rest[slash.len_utf8() + next.len_utf8()..];
                } else {
                    rest = &rest[slash.len_utf8()..];
                }
                at_string_start = false;
                continue;
            }
            if rest.starts_with(q) {
                out.push(q);
                rest = &rest[q.len_utf8()..];
                quote = None;
                at_string_start = false;
                continue;
            }
            if let Some((tok, val)) = match_path_token(rest, t) {
                rest = &rest[tok.len()..];
                if val.is_empty() && (at_string_start || out.ends_with('/')) {
                    if let Some(after) = rest.strip_prefix('/') {
                        rest = after;
                    }
                } else {
                    push_jsonpath_escaped(val, q, &mut out);
                    at_string_start = false;
                }
                continue;
            }
            let ch = rest.chars().next().unwrap();
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
            at_string_start = false;
            continue;
        }

        if rest.starts_with('\'') || rest.starts_with('"') {
            let q = rest.chars().next().unwrap();
            out.push(q);
            rest = &rest[q.len_utf8()..];
            quote = Some(q);
            at_string_start = true;
            continue;
        }
        if rest.starts_with('.') {
            let (dots, after_dots) = if let Some(after) = rest.strip_prefix("..") {
                ("..", after)
            } else {
                (".", &rest[1..])
            };
            let (member, has_token) = scan_jsonpath_dot_member(after_dots, t);
            if has_token {
                let rendered = render_path(member, t);
                if is_jsonpath_member_shorthand(&rendered) {
                    out.push_str(dots);
                    out.push_str(&rendered);
                } else {
                    if dots == ".." {
                        out.push_str("..");
                    }
                    out.push_str("['");
                    push_jsonpath_escaped(&rendered, '\'', &mut out);
                    out.push_str("']");
                }
                rest = &after_dots[member.len()..];
                continue;
            }
            out.push_str(dots);
            rest = after_dots;
            continue;
        }
        if let Some((tok, val)) = match_path_token(rest, t) {
            rest = &rest[tok.len()..];
            out.push('\'');
            push_jsonpath_escaped(val, '\'', &mut out);
            out.push('\'');
            continue;
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

/// Escape `value` for a `when` / `when_iter` string literal (supports `\\`,
/// `\"`, `\'`, `\n`, `\r`, `\t`).
fn push_when_escaped(value: &str, out: &mut String) {
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WhenStringMode {
    Literal,
    Glob,
    Regex,
}

fn when_string_mode_before(out: &str) -> WhenStringMode {
    let trimmed = out.trim_end();
    if let Some(before_lparen) = trimmed.strip_suffix('(')
        && let Some(before_method) = before_lparen.trim_end().strip_suffix("has_file")
        && let Some(before_dot) = before_method.trim_end().strip_suffix('.')
        && let Some(before_iter) = before_dot.trim_end().strip_suffix("iter")
        && before_iter
            .bytes()
            .last()
            .is_none_or(|b| !(b.is_ascii_alphanumeric() || b == b'_' || b == b'.'))
    {
        return WhenStringMode::Glob;
    }
    if let Some(before_kw) = trimmed.strip_suffix("matches")
        && before_kw
            .bytes()
            .last()
            .is_none_or(|b| !(b.is_ascii_alphanumeric() || b == b'_'))
    {
        return WhenStringMode::Regex;
    }
    WhenStringMode::Literal
}

/// [`render_path`] for an inner `for_each_*` rule's **`when_iter:`** expression.
///
/// - Inside string literals, substituted values are escaped for `when` string
///   syntax (`\\`, `\"`, `\'`, `\n`, `\r`, `\t`), so a path containing quotes
///   or backslashes cannot break out of the literal and alter the expression.
/// - Inside `iter.has_file("...")` string arguments, values are additionally
///   glob-escaped so glob metacharacters in a path match literally.
/// - After the `matches` regex operator, values are additionally regex-escaped.
/// - Outside string literals, any substituted token is emitted as a safely
///   quoted string literal (`"..."`) so a path cannot inject operators.
pub fn render_path_when_expr(template: &str, t: &PathTokens) -> String {
    let push_val = |mode: WhenStringMode, val: &str, out: &mut String| match mode {
        WhenStringMode::Literal => push_when_escaped(val, out),
        WhenStringMode::Glob => push_when_escaped(&glob_escape(val), out),
        WhenStringMode::Regex => push_when_escaped(&regex::escape(val), out),
    };
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    let mut quote: Option<(char, WhenStringMode)> = None;
    let mut at_string_start = false;
    while !rest.is_empty() {
        if let Some((q, mode)) = quote {
            if rest.starts_with('\\') {
                let mut chars = rest.chars();
                let slash = chars.next().unwrap();
                out.push(slash);
                if let Some(next) = chars.next() {
                    out.push(next);
                    rest = &rest[slash.len_utf8() + next.len_utf8()..];
                } else {
                    rest = &rest[slash.len_utf8()..];
                }
                at_string_start = false;
                continue;
            }
            if rest.starts_with(q) {
                out.push(q);
                rest = &rest[q.len_utf8()..];
                quote = None;
                at_string_start = false;
                continue;
            }
            if let Some((tok, val)) = match_path_token(rest, t) {
                rest = &rest[tok.len()..];
                if val.is_empty() && (at_string_start || out.ends_with('/')) {
                    if let Some(after) = rest.strip_prefix('/') {
                        rest = after;
                    }
                } else {
                    push_val(mode, val, &mut out);
                    at_string_start = false;
                }
                continue;
            }
            let ch = rest.chars().next().unwrap();
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
            at_string_start = false;
            continue;
        }

        if rest.starts_with('\'') || rest.starts_with('"') {
            let mode = when_string_mode_before(&out);
            let q = rest.chars().next().unwrap();
            out.push(q);
            rest = &rest[q.len_utf8()..];
            quote = Some((q, mode));
            at_string_start = true;
            continue;
        }
        if let Some((tok, val)) = match_path_token(rest, t) {
            let mode = when_string_mode_before(&out);
            rest = &rest[tok.len()..];
            out.push('"');
            push_val(mode, val, &mut out);
            out.push('"');
            continue;
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

/// [`render_path`] for a path option that also accepts reserved keywords (such
/// as `registry_paths_resolve.base:` accepting `registry_dir` / `lint_root`).
/// A literal template with no path tokens keeps its keyword meaning; when a
/// template contains path tokens and renders to a reserved keyword, `./` is
/// prefixed so the consumer resolves it as an explicit relative path instead of
/// the keyword.
fn render_path_keyword_path(template: &str, t: &PathTokens, keywords: &[&str]) -> String {
    if TokenSet::of(template) == TokenSet::NONE {
        return template.to_owned();
    }
    let rendered = render_path(template, t);
    if keywords.contains(&rendered.as_str()) {
        format!("./{rendered}")
    } else {
        rendered
    }
}

/// [`render_path`] for a **command argv** element. If substituting a path token
/// turns a non-flag template into a leading-dash string, the matched repo file
/// name is masquerading as an option to the spawned tool — e.g. a file named
/// `--write` rendered from `{path}` would flip `prettier --check {path}` into a
/// destructive `--write`. Prefix `./` so it is unambiguously a path (L13).
///
/// A template element the user *wrote* as a flag (`--check`, `--file={path}`)
/// already starts with `-`, so it is left untouched — only a leading dash
/// *introduced by substitution* is guarded.
#[must_use]
pub fn render_path_argv(template: &str, t: &PathTokens) -> String {
    let rendered = render_path(template, t);
    if rendered.starts_with('-') && !template.starts_with('-') {
        format!("./{rendered}")
    } else {
        rendered
    }
}

/// A path template a rule renders for each entry it checks: `pair`'s
/// `partner`, `unique_by`'s `key`, `cross_file`'s `register_as`, an argument
/// of `command`. In a rule an iteration built
/// ([`RuleSpec::rendered`](crate::RuleSpec::rendered)), the iteration has
/// substituted every token with the iterated entry's value already, so the
/// text is final and renders as it is: a value that spells a token (a
/// directory named `x{stem}`) is text, not a token to substitute again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathTemplate {
    text: String,
    /// Whether an iteration rendered the text (see the type's documentation).
    rendered: bool,
}

impl PathTemplate {
    /// `text` as the rule `spec` configures it: final when an iteration
    /// rendered `spec`.
    #[must_use]
    pub fn new(text: String, spec: &crate::RuleSpec) -> Self {
        Self {
            text,
            rendered: spec.rendered,
        }
    }

    /// The template as the rule holds it (for a rule an iteration built, with
    /// the iteration's values in place of its tokens).
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The tokens [`Self::render`] reads, for
    /// [`PathTokens::from_path_with`]: none when the text is final.
    #[must_use]
    pub fn tokens(&self) -> TokenSet {
        if self.rendered {
            TokenSet::NONE
        } else {
            TokenSet::of(&self.text)
        }
    }

    /// The template rendered for an entry with `tokens` ([`render_path`]), or
    /// the final text as it is.
    #[must_use]
    pub fn render(&self, tokens: &PathTokens) -> std::borrow::Cow<'_, str> {
        if self.rendered {
            std::borrow::Cow::Borrowed(&self.text)
        } else {
            std::borrow::Cow::Owned(render_path(&self.text, tokens))
        }
    }

    /// [`Self::render`] for a command argument ([`render_path_argv`]). A final
    /// text was rendered as an argument by the iteration, which guarded a
    /// leading dash its value introduced.
    #[must_use]
    pub fn render_argv(&self, tokens: &PathTokens) -> std::borrow::Cow<'_, str> {
        if self.rendered {
            std::borrow::Cow::Borrowed(&self.text)
        } else {
            std::borrow::Cow::Owned(render_path_argv(&self.text, tokens))
        }
    }
}

/// A template no iteration rendered: a rule's own default (`cross_file`'s
/// `{path}`), or one a test builds.
impl From<&str> for PathTemplate {
    fn from(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            rendered: false,
        }
    }
}

impl std::fmt::Display for PathTemplate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

/// Apply path-template substitution to every string inside a YAML mapping,
/// recursively into nested mappings and sequences. Non-string values pass
/// through unchanged. Used by nested-rule specs (e.g. `for_each_dir`) so that
/// the `{dir}` in a nested rule's `paths`, `pattern`, or `partner` field
/// resolves to the iterated entry's path at rule-build time.
pub(crate) fn render_mapping(m: ayaml::Mapping, tokens: &PathTokens) -> ayaml::Mapping {
    let mut out = ayaml::Mapping::with_capacity(m.len());
    for (k, v) in m {
        out.insert(k, render_value(v, tokens));
    }
    out
}

/// Recursive mate to [`render_mapping`] for arbitrary YAML values.
pub(crate) fn render_value(v: ayaml::Value, tokens: &PathTokens) -> ayaml::Value {
    use ayaml::Value;
    match v {
        Value::String(s) => Value::String(render_path(&s, tokens)),
        Value::Sequence(seq) => {
            Value::Sequence(seq.into_iter().map(|e| render_value(e, tokens)).collect())
        }
        Value::Mapping(m) => Value::Mapping(render_mapping(m, tokens)),
        other => other,
    }
}

/// Apply path-template substitution to every pattern in a `PathsSpec`.
/// `paths:` entries are globs, so token values are glob-escaped
/// ([`render_path_glob`]). A flat-list `!exclude` entry keeps its leading
/// `!` (it is template text, not a substituted value): the `!` is peeled off,
/// the remainder rendered, and the `!` restored.
pub fn render_paths_spec(spec: &PathsSpec, tokens: &PathTokens) -> PathsSpec {
    let flat = |s: &String| match s.strip_prefix('!') {
        Some(rest) => format!("!{}", render_path_glob(rest, tokens)),
        None => render_path_glob(s, tokens),
    };
    match spec {
        PathsSpec::Single(s) => PathsSpec::Single(flat(s)),
        PathsSpec::Many(v) => PathsSpec::Many(v.iter().map(flat).collect()),
        PathsSpec::IncludeExclude { include, exclude } => PathsSpec::IncludeExclude {
            include: include
                .iter()
                .map(|s| render_path_glob(s, tokens))
                .collect(),
            exclude: exclude
                .iter()
                .map(|s| render_path_glob(s, tokens))
                .collect(),
        },
    }
}

/// Written on both sides of each `{` of a value that an iteration substitutes
/// into the message of a nested rule that renders its message again
/// ([`NESTED_MESSAGE_TEMPLATE_KINDS`]), so no `{{` from a name reaches that
/// rendering: a private-use character, which a path has no reason to hold.
/// [`unescape_token_values`] takes the marks out of the finding's message.
const MESSAGE_BRACE_MARK: char = '\u{E001}';

/// Kinds whose rule renders its `message:` for each finding as a message
/// template (`{{ctx.primary}}`, `{{ctx.key}}`). In a nested rule of one of
/// them, each `{` of a substituted value is marked with a private-use
/// character, so a directory named `x{{ctx.primary}}` is shown as it is
/// named, not with the primary path in its place.
///
/// For alint's own crates, whose tests keep it in step with the rules. A
/// `constraint` rule (`{{row.FIELD}}`) cannot be nested, and is listed as a
/// kind that renders its message all the same.
#[doc(hidden)]
pub const NESTED_MESSAGE_TEMPLATE_KINDS: &[&str] = &[
    "constraint",
    "dir_contains",
    "dir_only_contains",
    "git_blame_age",
    "key_parity",
    "pair",
    "unique_by",
];

/// Append `v` with [`MESSAGE_BRACE_MARK`] on both sides of each `{`.
fn push_message_value(v: &str, out: &mut String) {
    if !v.contains('{') {
        out.push_str(v);
        return;
    }
    for ch in v.chars() {
        if ch == '{' {
            out.push(MESSAGE_BRACE_MARK);
            out.push('{');
            out.push(MESSAGE_BRACE_MARK);
        } else {
            out.push(ch);
        }
    }
}

/// Render a nested rule's `message:` for one iteration: with
/// [`render_path`], except that in a kind that renders its message again
/// ([`NESTED_MESSAGE_TEMPLATE_KINDS`]) each `{` of a value is marked.
pub(crate) fn render_nested_message(kind: &str, template: &str, tokens: &PathTokens) -> String {
    if NESTED_MESSAGE_TEMPLATE_KINDS.contains(&kind) {
        render_path_with(template, tokens, push_message_value)
    } else {
        render_path(template, tokens)
    }
}

/// Undo [`render_path_glob`]'s escaping of `tokens`' values inside a
/// user-facing `message`: a nested rule reports the glob it was built from
/// (`expected a file matching app/[[]slug[]]/*.tsx`), and the escape forms are
/// noise to the reader, who named the directory `app/[slug]`. The marks the
/// values carry in the message of a kind in [`NESTED_MESSAGE_TEMPLATE_KINDS`]
/// are taken out the same way. Only the exact escaped spellings of this
/// iteration's token values are replaced, so a user-written pattern is shown
/// as written.
#[must_use]
pub fn unescape_token_values(message: &str, tokens: &PathTokens) -> Option<String> {
    let mut pairs: Vec<(String, &str)> = Vec::new();
    for v in [
        &tokens.path,
        &tokens.dir,
        &tokens.basename,
        &tokens.stem,
        &tokens.parent_name,
        &tokens.ext,
    ] {
        let escaped = glob_escape(v);
        if escaped != *v {
            pairs.push((escaped, v));
        }
        if let Some(rest) = v.strip_prefix('!') {
            pairs.push((format!("{{!}}{}", glob_escape(rest)), v));
        }
        if v.contains('{') {
            let mut marked = String::with_capacity(v.len() + 8);
            push_message_value(v, &mut marked);
            pairs.push((marked, v));
        }
    }
    if pairs.is_empty() {
        return None;
    }
    // Longest first: `{path}`'s spelling contains `{dir}`'s and `{basename}`'s.
    pairs.sort_by_key(|(e, _)| std::cmp::Reverse(e.len()));
    let mut out = message.to_string();
    for (escaped, raw) in pairs {
        if out.contains(&escaped) {
            out = out.replace(&escaped, raw);
        }
    }
    (out != message).then_some(out)
}

/// Option keys of a nested rule that hold a **list of nested rule specs**
/// (each rendered recursively with [`render_nested_options`] for its own kind).
const NESTED_SPEC_LISTS: &[(&[&str], &str)] = &[(
    &["for_each_dir", "for_each_file", "every_matching_has"],
    "require",
)];

/// Defines [`NESTED_GLOB_OPTIONS`] and [`nested_glob_options`] from one list.
/// The lookup runs once per nested rule per iterated entry, so it is a `match`
/// rather than a scan of the table: once the compiler stopped unrolling the
/// scan, it cost ~300 instructions a call and `for_each_dir` 3% (v0.21.0).
macro_rules! nested_glob_options_table {
    ($([$($kind:literal),+ $(,)?] => [$($key:literal),* $(,)?]),* $(,)?) => {
        /// Option keys (beyond `paths:`, which is rendered by [`render_paths_spec`])
        /// whose values are compiled as **globs**, per rule kind (aliases included).
        /// A dotted key is a path into nested mappings (sequence elements are walked
        /// through), e.g. `targets.files`. Other syntax-bearing options (regexes,
        /// `JSONPath` queries, `when_iter:` / nested `when:` expressions,
        /// keyword-guarded paths, command arguments, and replacement templates)
        /// are escaped for their own syntax when nested rules are rendered; plain
        /// literal paths and text options (`pair.partner`, `pair_hash.target`,
        /// `cross_file`'s `file`, messages) are rendered literally so glob-escaping
        /// does not corrupt them.
        pub const NESTED_GLOB_OPTIONS: &[(&[&str], &[&str])] = &[$((&[$($kind),+], &[$($key),*])),*];

        /// The glob-typed option keys of `kind` (see [`NESTED_GLOB_OPTIONS`]).
        #[must_use]
        pub fn nested_glob_options(kind: &str) -> &'static [&'static str] {
            match kind {
                $($($kind)|+ => &[$($key),*],)*
                _ => &[],
            }
        }
    };
}

nested_glob_options_table! {
    ["dir_contains"] => ["select", "require"],
    ["dir_only_contains"] => ["select", "allow"],
    ["unique_by"] => ["select"],
    ["key_parity"] => ["peers"],
    ["for_each_dir", "for_each_file", "every_matching_has"] => ["select"],
    ["pair"] => ["primary"],
    ["pair_hash"] => ["source"],
    ["pair_changed_together"] => ["if_changed", "then_changed"],
    ["changeset_requires_path"] => ["add_glob", "when_changed"],
    ["generated_file_fresh"] => ["outputs"],
    ["git_no_denied_paths"] => ["denied"],
    ["import_gate"] => ["allow"],
    ["file_graph"] => [
        "nodes",
        "require.no_orphans.roots",
        "require.forbidden_edges.from",
        "require.forbidden_edges.to",
    ],
    ["cross_file", "cross_file_value_equals"] => ["source.files", "targets.files"],
    ["registry_paths_resolve"] => ["source", "orphans.space"],
}

/// Option keys of a nested rule that hold a **Rust regex**, per kind (aliases
/// included). Each substituted token value is escaped with [`regex::escape`]
/// ([`render_path_regex`]) so `.`, `(`, `[`, `+`, `?`, `*`, `^`, `$`, `\` in a
/// path match literally.
fn nested_regex_options(kind: &str) -> &'static [&'static str] {
    match kind {
        "file_content_matches"
        | "content_matches"
        | "file_content_forbidden"
        | "content_forbidden"
        | "file_header"
        | "header"
        | "file_footer"
        | "footer"
        | "filename_regex"
        | "git_commit_message"
        | "git_commit_signed_off"
        | "git_blame_age" => &["pattern"],
        "file_shebang" | "shebang" => &["shebang"],
        "git_commit_subject_matches"
        | "json_path_matches"
        | "yaml_path_matches"
        | "toml_path_matches"
        | "xml_path_matches"
        | "dotenv_path_matches"
        | "properties_path_matches"
        | "ini_path_matches"
        | "hcl_path_matches" => &["matches"],
        "git_commit_author_allowlist" => &["email_pattern", "name_pattern"],
        "command_idempotent" => &["files_pattern"],
        "import_gate" => &["import_pattern", "forbid"],
        "ordered_block" => &["select"],
        "for_each_match" => &["select", "require.matches", "require.forbid"],
        "cross_file" | "cross_file_value_equals" => &[
            "source.extract.regex",
            "targets.extract.regex",
            "targets.matches",
        ],
        "file_graph" => &[
            "edges.from_content.extract.regex",
            "edges.derive_target.from",
            "require.fresh.marker",
        ],
        "registry_paths_resolve" => &["extract.regex"],
        _ => &[],
    }
}

/// Option keys of a nested rule that hold an RFC 9535 **`JSONPath`** query, per
/// kind (aliases included). Rendered with [`render_path_jsonpath`].
fn nested_jsonpath_options(kind: &str) -> &'static [&'static str] {
    match kind {
        "json_path_equals"
        | "json_path_matches"
        | "json_path_absent"
        | "yaml_path_equals"
        | "yaml_path_matches"
        | "yaml_path_absent"
        | "toml_path_equals"
        | "toml_path_matches"
        | "toml_path_absent"
        | "xml_path_equals"
        | "xml_path_matches"
        | "xml_path_absent"
        | "dotenv_path_equals"
        | "dotenv_path_matches"
        | "dotenv_path_absent"
        | "properties_path_equals"
        | "properties_path_matches"
        | "properties_path_absent"
        | "ini_path_equals"
        | "ini_path_matches"
        | "ini_path_absent"
        | "hcl_path_equals"
        | "hcl_path_matches"
        | "hcl_path_absent" => &["path"],
        "cross_file" | "cross_file_value_equals" => &[
            "source.extract.json",
            "source.extract.yaml",
            "source.extract.toml",
            "source.extract.xml",
            "source.extract.dotenv",
            "source.extract.properties",
            "source.extract.ini",
            "source.extract.hcl",
            "targets.extract.json",
            "targets.extract.yaml",
            "targets.extract.toml",
            "targets.extract.xml",
            "targets.extract.dotenv",
            "targets.extract.properties",
            "targets.extract.ini",
            "targets.extract.hcl",
        ],
        "file_graph" => &[
            "edges.from_content.extract.json",
            "edges.from_content.extract.yaml",
            "edges.from_content.extract.toml",
            "edges.from_content.extract.xml",
            "edges.from_content.extract.dotenv",
            "edges.from_content.extract.properties",
            "edges.from_content.extract.ini",
            "edges.from_content.extract.hcl",
        ],
        "registry_paths_resolve" => &[
            "extract.json",
            "extract.yaml",
            "extract.toml",
            "extract.xml",
            "extract.dotenv",
            "extract.properties",
            "extract.ini",
            "extract.hcl",
            "exclude_query",
        ],
        _ => &[],
    }
}

/// Option keys of a nested rule that hold a `when_iter:` expression, per kind.
/// Rendered with [`render_path_when_expr`].
fn nested_when_options(kind: &str) -> &'static [&'static str] {
    match kind {
        "for_each_dir" | "for_each_file" | "every_matching_has" => &["when_iter"],
        _ => &[],
    }
}

/// Option keys of a nested rule that hold a path OR one of a fixed set of
/// reserved keywords (such as `registry_paths_resolve.base:`).
fn nested_keyword_path_options(kind: &str) -> &'static [(&'static str, &'static [&'static str])] {
    match kind {
        "registry_paths_resolve" => &[("base", &["registry_dir", "lint_root"])],
        _ => &[],
    }
}

/// Option keys of a nested rule that hold a command's arguments, per kind.
/// Each argument is rendered with [`render_path_argv`], so a value that
/// starts with `-` (a file named `--write`) reaches the command as a path
/// (`./--write`), not an option, as in a rule no iteration built.
fn nested_argv_options(kind: &str) -> &'static [&'static str] {
    match kind {
        "command" | "command_idempotent" | "generated_file_fresh" => &["command"],
        _ => &[],
    }
}

/// Option keys of a nested rule that hold a regex replacement template, which
/// the rule expands with each match's captures (`$1`, `${name}`), per kind. A
/// `$` in a substituted value is written `$$`, so a directory named `a$1`
/// reaches the expansion as text, not a capture reference.
fn nested_replacement_options(kind: &str) -> &'static [&'static str] {
    match kind {
        "file_graph" => &["edges.derive_target.to"],
        _ => &[],
    }
}

/// The option keys of a nested rule's kind that are not rendered as literal
/// text: globs, regexes, `JSONPath` queries, `when_iter` expressions,
/// keyword-guarded paths, command arguments, and replacement templates.
#[derive(Clone, Copy)]
struct OptionSyntax {
    globs: &'static [&'static str],
    regexes: &'static [&'static str],
    jsonpaths: &'static [&'static str],
    when_exprs: &'static [&'static str],
    keyword_paths: &'static [(&'static str, &'static [&'static str])],
    argv: &'static [&'static str],
    replacements: &'static [&'static str],
}

impl OptionSyntax {
    fn of(kind: &str) -> Self {
        Self {
            globs: nested_glob_options(kind),
            regexes: nested_regex_options(kind),
            jsonpaths: nested_jsonpath_options(kind),
            when_exprs: nested_when_options(kind),
            keyword_paths: nested_keyword_path_options(kind),
            argv: nested_argv_options(kind),
            replacements: nested_replacement_options(kind),
        }
    }
}

/// Render a nested rule's option mapping (`extra`) for one iteration. Like
/// [`render_mapping`], every string gets the iterated entry's tokens, but each
/// syntax-bearing option of `kind` is escaped for the syntax it holds:
/// glob-typed options ([`NESTED_GLOB_OPTIONS`]) with [`render_path_glob`],
/// regex options with [`render_path_regex`], `JSONPath` queries with
/// [`render_path_jsonpath`], `when_iter:` expressions with
/// [`render_path_when_expr`], keyword-bearing path options (such as
/// `registry_paths_resolve.base:`) with `./` disambiguation, command arguments
/// with [`render_path_argv`], replacement templates with each `$` of a value
/// doubled, and nested rule-spec lists (`for_each_dir.require`, ...) rendered
/// recursively per element for that element's own kind.
#[must_use]
pub(crate) fn render_nested_options(
    kind: &str,
    m: ayaml::Mapping,
    tokens: &PathTokens,
) -> ayaml::Mapping {
    render_options(kind, m, tokens, false)
}

/// [`render_nested_options`] for a nested spec's `extra`, without the string
/// keys in `parent_fields`, which the nested spec takes from its parent. With
/// no `tokens` (an enclosing iteration rendered the spec) the values are kept
/// as they are.
pub(crate) fn render_nested_extra(
    kind: &str,
    extra: &crate::yaml::Mapping,
    tokens: Option<&PathTokens>,
    parent_fields: &[&str],
) -> crate::yaml::Mapping {
    let rendered = match tokens {
        Some(tokens) => render_nested_options(kind, extra.parser().clone(), tokens),
        None => extra.parser().clone(),
    };
    crate::yaml::Mapping::from_parser(
        rendered
            .into_iter()
            .filter(|(k, _)| k.as_str().is_none_or(|s| !parent_fields.contains(&s)))
            .collect(),
    )
}

/// [`render_nested_options`]; `with_paths` also renders the mapping's own
/// `paths:` (a nested spec inside a nested spec list) as globs, and its
/// `message:` as [`render_nested_message`] does.
fn render_options(
    kind: &str,
    m: ayaml::Mapping,
    tokens: &PathTokens,
    with_paths: bool,
) -> ayaml::Mapping {
    const PATHS: &[&str] = &["paths", "paths.include", "paths.exclude"];
    let syntax = OptionSyntax::of(kind);
    let spec_list = NESTED_SPEC_LISTS
        .iter()
        .find(|(kinds, _)| kinds.contains(&kind))
        .map(|(_, key)| *key);
    let mut out = ayaml::Mapping::with_capacity(m.len());
    for (k, v) in m {
        let key = k.as_str().unwrap_or_default().to_string();
        let v = if spec_list == Some(key.as_str()) {
            render_nested_spec_list(v, tokens)
        } else if with_paths
            && key == "message"
            && let ayaml::Value::String(message) = &v
        {
            ayaml::Value::String(render_nested_message(kind, message, tokens))
        } else if with_paths
            && key == "when"
            && let ayaml::Value::String(when) = &v
        {
            ayaml::Value::String(render_path_when_expr(when, tokens))
        } else if with_paths && key == "paths" {
            let paths = OptionSyntax {
                globs: PATHS,
                ..syntax
            };
            render_keyed(v, &key, paths, tokens)
        } else {
            render_keyed(v, &key, syntax, tokens)
        };
        out.insert(k, v);
    }
    out
}

/// Render `v`, found at dotted key path `at`, as `syntax` says the option at
/// `at` is read: glob-escaping, regex-escaping, `JSONPath`-escaping,
/// `when_iter`-escaping, keyword-path disambiguation, command argument,
/// replacement template, or literally.
fn render_keyed(
    v: ayaml::Value,
    at: &str,
    syntax: OptionSyntax,
    tokens: &PathTokens,
) -> ayaml::Value {
    use ayaml::Value;
    match v {
        Value::String(s) if syntax.globs.contains(&at) => {
            Value::String(match s.strip_prefix('!') {
                Some(rest) => format!("!{}", render_path_glob(rest, tokens)),
                None => render_path_glob(&s, tokens),
            })
        }
        Value::String(s) if syntax.regexes.contains(&at) => {
            Value::String(render_path_regex(&s, tokens))
        }
        Value::String(s) if syntax.jsonpaths.contains(&at) => {
            Value::String(render_path_jsonpath(&s, tokens))
        }
        Value::String(s) if syntax.when_exprs.contains(&at) => {
            Value::String(render_path_when_expr(&s, tokens))
        }
        Value::String(s)
            if let Some((_, keywords)) = syntax.keyword_paths.iter().find(|(k, _)| *k == at) =>
        {
            Value::String(render_path_keyword_path(&s, tokens, keywords))
        }
        Value::String(s) if syntax.argv.contains(&at) => {
            Value::String(render_path_argv(&s, tokens))
        }
        Value::String(s) if syntax.replacements.contains(&at) => {
            Value::String(render_path_with(&s, tokens, |v, out| {
                out.push_str(&v.replace('$', "$$"));
            }))
        }
        Value::String(s) => Value::String(render_path(&s, tokens)),
        Value::Sequence(seq) => Value::Sequence(
            seq.into_iter()
                .map(|e| render_keyed(e, at, syntax, tokens))
                .collect(),
        ),
        Value::Mapping(m) => Value::Mapping(
            m.into_iter()
                .map(|(k, v)| {
                    let sub = match k.as_str() {
                        Some(name) => format!("{at}.{name}"),
                        None => at.to_string(),
                    };
                    (k, render_keyed(v, &sub, syntax, tokens))
                })
                .collect(),
        ),
        other => other,
    }
}

/// Render a list of nested rule specs, each for its own `kind` (its `paths:`
/// as globs too).
fn render_nested_spec_list(v: ayaml::Value, tokens: &PathTokens) -> ayaml::Value {
    use ayaml::Value;
    let Value::Sequence(seq) = v else {
        return render_value(v, tokens);
    };
    Value::Sequence(
        seq.into_iter()
            .map(|item| match item {
                Value::Mapping(spec) => {
                    let kind = spec
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    Value::Mapping(render_options(&kind, spec, tokens, true))
                }
                other => render_value(other, tokens),
            })
            .collect(),
    )
}

/// Substitute `{{namespace.key}}` placeholders in a message template. The
/// caller-supplied `resolve` closure returns the substituted value, or
/// `None` to leave the placeholder literal.
///
/// Whitespace inside the braces (`{{ ctx.primary }}`) is ignored so users
/// can format their messages for readability.
pub fn render_message<F>(template: &str, resolve: F) -> String
where
    F: Fn(&str, &str) -> Option<String>,
{
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            // Unterminated {{ — preserve rest literally.
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = after[..end].trim();
        let rendered = inner
            .split_once('.')
            .and_then(|(ns, key)| resolve(ns.trim(), key.trim()));
        if let Some(val) = rendered {
            out.push_str(&val);
        } else {
            out.push_str("{{");
            out.push_str(&after[..end]);
            out.push_str("}}");
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The path tokens' names, as a template spells them between braces.
    const TOKEN_NAMES: [&str; 6] = ["parent_name", "basename", "path", "stem", "dir", "ext"];

    /// The preparation build's tokens: none renders empty or like another, each
    /// is as wide as its token, and inside a preparation each value, alone or
    /// inside a path, leads back to the template that rendered it (#504).
    #[test]
    fn placeholder_tokens_are_distinct_non_empty_and_traceable() {
        let tokens = PathTokens::placeholder();
        // Exhaustive, so a new token field fails to compile here until
        // `TOKEN_NAMES` lists it.
        let PathTokens {
            path,
            dir,
            basename,
            stem,
            ext,
            parent_name,
        } = tokens.clone();
        let mut fields = [path, dir, basename, stem, ext, parent_name];
        let mut rendered: Vec<String> = TOKEN_NAMES
            .iter()
            .map(|name| render_path(&format!("{{{name}}}"), &tokens))
            .collect();
        fields.sort();
        rendered.sort();
        assert_eq!(fields.to_vec(), rendered, "every field is a known token");

        for name in TOKEN_NAMES {
            let template = format!("{{{name}}}");
            let in_path = format!("a/{template}.json");
            let glob = format!("{template}/*.json");
            let _preparing = Preparation::enter([template.as_str(), &in_path, &glob]);
            let value = render_path(&template, &tokens);
            assert_eq!(value, format!("_{name}_"));
            assert_eq!(value.len(), template.len(), "{name}");
            assert_ne!(TokenSet::of(&template), TokenSet::NONE, "{name}");
            assert_eq!(templates_rendering(&value), std::slice::from_ref(&template));
            assert_eq!(unrender_placeholders(&value, [template.as_str()]), template);
            assert_eq!(
                templates_rendering(&render_path(&in_path, &tokens)),
                std::slice::from_ref(&in_path)
            );
            assert_eq!(
                templates_rendering(&render_path_glob(&glob, &tokens)),
                std::slice::from_ref(&glob)
            );
        }
        let mut unique = rendered.clone();
        unique.dedup();
        assert_eq!(unique.len(), TOKEN_NAMES.len(), "{rendered:?}");
        // A lone `{dir}` keeps its value rather than collapsing as an empty one does.
        assert_eq!(render_path("{dir}/en.json", &tokens), "_dir_/en.json");
    }

    /// A value leads back to the templates of the outermost open preparation
    /// that render it, and to nothing outside one: a file called `_stem_.json`,
    /// or a top-level rule that wrote that name, has no template.
    #[test]
    fn templates_are_found_only_while_preparing() {
        const NONE: [&str; 0] = [];
        assert_eq!(templates_rendering("_stem_.json"), NONE);
        {
            let _outer = Preparation::enter(["{stem}.json", "_dir_/x", "{dir}/x", "plain"]);
            assert_eq!(templates_rendering("_stem_.json"), ["{stem}.json"]);
            // Text spelled like a placeholder is found as written, beside
            // a template with the token that renders the same.
            assert_eq!(templates_rendering("_dir_/x"), ["_dir_/x", "{dir}/x"]);
            assert_eq!(templates_rendering("plain"), ["plain"]);
            assert_eq!(templates_rendering("_stem_"), NONE, "no such template");
            {
                // An inner rule's templates are the outer's renderings: only
                // the outermost preparation's count.
                let _inner = Preparation::enter(["_stem_.json", "{basename}"]);
                assert_eq!(templates_rendering("_stem_.json"), ["{stem}.json"]);
                assert_eq!(templates_rendering("_basename_"), NONE);
            }
            assert_eq!(
                templates_rendering("_stem_.json"),
                ["{stem}.json"],
                "the outer one is still open"
            );
            std::thread::spawn(|| assert_eq!(templates_rendering("_stem_.json"), NONE))
                .join()
                .unwrap();
        }
        assert_eq!(templates_rendering("_stem_.json"), NONE, "closed on drop");
        let _next = Preparation::enter(["{ext}"]);
        assert_eq!(
            templates_rendering("_ext_"),
            ["{ext}"],
            "a new outermost one"
        );
    }

    /// Error messages show the template the configuration wrote, and only
    /// where a value rendered from one of the spec's templates stands.
    #[test]
    fn unrendering_restores_the_tokens() {
        assert_eq!(
            unrender_placeholders("invalid glob \"_dir_/[_stem_\"", ["{dir}/[{stem}"]),
            "invalid glob \"{dir}/[{stem}\""
        );
        // `{:?}` escapes a value's backslash; the template's is escaped alike.
        assert_eq!(
            unrender_placeholders(r#"pattern "\\d_stem_(""#, [r"\d{stem}("]),
            r#"pattern "\\d{stem}(""#
        );
        assert_eq!(
            unrender_placeholders(
                "_parent_name__basename_x_path_y_",
                ["{parent_name}{basename}x{path}y_"]
            ),
            "{parent_name}{basename}x{path}y_"
        );
        // A template whose value is not in the text, or that has no token,
        // changes nothing.
        assert_eq!(
            unrender_placeholders("a_b _ __", ["{stem}", "plain"]),
            "a_b _ __"
        );
        // A quote in the template: the message's `{:?}` escaping turns it
        // into `\"`, and only the escaped form of the value is in the text.
        assert_eq!(
            unrender_placeholders(r#"pattern a\"_stem_[: bad"#, [r#"a"{stem}["#]),
            r#"pattern a\"{stem}[: bad"#
        );
        assert_eq!(unrender_placeholders("", ["{stem}"]), "");
        assert_eq!(unrender_placeholders("_stem_", [] as [&str; 0]), "_stem_");
    }

    /// A kind, a rule id or a literal name that contains a placeholder's
    /// spelling is not a rendered value, even when a template renders that
    /// spelling on its own.
    #[test]
    fn unrendering_leaves_lookalike_text_alone() {
        let message = "nested rule #0 (`json_path_equals`): rule \"pkg_dir_layout.require[0]\": \
                       invalid JSONPath \"$[[\" in _path_/src_dir_x and \"_path_\" (a_dir_/b, c/_dir_d)";
        assert_eq!(
            unrender_placeholders(message, ["{path}", "{dir}", "$[["]),
            "nested rule #0 (`json_path_equals`): rule \"pkg_dir_layout.require[0]\": \
             invalid JSONPath \"$[[\" in {path}/src_dir_x and \"{path}\" (a_dir_/b, c/_dir_d)"
        );
    }

    #[test]
    fn kinds_outside_the_table_have_no_nested_glob_options() {
        // alint-rules' glob_selector_metadata_matches_the_transition_table pins
        // the lookup for every registered spelling, aliases included.
        assert_eq!(nested_glob_options("file_exists"), [] as [&str; 0]);
        assert_eq!(nested_glob_options(""), [] as [&str; 0]);
    }

    /// The pre-fast-path algorithms, kept as the reference the fast paths must
    /// match exactly.
    fn glob_escape_reference(s: &str) -> String {
        let escaped = globset::escape(s);
        if cfg!(windows) {
            escaped
        } else {
            escaped.replace('\\', "\\\\")
        }
    }

    fn literal_glob_path_reference(pattern: &str) -> Option<String> {
        if pattern.starts_with('!') {
            return None;
        }
        let mut out = String::with_capacity(pattern.len());
        let mut chars = pattern.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' if !cfg!(windows) => out.push(chars.next()?),
                '[' => {
                    let x = chars.next()?;
                    if matches!(x, '!' | '^') || chars.next()? != ']' {
                        return None;
                    }
                    out.push(x);
                }
                '*' | '?' | ']' | '{' | '}' => return None,
                c => out.push(c),
            }
        }
        Some(out)
    }

    /// Every string up to length 3 over an alphabet that covers each glob
    /// metacharacter, the escape character, `!` and a non-ASCII char.
    fn exhaustive_short_strings() -> Vec<String> {
        const ALPHABET: [char; 13] = [
            'a', '/', '.', '*', '?', '[', ']', '{', '}', '\\', '!', '\u{e9}', ' ',
        ];
        let mut out = vec![String::new()];
        for len in 1..=3 {
            let mut idx = vec![0usize; len];
            loop {
                out.push(idx.iter().map(|&i| ALPHABET[i]).collect());
                let mut pos = 0;
                loop {
                    idx[pos] += 1;
                    if idx[pos] < ALPHABET.len() {
                        break;
                    }
                    idx[pos] = 0;
                    pos += 1;
                    if pos == len {
                        break;
                    }
                }
                if pos == len {
                    break;
                }
            }
        }
        out
    }

    #[test]
    fn partial_tokens_render_like_full_tokens() {
        let templates = [
            "{dir}/{stem}.h",
            "{path}/README.md",
            "{parent_name}-{basename}",
            "{ext}",
            "include/{stem}{ext}.bak",
            "no tokens here",
            "{unknown}/{stem}",
            "{dir}",
        ];
        for path in [
            "src/mod/foo.c",
            "top.c",
            "a/b",
            "Makefile",
            "x/.hidden",
            "",
            "app/[slug]",
        ] {
            let full = PathTokens::from_path(Path::new(path));
            for template in templates {
                let partial = PathTokens::from_path_with(Path::new(path), TokenSet::of(template));
                assert_eq!(
                    render_path(template, &partial),
                    render_path(template, &full)
                );
                let message = format!("expected {}", render_path_glob(template, &partial));
                assert_eq!(
                    unescape_token_values(&message, &partial),
                    unescape_token_values(&message, &full),
                    "{path:?} {template:?}"
                );
                assert_eq!(
                    render_path_glob(template, &partial),
                    render_path_glob(template, &full)
                );
            }
        }
        assert_eq!(TokenSet::of("plain"), TokenSet::NONE);
        assert_eq!(
            TokenSet::of("{dir}").union(TokenSet::of("{stem}")),
            TokenSet::of("{dir}/{stem}")
        );
    }

    #[test]
    fn glob_escape_fast_path_matches_the_reference() {
        for s in exhaustive_short_strings() {
            assert_eq!(glob_escape(&s), glob_escape_reference(&s), "{s:?}");
            let mut pushed = String::from("pre/");
            push_glob_escaped(&mut pushed, &s);
            assert_eq!(
                pushed,
                format!("pre/{}", glob_escape_reference(&s)),
                "{s:?}"
            );
        }
    }

    #[test]
    fn literal_glob_path_fast_path_matches_the_reference() {
        for s in exhaustive_short_strings() {
            assert_eq!(
                literal_glob_path(&s),
                literal_glob_path_reference(&s),
                "{s:?}"
            );
            let escaped = glob_escape(&s);
            assert_eq!(
                literal_glob_path(&escaped),
                literal_glob_path_reference(&escaped),
                "escaped {s:?}"
            );
        }
    }

    #[test]
    fn literal_glob_path_resolves_escapes_and_rejects_real_globs() {
        // The inverse of glob_escape: an escaped metacharacter is literal.
        for name in ["[slug]", "a*b", "{x}", "q?"] {
            assert_eq!(
                literal_glob_path(&format!("app/{}/p.tsx", glob_escape(name))).as_deref(),
                Some(format!("app/{name}/p.tsx").as_str())
            );
        }
        assert_eq!(literal_glob_path("a/b.txt").as_deref(), Some("a/b.txt"));
        for glob in ["a/*.rs", "a/[ab].rs", "a/{x,y}", "!a.rs", "a/?", "a]"] {
            assert_eq!(literal_glob_path(glob), None, "{glob}");
        }
        #[cfg(not(windows))]
        {
            // `\` escapes on non-Windows: a dir literally named `a\b` renders as
            // `a\\b`, which must resolve back to `a\b` (a raw scan missed it).
            assert_eq!(
                literal_glob_path(&glob_escape("a\\b")).as_deref(),
                Some("a\\b")
            );
            assert_eq!(literal_glob_path("a\\*b").as_deref(), Some("a*b"));
        }
    }
    use std::path::Path;

    #[test]
    fn path_tokens_basic_rs_file() {
        let t = PathTokens::from_path(Path::new("crates/alint-core/src/lib.rs"));
        assert_eq!(t.path, "crates/alint-core/src/lib.rs");
        assert_eq!(t.dir, "crates/alint-core/src");
        assert_eq!(t.basename, "lib.rs");
        assert_eq!(t.stem, "lib");
        assert_eq!(t.ext, "rs");
        assert_eq!(t.parent_name, "src");
    }

    #[test]
    fn path_tokens_root_file() {
        let t = PathTokens::from_path(Path::new("README.md"));
        assert_eq!(t.path, "README.md");
        assert_eq!(t.dir, "");
        assert_eq!(t.basename, "README.md");
        assert_eq!(t.stem, "README");
        assert_eq!(t.ext, "md");
        assert_eq!(t.parent_name, "");
    }

    #[test]
    fn render_path_c_to_h() {
        let t = PathTokens::from_path(Path::new("src/mod/foo.c"));
        assert_eq!(render_path("{dir}/{stem}.h", &t), "src/mod/foo.h");
    }

    #[test]
    fn render_path_root_level_dir_collapses_its_separator() {
        // Audit 2026-10 finding 3: a root-level file has `{dir}` = "", so the
        // documented `{dir}/{stem}.h` used to render `/top.h` (never a
        // repo-relative path).
        let t = PathTokens::from_path(Path::new("top.c"));
        assert_eq!(render_path("{dir}/{stem}.h", &t), "top.h");
        assert_eq!(render_path("{dir}/include/{stem}.h", &t), "include/top.h");
        assert_eq!(render_path_glob("{dir}/*.h", &t), "*.h");
        // A non-empty dir and a mid-template empty token are unchanged.
        let nested = PathTokens::from_path(Path::new("src/foo.c"));
        assert_eq!(render_path("{dir}/{stem}.h", &nested), "src/foo.h");
        let no_ext = PathTokens::from_path(Path::new("src/Makefile"));
        assert_eq!(render_path("{dir}/{stem}.{ext}", &no_ext), "src/Makefile.");
    }

    #[test]
    fn render_path_glob_escapes_token_values_not_the_template() {
        // Audit 2026-10 finding 2: substituted values are real path text and must
        // not be reinterpreted as glob syntax; the user's template keeps its.
        let t = PathTokens::from_path(Path::new("app/[slug]"));
        assert_eq!(render_path_glob("{path}/*.tsx", &t), "app/[[]slug[]]/*.tsx");
        // The literal renderer leaves values untouched.
        assert_eq!(render_path("{path}/page.tsx", &t), "app/[slug]/page.tsx");
        let star = PathTokens::from_path(Path::new("pkgs/*"));
        assert_eq!(render_path_glob("{path}/**", &star), "pkgs/[*]/**");
        let brace = PathTokens::from_path(Path::new("pkgs/nobrace{"));
        assert_eq!(render_path_glob("{path}/x", &brace), "pkgs/nobrace[{]/x");
        // A value beginning with `!` at the start of a pattern stays literal
        // (a bare leading `!` would turn the pattern into an exclude).
        let bang = PathTokens::from_path(Path::new("!keep"));
        assert_eq!(render_path_glob("{path}/x", &bang), "{!}keep/x");
    }

    #[test]
    fn render_paths_spec_escaped_patterns_match_only_the_real_path() {
        use crate::scope::Scope;
        use crate::walker::FileIndex;
        let idx = FileIndex::from_entries(Vec::new());
        let matches = |dir: &str, pat: &str, candidate: &str| {
            let t = PathTokens::from_path(Path::new(dir));
            let spec = render_paths_spec(&PathsSpec::Single(pat.to_string()), &t);
            Scope::from_paths_spec(&spec)
                .unwrap()
                .matches(Path::new(candidate), &idx)
        };
        assert!(matches(
            "app/[slug]",
            "{path}/page.tsx",
            "app/[slug]/page.tsx"
        ));
        assert!(!matches("app/[slug]", "{path}/page.tsx", "app/s/page.tsx"));
        assert!(matches("pkgs/*", "{path}/*.rs", "pkgs/*/a.rs"));
        assert!(!matches("pkgs/*", "{path}/*.rs", "pkgs/other/a.rs"));
        assert!(matches(
            "pkgs/nobrace{",
            "{path}/*.rs",
            "pkgs/nobrace{/b.rs"
        ));
        assert!(matches("!keep", "{path}/x", "!keep/x"));
        // A template-level `!` exclude survives rendering.
        let t = PathTokens::from_path(Path::new("pkgs/a"));
        let spec = render_paths_spec(
            &PathsSpec::Many(vec!["{path}/**".into(), "!{path}/gen/**".into()]),
            &t,
        );
        let scope = Scope::from_paths_spec(&spec).unwrap();
        assert!(scope.matches(Path::new("pkgs/a/src/x.rs"), &idx));
        assert!(!scope.matches(Path::new("pkgs/a/gen/x.rs"), &idx));
        // Backslash is a legal filename byte off Windows; it must stay literal.
        #[cfg(not(windows))]
        assert!(matches("pkgs/a\\b", "{path}/x", "pkgs/a\\b/x"));
    }

    fn yaml(s: &str) -> ayaml::Mapping {
        crate::yaml::from_str(s).unwrap()
    }

    fn get<'a>(m: &'a ayaml::Mapping, path: &[&str]) -> &'a ayaml::Value {
        let mut v = m.get(path[0]).unwrap();
        for k in &path[1..] {
            v = match k.parse::<usize>() {
                Ok(i) => &v.as_sequence().unwrap()[i],
                Err(_) => v.get(*k).unwrap(),
            };
        }
        v
    }

    #[test]
    fn nested_options_escape_globs_per_kind_and_leave_regexes_literal() {
        let t = PathTokens::from_path(Path::new("app/[slug]"));
        // Globs: escaped (a template `!` exclude survives).
        let m = render_nested_options(
            "dir_contains",
            yaml("select: \"{path}\"\nrequire: [\"{basename}.tsx\", \"*.ts\"]"),
            &t,
        );
        assert_eq!(get(&m, &["select"]).as_str(), Some("app/[[]slug[]]"));
        assert_eq!(get(&m, &["require", "0"]).as_str(), Some("[[]slug[]].tsx"));
        let m = render_nested_options(
            "for_each_dir",
            yaml("select: [\"{path}/*\", \"!{path}/gen\"]"),
            &t,
        );
        assert_eq!(
            get(&m, &["select", "1"]).as_str(),
            Some("!app/[[]slug[]]/gen")
        );
        // Dotted keys reach into nested mappings and lists of mappings.
        let m = render_nested_options(
            "file_graph",
            yaml(
                "nodes: \"{path}/**\"\nrequire:\n  forbidden_edges:\n    - { from: \"{path}/a\", to: x }",
            ),
            &t,
        );
        assert_eq!(get(&m, &["nodes"]).as_str(), Some("app/[[]slug[]]/**"));
        assert_eq!(
            get(&m, &["require", "forbidden_edges", "0", "from"]).as_str(),
            Some("app/[[]slug[]]/a")
        );
        // A regex (`for_each_match.select`) is regex-escaped (not glob-escaped),
        // and a literal path template (`pair.partner`) stays literal.
        let m = render_nested_options("for_each_match", yaml("select: \"{path}\""), &t);
        assert_eq!(get(&m, &["select"]).as_str(), Some(r"app/\[slug\]"));
        let m = render_nested_options(
            "pair",
            yaml("primary: \"{path}/*.c\"\npartner: \"{path}/x.h\""),
            &t,
        );
        assert_eq!(get(&m, &["primary"]).as_str(), Some("app/[[]slug[]]/*.c"));
        assert_eq!(get(&m, &["partner"]).as_str(), Some("app/[slug]/x.h"));
        // A nested spec list renders each element for its own kind, `paths:`
        // as globs.
        let m = render_nested_options(
            "for_each_dir",
            yaml(
                "require:\n  - kind: dir_contains\n    select: \"{path}\"\n  - kind: file_exists\n    paths: \"{path}/x\"\n    message: \"{path}\"",
            ),
            &t,
        );
        assert_eq!(
            get(&m, &["require", "0", "select"]).as_str(),
            Some("app/[[]slug[]]")
        );
        assert_eq!(
            get(&m, &["require", "1", "paths"]).as_str(),
            Some("app/[[]slug[]]/x")
        );
        assert_eq!(
            get(&m, &["require", "1", "message"]).as_str(),
            Some("app/[slug]")
        );
    }

    #[test]
    fn nested_options_guard_arguments_double_dollars_and_mark_message_braces() {
        let t = PathTokens::from_path(Path::new("--x{stem}$1"));
        // A command argument: a leading dash the value introduced is a path.
        let m = render_nested_options(
            "command",
            yaml("command: [\"tool\", \"{path}\", \"--in={path}\"]"),
            &t,
        );
        assert_eq!(get(&m, &["command", "1"]).as_str(), Some("./--x{stem}$1"));
        assert_eq!(
            get(&m, &["command", "2"]).as_str(),
            Some("--in=--x{stem}$1")
        );
        // A replacement template: the value's `$` is text; the template's is
        // a capture reference. The regex beside it regex-escapes the token value
        // while keeping the template's `(.*)` capture intact.
        let m = render_nested_options(
            "file_graph",
            yaml("edges:\n  derive_target:\n    from: \"{path}(.*)\"\n    to: \"{path}/$1\""),
            &t,
        );
        assert_eq!(
            get(&m, &["edges", "derive_target", "to"]).as_str(),
            Some("--x{stem}$$1/$1")
        );
        assert_eq!(
            get(&m, &["edges", "derive_target", "from"]).as_str(),
            Some(r"\-\-x\{stem\}\$1(.*)")
        );
        // A message a kind renders again holds each `{` of a value marked, and
        // the marks come out of the finding's message; another kind's message
        // holds the value as it is.
        let m = render_nested_options(
            "for_each_dir",
            yaml(
                "require:\n  - kind: pair\n    message: \"{path}: {{ctx.primary}}\"\n  - kind: file_exists\n    message: \"{path}\"",
            ),
            &t,
        );
        let marked = get(&m, &["require", "0", "message"]).as_str().unwrap();
        assert_eq!(marked, "--x\u{E001}{\u{E001}stem}$1: {{ctx.primary}}");
        assert_eq!(
            unescape_token_values(marked, &t).as_deref(),
            Some("--x{stem}$1: {{ctx.primary}}")
        );
        assert_eq!(
            get(&m, &["require", "1", "message"]).as_str(),
            Some("--x{stem}$1")
        );
        assert_eq!(
            render_nested_message("unique_by", "{basename}", &t),
            "--x\u{E001}{\u{E001}stem}$1"
        );
        assert_eq!(
            render_nested_message("file_exists", "{basename}", &t),
            "--x{stem}$1"
        );
    }

    #[test]
    fn nested_options_escape_regex_jsonpath_when_iter_and_registry_base() {
        // Regex metacharacters in token values are escaped; surrounding pattern
        // syntax stays intact.
        let re_tok = PathTokens::from_path(Path::new("pkgs/a.b+(c)[0]"));
        let m = render_nested_options(
            "file_content_matches",
            yaml("pattern: \"^{basename}$\""),
            &re_tok,
        );
        assert_eq!(get(&m, &["pattern"]).as_str(), Some(r"^a\.b\+\(c\)\[0\]$"));

        // JSONPath: dot-member rewrites non-shorthand names to bracket form,
        // string literals escape quotes/backslashes, and plain/placeholder
        // identifiers stay shorthand.
        let jp_tok = PathTokens::from_path(Path::new("pkgs/a.b'\"[0]\\c"));
        let m = render_nested_options("json_path_equals", yaml("path: \"$.{basename}\""), &jp_tok);
        assert_eq!(get(&m, &["path"]).as_str(), Some(r#"$['a.b\'"[0]\\c']"#));
        assert_eq!(
            render_path_jsonpath("$['{basename}']", &jp_tok),
            r#"$['a.b\'"[0]\\c']"#
        );
        assert_eq!(
            render_path_jsonpath("$[\"{basename}\"]", &jp_tok),
            r#"$["a.b'\"[0]\\c"]"#
        );
        assert_eq!(
            render_path_jsonpath("$.{stem}", &PathTokens::placeholder()),
            "$._stem_"
        );
        assert_eq!(
            render_path_jsonpath("$..{basename}", &jp_tok),
            r#"$..['a.b\'"[0]\\c']"#
        );

        // `when_iter:`: escapes quotes/backslashes in string literals, glob-escapes
        // inside `iter.has_file(...)`, regex-escapes after `matches`, and quotes
        // bare tokens.
        let quote_tok = PathTokens::from_path(Path::new("pkgs/x\" or true or \""));
        let m = render_nested_options(
            "for_each_dir",
            yaml("when_iter: 'iter.basename == \"{basename}\"'"),
            &quote_tok,
        );
        assert_eq!(
            get(&m, &["when_iter"]).as_str(),
            Some(r#"iter.basename == "x\" or true or \"""#)
        );
        let glob_tok = PathTokens::from_path(Path::new("pkgs/a[0]*"));
        assert_eq!(
            render_path_when_expr(r#"iter.has_file("{basename}.toml")"#, &glob_tok),
            r#"iter.has_file("a[[]0[]][*].toml")"#
        );
        assert_eq!(
            render_path_when_expr(r#"iter.basename matches "^{basename}-""#, &re_tok),
            r#"iter.basename matches "^a\\.b\\+\\(c\\)\\[0\\]-""#
        );
        assert_eq!(
            render_path_when_expr("iter.basename == {basename}", &quote_tok),
            r#"iter.basename == "x\" or true or \"""#
        );

        // `registry_paths_resolve.base:`: literal keywords keep their meaning,
        // while interpolated paths colliding with keywords get a `./` prefix.
        let kw_tok = PathTokens::from_path(Path::new("lint_root"));
        let m = render_nested_options("registry_paths_resolve", yaml("base: \"{path}\""), &kw_tok);
        assert_eq!(get(&m, &["base"]).as_str(), Some("./lint_root"));
        let m = render_nested_options(
            "registry_paths_resolve",
            yaml("base: \"lint_root\""),
            &kw_tok,
        );
        assert_eq!(get(&m, &["base"]).as_str(), Some("lint_root"));
        let dir_kw_tok = PathTokens::from_path(Path::new("registry_dir"));
        let m = render_nested_options(
            "registry_paths_resolve",
            yaml("base: \"{path}\""),
            &dir_kw_tok,
        );
        assert_eq!(get(&m, &["base"]).as_str(), Some("./registry_dir"));

        // `cross_file.targets.matches` is regex-escaped in both glob and list forms,
        // and `when:` on a nested-in-nested rule spec is escaped as a `when_iter` expr.
        let m = render_nested_options(
            "cross_file",
            yaml("targets:\n  files: \"{path}/*.txt\"\n  matches: \"^{basename}$\""),
            &re_tok,
        );
        assert_eq!(
            get(&m, &["targets", "matches"]).as_str(),
            Some(r"^a\.b\+\(c\)\[0\]$")
        );
        let m = render_nested_options(
            "for_each_dir",
            yaml(
                "require:\n  - kind: file_exists\n    paths: \"{path}/x\"\n    when: 'iter.has_file(\"{basename}.ok\")'",
            ),
            &glob_tok,
        );
        assert_eq!(
            get(&m, &["require", "0", "when"]).as_str(),
            Some(r#"iter.has_file("a[[]0[]][*].ok")"#)
        );
    }

    #[test]
    fn a_template_an_iteration_rendered_renders_as_it_is() {
        let t = PathTokens::from_path(Path::new("pk/a.c"));
        let own = PathTemplate::from("pk/x{stem}/{stem}.h");
        assert_eq!(own.tokens(), TokenSet::of("{stem}"));
        assert_eq!(own.render(&t), "pk/xa/a.h");
        let rendered = PathTemplate {
            text: "pk/x{stem}/-{stem}.h".to_owned(),
            rendered: true,
        };
        assert_eq!(rendered.tokens(), TokenSet::NONE);
        assert_eq!(rendered.render(&t), "pk/x{stem}/-{stem}.h");
        assert_eq!(rendered.render_argv(&t), "pk/x{stem}/-{stem}.h");
        assert_eq!(rendered.as_str(), "pk/x{stem}/-{stem}.h");
        let argument = PathTemplate::from("{stem}");
        let dashed = PathTokens::from_path(Path::new("-a.c"));
        assert_eq!(argument.render_argv(&dashed), "./-a");
        assert_eq!(argument.render(&dashed), "-a");
    }

    #[test]
    fn literal_glob_path_decodes_only_escapes() {
        assert_eq!(
            literal_glob_path("app/[[]slug[]]/page.tsx").as_deref(),
            Some("app/[slug]/page.tsx")
        );
        assert_eq!(literal_glob_path("pkgs/[*]/x").as_deref(), Some("pkgs/*/x"));
        assert_eq!(
            literal_glob_path("plain/path").as_deref(),
            Some("plain/path")
        );
        assert_eq!(literal_glob_path("app/*.tsx"), None);
        assert_eq!(literal_glob_path("app/[ab]"), None);
        assert_eq!(literal_glob_path("a{b,c}"), None);
        for raw in ["app/[slug]", "x/{y}", "q?/z*"] {
            assert_eq!(literal_glob_path(&glob_escape(raw)).as_deref(), Some(raw));
        }
    }

    #[test]
    fn unescape_token_values_restores_the_real_path_in_messages() {
        let t = PathTokens::from_path(Path::new("app/[slug]"));
        assert_eq!(
            unescape_token_values("expected a file matching [app/[[]slug[]]/*.tsx]", &t).as_deref(),
            Some("expected a file matching [app/[slug]/*.tsx]")
        );
        // A user-written class is not a token value: left alone.
        assert_eq!(unescape_token_values("matching [ab]", &t), None);
        let plain = PathTokens::from_path(Path::new("src/a"));
        assert_eq!(unescape_token_values("anything", &plain), None);
    }

    #[cfg(not(windows))]
    #[test]
    fn unescape_token_values_restores_a_backslash_in_a_name() {
        // A directory literally named `a\b` is escaped to `a\\b` in the glob;
        // the message shows the real name.
        let t = PathTokens::from_path(Path::new("app/a\\b"));
        let msg = format!(
            "expected a file matching [{}]",
            render_path_glob("{path}/page.tsx", &t)
        );
        assert_eq!(msg, "expected a file matching [app/a\\\\b/page.tsx]");
        assert_eq!(
            unescape_token_values(&msg, &t).as_deref(),
            Some("expected a file matching [app/a\\b/page.tsx]")
        );
    }

    #[test]
    fn render_path_unknown_token_preserved() {
        let t = PathTokens::from_path(Path::new("a.c"));
        assert_eq!(render_path("{bogus}/{stem}.x", &t), "{bogus}/a.x");
    }

    #[test]
    fn render_path_does_not_resubstitute_token_in_value() {
        // L8: a file literally named `a{ext}.c` has stem `a{ext}`. The `{ext}`
        // that comes FROM the path value must NOT be expanded by the `{ext}`
        // substitution (the old repeated-replace pass produced `ac.h`).
        let t = PathTokens::from_path(Path::new("a{ext}.c"));
        assert_eq!(t.stem, "a{ext}");
        assert_eq!(render_path("{stem}.h", &t), "a{ext}.h");
    }

    #[test]
    fn render_path_argv_guards_leading_dash_from_substitution() {
        // L13: a repo file named like an option must not flip a trusted command.
        let evil = PathTokens::from_path(Path::new("--write"));
        assert_eq!(render_path_argv("{path}", &evil), "./--write");
        // A flag the *user* wrote is left alone (it already starts with `-`).
        let normal = PathTokens::from_path(Path::new("src/main.rs"));
        assert_eq!(render_path_argv("--check", &normal), "--check");
        // A path embedded after `=` keeps the dash inside the value (no option).
        assert_eq!(render_path_argv("--file={path}", &evil), "--file=--write");
        // The ordinary case is unchanged.
        assert_eq!(render_path_argv("{path}", &normal), "src/main.rs");
    }

    #[test]
    fn render_message_simple() {
        let out = render_message("{{ctx.primary}} → {{ctx.partner}}", |ns, key| {
            match (ns, key) {
                ("ctx", "primary") => Some("a.c".into()),
                ("ctx", "partner") => Some("a.h".into()),
                _ => None,
            }
        });
        assert_eq!(out, "a.c → a.h");
    }

    #[test]
    fn render_message_ignores_inner_whitespace() {
        let out = render_message("[{{ ctx . primary }}]", |ns, key| {
            if ns == "ctx" && key == "primary" {
                Some("x".into())
            } else {
                None
            }
        });
        assert_eq!(out, "[x]");
    }

    #[test]
    fn render_message_unknown_key_preserved() {
        let out = render_message("{{ctx.unknown}}", |_, _| None);
        assert_eq!(out, "{{ctx.unknown}}");
    }

    #[test]
    fn render_message_unterminated_is_preserved() {
        let out = render_message("before {{ctx.primary", |_, _| Some("X".into()));
        assert_eq!(out, "before {{ctx.primary");
    }

    #[test]
    fn render_message_no_placeholders() {
        let out = render_message("plain text", |_, _| Some("never".into()));
        assert_eq!(out, "plain text");
    }
}
