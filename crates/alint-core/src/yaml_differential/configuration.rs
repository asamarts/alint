//! Generated configurations: `version:`, the top-level settings and `rules:`
//! with real rule kinds and options, so that the typed decode into `Config`
//! and the decode of each rule into `RuleSpec` run on many inputs. Their
//! scalars come in every style, with core tags now and then, and each value
//! has the wrong type or an invalid level now and then, so few whole
//! configurations are valid: at seed 1, both parsers decode 5% of them into
//! `Config` and 44% of their rules into `RuleSpec`, to the same value, and
//! reject most of the rest at the same place. The bounded run's floors keep
//! these shares from falling.

use std::fmt::Write as _;

use super::generate::Rng;

const KINDS: &[&str] = &[
    "file_exists",
    "file_absent",
    "dir_exists",
    "dir_absent",
    "file_header",
    "file_footer",
    "file_max_size",
    "file_is_ascii",
    "filename_case",
    "line_endings",
    "indent_style",
    "command",
    "pair",
    "for_each_dir",
    "key_parity",
    "file_content_matches",
];

const LEVELS: &[&str] = &[
    "error",
    "error",
    "warning",
    "info",
    "off",
    "Error",
    "'error'",
    "\"warning\"",
    "!!str info",
    "~",
    "",
    "1",
];

const BOOLEANS: &[&str] = &[
    "true",
    "false",
    "true",
    "True",
    "FALSE",
    "yes",
    "on",
    "'true'",
    "!!bool true",
    "!!str true",
    "~",
    "1",
];

const VERSIONS: &[&str] = &[
    "1",
    "1",
    "1",
    "1",
    "'1'",
    "\"1\"",
    "!!int 1",
    "!!str 1",
    "!!float 1",
    "!!int '1'",
    "1.0",
    "0x1",
    "+1",
    "~",
    "",
    "2",
    "-1",
    "true",
];

const SIZES: &[&str] = &[
    "1048576",
    "null",
    "~",
    "0",
    "-1",
    "1_000",
    "0x10",
    "'1'",
    "!!int '1'",
    "1e3",
    "!!float 1",
    "!!str 10",
];

const GLOBS: &[&str] = &[
    "src/**",
    "'**/*.rs'",
    "\"docs/**/*.md\"",
    "Cargo.toml",
    "'!target/**'",
    "README.md",
    "!!str a/**",
];

/// A value written in one of several styles, its text unchanged when plain.
fn styled(rng: &mut Rng, text: &str) -> String {
    match rng.below(12) {
        0..=6 => text.to_string(),
        7 => format!("'{text}'"),
        8 => format!("\"{text}\""),
        9 => format!("!!str {text}"),
        10 => format!("&s{} {text}", rng.below(4)),
        _ => format!("|-\n      {text}"),
    }
}

/// A list of `items`, in block style at `indent` or in flow style.
fn list(rng: &mut Rng, out: &mut String, indent: usize, items: &[String]) {
    if rng.chance(1, 2) {
        let _ = writeln!(out, " [{}]", items.join(", "));
    } else {
        out.push('\n');
        for item in items {
            let _ = writeln!(out, "{:indent$}- {item}", "");
        }
    }
}

fn paths(rng: &mut Rng, out: &mut String) {
    out.push_str("    paths:");
    match rng.below(4) {
        0 => {
            let glob = rng.pick(GLOBS);
            let _ = writeln!(out, " {glob}");
        }
        1 | 2 => {
            let globs: Vec<String> = (0..=rng.below(3))
                .map(|_| rng.pick(GLOBS).to_string())
                .collect();
            list(rng, out, 6, &globs);
        }
        _ => {
            let include = rng.pick(GLOBS);
            let exclude = rng.pick(GLOBS);
            if rng.chance(1, 2) {
                let _ = writeln!(out, " {{include: {include}, exclude: [{exclude}]}}");
            } else {
                let _ = writeln!(
                    out,
                    "\n      include: {include}\n      exclude:\n        - {exclude}"
                );
            }
        }
    }
}

/// The options of a rule of `kind`, which land in `RuleSpec::extra`.
fn options(rng: &mut Rng, out: &mut String, kind: &str) {
    let option = match kind {
        "file_header" | "file_footer" | "file_content_matches" => {
            let pattern = rng.pick(&["'^// SPDX'", "\"^#!\"", "'(?m)^\\s+$'", "~", "1", "[a, b]"]);
            format!("pattern: {pattern}")
        }
        "file_max_size" => format!("max_bytes: {}", rng.pick(SIZES)),
        "filename_case" => format!("case: {}", rng.pick(&["snake", "kebab", "'pascal'", "1"])),
        "line_endings" => format!("target: {}", rng.pick(&["lf", "crlf", "!!str lf"])),
        "indent_style" => format!("style: {}", rng.pick(&["spaces", "tabs", "~"])),
        "command" => format!("command: [{}]", rng.pick(&["echo, hi", "'true'", ""])),
        "pair" => format!(
            "primary: {}\n    partner: {}",
            rng.pick(GLOBS),
            rng.pick(&["'{dir}/b'", "\"{stem}.h\""])
        ),
        "for_each_dir" => "select: 'crates/*'\n    require:\n      - kind: file_exists\n        \
                           paths: '{path}/Cargo.toml'"
            .to_string(),
        "key_parity" => format!("files: {}\n    format: json", rng.pick(GLOBS)),
        _ => format!("git_tracked_only: {}", rng.pick(BOOLEANS)),
    };
    let _ = writeln!(out, "    {option}");
}

