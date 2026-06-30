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
pub fn render_path(template: &str, t: &PathTokens) -> String {
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
            out.push_str(val);
            rest = &at_brace[tok.len()..];
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
pub fn render_paths_spec(spec: &PathsSpec, tokens: &PathTokens) -> PathsSpec {
    match spec {
        PathsSpec::Single(s) => PathsSpec::Single(render_path(s, tokens)),
        PathsSpec::Many(v) => PathsSpec::Many(v.iter().map(|s| render_path(s, tokens)).collect()),
        PathsSpec::IncludeExclude { include, exclude } => PathsSpec::IncludeExclude {
            include: include.iter().map(|s| render_path(s, tokens)).collect(),
            exclude: exclude.iter().map(|s| render_path(s, tokens)).collect(),
        },
    }
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
