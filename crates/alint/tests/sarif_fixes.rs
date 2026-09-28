//! End-to-end gate: SARIF `result.fixes[]` for fixable *located* findings.
//!
//! `alint check --format sarif` attaches the concrete edit a fix would make
//! (source region + replacement text) to each fixable finding, so a SARIF
//! consumer (GitHub Code Scanning's "fix" suggestions) can preview/apply it
//! without running `alint fix`. These drive the real binary end-to-end:
//! `engine.run` -> `attach_proposed_edits` -> `write_sarif`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn alint_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn check_sarif(dir: &Path) -> serde_json::Value {
    let out = Command::new(alint_bin())
        .args(["--format", "sarif", "check", "."])
        .current_dir(dir)
        .output()
        .expect("spawn alint");
    // A live finding means a non-zero (findings) exit; stdout carries the SARIF.
    assert_eq!(
        out.status.code(),
        Some(1),
        "expected a findings exit; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid SARIF JSON ({e}):\n{}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

/// Run `alint <extra...> check .` and parse stdout JSON (a live finding -> exit 1).
fn check_json(dir: &Path, extra: &[&str]) -> serde_json::Value {
    let mut args: Vec<&str> = extra.to_vec();
    args.extend(["check", "."]);
    let out = Command::new(alint_bin())
        .args(&args)
        .current_dir(dir)
        .output()
        .expect("spawn alint");
    assert_eq!(
        out.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}):\n{}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

/// 1-based (line, col) -> byte offset in `bytes` (UTF-8). Columns count Unicode
/// scalars; a leading BOM at offset 0 is not a column (matching `byte_to_line_col`).
/// An `end_column` one past a line's last scalar (SARIF's exclusive convention,
/// which for a region ending at a `\n` lands one past that newline on its line)
/// maps to the byte AFTER the newline.
fn region_to_byte(bytes: &[u8], line: usize, col: usize) -> usize {
    let text = std::str::from_utf8(bytes).expect("utf8");
    let (mut cur_line, mut cur_col) = (1usize, 1usize);
    for (idx, ch) in text.char_indices() {
        if cur_line == line && cur_col == col {
            return idx;
        }
        if idx == 0 && ch == '\u{feff}' {
            continue;
        }
        if ch == '\n' {
            if cur_line == line && col == cur_col + 1 {
                return idx + ch.len_utf8(); // one past the newline
            }
            cur_line += 1;
            cur_col = 1;
        } else {
            cur_col += 1;
        }
    }
    bytes.len()
}

/// A `region` JSON object -> its `(start_byte, end_byte)` in `bytes`.
fn region_bytes(bytes: &[u8], r: &serde_json::Value) -> (usize, usize) {
    let u = |k: &str| usize::try_from(r[k].as_u64().unwrap()).unwrap();
    (
        region_to_byte(bytes, u("start_line"), u("start_column")),
        region_to_byte(bytes, u("end_line"), u("end_column")),
    )
}

/// THE fidelity invariant, tested directly: applying every advertised
/// `proposed_edit` (agent format) for `file` to `original` reproduces EXACTLY what
/// `alint fix` writes -- robust to the edit representation (minimal insert vs
/// value-span replace vs composed span). Returns the advertised edit count.
fn assert_fidelity(dir: &Path, file: &str, original: &[u8]) -> usize {
    // What `alint fix` writes (a copy of the same config + file).
    let fixdir = tempfile::tempdir().unwrap();
    std::fs::copy(dir.join(".alint.yml"), fixdir.path().join(".alint.yml")).unwrap();
    std::fs::write(fixdir.path().join(file), original).unwrap();
    let out = Command::new(alint_bin())
        .args(["fix", "."])
        .current_dir(fixdir.path())
        .output()
        .expect("spawn alint fix");
    assert!(matches!(out.status.code(), Some(0 | 1)));
    let fixout = std::fs::read(fixdir.path().join(file)).unwrap();
    // The advertised edits, converted to byte splices.
    let agent = check_json(dir, &["--format", "agent"]);
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for v in agent["violations"].as_array().unwrap() {
        for e in v["proposed_edit"].as_array().into_iter().flatten() {
            if e["path"] == file {
                let (s, en) = region_bytes(original, &e["region"]);
                edits.push((s, en, e["inserted"].as_str().unwrap().to_string()));
            }
        }
    }
    let n = edits.len();
    edits.sort_by_key(|e| std::cmp::Reverse(e.0)); // apply descending so offsets stay valid
    let mut buf = original.to_vec();
    for (s, en, ins) in edits {
        buf.splice(s..en, ins.bytes().collect::<Vec<_>>());
    }
    assert_eq!(
        String::from_utf8_lossy(&buf),
        String::from_utf8_lossy(&fixout),
        "applying advertised edits must reproduce `alint fix` for {file}"
    );
    n
}

/// W3b: the machine formats carry a `proposed_edit` (source region + replacement)
/// -- `agent` ALWAYS, `json` only under `--include-fixes` -- with the same
/// Safe-only edit SARIF advertises. Drives the real binary + the CLI flag gating.
#[test]
fn agent_and_json_carry_proposed_edit_per_the_include_fixes_flag() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: v-pins\n",
            "    kind: json_path_matches\n",
            "    paths: \"**/*.json\"\n",
            "    path: \"$.deps.*\"\n",
            "    matches: \"^v\"\n",
            "    level: error\n",
            "    fix: { replace: { pattern: \"^\", replacement: \"v\", applicability: safe } }\n",
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("app.json"),
        "{\n  \"deps\": {\n    \"bad\": \"2.0\"\n  }\n}\n",
    )
    .unwrap();

    // agent: always-on (mirrors its always-on fix_command).
    let agent = check_json(dir.path(), &["--format", "agent"]);
    assert!(
        !agent["violations"][0]["proposed_edit"].is_null(),
        "agent must carry proposed_edit with no flag"
    );

    // json WITHOUT the flag: no proposed_edit (attach didn't run).
    let plain = check_json(dir.path(), &["--format", "json"]);
    assert!(
        plain["results"][0]["violations"][0]["proposed_edit"].is_null(),
        "json must not carry proposed_edit without --include-fixes"
    );

    // json --include-fixes: present.
    let with = check_json(dir.path(), &["--format", "json", "--include-fixes"]);
    assert!(
        !with["results"][0]["violations"][0]["proposed_edit"].is_null(),
        "json --include-fixes must carry proposed_edit"
    );

    // ...and the advertised edit, applied, reproduces `alint fix`.
    assert_fidelity(
        dir.path(),
        "app.json",
        b"{\n  \"deps\": {\n    \"bad\": \"2.0\"\n  }\n}\n",
    );
}

