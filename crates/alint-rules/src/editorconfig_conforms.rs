//! `editorconfig_conforms` — enforce formatting policy directly from confined
//! repository `.editorconfig` files, retaining declaration provenance.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use alint_core::{
    AnalysisFailureKind, Context, Error, Level, RelatedLocation, Result, Rule, RuleSpec, Scope,
    SourceRange, Violation, missing_file_violation, too_large_file_violation,
    unreadable_file_violation,
};
use regex::Regex;

#[derive(Debug, Clone)]
struct Declaration {
    value: String,
    path: PathBuf,
    line: usize,
    end_column: usize,
}

#[derive(Debug)]
struct Section {
    glob: EditorConfigGlob,
    declarations: Vec<(String, Declaration)>,
}

#[derive(Debug)]
struct PolicyDocument {
    root: bool,
    sections: Vec<Section>,
}

#[derive(Debug)]
enum PolicyState {
    Ready(PolicyDocument),
    Failed,
}

#[derive(Debug)]
struct EditorConfigGlob {
    regex: Regex,
    numeric_ranges: Vec<(usize, RangeInclusive<i64>)>,
}

impl EditorConfigGlob {
    fn new(directory: &Path, pattern: &str) -> std::result::Result<Self, String> {
        if pattern.is_empty() {
            return Err("empty EditorConfig section pattern".into());
        }
        if pattern.len() > 4096 {
            return Err("EditorConfig section pattern exceeds 4096 bytes".into());
        }
        let directory = crate::slash(directory);
        let has_separator = glob_has_path_separator(pattern);
        let starts_with_separator = pattern.starts_with('/');
        let pattern = pattern.strip_prefix('/').unwrap_or(pattern);
        let mut body = String::new();
        let mut numeric_ranges = Vec::new();
        translate_glob(pattern, &mut body, &mut numeric_ranges)?;

        let mut source = String::from("^");
        if !directory.is_empty() {
            source.push_str(&regex::escape(&directory));
            source.push('/');
        }
        if !has_separator {
            source.push_str("(?:.*/)?");
        } else if starts_with_separator {
            // The leading slash anchors to the EditorConfig file's directory;
            // it is not part of the repository-relative candidate path.
        }
        source.push_str(&body);
        source.push('$');
        let regex = Regex::new(&source)
            .map_err(|error| format!("invalid EditorConfig glob `{pattern}`: {error}"))?;
        Ok(Self {
            regex,
            numeric_ranges,
        })
    }

    fn is_match(&self, path: &Path) -> bool {
        let candidate = crate::slash(path);
        let Some(captures) = self.regex.captures(&candidate) else {
            return false;
        };
        self.numeric_ranges.iter().all(|(index, range)| {
            captures.get(*index).is_none_or(|value| {
                value
                    .as_str()
                    .parse::<i64>()
                    .is_ok_and(|n| range.contains(&n))
            })
        })
    }
}

/// A slash inside a complete character class is literal glob text, not a path
/// separator. This distinction controls whether the section may match at any
/// depth below its `.editorconfig` directory.
fn glob_has_path_separator(pattern: &str) -> bool {
    let mut cursor = 0usize;
    while cursor < pattern.len() {
        let rest = &pattern[cursor..];
        let ch = rest.chars().next().expect("cursor is in bounds");
        match ch {
            '[' => {
                cursor += find_unescaped(rest, ']').map_or(1, |end| end + 1);
            }
            '/' => return true,
            '\\' => {
                cursor += 1;
                if cursor < pattern.len() {
                    let escaped = pattern[cursor..]
                        .chars()
                        .next()
                        .expect("cursor is in bounds");
                    if escaped == '/' {
                        return true;
                    }
                    cursor += escaped.len_utf8();
                }
            }
            _ => cursor += ch.len_utf8(),
        }
    }
    false
}

fn translate_glob(
    pattern: &str,
    output: &mut String,
    numeric_ranges: &mut Vec<(usize, RangeInclusive<i64>)>,
) -> std::result::Result<(), String> {
    let mut cursor = 0usize;
    while cursor < pattern.len() {
        let rest = &pattern[cursor..];
        if rest.starts_with("/**/") {
            output.push_str("(?:/|/.*/)");
            cursor += 4;
            continue;
        }
        let ch = rest.chars().next().expect("cursor is in bounds");
        match ch {
            '\\' => {
                cursor += 1;
                if cursor < pattern.len() {
                    let escaped = pattern[cursor..]
                        .chars()
                        .next()
                        .expect("cursor is in bounds");
                    output.push_str(&regex::escape(&escaped.to_string()));
                    cursor += escaped.len_utf8();
                } else {
                    output.push_str("\\\\");
                }
            }
            '?' => {
                output.push_str("[^/]");
                cursor += 1;
            }
            '*' => {
                if rest.starts_with("**") {
                    output.push_str(".*");
                    cursor += 2;
                } else {
                    output.push_str("[^/]*");
                    cursor += 1;
                }
            }
            '[' => {
                let Some(end) = find_unescaped(rest, ']') else {
                    output.push_str("\\[");
                    cursor += 1;
                    continue;
                };
                let whole = &rest[..=end];
                let inner = &rest[1..end];
                if inner.contains('/') || inner.is_empty() || inner == "!" {
                    output.push_str(&regex::escape(whole));
                } else {
                    output.push('[');
                    let mut chars = inner.chars().peekable();
                    if chars.peek() == Some(&'!') {
                        output.push('^');
                        chars.next();
                    }
                    for class_ch in chars {
                        if matches!(class_ch, '\\' | '[' | ']' | '-' | '^' | '&' | '~') {
                            output.push('\\');
                        }
                        output.push(class_ch);
                    }
                    output.push(']');
                }
                cursor += whole.len();
            }
            '{' => {
                let Some(end) = matching_brace(rest) else {
                    output.push_str("\\{");
                    cursor += 1;
                    continue;
                };
                let whole = &rest[..=end];
                let inner = &rest[1..end];
                if let Some((start, finish)) = parse_numeric_range(inner) {
                    let capture = numeric_ranges.len() + 1;
                    output.push_str("([+-]?(?:0|[1-9][0-9]*))");
                    numeric_ranges.push((capture, start.min(finish)..=start.max(finish)));
                } else {
                    let alternatives = split_alternatives(inner);
                    if alternatives.len() == 1 {
                        output.push_str(&regex::escape(whole));
                    } else {
                        output.push_str("(?:");
                        for (index, alternative) in alternatives.iter().enumerate() {
                            if index > 0 {
                                output.push('|');
                            }
                            translate_glob(alternative, output, numeric_ranges)?;
                        }
                        output.push(')');
                    }
                }
                cursor += whole.len();
            }
            _ => {
                output.push_str(&regex::escape(&ch.to_string()));
                cursor += ch.len_utf8();
            }
        }
    }
    Ok(())
}

