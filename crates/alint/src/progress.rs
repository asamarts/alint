//! Side-channel progress reporting for slow CLI operations
//! (currently `alint suggest`; lifted to `check` / `fix` later
//! if those grow long-running paths).
//!
//! ## Stream contract
//!
//! Progress always lands on **stderr**. `stdout` carries the
//! command's structured output (human / yaml / json) byte-for-
//! byte clean, regardless of progress activity. An agent piping
//! `alint suggest --format=json | jq` still sees spinners on its
//! terminal because stderr passes through.
//!
//! ## TTY awareness
//!
//! `ProgressMode::Auto` (the default) draws when
//! `stderr.is_terminal()` is true and falls back to silent
//! otherwise — captured stderr (CI logs, `2> file`) gets no
//! carriage-return junk.
//!
//! ## Null-handle pattern
//!
//! `Progress::new(ProgressMode::Never)` returns a fully-
//! functional handle whose every method is a no-op. Suggester
//! code threads `&Progress` without branching on visibility, so
//! tests pass `Progress::null()` and exercise the same path as
//! production with progress disabled.

use std::io::IsTerminal;
use std::str::FromStr;
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

/// User-selected progress mode. Resolves at `Progress::new`
/// time — `Auto` becomes `Always`/`Never` based on TTY state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProgressMode {
    /// Draw when stderr is a TTY; silent otherwise.
    #[default]
    Auto,
    /// Force-on; useful for demos and tests.
    Always,
    /// Silent. Used by `--quiet` and `--progress=never`.
    Never,
}

impl FromStr for ProgressMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Self::Auto),
            "always" => Ok(Self::Always),
            "never" => Ok(Self::Never),
            other => Err(format!(
                "invalid progress mode {other:?}; expected one of `auto`, `always`, `never`"
            )),
        }
    }
}

/// A side-channel progress reporter. Construct once per command,
/// pass `&Progress` to long-running functions. Drop at end of
/// command to clear any in-flight bars.
///
/// Two visibility flags are tracked separately:
///
/// - `inner: Some(_)` — animated bars are drawn (TTY +
///   `Auto`/`Always`).
/// - `summary_to_stderr` — short status / summary lines emit
///   to stderr unconditionally except in `Never`. This means
///   piped runs (CI, captured stderr) still see one-line
///   milestones without the carriage-return junk of bars.
#[derive(Debug)]
pub struct Progress {
    /// `Some` when bars should be drawn; `None` for the silent
    /// mode (CI, --quiet, non-TTY auto).
    inner: Option<MultiProgress>,
    /// Whether status/summary one-liners write to stderr.
    /// `true` for `Auto`/`Always`; `false` for `Never`.
    emit_summary: bool,
}

impl Progress {
    /// Build a new reporter. `Auto` resolves against
    /// `stderr.is_terminal()`. Production callers pass the
    /// CLI-flag value; tests pass `ProgressMode::Never` (or use
    /// the `null()` shortcut).
    pub fn new(mode: ProgressMode) -> Self {
        // Animated bars only when stderr is a real TTY —
        // carriage-return repaints in a captured log file are
        // noise, not progress. Even `--progress=always` won't
        // force bars onto a non-TTY: indicatif silences itself
        // for non-TTY draw targets, and eprintln-based status
        // lines (below) cover the captured-log case more
        // usefully.
        let bars_visible = !matches!(mode, ProgressMode::Never) && std::io::stderr().is_terminal();
        let inner =
            bars_visible.then(|| MultiProgress::with_draw_target(ProgressDrawTarget::stderr()));
        // Status / summary lines write to stderr in every mode
        // except `Never`. Captured stderr (CI logs) still gets
        // the one-line milestones — useful for "what was
        // happening when this run failed?" diagnostics.
        let emit_summary = !matches!(mode, ProgressMode::Never);
        Self {
            inner,
            emit_summary,
        }
    }

    /// Shortcut for tests that want every method to be a no-op
    /// without TTY probing. Production callers use
    /// `Progress::new(ProgressMode::Never)`.
    #[cfg(test)]
    pub fn null() -> Self {
        Self {
            inner: None,
            emit_summary: false,
        }
    }

    /// Begin a new phase with optional known total.
    /// `total: None` produces a spinner; `Some(n)` produces a
    /// determinate bar with ETA.
    ///
    /// When bars aren't being drawn but status output is still
    /// enabled (`--progress=auto` on a non-TTY,
    /// `--progress=always` on a non-TTY, or any captured
    /// stderr scenario), the returned [`Phase`] writes a
    /// one-line `<label> — <summary>` milestone on `finish` so
    /// captured logs retain phase boundaries.
    pub fn phase(&self, label: &str, total: Option<u64>) -> Phase {
        let Some(multi) = &self.inner else {
            return Phase {
                bar: None,
                fallback_label: self.emit_summary.then(|| label.to_string()),
            };
        };
        let bar = if let Some(n) = total {
            let pb = multi.add(ProgressBar::new(n));
            pb.set_style(determinate_style());
            pb
        } else {
            let pb = multi.add(ProgressBar::new_spinner());
            pb.set_style(spinner_style());
            pb.enable_steady_tick(Duration::from_millis(120));
            pb
        };
        bar.set_message(label.to_string());
        Phase {
            bar: Some(bar),
            fallback_label: None,
        }
    }

