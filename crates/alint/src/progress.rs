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

use alint_output::{GlyphSet, sanitize_terminal};
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
    /// `true` for `Always`, and for `Auto` when stderr is a TTY; `false` for `Never`.
    emit_summary: bool,
    color: bool,
    glyphs: GlyphSet,
}

impl Progress {
    /// Build a new reporter. `Auto` resolves against
    /// `stderr.is_terminal()`. Production callers use [`Self::with_style`]
    /// to thread `--color` and `--ascii`; tests pass `ProgressMode::Never`
    /// (or use the `null()` shortcut).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(mode: ProgressMode) -> Self {
        Self::with_style(mode, true, GlyphSet::UNICODE)
    }

    /// Build a new reporter honoring resolved color and glyph preferences.
    pub fn with_style(mode: ProgressMode, color: bool, glyphs: GlyphSet) -> Self {
        let is_tty = std::io::stderr().is_terminal();
        // Animated bars only when stderr is a real TTY —
        // carriage-return repaints in a captured log file are
        // noise, not progress.
        let bars_visible = !matches!(mode, ProgressMode::Never) && is_tty;
        let inner =
            bars_visible.then(|| MultiProgress::with_draw_target(ProgressDrawTarget::stderr()));
        // `Auto` renders only when stderr is a TTY; `Always` forces milestone
        // lines even on non-TTY stderr; `Never` silences everything.
        let emit_summary = match mode {
            ProgressMode::Auto => is_tty,
            ProgressMode::Always => true,
            ProgressMode::Never => false,
        };
        Self {
            inner,
            emit_summary,
            color,
            glyphs,
        }
    }

    /// Shortcut for tests that want every method to be a no-op
    /// without TTY probing. Production callers use
    /// `Progress::with_style(ProgressMode::Never, ...)`.
    #[cfg(test)]
    pub fn null() -> Self {
        Self {
            inner: None,
            emit_summary: false,
            color: false,
            glyphs: GlyphSet::UNICODE,
        }
    }

    /// Begin a new phase with optional known total.
    /// `total: None` produces a spinner; `Some(n)` produces a
    /// determinate bar with ETA.
    pub fn phase(&self, label: &str, total: Option<u64>) -> Phase {
        let safe_label = sanitize_terminal(label).into_owned();
        let Some(multi) = &self.inner else {
            return Phase {
                bar: None,
                fallback_label: self.emit_summary.then_some(safe_label),
                bullet: self.glyphs.bullet,
            };
        };
        let bar = if let Some(n) = total {
            let pb = multi.add(ProgressBar::new(n));
            pb.set_style(determinate_style(self.color));
            pb
        } else {
            let pb = multi.add(ProgressBar::new_spinner());
            pb.set_style(spinner_style(self.color));
            pb.enable_steady_tick(Duration::from_millis(120));
            pb
        };
        bar.set_message(safe_label);
        Phase {
            bar: Some(bar),
            fallback_label: None,
            bullet: self.glyphs.bullet,
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
        let safe = sanitize_terminal(message);
        let bullet = self.glyphs.bullet;
        if let Some(multi) = &self.inner {
            let _ = multi.println(format!("{bullet} {safe}"));
        } else {
            eprintln!("{bullet} {safe}");
        }
    }

    /// Print a final summary line on stderr. Visible in every
    /// non-silent mode; suppressed when progress is `Never`.
    pub fn summary(&self, message: &str) {
        if !self.emit_summary {
            return;
        }
        let safe = sanitize_terminal(message);
        if let Some(multi) = &self.inner {
            let _ = multi.println(&safe);
        } else {
            eprintln!("{safe}");
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
    /// captured-stderr scenarios (`--progress=always` on non-TTY).
    fallback_label: Option<String>,
    bullet: &'static str,
}

impl Phase {
    #[cfg(test)]
    fn null() -> Self {
        Self {
            bar: None,
            fallback_label: None,
            bullet: GlyphSet::UNICODE.bullet,
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
            bar.set_message(sanitize_terminal(msg).into_owned());
        }
    }

    /// Mark the phase complete. With a drawing bar: leaves the
    /// final summary on screen via indicatif. Without a bar
    /// (captured-stderr mode): emits one milestone line so
    /// CI logs retain phase boundaries. With neither (silent /
    /// null mode): no-op.
    pub fn finish(self, summary: &str) {
        let safe = sanitize_terminal(summary);
        if let Some(bar) = self.bar {
            bar.finish_with_message(safe.into_owned());
        } else if let Some(label) = self.fallback_label {
            eprintln!("{} {label} - {safe}", self.bullet);
        }
    }
}

fn determinate_style(color: bool) -> ProgressStyle {
    // "{spinner} label [bar] pos/len (eta)" — width is auto via
    // `wide_bar`. Strip color directives when colors are disabled.
    let template = if color {
        "{spinner:.cyan} {msg} [{wide_bar:.cyan/blue}] {pos}/{len} ({eta})"
    } else {
        "{spinner} {msg} [{wide_bar}] {pos}/{len} ({eta})"
    };
    ProgressStyle::with_template(template)
        .expect("valid indicatif template")
        .progress_chars("=> ")
}

fn spinner_style(color: bool) -> ProgressStyle {
    let template = if color {
        "{spinner:.cyan} {msg} {elapsed:.dim}"
    } else {
        "{spinner} {msg} {elapsed}"
    };
    ProgressStyle::with_template(template).expect("valid indicatif template")
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