fn find_unescaped(text: &str, needle: char) -> Option<usize> {
    let mut escaped = false;
    for (index, ch) in text.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == needle {
            return Some(index);
        }
    }
    None
}

fn matching_brace(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
        } else if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn split_alternatives(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth = depth.saturating_sub(1);
        } else if ch == ',' && depth == 0 {
            parts.push(&text[start..index]);
            start = index + 1;
        }
    }
    parts.push(&text[start..]);
    parts
}

fn parse_numeric_range(text: &str) -> Option<(i64, i64)> {
    let (start, end) = text.split_once("..")?;
    if end.contains("..") {
        return None;
    }
    let (start, end) = (start.parse().ok()?, end.parse().ok()?);
    (start < end).then_some((start, end))
}

#[derive(Debug)]
pub struct EditorconfigConformsRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
}

impl Rule for EditorconfigConformsRule {
    alint_core::rule_common_impl!();

    fn requires_full_index(&self) -> bool {
        // A policy change can invalidate every unchanged selected file.
        true
    }

    fn supports_expect_matches(&self) -> bool {
        true
    }

    fn scope_matches_any(&self, index: &alint_core::FileIndex) -> bool {
        self.scope
            .candidate_files(index)
            .any(|entry| self.scope.matches(&entry.path, index))
    }

    fn visit_sensitive_paths(
        &self,
        explicit: bool,
        ctx: &Context<'_>,
        visit: &mut dyn FnMut(&Path),
    ) {
        let manifest_sensitive = self.scope.sensitive_manifest_sources().next().is_some();
        for path in self.scope.sensitive_manifest_sources() {
            visit(path);
        }
        if !explicit && !manifest_sensitive {
            return;
        }
        for entry in self.scope.candidate_files(ctx.index) {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            visit(&entry.path);
            for cfg in ancestor_configs(&entry.path) {
                if ctx.index.contains_file(&cfg) {
                    visit(&cfg);
                }
            }
        }
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let selected: Vec<_> = self
            .scope
            .candidate_files(ctx.index)
            .filter(|entry| self.scope.matches(&entry.path, ctx.index))
            .collect();
        let mut policies = HashMap::new();
        let mut failures = BTreeMap::new();
        for entry in &selected {
            // Search from the target outward and stop at the nearest `root =
            // true`. Besides matching EditorConfig discovery semantics, this
            // avoids reading or diagnosing an outer policy that the nearer
            // root deliberately hides.
            for path in ancestor_configs(&entry.path).into_iter().rev() {
                if !ctx.index.contains_file(&path) {
                    continue;
                }
                let state = policies
                    .entry(path)
                    .or_insert_with_key(|path| load_policy(ctx, path, &mut failures));
                match state {
                    PolicyState::Ready(policy) if policy.root => break,
                    PolicyState::Failed => {
                        // This target has no policy verdict. Do not inspect
                        // farther-out files whose visibility depends on the
                        // malformed policy's unavailable `root` value.
                        break;
                    }
                    PolicyState::Ready(_) => {}
                }
            }
        }

        let mut violations = failures.into_values().collect::<Vec<_>>();
        let mut invalid_declarations = HashSet::new();
        for entry in selected {
            let chain = applicable_policy_chain(&entry.path, &policies);
            if chain
                .iter()
                .any(|path| matches!(policies.get(path), Some(PolicyState::Failed)))
            {
                continue;
            }
            let effective = effective_policy(&entry.path, &chain, &policies);
            let bytes = match crate::io::read_capped(&ctx.root.join(&entry.path)) {
                Ok(bytes) => bytes,
                Err(crate::io::ReadCapError::TooLarge(size)) => {
                    violations.push(too_large_file_violation(&entry.path, size));
                    continue;
                }
                Err(crate::io::ReadCapError::Io(error))
                    if error.kind() == std::io::ErrorKind::NotFound =>
                {
                    violations.push(missing_file_violation(&entry.path));
                    continue;
                }
                Err(crate::io::ReadCapError::Io(error)) => {
                    violations.push(unreadable_file_violation(&entry.path, &error));
                    continue;
                }
            };
            self.evaluate_file(
                &entry.path,
                &bytes,
                &effective,
                &mut invalid_declarations,
                &mut violations,
            );
        }
        Ok(violations)
    }
}