/// A `replace` fix on a `json_path_matches` rule (a located fixer) renders a
/// `result.fixes[]` whose `deletedRegion` is a 1-based line/column span and
/// whose `insertedContent` is the replacement. Only the *violating* node
/// contributes a replacement — an already-compliant sibling does not.
#[test]
fn sarif_carries_a_located_replace_fix_with_line_column_region() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: v-pins\n",
            "    kind: json_path_matches\n",
            "    paths: \"**/*.json\"\n",
            "    path: \"$.deps.*\"\n",
            "    matches: \"^v\"\n",
            "    level: error\n",
            "    fix:\n",
            "      replace:\n",
            "        pattern: \"^\"\n",
            "        replacement: \"v\"\n",
            "        applicability: safe\n",
        ),
    )
    .unwrap();
    // `ok` already matches `^v`; `bad` does not, on line 4.
    std::fs::write(
        dir.path().join("app.json"),
        "{\n  \"deps\": {\n    \"ok\": \"v9\",\n    \"bad\": \"2.0\"\n  }\n}\n",
    )
    .unwrap();

    let sarif = check_sarif(dir.path());
    let result = &sarif["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "v-pins");

    let changes = &result["fixes"][0]["artifactChanges"];
    assert_eq!(changes[0]["artifactLocation"]["uri"], "app.json");
    let replacements = changes[0]["replacements"].as_array().unwrap();
    // Exactly one replacement: the compliant `ok` node is not rewritten.
    assert_eq!(replacements.len(), 1);
    // The change is a 1-based line/column span on the violating line 4.
    assert_eq!(replacements[0]["deletedRegion"]["startLine"], 4);

    // And the advertised deletedRegion + insertedContent, applied, reproduce
    // `alint fix` (checked via the agent surface, which derives the same edits).
    assert_fidelity(
        dir.path(),
        "app.json",
        b"{\n  \"deps\": {\n    \"ok\": \"v9\",\n    \"bad\": \"2.0\"\n  }\n}\n",
    );
}