    /// One-shot status line for fast phases that don't need a
    /// bar. When bars are visible, queued through indicatif's
    /// `println` so it interleaves cleanly above any active bar.
    /// Otherwise written directly to stderr so captured logs
    /// see the milestone.
    pub fn status(&self, message: &str) {
        if !self.emit_summary {
            return;
        }
        if let Some(multi) = &self.inner {
            let _ = multi.println(format!("· {message}"));
        } else {
            // No bars in flight (captured stderr or
            // `--progress=auto` on a non-TTY). Direct write.
            eprintln!("· {message}");
        }
    }

    /// Print a final summary line on stderr. Visible in every
    /// non-silent mode; suppressed when progress is `Never`.
    pub fn summary(&self, message: &str) {
        if !self.emit_summary {
            return;
        }
        if let Some(multi) = &self.inner {
            let _ = multi.println(message);
        } else {
            eprintln!("{message}");
        }
    }
}

/// A handle to one phase. Drop or call [`Phase::finish`] to
/// clean up the bar (or emit a captured-log milestone, if
/// bars aren't drawing).
#[derive(Debug)]
pub struct Phase {
    /// The drawing bar, when bars are visible. `None` when
    /// stderr isn't a TTY or `--progress=never`.
    bar: Option<ProgressBar>,
    /// When set, `finish` writes a one-line milestone to
    /// stderr instead of a bar update. Active only in
    /// captured-stderr scenarios (no TTY but progress enabled).
    fallback_label: Option<String>,
}

impl Phase {
    #[cfg(test)]
    fn null() -> Self {
        Self {
            bar: None,
            fallback_label: None,
        }
    }

    /// Advance the bar by `n` units. No-op for spinners (their
    /// `enable_steady_tick` does the animation) and no-op when
    /// progress is hidden.
    pub fn inc(&self, n: u64) {
        if let Some(bar) = &self.bar {
            bar.inc(n);
        }
    }

    /// Replace the in-progress message text. Useful for
    /// "Blaming src/foo.rs"-style transient detail under a
    /// fixed phase label.
    pub fn set_message(&self, msg: &str) {
        if let Some(bar) = &self.bar {
            bar.set_message(msg.to_string());
        }
    }

    /// Mark the phase complete. With a drawing bar: leaves the
    /// final summary on screen via indicatif. Without a bar
    /// (captured-stderr mode): emits one milestone line so
    /// CI logs retain phase boundaries. With neither (silent /
    /// null mode): no-op.
    pub fn finish(self, summary: &str) {
        if let Some(bar) = self.bar {
            bar.finish_with_message(summary.to_string());
        } else if let Some(label) = self.fallback_label {
            eprintln!("· {label} — {summary}");
        }
    }
}

fn determinate_style() -> ProgressStyle {
    // "{spinner} label [bar] pos/len (eta)" — width is auto via
    // `wide_bar`. `dim` template tokens render in a muted style
    // so the bar foregrounds the active count.
    ProgressStyle::with_template(
        "{spinner:.cyan} {msg} [{wide_bar:.cyan/blue}] {pos}/{len} ({eta})",
    )
    .expect("valid indicatif template")
    .progress_chars("=> ")
}

fn spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner:.cyan} {msg} {elapsed:.dim}")
        .expect("valid indicatif template")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_handle_methods_are_noops() {
        // The null handle shape — used by every unit test below
        // and by --progress=never in production — must never
        // panic regardless of how it's exercised.
        let p = Progress::null();
        let phase = p.phase("blaming", Some(100));
        phase.inc(50);
        phase.set_message("src/foo.rs");
        phase.finish("done");
        p.status("loading config");
        p.summary("0 proposals");
    }

    #[test]
    fn never_mode_resolves_to_hidden() {
        // Sanity: ProgressMode::Never always produces a hidden
        // reporter regardless of the host's TTY state.
        let p = Progress::new(ProgressMode::Never);
        assert!(p.inner.is_none());
    }

    #[test]
    fn from_str_round_trip() {
        assert_eq!("auto".parse::<ProgressMode>().unwrap(), ProgressMode::Auto);
        assert_eq!(
            "always".parse::<ProgressMode>().unwrap(),
            ProgressMode::Always
        );
        assert_eq!(
            "never".parse::<ProgressMode>().unwrap(),
            ProgressMode::Never
        );
        assert!("verbose".parse::<ProgressMode>().is_err());
    }

    #[test]
    fn phase_null_methods_no_panic() {
        // Phase::null is what gets returned from a hidden
        // Progress; cover every public method.
        let phase = Phase::null();
        phase.inc(0);
        phase.inc(1_000_000);
        phase.set_message("");
        phase.finish("");
    }
}
