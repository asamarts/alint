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

impl PathTokens {
    /// Derive tokens from a relative path. Missing components (e.g. a path
    /// with no parent, or no extension) resolve to the empty string.
    pub fn from_path(rel: &Path) -> Self {
        Self {
            path: rel.display().to_string(),
            dir: rel
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            basename: rel
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string(),
            stem: rel
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string(),
            ext: rel
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string(),
            parent_name: rel
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string(),
        }
    }
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
        out.push_str(&glob_escape(v));
    })
}

/// Escape `s` so it matches only itself when compiled as an alint glob
/// (`globset`, the dialect `Scope` uses): the metacharacters `* ? [ ] { }` are
/// wrapped in a one-character class (`[` -> `[[]`), and on non-Windows a `\`
/// (a legal filename byte there, and globset's escape character) is
/// backslash-escaped. On Windows `\` is the path separator, never part of a
/// name, so it is left as-is.
#[must_use]
pub fn glob_escape(s: &str) -> String {
    let escaped = globset::escape(s);
    if cfg!(windows) {
        escaped
    } else {
        escaped.replace('\\', "\\\\")
    }
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

/// Substitute `{{namespace.key}}` placeholders in a message template. The
/// caller-supplied `resolve` closure returns the substituted value, or
/// `None` to leave the placeholder literal.
///
/// Whitespace inside the braces (`{{ ctx.primary }}`) is ignored so users
/// can format their messages for readability.
/// Apply path-template substitution to every string inside a YAML mapping,
/// recursively into nested mappings and sequences. Non-string values pass
/// through unchanged. Used by nested-rule specs (e.g. `for_each_dir`) so that
/// the `{dir}` in a nested rule's `paths`, `pattern`, or `partner` field
/// resolves to the iterated entry's path at rule-build time.
pub fn render_mapping(m: serde_yaml_ng::Mapping, tokens: &PathTokens) -> serde_yaml_ng::Mapping {
    let mut out = serde_yaml_ng::Mapping::with_capacity(m.len());
    for (k, v) in m {
        out.insert(k, render_value(v, tokens));
    }
    out
}

/// Recursive mate to [`render_mapping`] for arbitrary YAML values.
pub fn render_value(v: serde_yaml_ng::Value, tokens: &PathTokens) -> serde_yaml_ng::Value {
    use serde_yaml_ng::Value;
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

/// The literal path a glob matches when it is a plain path whose only
/// metacharacters are [`glob_escape`]d (`app/[[]slug[]]/page.tsx` ->
/// `app/[slug]/page.tsx`); `None` when it has any real glob syntax.
#[must_use]
pub fn glob_literal(pattern: &str) -> Option<String> {
    const META: &[char] = &['*', '?', '[', ']', '{', '}'];
    let mut out = String::with_capacity(pattern.len());
    let mut rest = pattern;
    while let Some(c) = rest.chars().next() {
        if c == '[' {
            // Only a one-character class of a metacharacter is an escape.
            let mut it = rest[1..].chars();
            match (it.next(), it.next()) {
                (Some(m), Some(']')) if META.contains(&m) => {
                    out.push(m);
                    rest = &rest[1 + m.len_utf8() + 1..];
                    continue;
                }
                _ => return None,
            }
        }
        if !cfg!(windows) && c == '\\' {
            let next = rest[1..].chars().next()?;
            out.push(next);
            rest = &rest[1 + next.len_utf8()..];
            continue;
        }
        if META.contains(&c) {
            return None;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    Some(out)
}

/// Undo [`render_path_glob`]'s escaping of `tokens`' values inside a
/// user-facing `message`: a nested rule reports the glob it was built from
/// (`expected a file matching app/[[]slug[]]/*.tsx`), and the escape forms are
/// noise to the reader, who named the directory `app/[slug]`. Only the exact
/// escaped spellings of this iteration's token values are replaced, so a
/// user-written pattern is shown as written.
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

/// Option keys (beyond `paths:`, which is rendered by [`render_paths_spec`])
/// whose values are compiled as **globs**, per rule kind (aliases included).
/// A dotted key is a path into nested mappings (sequence elements are walked
/// through), e.g. `targets.files`. Every other string option is rendered
/// LITERALLY: a regex (`for_each_match.select`, `pattern`), a literal path or
/// template (`pair.partner`, `pair_hash.target`, `cross_file`'s `file`), a
/// message. Glob-escaping a regex or a literal path would corrupt it.
///
/// Kept in sync with the rule kinds by `nested_glob_options_cover_the_schema`,
/// which flags any schema option documented as a glob that is neither listed
/// here nor explicitly exempted.
pub const NESTED_GLOB_OPTIONS: &[(&[&str], &[&str])] = &[
    (&["dir_contains"], &["select", "require"]),
    (&["dir_only_contains"], &["select", "allow"]),
    (&["unique_by"], &["select"]),
    (
        &["for_each_dir", "for_each_file", "every_matching_has"],
        &["select"],
    ),
    (&["pair"], &["primary"]),
    (&["pair_hash"], &["source"]),
    (&["pair_changed_together"], &["if_changed", "then_changed"]),
    (&["changeset_requires_path"], &["add_glob", "when_changed"]),
    (&["generated_file_fresh"], &["outputs"]),
    (&["git_no_denied_paths"], &["denied"]),
    (&["import_gate"], &["allow"]),
    (
        &["file_graph"],
        &[
            "nodes",
            "require.no_orphans.roots",
            "require.forbidden_edges.from",
            "require.forbidden_edges.to",
        ],
    ),
    (
        &["cross_file", "cross_file_value_equals"],
        &["source.files", "targets.files"],
    ),
    (&["registry_paths_resolve"], &["source", "orphans.space"]),
];

/// The glob-typed option keys of `kind` (see [`NESTED_GLOB_OPTIONS`]).
#[must_use]
pub fn nested_glob_options(kind: &str) -> &'static [&'static str] {
    NESTED_GLOB_OPTIONS
        .iter()
        .find(|(kinds, _)| kinds.contains(&kind))
        .map_or(&[], |(_, keys)| keys)
}

/// Render a nested rule's option mapping (`extra`) for one iteration. Like
/// [`render_mapping`], every string gets the iterated entry's tokens, but a
/// glob-typed option of `kind` ([`NESTED_GLOB_OPTIONS`]) is rendered with
/// [`render_path_glob`] (a leading template `!` exclude is kept), and a nested
/// rule-spec list (`for_each_dir.require`, ...) is rendered per element for
/// that element's own kind. So a directory named `app/[slug]` reaches a nested
/// `dir_contains` `select: "{path}"` as the literal `app/[[]slug[]]`, not a
/// character class that matches `app/s`.
#[must_use]
pub fn render_nested_options(
    kind: &str,
    m: serde_yaml_ng::Mapping,
    tokens: &PathTokens,
) -> serde_yaml_ng::Mapping {
    render_options(kind, m, tokens, false)
}

/// [`render_nested_options`]; `with_paths` also renders the mapping's own
/// `paths:` (a nested spec inside a nested spec list) as globs.
fn render_options(
    kind: &str,
    m: serde_yaml_ng::Mapping,
    tokens: &PathTokens,
    with_paths: bool,
) -> serde_yaml_ng::Mapping {
    const PATHS: &[&str] = &["paths", "paths.include", "paths.exclude"];
    let globs = nested_glob_options(kind);
    let spec_list = NESTED_SPEC_LISTS
        .iter()
        .find(|(kinds, _)| kinds.contains(&kind))
        .map(|(_, key)| *key);
    let mut out = serde_yaml_ng::Mapping::with_capacity(m.len());
    for (k, v) in m {
        let key = k.as_str().unwrap_or_default().to_string();
        let v = if spec_list == Some(key.as_str()) {
            render_nested_spec_list(v, tokens)
        } else if with_paths && key == "paths" {
            render_keyed(v, &key, PATHS, tokens)
        } else {
            render_keyed(v, &key, globs, tokens)
        };
        out.insert(k, v);
    }
    out
}

/// Render `v`, found at dotted key path `at`, glob-escaping where `at` is one
/// of `globs`.
fn render_keyed(
    v: serde_yaml_ng::Value,
    at: &str,
    globs: &[&str],
    tokens: &PathTokens,
) -> serde_yaml_ng::Value {
    use serde_yaml_ng::Value;
    match v {
        Value::String(s) if globs.contains(&at) => Value::String(match s.strip_prefix('!') {
            Some(rest) => format!("!{}", render_path_glob(rest, tokens)),
            None => render_path_glob(&s, tokens),
        }),
        Value::String(s) => Value::String(render_path(&s, tokens)),
        Value::Sequence(seq) => Value::Sequence(
            seq.into_iter()
                .map(|e| render_keyed(e, at, globs, tokens))
                .collect(),
        ),
        Value::Mapping(m) => Value::Mapping(
            m.into_iter()
                .map(|(k, v)| {
                    let sub = match k.as_str() {
                        Some(name) => format!("{at}.{name}"),
                        None => at.to_string(),
                    };
                    (k, render_keyed(v, &sub, globs, tokens))
                })
                .collect(),
        ),
        other => other,
    }
}

/// Render a list of nested rule specs, each for its own `kind` (its `paths:`
/// as globs too).
fn render_nested_spec_list(v: serde_yaml_ng::Value, tokens: &PathTokens) -> serde_yaml_ng::Value {
    use serde_yaml_ng::Value;
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

    fn yaml(s: &str) -> serde_yaml_ng::Mapping {
        serde_yaml_ng::from_str(s).unwrap()
    }

    fn get<'a>(m: &'a serde_yaml_ng::Mapping, path: &[&str]) -> &'a serde_yaml_ng::Value {
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
        // A regex (`for_each_match.select`) and a literal path template
        // (`pair.partner`) are NOT glob-escaped.
        let m = render_nested_options("for_each_match", yaml("select: \"{path}\""), &t);
        assert_eq!(get(&m, &["select"]).as_str(), Some("app/[slug]"));
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
    fn glob_literal_decodes_only_escapes() {
        assert_eq!(
            glob_literal("app/[[]slug[]]/page.tsx").as_deref(),
            Some("app/[slug]/page.tsx")
        );
        assert_eq!(glob_literal("pkgs/[*]/x").as_deref(), Some("pkgs/*/x"));
        assert_eq!(glob_literal("plain/path").as_deref(), Some("plain/path"));
        assert_eq!(glob_literal("app/*.tsx"), None);
        assert_eq!(glob_literal("app/[ab]"), None);
        assert_eq!(glob_literal("a{b,c}"), None);
        for raw in ["app/[slug]", "x/{y}", "q?/z*"] {
            assert_eq!(glob_literal(&glob_escape(raw)).as_deref(), Some(raw));
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

    #[test]
    fn nested_glob_options_cover_the_schema() {
        // Drift guard: an option the config schema documents as a glob must be
        // listed in NESTED_GLOB_OPTIONS (or exempted here with a reason), or a
        // nested rule renders it unescaped and `app/[slug]` becomes a class.
        const EXEMPT: &[(&str, &str)] = &[
            // `paths:` is rendered by `render_paths_spec`.
            ("*", "paths"),
            // Booleans whose description mentions globs.
            ("*", "respect_gitignore"),
            ("registry_paths_resolve", "entries_are_globs"),
        ];
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/v1/config.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            // Not in a workspace checkout (a packaged crate): nothing to check.
            return;
        };
        let schema: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut missing = Vec::new();
        for def in schema["$defs"].as_object().unwrap().values() {
            let Some(props) = def["properties"].as_object() else {
                continue;
            };
            let kinds: Vec<&str> = match &props.get("kind") {
                Some(k) if k["enum"].is_array() => k["enum"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect(),
                Some(k) if k["const"].is_string() => vec![k["const"].as_str().unwrap()],
                _ => continue,
            };
            for (key, prop) in props {
                if !prop.to_string().to_lowercase().contains("glob") {
                    continue;
                }
                for kind in &kinds {
                    let exempt = EXEMPT
                        .iter()
                        .any(|(k, o)| (*k == "*" || k == kind) && o == key);
                    let listed = nested_glob_options(kind)
                        .iter()
                        .any(|g| g.split('.').next() == Some(key.as_str()));
                    if !exempt && !listed {
                        missing.push(format!("{kind}.{key}"));
                    }
                }
            }
        }
        assert!(
            missing.is_empty(),
            "glob options missing from NESTED_GLOB_OPTIONS: {missing:?}"
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
