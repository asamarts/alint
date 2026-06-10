//! `import_gate` — forbid imports whose extracted target matches
//! a regex, within a path scope. An architectural import
//! firewall (k8s `staging/` layering, airflow core/providers,
//! `torch._C`, prometheus-imports). Matches the **extracted
//! import target** (not the raw line) and supports `allow`
//! exemptions — the precise, low-false-positive specialisation
//! of `file_content_forbidden`. Per-file rule. Design +
//! open-question resolutions: `docs/design/v0.10/import_gate.md`.
//!
//! ```yaml
//! - id: staging-no-main-module
//!   kind: import_gate
//!   paths: "staging/src/k8s.io/**/*.go"
//!   language: go                          # go|python|rust|js|scala|java|dart|nix|generic
//!   forbid: "^k8s\\.io/kubernetes/"       # regex on the EXTRACTED target
//!   allow: ["staging/src/k8s.io/legacy/**"]
//!   level: error
//! ```

use std::path::Path;

use alint_core::{
    Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation, eval_per_file,
};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
// Rename in the schema so the derived `$defs` entry does not collide with
// commented_out_code's (different) `Language` enum.
#[schemars(rename = "ImportLanguage")]
enum Language {
    Go,
    Python,
    Rust,
    Js,
    Scala,
    Java,
    Dart,
    Nix,
    /// No preset — an explicit `import_pattern` is required.
    Generic,
}

impl Language {
    /// Default import-line regex; **capture group 1 is the
    /// imported target**. Line-based (not a grammar — see the
    /// design doc's false-positive section); users override with
    /// `import_pattern` for edge cases.
    fn default_pattern(self) -> Option<&'static str> {
        Some(match self {
            // `import "x"` / `import alias "x"` / `import _ "x"`,
            // or a grouped-block member line (`\t"x"`, `\t_ "x"`,
            // `\talias "x"`), end-anchored (optional trailing
            // line comment) so a mid-statement string can't match.
            Self::Go => r#"^\s*(?:import\s+)?(?:_\s+|[A-Za-z][\w.]*\s+)?"([^"]+)"\s*(?://.*)?$"#,
            // `import a.b` or `from a.b import c` -> `a.b`.
            Self::Python => r"^\s*(?:from|import)\s+([\w.]+)",
            // `use a::b::c;` / `pub use a::{b, c};` -> `a::b::c` / `a::`.
            Self::Rust => r"^\s*(?:pub\s+)?use\s+([\w:]+)",
            // `import x from "m"`, `import "m"`, `require("m")`,
            // `import("m")` -> `m`.
            Self::Js => r#"(?:from\s*|require\s*\(\s*|import\s*\(\s*|import\s+)['"]([^'"]+)['"]"#,
            // `import a.b.c`, `import a.b.{c, d}`, `import a.b._` -> the
            // dotted path (a trailing `.`/`_` on selector imports is
            // harmless for prefix forbids).
            Self::Scala => r"^\s*import\s+([\w.]+)",
            // `import a.b.C;`, `import static a.b.C.m;`, `import a.b.*;`.
            Self::Java => r"^\s*import\s+(?:static\s+)?([\w.]+)",
            // `import 'package:foo/bar.dart';` / `export "dart:async";`
            // -> the quoted URI.
            Self::Dart => r#"^\s*(?:import|export)\s+['"]([^'"]+)['"]"#,
            // The `import` builtin: `import ./mod.nix`, `import <nixpkgs>`,
            // `let x = import ../foo;` -> the path expression. The NixOS
            // `imports = [ ... ]` module-list form is multi-target; gate
            // it with `language: generic` + a custom `import_pattern`.
            Self::Nix => r#"\bimport\s+(<[^>]+>|\.\.?/[^\s;{(]+|"[^"]+")"#,
            Self::Generic => return None,
        })
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Regex tested against the extracted import target.
    forbid: String,
    /// Built-in import-line pattern preset (capture group 1 = the imported
    /// target). Omit to require an explicit `import_pattern`.
    #[serde(default)]
    language: Option<Language>,
    /// Explicit import-line regex (capture group 1 = target).
    /// Overrides the `language` preset.
    #[serde(default)]
    import_pattern: Option<String>,
    /// File globs inside the scope that are exempt from the gate.
    #[serde(default)]
    allow: Vec<String>,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct ImportGateRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    forbid_src: String,
    forbid: Regex,
    import_re: Regex,
    allow: Option<Scope>,
    /// Blank `//` and `/* … */` comments before matching. Set only
    /// for the `language: js` preset, whose pattern is unanchored and
    /// so matches `import("…")` inside a `JSDoc` `@typedef {import(…)}`
    /// — a type-only annotation, not a real import (eslint / svelte).
    /// The anchored presets (`^\s*import …`) can't match a comment
    /// line, so they don't need it.
    strip_comments: bool,
}