/// Fidelity regression (audit 2026-09-20): a `*_path_matches` file with MULTIPLE
/// failing nodes must advertise a fix that, APPLIED, reproduces exactly what
/// `alint fix` writes for every node -- not a subset. (The edit is derived from
/// the engine's composed pass, so it is one minimal span covering all three nodes
/// rather than three separate edits; the aggregate is faithful.)
#[test]
fn machine_surfaces_advertise_every_edit_fix_would_write() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: v-pins\n",
            "    kind: json_path_matches\n",
            "    paths: \"**/*.json\"\n",
            "    path: \"$.deps.*\"\n",
            "    matches: \"^v\"\n",
            "    level: error\n",
            "    fix: { replace: { pattern: \"^\", replacement: \"v\", applicability: safe } }\n",
        ),
    )
    .unwrap();
    // THREE failing nodes, distinct values.
    let original =
        "{\n  \"deps\": {\n    \"a\": \"1.0\",\n    \"b\": \"2.0\",\n    \"c\": \"3.0\"\n  }\n}\n";
    std::fs::write(dir.path().join("app.json"), original).unwrap();

    // The advertised edit(s), applied, reproduce EXACTLY what `alint fix` writes
    // for all three nodes -- the composed change is one minimal span rather than
    // three separate edits, but the aggregate is faithful.
    let n = assert_fidelity(dir.path(), "app.json", original.as_bytes());
    assert!(n >= 1, "a multi-node fixable file must advertise a fix");
    // SARIF carries the same composed fix.
    let sarif = check_sarif(dir.path());
    assert!(
        !sarif["runs"][0]["results"][0]["fixes"].is_null(),
        "SARIF must advertise the fix"
    );
}

/// A whole-file normalizer (`no_trailing_whitespace` -> the
/// `file_trim_trailing_whitespace` fixer, which rewrites the whole file via
/// `SetContent`) renders a *minimal* changed span (not a full-artifact rewrite),
/// so independent same-file fixes compose. Trimming `a   \nb\n` -> `a\nb\n` is a
/// pure deletion of the three spaces on line 1.
#[test]
fn sarif_carries_a_whole_file_fix_as_a_minimal_span() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: no-ws\n",
            "    kind: no_trailing_whitespace\n",
            "    paths: \"**/*.txt\"\n",
            "    level: error\n",
            "    fix:\n",
            "      file_trim_trailing_whitespace: {}\n",
        ),
    )
    .unwrap();
    // Line 1 has three trailing spaces (columns 2-4), deleted.
    std::fs::write(dir.path().join("bad.txt"), "a   \nb\n").unwrap();

    let sarif = check_sarif(dir.path());
    let result = &sarif["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "no-ws");
    let repl = &result["fixes"][0]["artifactChanges"][0]["replacements"][0];
    let region = &repl["deletedRegion"];
    assert_eq!(region["startLine"], 1);
    assert_eq!(region["startColumn"], 2);
    assert_eq!(region["endLine"], 1);
    assert_eq!(region["endColumn"], 5);
    // Pure deletion: no insertedContent.
    assert!(repl["insertedContent"].is_null());
}

