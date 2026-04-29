//! CLI flag inventory snapshot.
//!
//! Each subcommand's flag list (long names, short names, value
//! type, required-ness) gets captured into a structured snapshot
//! so flag-name *renames* and *removals* surface independently of
//! the help-text snapshots in `tests/cli/help-*.toml`. Help-text
//! snapshots are byte-for-byte; they catch rewording too. This
//! test is the structural complement: it diffs the *shape* of
//! the CLI surface, ignoring prose.
//!
//! How it works: invokes `alint <sub> --help`, parses the
//! `Options:` section into a sorted list of flag identifiers
//! (`-q, --quiet <BOOL>` style), and asserts against a
//! checked-in snapshot under `tests/snapshots/cli-flags.txt`.
//! Run with `UPDATE_SNAPSHOTS=1` to refresh after an intentional
//! flag change.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;

const SUBCOMMANDS: &[&str] = &[
    "", // top-level
    "check",
    "list",
    "explain",
    "fix",
    "facts",
    "init",
    "suggest",
    "export-agents-md",
];

fn alint_bin() -> PathBuf {
    // Cargo sets CARGO_BIN_EXE_<name> for integration tests.
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("cli-flags.txt")
}

/// Pull the `Options:` section out of `--help` output and reduce
/// it to a sorted list of flag headers (everything up to the
/// first description column). Help-text indentation and prose
/// after the flag varies by clap version; the flag prefix is
/// stable.
fn parse_flags(help: &str) -> Vec<String> {
    let mut in_options = false;
    let mut flags: Vec<String> = Vec::new();
    for line in help.lines() {
        let trimmed_left = line.trim_start();
        if trimmed_left.starts_with("Options:") || trimmed_left.starts_with("Global Options:") {
            in_options = true;
            continue;
        }
        // Section break (e.g. "Commands:", "Arguments:") ends
        // the Options block. clap emits these as zero-indent
        // headers ending in `:`.
        if in_options
            && !line.starts_with(' ')
            && line.trim_end().ends_with(':')
            && !line.is_empty()
        {
            in_options = false;
            continue;
        }
        if !in_options {
            continue;
        }
        // Flag lines start with whitespace + `-`. Description
        // continuations are deeper-indented prose without a
        // leading dash.
        let indent_stripped = line.trim_start();
        if !indent_stripped.starts_with('-') {
            continue;
        }
        // Cut at the description gap (clap renders ≥2 spaces
        // between the flag header and the description). If the
        // line is just a flag header (no description on this
        // line), the whole trimmed line is the header.
        let header = match indent_stripped.find("  ") {
            Some(i) => indent_stripped[..i].trim_end(),
            None => indent_stripped.trim_end(),
        };
        flags.push(header.to_string());
    }
    flags.sort();
    flags
}

fn collect_inventory() -> String {
    let bin = alint_bin();
    let mut out = String::new();
    out.push_str("# CLI flag inventory — see cli_flag_inventory.rs\n");
    out.push_str("# Run with UPDATE_SNAPSHOTS=1 to refresh after intentional flag changes.\n\n");
    for &sub in SUBCOMMANDS {
        let mut cmd = Command::new(&bin);
        if !sub.is_empty() {
            cmd.arg(sub);
        }
        cmd.arg("--help");
        let out_bytes = cmd
            .output()
            .unwrap_or_else(|e| panic!("spawn alint {sub} --help: {e}"))
            .stdout;
        let help = String::from_utf8(out_bytes)
            .unwrap_or_else(|e| panic!("alint {sub} --help non-UTF-8: {e}"));
        let flags = parse_flags(&help);
        let label = if sub.is_empty() { "<top-level>" } else { sub };
        writeln!(out, "=== {label} ===").unwrap();
        for f in flags {
            writeln!(out, "  {f}").unwrap();
        }
        out.push('\n');
    }
    out
}

#[test]
fn cli_flag_inventory_matches_snapshot() {
    let actual = collect_inventory();
    let path = snapshot_path();

    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).expect("write snapshot");
        eprintln!("wrote {}", path.display());
        return;
    }

    let expected = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");

    if expected != actual {
        // Show the first diverging window so the rename or
        // removal is obvious from the panic message.
        let exp_lines: Vec<&str> = expected.lines().collect();
        let act_lines: Vec<&str> = actual.lines().collect();
        let max = exp_lines.len().max(act_lines.len());
        let mut first = max;
        for i in 0..max {
            if exp_lines.get(i).copied().unwrap_or("")
                != act_lines.get(i).copied().unwrap_or("")
            {
                first = i;
                break;
            }
        }
        let lo = first.saturating_sub(4);
        let hi = (first + 5).min(max);
        let mut window = String::new();
        for i in lo..hi {
            let e = exp_lines.get(i).copied().unwrap_or("<EOF>");
            let a = act_lines.get(i).copied().unwrap_or("<EOF>");
            if e == a {
                writeln!(window, "  {i:>4} | {e}").unwrap();
            } else {
                writeln!(window, "- {i:>4} | {e}").unwrap();
                writeln!(window, "+ {i:>4} | {a}").unwrap();
            }
        }
        panic!(
            "CLI flag inventory drift detected.\n\
             Expected file: {}\n\
             Run with UPDATE_SNAPSHOTS=1 to refresh after an intentional flag change.\n\n\
             First diverging window:\n{window}",
            path.display(),
        );
    }
}