impl Rule for ImportGateRule {
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

impl PerFileRule for ImportGateRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        // Sanctioned exceptions: a file in scope but on the allow
        // list is exempt from the gate entirely.
        if self
            .allow
            .as_ref()
            .is_some_and(|a| a.matches(path, ctx.index))
        {
            return Ok(Vec::new());
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Ok(Vec::new());
        };
        // Blank comments first for the `js` preset (line numbers are
        // preserved, so violation locations stay correct).
        let stripped;
        let scan_text = if self.strip_comments {
            stripped = strip_js_comments(text);
            stripped.as_str()
        } else {
            text
        };
        let mut violations = Vec::new();
        for (i, line) in scan_text.lines().enumerate() {
            let Some(caps) = self.import_re.captures(line) else {
                continue;
            };
            let Some(target) = caps.get(1).map(|m| m.as_str()) else {
                continue;
            };
            if self.forbid.is_match(target) {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "forbidden import {target:?} at this scope (matches /{}/)",
                        self.forbid_src
                    )
                });
                violations.push(
                    Violation::new(msg)
                        .with_path(std::sync::Arc::<Path>::from(path))
                        .with_location(i + 1, 1),
                );
            }
        }
        Ok(violations)
    }
}

/// Replace `//` line comments and `/* … */` block comments (incl.
/// `JSDoc` `/** … */`) with spaces, preserving newlines so line
/// numbers and the per-line scanner are unaffected. String literals
/// (`'…'`, `"…"`, `` `…` ``) are passed through verbatim — the import
/// *target* is itself a quoted string, so blanking strings would
/// erase what we extract. Not a full JS lexer: regex literals are
/// treated as code, so a regex containing `//` or `/*` could be
/// mis-stripped (vanishingly rare on an import-bearing line). Enough
/// to stop the `js` preset matching `import(…)` inside a comment.
fn strip_js_comments(src: &str) -> String {
    enum S {
        Code,
        Line,
        Block,
        Str(char),
    }
    let mut out = String::with_capacity(src.len());
    let mut state = S::Code;
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match state {
            S::Code => match c {
                '/' if chars.peek() == Some(&'/') => {
                    chars.next();
                    out.push_str("  ");
                    state = S::Line;
                }
                '/' if chars.peek() == Some(&'*') => {
                    chars.next();
                    out.push_str("  ");
                    state = S::Block;
                }
                '\'' | '"' | '`' => {
                    out.push(c);
                    state = S::Str(c);
                }
                _ => out.push(c),
            },
            S::Line => {
                if c == '\n' {
                    out.push('\n');
                    state = S::Code;
                } else {
                    out.push(if c == '\r' { '\r' } else { ' ' });
                }
            }
            S::Block => {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    out.push_str("  ");
                    state = S::Code;
                } else if c == '\n' || c == '\r' {
                    out.push(c);
                } else {
                    out.push(' ');
                }
            }
            S::Str(q) => {
                out.push(c);
                if c == '\\' {
                    if let Some(n) = chars.next() {
                        out.push(n);
                    }
                } else if c == q {
                    state = S::Code;
                }
            }
        }
    }
    out
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    if spec.paths.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "import_gate requires a `paths` field (the scope the gate applies to)",
        ));
    }
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;

    // Explicit `import_pattern` wins; else the `language` preset;
    // else (no preset, no pattern) it's a config error.
    let pattern_src: String = match (&opts.import_pattern, opts.language) {
        (Some(p), _) => p.clone(),
        (None, Some(lang)) => lang
            .default_pattern()
            .ok_or_else(|| {
                Error::rule_config(
                    &spec.id,
                    "import_gate `language: generic` requires an explicit `import_pattern`",
                )
            })?
            .to_string(),
        (None, None) => {
            return Err(Error::rule_config(
                &spec.id,
                "import_gate requires `language:` (go/python/rust/js/scala/java/dart/nix) or `import_pattern:`",
            ));
        }
    };
    let import_re = Regex::new(&pattern_src)
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid `import_pattern`: {e}")))?;
    let forbid = Regex::new(&opts.forbid)
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid `forbid` regex: {e}")))?;
    let allow = if opts.allow.is_empty() {
        None
    } else {
        Some(
            Scope::from_patterns(&opts.allow)
                .map_err(|e| Error::rule_config(&spec.id, format!("invalid `allow` glob: {e}")))?,
        )
    };

    // The `js` preset's pattern is unanchored, so it matches
    // `import(…)` inside a JSDoc comment; blank comments first. A
    // custom `import_pattern` or any other (anchored) preset opts out.
    let strip_comments =
        opts.import_pattern.is_none() && matches!(opts.language, Some(Language::Js));

    Ok(Box::new(ImportGateRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        forbid_src: opts.forbid,
        forbid,
        import_re,
        allow,
        strip_comments,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(language: Language, forbid: &str, allow: &[&str]) -> ImportGateRule {
        let pattern = language.default_pattern().expect("preset has a pattern");
        ImportGateRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            scope: Scope::from_patterns(&["**/*".to_string()]).unwrap(),
            forbid_src: forbid.into(),
            forbid: Regex::new(forbid).unwrap(),
            import_re: Regex::new(pattern).unwrap(),
            allow: if allow.is_empty() {
                None
            } else {
                Some(
                    Scope::from_patterns(
                        &allow.iter().map(ToString::to_string).collect::<Vec<_>>(),
                    )
                    .unwrap(),
                )
            },
            strip_comments: matches!(language, Language::Js),
        }
    }

    fn eval(r: &ImportGateRule, path: &str, src: &str) -> Vec<Violation> {
        let idx = alint_core::FileIndex::from_entries(vec![alint_core::FileEntry {
            path: Path::new(path).into(),
            is_dir: false,
            size: 1,
        }]);
        let ctx = Context {
            root: Path::new("/"),
            index: &idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        r.evaluate_file(&ctx, Path::new(path), src.as_bytes())
            .unwrap()
    }

    #[test]
    fn go_grouped_and_single_imports_are_gated() {
        let r = rule(Language::Go, r"^k8s\.io/kubernetes/", &[]);
        let src = "package x\n\nimport (\n\t\"fmt\"\n\t\"k8s.io/kubernetes/pkg/api\"\n)\n\nimport \"k8s.io/kubernetes/cmd\"\n";
        let v = eval(&r, "staging/a.go", src);
        assert_eq!(v.len(), 2, "both forbidden imports flagged: {v:?}");
        assert!(v[0].message.contains("k8s.io/kubernetes/pkg/api"));
        assert!(v.iter().all(|x| !x.message.contains("\"fmt\"")));
    }

    #[test]
    fn target_not_raw_line_no_false_positive_on_comment() {
        let r = rule(Language::Go, r"^k8s\.io/kubernetes/", &[]);
        // The forbidden path appears only in a comment / string,
        // not an actual import → no violation.
        let src = "package x\n// see k8s.io/kubernetes/pkg for context\nvar s = \"k8s.io/kubernetes/x is a path\"\n";
        assert!(eval(&r, "staging/a.go", src).is_empty());
    }

    #[test]
    fn allow_glob_exempts_a_scoped_file() {
        let r = rule(
            Language::Go,
            r"^k8s\.io/kubernetes/",
            &["staging/legacy/**"],
        );
        let src = "import \"k8s.io/kubernetes/pkg\"\n";
        assert_eq!(eval(&r, "staging/a.go", src).len(), 1);
        assert!(eval(&r, "staging/legacy/old.go", src).is_empty());
    }

    #[test]
    fn python_from_and_import_forms() {
        let r = rule(Language::Python, r"^airflow\.providers", &[]);
        let src =
            "from airflow.providers.amazon import S3\nimport airflow.providers.google\nimport os\n";
        let v = eval(&r, "airflow/core/x.py", src);
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(
            eval(
                &r,
                "airflow/core/x.py",
                "import os\nfrom airflow.models import DAG\n"
            )
            .is_empty()
        );
    }

    #[test]
    fn rust_use_paths() {
        let r = rule(Language::Rust, r"^crate::secrets", &[]);
        let src = "use crate::secrets::Key;\npub use std::process::Command;\n";
        let v = eval(&r, "src/a.rs", src);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("crate::secrets"));
    }

    #[test]
    fn js_import_and_require() {
        let r = rule(Language::Js, r"^lodash", &[]);
        let src = "import _ from \"lodash\";\nconst x = require('lodash/fp');\nimport y from \"react\";\n";
        assert_eq!(eval(&r, "src/a.js", src).len(), 2);
    }

    #[test]
    fn js_jsdoc_type_import_is_ignored_but_real_import_fires() {
        // The `language: js` preset must not treat a JSDoc type-only
        // `import(...)` as a real import (eslint `@typedef {import(...)}`
        // / svelte `import('../compiler/...')` in JSDoc) — but a genuine
        // import of the same path on a code line still fires.
        let r = rule(Language::Js, r"^\.\./compiler", &[]);
        let src = "/**\n\
                   * @typedef {import('../compiler/types').Foo} Foo\n\
                   * @param {Array<import(\"../compiler/x\")>} a\n\
                   */\n\
                   export function f(a) { return a; }\n";
        assert!(
            eval(&r, "src/runtime.js", src).is_empty(),
            "JSDoc type-imports must not fire the gate"
        );
        // The real static import is still caught.
        let src2 = "import { x } from '../compiler/internal';\n";
        assert_eq!(eval(&r, "src/runtime.js", src2).len(), 1);
    }

    #[test]
    fn js_line_comment_import_is_ignored() {
        let r = rule(Language::Js, r"^lodash", &[]);
        let src = "// import _ from \"lodash\";\nconst ok = 1;\n";
        assert!(eval(&r, "src/a.js", src).is_empty());
    }

    #[test]
    fn js_block_comment_does_not_swallow_a_trailing_real_import() {
        // A `/* … */` on the same line as a real import must blank only
        // the comment, leaving the import matchable.
        let r = rule(Language::Js, r"^lodash", &[]);
        let src = "/* uses */ import _ from \"lodash\";\n";
        assert_eq!(eval(&r, "src/a.js", src).len(), 1);
    }

    #[test]
    fn scala_import_paths() {
        let r = rule(Language::Scala, r"^scala\.sys", &[]);
        let src =
            "import scala.sys.process._\nimport scala.collection.mutable\nimport java.util.List\n";
        let v = eval(&r, "src/a.scala", src);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("scala.sys"));
    }

    #[test]
    fn java_import_static_and_wildcard() {
        let r = rule(Language::Java, r"^com\.internal", &[]);
        let src = "import com.internal.Secret;\nimport static com.internal.Util.helper;\nimport java.util.List;\nimport com.internal.*;\n";
        let v = eval(&r, "src/A.java", src);
        assert_eq!(v.len(), 3, "{v:?}");
        assert!(v.iter().all(|x| !x.message.contains("java.util.List")));
    }

    #[test]
    fn dart_import_and_export_uris() {
        let r = rule(Language::Dart, r"^package:legacy/", &[]);
        let src = "import 'package:legacy/old.dart';\nexport \"package:legacy/api.dart\";\nimport 'dart:async';\n";
        let v = eval(&r, "lib/a.dart", src);
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(v.iter().all(|x| !x.message.contains("dart:async")));
    }

    #[test]
    fn nix_import_builtin_paths() {
        let r = rule(Language::Nix, r"^<nixpkgs>$", &[]);
        let src = "let pkgs = import <nixpkgs> { };\nin import ./local.nix\n";
        let v = eval(&r, "default.nix", src);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("nixpkgs"));
    }

    #[test]
    fn build_errors_on_generic_without_pattern_and_bad_regex() {
        let mut spec = crate::test_support::spec_yaml(
            "id: t\nkind: import_gate\npaths: \"**/*\"\nlanguage: generic\nforbid: x\nlevel: error\n",
        );
        assert!(build(&spec).unwrap_err().to_string().contains("generic"));
        spec = crate::test_support::spec_yaml(
            "id: t\nkind: import_gate\npaths: \"**/*\"\nlanguage: rust\nforbid: \"[\"\nlevel: error\n",
        );
        assert!(build(&spec).unwrap_err().to_string().contains("forbid"));
    }
}