/// Fidelity gate (audit HIGH): SARIF must NOT advertise an edit that `alint fix`
/// demotes and never writes. A `remove_value` batch that can only partially
/// remove is refused all-or-nothing (the `Absent` verifier fails), so `fix`
/// writes nothing — and SARIF must therefore carry no `fixes`, not a partial
/// (here: secret-leaking) removal.
#[test]
fn sarif_omits_a_fix_the_pipeline_would_demote() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: no-secret\n",
            "    kind: hcl_path_absent\n",
            "    paths: \"**/*.tf\"\n",
            "    path: \"$..secret\"\n",
            "    level: error\n",
            "    fix: { remove_value: { applicability: safe } }\n",
        ),
    )
    .unwrap();
    // A top-level secret + two block secrets: the block members can't be removed,
    // so the whole batch demotes (all-or-nothing) and `fix` writes nothing.
    std::fs::write(
        dir.path().join("main.tf"),
        "secret = \"top\"\nitem {\n  secret = \"a\"\n}\nitem {\n  secret = \"b\"\n}\n",
    )
    .unwrap();

    let sarif = check_sarif(dir.path());
    let result = &sarif["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "no-secret");
    assert!(
        result["fixes"].is_null(),
        "a demoted (never-written) fix must not be advertised: {result}"
    );
}

/// Fidelity gate (audit HIGH): overlapping candidate edits (nested `$..a`) must
/// not all be advertised (that is invalid SARIF and would corrupt the file).
/// Only the edit the pipeline's overlap-skip keeps is emitted, and applying it
/// reproduces `alint fix`'s result.
#[test]
fn sarif_advertises_only_the_overlap_survivor() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: no-a\n",
            "    kind: xml_path_absent\n",
            "    paths: \"**/*.xml\"\n",
            "    path: \"$..a\"\n",
            "    level: error\n",
            "    fix: { remove_value: { applicability: safe } }\n",
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("n.xml"),
        "<root>\n  <a>\n    <a>x</a>\n  </a>\n</root>\n",
    )
    .unwrap();

    let sarif = check_sarif(dir.path());
    let replacements =
        sarif["runs"][0]["results"][0]["fixes"][0]["artifactChanges"][0]["replacements"]
            .as_array()
            .unwrap();
    // Exactly one edit (the outer <a>), not the two overlapping candidates.
    assert_eq!(replacements.len(), 1);
    let region = &replacements[0]["deletedRegion"];
    assert_eq!(region["startLine"], 2);
    assert_eq!(region["startColumn"], 1);
    assert_eq!(region["endLine"], 4);
    assert_eq!(region["endColumn"], 8);
}

/// A create fixer (`file_exists` -> `file_create`) surfaces a `CreateFile` fix:
/// an empty region at `(1,1)` with the new file's content. The violation is
/// path-less, so the target comes from the fixer's own edit.
#[test]
fn sarif_carries_a_create_file_fix() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: need-license\n",
            "    kind: file_exists\n",
            "    paths: \"LICENSE\"\n",
            "    level: error\n",
            "    fix: { file_create: { content: \"MIT\\n\" } }\n",
        ),
    )
    .unwrap();

    let sarif = check_sarif(dir.path());
    let change = &sarif["runs"][0]["results"][0]["fixes"][0]["artifactChanges"][0];
    assert_eq!(change["artifactLocation"]["uri"], "LICENSE");
    let repl = &change["replacements"][0];
    assert_eq!(repl["deletedRegion"]["startLine"], 1);
    assert_eq!(repl["deletedRegion"]["startColumn"], 1);
    assert_eq!(repl["deletedRegion"]["endLine"], 1);
    assert_eq!(repl["deletedRegion"]["endColumn"], 1);
    assert_eq!(repl["insertedContent"]["text"], "MIT\n");
}