fn rule(rng: &mut Rng, out: &mut String, index: usize, anchored: &mut Vec<usize>) {
    out.push_str("  -");
    if !anchored.is_empty() && rng.chance(1, 5) {
        let base = anchored[rng.below(anchored.len())];
        let _ = writeln!(out, " <<: *rule{base}");
        out.push_str("   ");
    } else if rng.chance(1, 5) {
        anchored.push(index);
        let _ = writeln!(out, " &rule{index}");
        out.push_str("   ");
    }
    let id = styled(rng, &format!("r{index}"));
    let id = if rng.chance(1, 20) {
        rng.pick(&["1", "~", "[a]", "true"]).to_string()
    } else {
        id
    };
    let _ = writeln!(out, " id: {id}");
    let kind = rng.pick(KINDS);
    let _ = writeln!(out, "    kind: {}", styled(rng, kind));
    let _ = writeln!(out, "    level: {}", rng.pick(LEVELS));
    if rng.chance(3, 4) {
        paths(rng, out);
    }
    if rng.chance(1, 2) {
        let message = rng.pick(&[
            "keep it tidy",
            "'{{vars.team}} owns {{path}}'",
            "\"missing\\tfile\"",
            "|\n      two\n      lines",
            ">-\n      folded\n      text",
            "1",
            "~",
        ]);
        let _ = writeln!(out, "    message: {message}");
    }
    for (field, values) in [
        ("when", &["facts.is_rust", "'not facts.x'", "1"][..]),
        ("policy_url", &["https://x/y", "~"][..]),
        ("expect_matches", BOOLEANS),
        ("sensitive", BOOLEANS),
    ] {
        if rng.chance(1, 6) {
            let _ = writeln!(out, "    {field}: {}", rng.pick(values));
        }
    }
    if rng.chance(1, 6) {
        let fix = rng.pick(&[
            "{file_create: {content: \"x\\n\"}}",
            "\n      file_remove: {}",
            "{file_append: {content: '# end'}}",
            "{file_create: {}, file_remove: {}}",
            "~",
        ]);
        let _ = writeln!(out, "    fix: {fix}");
    }
    if rng.chance(1, 8) {
        let filter = rng.pick(&["{has_ancestor: Cargo.toml}", "{has_ancestor: [a, b]}", "1"]);
        let _ = writeln!(out, "    scope_filter: {filter}");
    }
    options(rng, out, kind);
}

/// A configuration document.
pub(super) fn configuration(rng: &mut Rng) -> String {
    let mut out = String::new();
    if rng.chance(1, 6) {
        out.push_str(rng.pick(&["---\n", "# alint\n", "%YAML 1.2\n---\n"]));
    }
    if !rng.chance(1, 12) {
        let _ = writeln!(out, "version: {}", rng.pick(VERSIONS));
    }
    if rng.chance(1, 4) {
        out.push_str("ignore:");
        let globs: Vec<String> = (0..=rng.below(2))
            .map(|_| rng.pick(GLOBS).to_string())
            .collect();
        list(rng, &mut out, 2, &globs);
    }
    if rng.chance(1, 4) {
        let _ = writeln!(out, "respect_gitignore: {}", rng.pick(BOOLEANS));
    }
    if rng.chance(1, 5) {
        let value = rng.pick(&["platform", "'1'", "1", "true", "~", "!!str 1", "[x]"]);
        let _ = writeln!(out, "vars:\n  team: {value}\n  other: x");
    }
    if rng.chance(1, 5) {
        let _ = writeln!(out, "fix_size_limit: {}", rng.pick(SIZES));
    }
    if rng.chance(1, 8) {
        let _ = writeln!(out, "nested_configs: {}", rng.pick(BOOLEANS));
    }
    if rng.chance(1, 6) {
        let entry = rng.pick(&[
            "alint://bundled/oss-baseline@v1",
            "{url: ./base.yml, only: [a]}",
            "{url: ./base.yml, except: b}",
            "1",
        ]);
        let _ = writeln!(out, "extends:\n  - {entry}");
    }
    if rng.chance(1, 8) {
        let fact = rng.pick(&[
            "{id: is_rust, any_file_exists: [Cargo.toml]}",
            "{id: n, count_files: '**/*.rs'}",
            "{id: x}",
        ]);
        let _ = writeln!(out, "facts:\n  - {fact}");
    }
    out.push_str("rules:\n");
    let mut anchored = Vec::new();
    for index in 0..=rng.below(4) {
        rule(rng, &mut out, index, &mut anchored);
    }
    out
}
