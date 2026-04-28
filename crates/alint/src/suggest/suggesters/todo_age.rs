//! Stale-TODO suggester. Finds TODO / FIXME / XXX / HACK
//! markers in source files, blames their lines, counts how
//! many are older than the default threshold (180 days), and
//! proposes a `git_blame_age` rule when ≥3 are stale.
//!
//! Eats our own dogfood: the rule kind we propose here
//! shipped in v0.7.3 and is the natural use case for
//! `alint suggest` — a bare repo without any custom rules
//! probably has stale debt.
//!
//! Confidence: MEDIUM. The stale count is a real signal but
//! formatter sweeps and squash-merges affect the exact count
//! (see `docs/rules.md` § `git_blame_age`).

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use regex::Regex;

use alint_core::git::{BlameCache, BlameLine};

use crate::progress::Progress;
use crate::suggest::proposal::{Confidence, Evidence, Proposal, ProposalKind};
use crate::suggest::scan::Scan;

const STALE_THRESHOLD_DAYS: u64 = 180;
const MIN_STALE_HITS: usize = 3;

pub fn propose(scan: &Scan, progress: &Progress) -> Vec<Proposal> {
    if !scan.has_git {
        // No git → no blame → nothing to propose.
        return Vec::new();
    }

    let candidates: Vec<PathBuf> = scan
        .text_files()
        .filter(|e| is_source_file(&e.path))
        .map(|e| e.path.clone())
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }

    let phase = progress.phase(
        "Blaming candidate sources for stale TODOs",
        Some(candidates.len() as u64),
    );
    let pattern = Regex::new(r"\b(TODO|FIXME|XXX|HACK)\b").expect("static regex compiles");

    // Quick textual prefilter: only blame files that actually
    // contain a TODO marker. Avoids blaming the entire
    // codebase on a clean repo.
    let mut files_with_markers: Vec<PathBuf> = Vec::new();
    for path in &candidates {
        phase.set_message(&format!("scanning {}", path.display()));
        let full = scan.root.join(path);
        if let Ok(bytes) = std::fs::read(&full)
            && let Ok(text) = std::str::from_utf8(&bytes)
            && pattern.is_match(text)
        {
            files_with_markers.push(path.clone());
        }
        phase.inc(1);
    }
    phase.finish(&format!(
        "Marker prefilter matched {} of {} files",
        files_with_markers.len(),
        candidates.len(),
    ));

    if files_with_markers.is_empty() {
        return Vec::new();
    }

    // Now blame only the files that contain markers.
    let blame_phase = progress.phase(
        "Computing blame for marker hits",
        Some(files_with_markers.len() as u64),
    );
    let cache = BlameCache::new(scan.root.clone());
    let now = SystemTime::now();
    let stale_threshold = Duration::from_secs(STALE_THRESHOLD_DAYS * 86_400);
    let mut stale_total = 0usize;
    let mut stale_paths: Vec<PathBuf> = Vec::new();
    for path in &files_with_markers {
        blame_phase.set_message(&format!("blaming {}", path.display()));
        let Some(blame) = cache.get(path) else {
            blame_phase.inc(1);
            continue;
        };
        let stale_count = count_stale_markers(&blame, &pattern, now, stale_threshold);
        if stale_count > 0 {
            stale_total += stale_count;
            stale_paths.push(path.clone());
        }
        blame_phase.inc(1);
    }
    blame_phase.finish(&format!(
        "{} stale marker{} across {} file{}",
        stale_total,
        if stale_total == 1 { "" } else { "s" },
        stale_paths.len(),
        if stale_paths.len() == 1 { "" } else { "s" },
    ));

    if stale_total < MIN_STALE_HITS {
        return Vec::new();
    }

    let preview = preview_paths(&stale_paths, 3);
    let evidence = vec![
        Evidence {
            message: format!(
                "{stale_total} TODO/FIXME/XXX/HACK marker{} older than {STALE_THRESHOLD_DAYS} days",
                if stale_total == 1 { "" } else { "s" },
            ),
        },
        Evidence {
            message: format!(
                "Stale across {} file{} ({preview}).",
                stale_paths.len(),
                if stale_paths.len() == 1 { "" } else { "s" },
            ),
        },
    ];

    let yaml = format!(
        r#"- id: stale-todos
  kind: git_blame_age
  paths:
    include: ["**/*.{{rs,ts,tsx,js,jsx,py,go,java,kt,rb}}"]
    exclude:
      - "**/*test*/**"
      - "**/fixtures/**"
      - "vendor/**"
  pattern: '\b(TODO|FIXME|XXX|HACK)\b'
  max_age_days: {STALE_THRESHOLD_DAYS}
  level: warning
  message: "`{{{{ctx.match}}}}` is over {STALE_THRESHOLD_DAYS} days old — resolve, convert to a tracked issue, or remove."
"#
    );

    vec![Proposal {
        id: "stale-todos".into(),
        kind: ProposalKind::Rule {
            kind: "git_blame_age".into(),
            yaml,
        },
        confidence: Confidence::Medium,
        summary: format!(
            "{stale_total} stale TODO/FIXME marker{} — `git_blame_age` would flag them.",
            if stale_total == 1 { "" } else { "s" },
        ),
        evidence,
    }]
}