/// A finding whose rule has no fixer carries no `fixes` key — the ordinary
/// SARIF shape is unchanged for non-fixable findings.
#[test]
fn sarif_omits_fixes_for_a_finding_with_no_fixer() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: no-secrets\n",
            "    kind: json_path_absent\n",
            "    paths: \"**/*.json\"\n",
            "    path: \"$.secret\"\n",
            "    level: error\n",
        ),
    )
    .unwrap();
    std::fs::write(dir.path().join("app.json"), "{\n  \"secret\": \"x\"\n}\n").unwrap();

    let sarif = check_sarif(dir.path());
    let result = &sarif["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "no-secrets");
    assert!(
        result["fixes"].is_null(),
        "a no-fixer finding must not carry a fixes key"
    );
}

/// CRITICAL regression (audit 2026-09-20): two located rules whose spans OVERLAP
/// on one file must be DECONFLICTED across rules (as `alint fix`'s per-file batch
/// does), so the surfaces never advertise two overlapping edits that corrupt on
/// apply. `$..a` (remove the outer block) and `$..b` (remove the nested one)
/// overlap; only the surviving outer edit is advertised.
#[test]
fn cross_rule_overlapping_located_edits_are_deconflicted() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: no-a\n",
            "    kind: xml_path_absent\n",
            "    paths: \"**/*.xml\"\n",
            "    path: \"$..a\"\n",
            "    level: error\n",
            "    fix: { remove_value: { applicability: safe } }\n",
            "  - id: no-b\n",
            "    kind: xml_path_absent\n",
            "    paths: \"**/*.xml\"\n",
            "    path: \"$..b\"\n",
            "    level: error\n",
            "    fix: { remove_value: { applicability: safe } }\n",
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("n.xml"),
        "<root>\n  <a>\n    <b>x</b>\n  </a>\n</root>\n",
    )
    .unwrap();

    let sarif = check_sarif(dir.path());
    let repls: Vec<_> = sarif["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|r| r["fixes"].as_array().into_iter().flatten())
        .flat_map(|f| f["artifactChanges"].as_array().into_iter().flatten())
        .flat_map(|c| c["replacements"].as_array().into_iter().flatten())
        .collect();
    assert_eq!(
        repls.len(),
        1,
        "cross-rule overlapping edits must deconflict to the single survivor; got {repls:?}"
    );
    // The survivor removes the OUTER <a> block (lines 2-4), matching `alint fix`.
    assert_eq!(repls[0]["deletedRegion"]["startLine"], 2);
    assert_eq!(repls[0]["deletedRegion"]["endLine"], 4);
}

/// HIGH regression (audit 2026-09-20): whole-file normalizers are advertised
/// COMPOSED (each sees the previous one's output), so `no_trailing_whitespace` +
/// `final_newline` on a last line of trailing spaces with no final newline
/// advertises only the trim -- NOT a spurious `final_newline` edit `fix` skips
/// (after the trim the file already ends in a newline).
#[test]
fn whole_file_normalizers_are_advertised_composed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: no-ws\n",
            "    kind: no_trailing_whitespace\n",
            "    paths: \"**/*.txt\"\n",
            "    level: error\n",
            "    fix: { file_trim_trailing_whitespace: {} }\n",
            "  - id: fnl\n",
            "    kind: final_newline\n",
            "    paths: \"**/*.txt\"\n",
            "    level: error\n",
            "    fix: { file_append_final_newline: {} }\n",
        ),
    )
    .unwrap();
    std::fs::write(dir.path().join("f.txt"), "a\n  ").unwrap();

    let agent = check_json(dir.path(), &["--format", "agent"]);
    let edits: Vec<_> = agent["violations"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|v| v["proposed_edit"].as_array().into_iter().flatten())
        .collect();
    assert_eq!(
        edits.len(),
        1,
        "composed: only the trim is advertised, not a spurious newline; got {edits:?}"
    );
    assert_eq!(edits[0]["inserted"], "");
    assert_eq!(edits[0]["region"]["start_line"], 2);
    assert_eq!(edits[0]["region"]["start_column"], 1);
    assert_eq!(edits[0]["region"]["end_column"], 3);
}

