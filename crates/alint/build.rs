//! Bake commit SHA + build date into the alint binary so
//! `alint --version` includes them. Useful for reproducing bug
//! reports — adopters can paste the full version string and a
//! maintainer can pinpoint the exact commit and build date.
//!
//! Falls back gracefully when git isn't available (e.g. when
//! the crate is built from a published tarball without a git
//! tree). In that case, both env vars are set to `unknown` and
//! `--version` prints just the workspace version.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let sha = git_short_sha().unwrap_or_else(|| "unknown".to_string());
    let date = build_date_utc();
    println!("cargo:rustc-env=ALINT_GIT_SHA={sha}");
    println!("cargo:rustc-env=ALINT_BUILD_DATE={date}");
    // Re-run when the HEAD changes so the SHA stays fresh.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
}

fn git_short_sha() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn build_date_utc() -> String {
    // YYYY-MM-DD UTC, derived from system clock at build time.
    // Avoids pulling in `chrono` for one date string — manual
    // computation from epoch seconds is good enough.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days_since_epoch = (now / 86_400) as i64;
    let (year, month, day) = days_to_ymd(days_since_epoch);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Convert days-since-epoch (1970-01-01) to (year, month, day).
/// Standard astronomical algorithm; valid for any year ≥ 1970.
fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    // Shift so day 0 = 1970-03-01 (start of leap-year cycle).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = (if m <= 2 { y + 1 } else { y }) as i32;
    (y, m, d)
}