fn count_stale_markers(
    blame: &[BlameLine],
    pattern: &Regex,
    now: SystemTime,
    threshold: Duration,
) -> usize {
    blame
        .iter()
        .filter(|line| pattern.is_match(&line.content))
        .filter(|line| now.duration_since(line.author_time).is_ok_and(|age| age > threshold))
        .count()
}

fn is_source_file(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some(
            "rs" | "ts"
                | "tsx"
                | "js"
                | "jsx"
                | "py"
                | "go"
                | "java"
                | "kt"
                | "rb"
                | "c"
                | "cpp"
                | "h"
                | "hpp"
                | "cs"
                | "swift"
                | "php"
        )
    )
}

fn preview_paths(paths: &[PathBuf], max: usize) -> String {
    let mut s: Vec<String> = paths
        .iter()
        .take(max)
        .map(|p| p.display().to_string())
        .collect();
    if paths.len() > max {
        s.push(format!("+{} more", paths.len() - max));
    }
    s.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    fn line(n: usize, content: &str, age_days: u64) -> BlameLine {
        let author_time = SystemTime::now() - Duration::from_secs(age_days * 86_400);
        BlameLine {
            line_number: n,
            author_time,
            content: content.into(),
        }
    }

    #[test]
    fn counts_only_old_marker_lines() {
        let pattern = Regex::new(r"\b(TODO|FIXME)\b").unwrap();
        let now = SystemTime::now();
        let threshold = Duration::from_secs(180 * 86_400);
        let lines = vec![
            line(1, "// TODO: ancient", 365),
            line(2, "// fresh TODO", 30),
            line(3, "regular code", 365),
            line(4, "// FIXME: old", 200),
        ];
        let count = count_stale_markers(&lines, &pattern, now, threshold);
        assert_eq!(
            count, 2,
            "lines 1 and 4 are stale; 2 is too young; 3 has no marker"
        );
    }

    #[test]
    fn future_dates_dont_panic() {
        // A blame timestamp in the future (clock skew) must
        // not crash — duration_since returns Err and the
        // filter drops the line.
        let pattern = Regex::new("TODO").unwrap();
        let future = UNIX_EPOCH + Duration::from_secs(u64::MAX / 2);
        let lines = vec![BlameLine {
            line_number: 1,
            author_time: future,
            content: "TODO".into(),
        }];
        let count =
            count_stale_markers(&lines, &pattern, SystemTime::now(), Duration::from_secs(0));
        assert_eq!(count, 0);
    }

    #[test]
    fn no_op_outside_git() {
        let scan = Scan::for_test(
            crate::init::Detection::default(),
            alint_core::FileIndex::default(),
            Vec::new(),
        );
        // has_git defaults to false in for_test
        assert!(propose(&scan, &Progress::null()).is_empty());
    }

    #[test]
    fn source_file_filter_covers_common_extensions() {
        for ext in ["rs", "ts", "py", "go", "java", "rb"] {
            let path = PathBuf::from(format!("a.{ext}"));
            assert!(is_source_file(&path), "{ext} should match");
        }
        assert!(!is_source_file(std::path::Path::new("README.md")));
        assert!(!is_source_file(std::path::Path::new("Cargo.toml")));
    }
}