impl EditorconfigConformsRule {
    fn evaluate_file(
        &self,
        path: &Path,
        bytes: &[u8],
        policy: &HashMap<String, Declaration>,
        invalid_declarations: &mut HashSet<(PathBuf, usize, String)>,
        violations: &mut Vec<Violation>,
    ) {
        let charset = parse_charset(policy.get("charset"), invalid_declarations, violations);
        self.check_charset(path, bytes, policy.get("charset").zip(charset), violations);

        let Some(text) = decode_text(bytes, charset) else {
            return;
        };
        if crate::io::looks_binary(bytes) && charset.is_none() {
            return;
        }

        self.check_indentation(path, &text, policy, invalid_declarations, violations);
        self.check_line_endings(path, &text, policy, invalid_declarations, violations);
        self.check_trailing_whitespace(path, &text, policy, invalid_declarations, violations);
        self.check_final_newline(path, &text, policy, invalid_declarations, violations);
        self.check_line_width(path, &text, policy, invalid_declarations, violations);
    }

    fn check_charset(
        &self,
        path: &Path,
        bytes: &[u8],
        configured: Option<(&Declaration, Charset)>,
        violations: &mut Vec<Violation>,
    ) {
        let Some((declaration, expected)) = configured else {
            return;
        };
        if charset_matches(bytes, expected) {
            return;
        }
        self.push_finding(
            path,
            SourceRange::new(1, 1, 1, 1),
            "charset",
            declaration,
            format!(
                "file does not use EditorConfig charset `{}`",
                declaration.value
            ),
            violations,
        );
    }

    fn check_indentation(
        &self,
        path: &Path,
        text: &str,
        policy: &HashMap<String, Declaration>,
        invalid_declarations: &mut HashSet<(PathBuf, usize, String)>,
        violations: &mut Vec<Violation>,
    ) {
        let widths = indentation_widths(policy, invalid_declarations, violations);
        let Some(declaration) = policy.get("indent_style") else {
            return;
        };
        let (expected, detail) = match declaration.value.to_ascii_lowercase().as_str() {
            "tab" => (IndentPolicy::Tabs(widths.tab_width), "tab"),
            "space" => (IndentPolicy::Spaces(widths.indent_size), "space"),
            _ => {
                invalid_policy_value(
                    "indent_style",
                    declaration,
                    "expected `tab`, `space`, or `unset`",
                    invalid_declarations,
                    violations,
                );
                return;
            }
        };
        let Some((line, start_column, end_column)) = first_bad_indent(text, expected) else {
            return;
        };
        self.push_finding(
            path,
            SourceRange::new(line, start_column, line, end_column),
            "indent_style",
            declaration,
            format!("line {line} does not use EditorConfig {detail} indentation"),
            violations,
        );
    }

    fn check_line_endings(
        &self,
        path: &Path,
        text: &str,
        policy: &HashMap<String, Declaration>,
        invalid_declarations: &mut HashSet<(PathBuf, usize, String)>,
        violations: &mut Vec<Violation>,
    ) {
        let Some(declaration) = policy.get("end_of_line") else {
            return;
        };
        let expected = match declaration.value.to_ascii_lowercase().as_str() {
            "lf" => LineEnding::Lf,
            "crlf" => LineEnding::Crlf,
            "cr" => LineEnding::Cr,
            _ => {
                invalid_policy_value(
                    "end_of_line",
                    declaration,
                    "expected `lf`, `crlf`, `cr`, or `unset`",
                    invalid_declarations,
                    violations,
                );
                return;
            }
        };
        let Some(range) = first_bad_line_ending(text, expected) else {
            return;
        };
        let line = range.start_line();
        self.push_finding(
            path,
            Some(range),
            "end_of_line",
            declaration,
            format!(
                "line {line} does not use EditorConfig `{}` endings",
                declaration.value
            ),
            violations,
        );
    }

    fn check_trailing_whitespace(
        &self,
        path: &Path,
        text: &str,
        policy: &HashMap<String, Declaration>,
        invalid_declarations: &mut HashSet<(PathBuf, usize, String)>,
        violations: &mut Vec<Violation>,
    ) {
        let Some(declaration) = policy.get("trim_trailing_whitespace") else {
            return;
        };
        match parse_bool(&declaration.value) {
            Some(true) => {
                let Some((line, start_column, end_column)) = first_trailing_whitespace(text) else {
                    return;
                };
                self.push_finding(
                    path,
                    SourceRange::new(line, start_column, line, end_column),
                    "trim_trailing_whitespace",
                    declaration,
                    format!("line {line} has trailing whitespace"),
                    violations,
                );
            }
            Some(false) => {}
            None => invalid_policy_value(
                "trim_trailing_whitespace",
                declaration,
                "expected `true`, `false`, or `unset`",
                invalid_declarations,
                violations,
            ),
        }
    }

    fn check_final_newline(
        &self,
        path: &Path,
        text: &str,
        policy: &HashMap<String, Declaration>,
        invalid_declarations: &mut HashSet<(PathBuf, usize, String)>,
        violations: &mut Vec<Violation>,
    ) {
        let Some(declaration) = policy.get("insert_final_newline") else {
            return;
        };
        let Some(required) = parse_bool(&declaration.value) else {
            invalid_policy_value(
                "insert_final_newline",
                declaration,
                "expected `true`, `false`, or `unset`",
                invalid_declarations,
                violations,
            );
            return;
        };
        let has_newline = text.ends_with(['\n', '\r']);
        if text.is_empty() || required == has_newline {
            return;
        }
        let detail = if required {
            "file does not end with a newline"
        } else {
            "file ends with a newline but EditorConfig forbids one"
        };
        self.push_finding(
            path,
            final_newline_range(text),
            "insert_final_newline",
            declaration,
            detail.into(),
            violations,
        );
    }