/// CRITICAL regression (audit 2026-09-21): when an earlier whole-file fixer SHIFTS
/// byte offsets (a prepended header), a later fixer's advertised edit must be in
/// the ORIGINAL file's coordinate frame, not the intermediate buffer's -- else
/// applying it corrupts unrelated content (here it deleted bytes from `SECRETXY`).
/// The composed derivation puts every edit in the original frame.
#[test]
fn whole_file_edits_advertised_in_the_original_coordinate_frame() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: hdr\n",
            "    kind: file_header\n",
            "    paths: \"**/*.txt\"\n",
            "    pattern: \"^HEADER\"\n",
            "    level: error\n",
            "    fix: { file_prepend: { content: \"HEADER\\n\" } }\n",
            "  - id: no-ws\n",
            "    kind: no_trailing_whitespace\n",
            "    paths: \"**/*.txt\"\n",
            "    level: error\n",
            "    fix: { file_trim_trailing_whitespace: {} }\n",
        ),
    )
    .unwrap();
    // Line 1 has trailing whitespace (no-ws fires); line 2 is unrelated content the
    // buggy intermediate-frame edit corrupted.
    let original = b"aaaa   \nSECRETXY\n";
    std::fs::write(dir.path().join("f.txt"), original).unwrap();
    assert_fidelity(dir.path(), "f.txt", original);
}

/// HIGH regression (audit 2026-09-21): a located `set_value` and a whole-file
/// normalizer on ONE file must not advertise OVERLAPPING edits that corrupt on
/// apply. The composed derivation (`stage_fixes`) deconflicts/defers exactly as
/// `fix` does per pass, so the advertised set is a single, non-corrupting edit. (It
/// is the single-pass result -- `set_value` defers to the next pass when `no_ws`
/// buffers the file -- matching `fix --diff`'s documented single-pass preview.)
#[test]
fn located_and_whole_file_on_one_file_do_not_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        concat!(
            "version: 1\n",
            "rules:\n",
            "  - id: set-k\n",
            "    kind: properties_path_equals\n",
            "    paths: \"**/*.properties\"\n",
            "    path: \"$.key\"\n",
            "    equals: \"newval\"\n",
            "    level: error\n",
            "    fix: { set_value: { applicability: safe } }\n",
            "  - id: no-ws\n",
            "    kind: no_trailing_whitespace\n",
            "    paths: \"**/*.properties\"\n",
            "    level: error\n",
            "    fix: { file_trim_trailing_whitespace: {} }\n",
        ),
    )
    .unwrap();
    let original = b"key=oldval   \n";
    std::fs::write(dir.path().join("app.properties"), original).unwrap();

    // At most one advertised edit for the file (no overlapping pair), and applying
    // it yields a VALID, non-corrupting result -- the trailing whitespace trimmed,
    // never the old overlap corruption (`key=newval` with the final newline eaten).
    let agent = check_json(dir.path(), &["--format", "agent"]);
    let edits: Vec<_> = agent["violations"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|v| v["proposed_edit"].as_array().into_iter().flatten())
        .collect();
    assert!(edits.len() <= 1, "no overlapping pair; got {edits:?}");
    let mut buf = original.to_vec();
    for e in &edits {
        let (s, en) = region_bytes(original, &e["region"]);
        buf.splice(
            s..en,
            e["inserted"].as_str().unwrap().bytes().collect::<Vec<_>>(),
        );
    }
    assert_eq!(
        String::from_utf8_lossy(&buf),
        "key=oldval\n",
        "applying the advertised edit is a valid (trimmed) subset, never corrupt"
    );
}