    fn check_line_width(
        &self,
        path: &Path,
        text: &str,
        policy: &HashMap<String, Declaration>,
        invalid_declarations: &mut HashSet<(PathBuf, usize, String)>,
        violations: &mut Vec<Violation>,
    ) {
        let Some(declaration) = policy.get("max_line_length") else {
            return;
        };
        if declaration.value.eq_ignore_ascii_case("off") {
            return;
        }
        let Ok(maximum) = declaration.value.parse::<usize>() else {
            invalid_policy_value(
                "max_line_length",
                declaration,
                "expected a positive integer, `off`, or `unset`",
                invalid_declarations,
                violations,
            );
            return;
        };
        if maximum == 0 {
            invalid_policy_value(
                "max_line_length",
                declaration,
                "expected a positive integer, `off`, or `unset`",
                invalid_declarations,
                violations,
            );
            return;
        }
        let Some((line, width)) = first_overlong_line(text, maximum) else {
            return;
        };
        self.push_finding(
            path,
            SourceRange::new(line, maximum + 1, line, width + 1),
            "max_line_length",
            declaration,
            format!("line {line} is {width} chars wide; EditorConfig max is {maximum}"),
            violations,
        );
    }

    fn push_finding(
        &self,
        path: &Path,
        range: Option<SourceRange>,
        property: &str,
        declaration: &Declaration,
        default_message: String,
        violations: &mut Vec<Violation>,
    ) {
        let message = self.message.clone().unwrap_or(default_message);
        let mut finding = Violation::new(message)
            .with_path(Arc::<Path>::from(path))
            .with_baseline_key(format!(
                "{property}\0{}\0{}",
                crate::slash(&declaration.path),
                declaration.line
            ));
        if let Some(range) = range {
            finding = finding.with_range(range);
        }
        let policy_range = SourceRange::new(
            declaration.line,
            1,
            declaration.line,
            declaration.end_column,
        );
        let mut related = RelatedLocation::new(
            declaration.path.clone(),
            format!("EditorConfig `{property}` policy"),
        );
        if let Some(range) = policy_range {
            related = related.with_range(range);
        }
        violations.push(finding.with_related_location(related));
    }
}

fn ancestor_configs(path: &Path) -> Vec<PathBuf> {
    let mut configs = Vec::new();
    let mut directory = path.parent();
    while let Some(current) = directory {
        configs.push(current.join(".editorconfig"));
        if current.as_os_str().is_empty() {
            break;
        }
        directory = current.parent();
    }
    configs.reverse();
    configs
}

fn load_policy(
    ctx: &Context<'_>,
    path: &Path,
    failures: &mut BTreeMap<PathBuf, Violation>,
) -> PolicyState {
    let failure = match crate::io::read_capped(&ctx.root.join(path)) {
        Ok(bytes) => {
            let Ok(text) = std::str::from_utf8(&bytes) else {
                failures.insert(
                    path.to_path_buf(),
                    policy_parse_failure(path, 1, "EditorConfig file is not valid UTF-8"),
                );
                return PolicyState::Failed;
            };
            return match parse_policy(path, text) {
                Ok(policy) => PolicyState::Ready(policy),
                Err((line, message)) => {
                    failures.insert(
                        path.to_path_buf(),
                        policy_parse_failure(path, line, message),
                    );
                    PolicyState::Failed
                }
            };
        }
        Err(crate::io::ReadCapError::TooLarge(size)) => too_large_file_violation(path, size),
        Err(crate::io::ReadCapError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            missing_file_violation(path)
        }
        Err(crate::io::ReadCapError::Io(error)) => unreadable_file_violation(path, &error),
    };
    failures.insert(path.to_path_buf(), failure);
    PolicyState::Failed
}

fn parse_policy(path: &Path, text: &str) -> std::result::Result<PolicyDocument, (usize, String)> {
    let bytes = text.as_bytes();
    let mut line = 1;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\r' && bytes.get(index + 1) != Some(&b'\n') {
            return Err((
                line,
                "EditorConfig files must use LF or CRLF line separators".into(),
            ));
        }
        if *byte == b'\n' {
            line += 1;
        }
    }
    let directory = path.parent().unwrap_or_else(|| Path::new(""));
    let mut root = false;
    let mut sections = Vec::new();
    let mut current: Option<Section> = None;
    for (line_index, original) in text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .lines()
        .enumerate()
    {
        let line_number = line_index + 1;
        let line = original.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if line.starts_with('[') {
            let Some(pattern) = line
                .strip_prefix('[')
                .and_then(|value| value.strip_suffix(']'))
            else {
                return Err((line_number, "malformed EditorConfig section header".into()));
            };
            if let Some(section) = current.take() {
                sections.push(section);
            }
            let glob = EditorConfigGlob::new(directory, pattern)
                .map_err(|message| (line_number, message))?;
            current = Some(Section {
                glob,
                declarations: Vec::new(),
            });
            continue;
        }
        let Some((key, value)) = split_property(line) else {
            return Err((
                line_number,
                "malformed EditorConfig property declaration".into(),
            ));
        };
        let key = key.to_ascii_lowercase();
        if current.is_none() {
            if key == "root" {
                root = parse_bool(value).ok_or_else(|| {
                    (
                        line_number,
                        "EditorConfig `root` must be `true` or `false`".into(),
                    )
                })?;
            }
            continue;
        }
        if !is_supported_property(&key) {
            continue;
        }
        let declaration = Declaration {
            value: value.to_string(),
            path: path.to_path_buf(),
            line: line_number,
            end_column: original.chars().count() + 1,
        };
        current
            .as_mut()
            .expect("section checked above")
            .declarations
            .push((key, declaration));
    }
    if let Some(section) = current {
        sections.push(section);
    }
    Ok(PolicyDocument { root, sections })
}

fn split_property(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    let value = value.trim();
    (!key.is_empty()).then_some((key, value))
}

fn is_supported_property(key: &str) -> bool {
    matches!(
        key,
        "indent_style"
            | "indent_size"
            | "tab_width"
            | "end_of_line"
            | "charset"
            | "trim_trailing_whitespace"
            | "insert_final_newline"
            | "max_line_length"
    )
}

fn applicable_policy_chain(path: &Path, policies: &HashMap<PathBuf, PolicyState>) -> Vec<PathBuf> {
    let mut chain = Vec::new();
    for candidate in ancestor_configs(path).into_iter().rev() {
        let Some(state) = policies.get(&candidate) else {
            continue;
        };
        let stop = matches!(state, PolicyState::Ready(policy) if policy.root)
            || matches!(state, PolicyState::Failed);
        chain.push(candidate);
        if stop {
            break;
        }
    }
    chain.reverse();
    chain
}

fn effective_policy(
    target: &Path,
    chain: &[PathBuf],
    policies: &HashMap<PathBuf, PolicyState>,
) -> HashMap<String, Declaration> {
    let mut effective = HashMap::new();
    for path in chain {
        let Some(PolicyState::Ready(policy)) = policies.get(path) else {
            continue;
        };
        for section in &policy.sections {
            if !section.glob.is_match(target) {
                continue;
            }
            for (key, declaration) in &section.declarations {
                if declaration.value.eq_ignore_ascii_case("unset") {
                    effective.remove(key);
                } else {
                    effective.insert(key.clone(), declaration.clone());
                }
            }
        }
    }
    effective
}

fn policy_parse_failure(path: &Path, line: usize, message: impl Into<String>) -> Violation {
    Violation::new(message.into())
        .with_path(path.to_path_buf())
        .with_location(line, 1)
        .as_analysis_failure(AnalysisFailureKind::ResourceParse)
}

fn invalid_policy_value(
    property: &str,
    declaration: &Declaration,
    expected: &str,
    seen: &mut HashSet<(PathBuf, usize, String)>,
    violations: &mut Vec<Violation>,
) {
    let identity = (
        declaration.path.clone(),
        declaration.line,
        property.to_string(),
    );
    if seen.insert(identity) {
        violations.push(policy_parse_failure(
            &declaration.path,
            declaration.line,
            format!(
                "unsupported EditorConfig `{property}` value `{}`; {expected}",
                declaration.value
            ),
        ));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Charset {
    Latin1,
    Utf8,
    Utf8Bom,
    Utf16Be,
    Utf16Le,
}

fn parse_charset(
    declaration: Option<&Declaration>,
    seen: &mut HashSet<(PathBuf, usize, String)>,
    violations: &mut Vec<Violation>,
) -> Option<Charset> {
    let declaration = declaration?;
    let charset = match declaration.value.to_ascii_lowercase().as_str() {
        "latin1" => Some(Charset::Latin1),
        "utf-8" => Some(Charset::Utf8),
        "utf-8-bom" => Some(Charset::Utf8Bom),
        "utf-16be" => Some(Charset::Utf16Be),
        "utf-16le" => Some(Charset::Utf16Le),
        _ => None,
    };
    if charset.is_none() {
        invalid_policy_value(
            "charset",
            declaration,
            "expected `latin1`, `utf-8`, `utf-8-bom`, `utf-16be`, `utf-16le`, or `unset`",
            seen,
            violations,
        );
    }
    charset
}

fn charset_matches(bytes: &[u8], charset: Charset) -> bool {
    match charset {
        Charset::Latin1 => {
            !bytes.starts_with(b"\xef\xbb\xbf")
                && !bytes.starts_with(b"\xff\xfe")
                && !bytes.starts_with(b"\xfe\xff")
        }
        Charset::Utf8 => !bytes.starts_with(b"\xef\xbb\xbf") && std::str::from_utf8(bytes).is_ok(),
        Charset::Utf8Bom => {
            bytes.starts_with(b"\xef\xbb\xbf")
                && !bytes[3..].starts_with(b"\xef\xbb\xbf")
                && std::str::from_utf8(&bytes[3..]).is_ok()
        }
        Charset::Utf16Be => valid_utf16(bytes, true),
        Charset::Utf16Le => valid_utf16(bytes, false),
    }
}

fn valid_utf16(bytes: &[u8], big_endian: bool) -> bool {
    let bom = if big_endian { b"\xfe\xff" } else { b"\xff\xfe" };
    if !bytes.starts_with(bom) || !(bytes.len() - 2).is_multiple_of(2) {
        return false;
    }
    let words = bytes[2..].as_chunks::<2>().0.iter().map(|pair| {
        if big_endian {
            u16::from_be_bytes(*pair)
        } else {
            u16::from_le_bytes(*pair)
        }
    });
    char::decode_utf16(words).all(|item| item.is_ok())
}

fn decode_text(bytes: &[u8], charset: Option<Charset>) -> Option<std::borrow::Cow<'_, str>> {
    match charset {
        Some(Charset::Latin1) => Some(std::borrow::Cow::Owned(
            bytes.iter().map(|byte| char::from(*byte)).collect(),
        )),
        Some(Charset::Utf8Bom) if bytes.starts_with(b"\xef\xbb\xbf") => {
            std::str::from_utf8(&bytes[3..])
                .ok()
                .map(std::borrow::Cow::Borrowed)
        }
        Some(Charset::Utf16Be) if valid_utf16(bytes, true) => {
            Some(std::borrow::Cow::Owned(decode_utf16(bytes, true)))
        }
        Some(Charset::Utf16Le) if valid_utf16(bytes, false) => {
            Some(std::borrow::Cow::Owned(decode_utf16(bytes, false)))
        }
        Some(Charset::Utf8) | None => std::str::from_utf8(bytes)
            .ok()
            .map(|text| std::borrow::Cow::Borrowed(crate::io::without_leading_bom(text))),
        _ => None,
    }
}

fn decode_utf16(bytes: &[u8], big_endian: bool) -> String {
    let words = bytes[2..].as_chunks::<2>().0.iter().map(|pair| {
        if big_endian {
            u16::from_be_bytes(*pair)
        } else {
            u16::from_le_bytes(*pair)
        }
    });
    char::decode_utf16(words)
        .map(|item| item.expect("validated UTF-16"))
        .collect()
}

fn parse_bool(value: &str) -> Option<bool> {
    if value.eq_ignore_ascii_case("true") {
        Some(true)
    } else if value.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct IndentationWidths {
    indent_size: Option<usize>,
    tab_width: Option<usize>,
}

fn indentation_widths(
    policy: &HashMap<String, Declaration>,
    seen: &mut HashSet<(PathBuf, usize, String)>,
    violations: &mut Vec<Violation>,
) -> IndentationWidths {
    let explicit_tab_width = policy
        .get("tab_width")
        .and_then(|declaration| parse_positive("tab_width", declaration, seen, violations));
    let indent_size = policy.get("indent_size").and_then(|declaration| {
        if declaration.value.eq_ignore_ascii_case("tab") {
            explicit_tab_width
        } else {
            parse_positive("indent_size", declaration, seen, violations)
        }
    });
    IndentationWidths {
        indent_size,
        tab_width: explicit_tab_width.or(indent_size),
    }
}

fn parse_positive(
    property: &str,
    declaration: &Declaration,
    seen: &mut HashSet<(PathBuf, usize, String)>,
    violations: &mut Vec<Violation>,
) -> Option<usize> {
    match declaration.value.parse::<usize>() {
        Ok(value) if value > 0 => Some(value),
        _ => {
            invalid_policy_value(
                property,
                declaration,
                "expected a positive integer or `unset`",
                seen,
                violations,
            );
            None
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum IndentPolicy {
    Tabs(Option<usize>),
    Spaces(Option<usize>),
}

fn first_bad_tab_indent(lead: &str, width: Option<usize>) -> Option<(usize, usize)> {
    let mut first_space = None;
    for (offset, byte) in lead.bytes().enumerate() {
        match byte {
            b' ' => {
                first_space.get_or_insert(offset);
            }
            b'\t' if first_space.is_some() => {
                let column = first_space.expect("space position recorded") + 1;
                return Some((column, column + 1));
            }
            b'\t' => {}
            _ => unreachable!("indent prefix contains only spaces and tabs"),
        }
    }
    let first_space = first_space?;
    width
        .filter(|width| lead.len() - first_space >= *width)
        .map(|_| (first_space + 1, lead.len() + 1))
}

fn first_bad_indent(text: &str, policy: IndentPolicy) -> Option<(usize, usize, usize)> {
    for (index, (_, line)) in crate::io::physical_lines(text).enumerate() {
        let lead = line
            .char_indices()
            .find(|(_, ch)| *ch != ' ' && *ch != '\t')
            .map_or(line, |(offset, _)| &line[..offset]);
        if lead.len() == line.len() {
            continue;
        }
        let bad = match policy {
            IndentPolicy::Tabs(width) => first_bad_tab_indent(lead, width),
            IndentPolicy::Spaces(width) => lead.bytes().position(|byte| byte == b'\t').map_or_else(
                || {
                    width
                        .filter(|width| !lead.len().is_multiple_of(*width))
                        .map(|_| (1, lead.len() + 1))
                },
                |offset| {
                    let column = offset + 1;
                    Some((column, column + 1))
                },
            ),
        };
        if let Some((start_column, end_column)) = bad {
            return Some((index + 1, start_column, end_column));
        }
    }
    None
}

#[derive(Debug, Clone, Copy)]
enum LineEnding {
    Lf,
    Crlf,
    Cr,
}

fn first_bad_line_ending(text: &str, expected: LineEnding) -> Option<SourceRange> {
    let bytes = text.as_bytes();
    let mut line = 1usize;
    let mut line_start = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' => {
                let crlf = bytes.get(index + 1) == Some(&b'\n');
                let valid = matches!(expected, LineEnding::Crlf) && crlf
                    || matches!(expected, LineEnding::Cr) && !crlf;
                if !valid {
                    let column = text[line_start..index].chars().count() + 1;
                    return SourceRange::new(line, column, line + 1, 1);
                }
                index += usize::from(crlf);
                line += 1;
                line_start = index + 1;
            }
            b'\n' => {
                if !matches!(expected, LineEnding::Lf) {
                    let column = text[line_start..index].chars().count() + 1;
                    return SourceRange::new(line, column, line + 1, 1);
                }
                line += 1;
                line_start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn first_trailing_whitespace(text: &str) -> Option<(usize, usize, usize)> {
    for (index, (_, line)) in crate::io::physical_lines(text).enumerate() {
        let trimmed = line.trim_end_matches([' ', '\t']);
        if trimmed.len() != line.len() {
            return Some((
                index + 1,
                trimmed.chars().count() + 1,
                line.chars().count() + 1,
            ));
        }
    }
    None
}

fn first_overlong_line(text: &str, maximum: usize) -> Option<(usize, usize)> {
    for (index, (_, line)) in crate::io::physical_lines(text).enumerate() {
        let width = line.chars().count();
        if width > maximum {
            return Some((index + 1, width));
        }
    }
    None
}

fn end_coordinates(text: &str) -> (usize, usize) {
    let point = SourceRange::from_byte_range(text, text.len()..text.len())
        .expect("end of valid UTF-8 is a valid source position");
    (point.start_line(), point.start_column())
}

fn end_point(text: &str) -> Option<SourceRange> {
    let (line, column) = end_coordinates(text);
    SourceRange::new(line, column, line, column)
}

fn final_newline_range(text: &str) -> Option<SourceRange> {
    if let Some(prefix) = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix(['\n', '\r']))
    {
        let (line, column) = end_coordinates(prefix);
        return SourceRange::new(line, column, line + 1, 1);
    }
    end_point(text)
}

pub static META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::FullIndex { path_scope: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::OtherFiles,
    formats: std::borrow::Cow::Borrowed(&[]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[]),
    expect_matches: true,
    trace: alint_core::TraceMeta::SELECTION,
    nested_rules: false,
};

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    if spec.paths.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "editorconfig_conforms requires a `paths` field",
        ));
    }
    Ok(Box::new(EditorconfigConformsRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};

    fn rule() -> Box<dyn Rule> {
        build(&spec_yaml(
            "id: editorconfig\nkind: editorconfig_conforms\npaths: '**/*.txt'\nlevel: error\n",
        ))
        .unwrap()
    }

    #[test]
    fn glob_supports_alternation_ranges_double_star_and_literal_classes() {
        let cases = [
            ("*.{js,ts}", "src/a.ts", true),
            ("/src/**/test?.[ch]", "src/unit/test1.c", true),
            ("file{1..3}.txt", "file2.txt", true),
            ("file{1..3}.txt", "file4.txt", false),
            ("file{3..1}.txt", "file2.txt", false),
            ("[a-c].txt", "a.txt", true),
            ("[a-c].txt", "-.txt", true),
            ("[a-c].txt", "b.txt", false),
            ("[!a-c].txt", "b.txt", true),
            ("[!a-c].txt", "-.txt", false),
            ("[!a-c].txt", "also.txt", false),
            (r"[\]ab].txt", "].txt", true),
            (r"[\]ab].txt", r"\.txt", false),
            (r"[\!a].txt", "!.txt", true),
            (r"[\!a].txt", r"\.txt", false),
            (r"literal\*.txt", "literal*.txt", true),
            (r"literal\*.txt", "literalx.txt", false),
            ("ab[e/]cd.i", "ab[e/]cd.i", true),
            ("ab[e/]cd.i", "nested/ab[e/]cd.i", true),
        ];
        for (pattern, path, expected) in cases {
            let glob = EditorConfigGlob::new(Path::new(""), pattern).unwrap();
            assert_eq!(
                glob.is_match(Path::new(path)),
                expected,
                "{pattern} / {path}"
            );
        }
    }

    #[test]
    fn parser_accepts_valid_preamble_pairs_and_rejects_nonstandard_syntax() {
        let policy = parse_policy(
            Path::new(".editorconfig"),
            "vendor_extension = enabled\nroot = true\n[*.txt]\nmax_line_length = off\n",
        )
        .unwrap();
        assert!(policy.root);
        assert_eq!(policy.sections.len(), 1);

        let colon = parse_policy(
            Path::new(".editorconfig"),
            "root = true\n[*.txt]\nmax_line_length: 80\n",
        )
        .unwrap_err();
        assert_eq!(colon.0, 3);

        let lone_cr = parse_policy(
            Path::new(".editorconfig"),
            "vendor_extension = enabled\rroot = true\n[*.txt]\n",
        )
        .unwrap_err();
        assert_eq!(lone_cr.0, 1);
        assert!(lone_cr.1.contains("LF or CRLF"));
    }

    #[test]
    fn slash_inside_character_class_does_not_anchor_policy_section() {
        let policy = concat!(
            "root = true\n",
            "[ab[e/]cd.txt]\n",
            "insert_final_newline = true\n",
        );
        let (tmp, index) = tempdir_with_files(&[
            (".editorconfig", policy.as_bytes()),
            ("ab[e/]cd.txt", b"root"),
            ("nested/ab[e/]cd.txt", b"nested"),
        ]);

        let mut paths = rule()
            .evaluate(&ctx(tmp.path(), &index))
            .unwrap()
            .into_iter()
            .map(|finding| finding.path.unwrap().to_path_buf())
            .collect::<Vec<_>>();
        paths.sort();
        assert_eq!(
            paths,
            [
                PathBuf::from("ab[e/]cd.txt"),
                PathBuf::from("nested/ab[e/]cd.txt"),
            ]
        );
    }

    #[test]
    fn nearer_root_policy_hides_malformed_or_oversized_outer_policy() {
        let outer = "root = true\n[*.txt\ninsert_final_newline = false\n";
        let nested = "root = true\n[*.txt]\ninsert_final_newline = true\n";
        let (tmp, index) = tempdir_with_files(&[
            (".editorconfig", outer.as_bytes()),
            ("nested/.editorconfig", nested.as_bytes()),
            ("nested/one.txt", b"one\n"),
        ]);

        let violations = rule().evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert!(
            violations.is_empty(),
            "a nearer `root = true` policy must hide malformed outer policy: {violations:#?}"
        );

        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(tmp.path().join(".editorconfig"))
            .unwrap();
        file.set_len(crate::io::MAX_ANALYZE_BYTES + 1).unwrap();
        let violations = rule().evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert!(
            violations.is_empty(),
            "a nearer `root = true` policy must hide oversized outer policy: {violations:#?}"
        );
    }

    #[test]
    fn nested_policy_root_and_unset_follow_editorconfig_precedence() {
        let root_policy = concat!(
            "root = true\n",
            "[*]\n",
            "insert_final_newline = true\n",
            "trim_trailing_whitespace = true\n",
            "[*.txt]\n",
            "max_line_length = 8\n",
        );
        let nested_policy = concat!(
            "root = true\n",
            "[*.txt]\n",
            "trim_trailing_whitespace = unset\n",
            "max_line_length = 20\n",
        );
        let (tmp, index) = tempdir_with_files(&[
            (".editorconfig", root_policy.as_bytes()),
            ("root.txt", b"too long here   "),
            ("sub/.editorconfig", nested_policy.as_bytes()),
            ("sub/ok.txt", b"trailing allowed   \n"),
        ]);
        let violations = rule().evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(violations.len(), 3);
        assert!(
            violations
                .iter()
                .all(|finding| finding.path.as_deref() == Some(Path::new("root.txt")))
        );
        assert!(
            violations
                .iter()
                .all(|finding| !finding.related_locations.is_empty())
        );
    }

    #[test]
    fn supported_properties_fire_with_policy_provenance() {
        let policy = concat!(
            "root = true\n",
            "[*.txt]\n",
            "charset = utf-8\n",
            "indent_style = space\n",
            "indent_size = 2\n",
            "end_of_line = lf\n",
            "trim_trailing_whitespace = true\n",
            "insert_final_newline = true\n",
            "max_line_length = 5\n",
        );
        let (tmp, index) = tempdir_with_files(&[
            (".editorconfig", policy.as_bytes()),
            ("bad.txt", b"\tlong line  \r\n"),
        ]);
        let violations = rule().evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(violations.len(), 4);
        assert!(
            violations
                .iter()
                .all(|finding| { finding.related_locations[0].path == Path::new(".editorconfig") })
        );
        assert!(violations.iter().all(|finding| finding.range.is_some()));
    }

    #[test]
    fn cr_files_use_physical_lines_for_all_text_checks() {
        let policy = concat!(
            "root = true\n",
            "[trim.txt]\n",
            "end_of_line = cr\n",
            "trim_trailing_whitespace = true\n",
            "[indent.txt]\n",
            "end_of_line = cr\n",
            "indent_style = space\n",
            "[width.txt]\n",
            "end_of_line = cr\n",
            "max_line_length = 3\n",
        );
        let (tmp, index) = tempdir_with_files(&[
            (".editorconfig", policy.as_bytes()),
            ("trim.txt", b"bad \rok\r"),
            ("indent.txt", b"ok\r\tbad\r"),
            ("width.txt", b"abc\rdef\r"),
        ]);
        let violations = rule().evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(violations.len(), 2);

        let trim = violations
            .iter()
            .find(|finding| finding.path.as_deref() == Some(Path::new("trim.txt")))
            .unwrap();
        let trim_range = trim.range.as_ref().unwrap();
        assert_eq!(
            (
                trim_range.start_line(),
                trim_range.start_column(),
                trim_range.end_line(),
                trim_range.end_column(),
            ),
            (1, 4, 1, 5)
        );

        let indent = violations
            .iter()
            .find(|finding| finding.path.as_deref() == Some(Path::new("indent.txt")))
            .unwrap();
        let indent_range = indent.range.as_ref().unwrap();
        assert_eq!(
            (
                indent_range.start_line(),
                indent_range.start_column(),
                indent_range.end_line(),
                indent_range.end_column(),
            ),
            (2, 1, 2, 2)
        );
    }

    #[test]
    fn tab_indentation_allows_only_a_sub_tab_width_space_remainder() {
        let policy = concat!(
            "root = true\n",
            "[*.txt]\n",
            "indent_style = tab\n",
            "indent_size = 4\n",
            "tab_width = 4\n",
        );
        let (tmp, index) = tempdir_with_files(&[
            (".editorconfig", policy.as_bytes()),
            ("good.txt", b"\t  aligned\n"),
            ("space-before-tab.txt", b"  \tbad\n"),
            ("full-space-tab.txt", b"\t    bad\n"),
        ]);
        let violations = rule().evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(violations.len(), 2);
        assert!(
            violations
                .iter()
                .all(|finding| finding.path.as_deref() != Some(Path::new("good.txt")))
        );
    }

    #[test]
    fn charset_checks_utf8_bom_and_utf16() {
        assert!(charset_matches(b"hello", Charset::Utf8));
        assert!(!charset_matches(b"\xef\xbb\xbfhello", Charset::Utf8));
        assert!(charset_matches(b"\xef\xbb\xbfhello", Charset::Utf8Bom));
        assert!(!charset_matches(
            b"\xef\xbb\xbf\xef\xbb\xbfhello",
            Charset::Utf8Bom
        ));
        assert!(charset_matches(b"\xff\xfeh\0i\0", Charset::Utf16Le));
        assert!(!charset_matches(b"\xfe\xff\xd8\0", Charset::Utf16Be));
    }

    #[test]
    fn invalid_supported_value_is_one_parse_failure() {
        let policy = "root = true\n[*.txt]\nend_of_line = platform\n";
        let (tmp, index) = tempdir_with_files(&[
            (".editorconfig", policy.as_bytes()),
            ("one.txt", b"one\n"),
            ("two.txt", b"two\n"),
        ]);
        let violations = rule().evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceParse)
        );
        assert_eq!(
            violations[0].path.as_deref(),
            Some(Path::new(".editorconfig"))
        );
    }

    #[test]
    fn invalid_indentation_values_fail_even_without_indent_style() {
        let policy = "root = true\n[*.txt]\ntab_width = wide\n";
        let (tmp, index) =
            tempdir_with_files(&[(".editorconfig", policy.as_bytes()), ("one.txt", b"one\n")]);
        let violations = rule().evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceParse)
        );
    }

    #[test]
    fn malformed_and_over_limit_policy_files_fail_loud() {
        let malformed = "root = true\n[*.txt\nindent_style = space\n";
        let (tmp, index) = tempdir_with_files(&[
            (".editorconfig", malformed.as_bytes()),
            ("one.txt", b"one\n"),
        ]);
        let violations = rule().evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceParse)
        );

        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(tmp.path().join(".editorconfig"))
            .unwrap();
        file.set_len(crate::io::MAX_ANALYZE_BYTES + 1).unwrap();
        let violations = rule().evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceRead)
        );
    }
}
