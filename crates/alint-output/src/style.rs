//! Styles, glyphs, and color-choice plumbing for the human
//! formatter.
//!
//! Two independent axes are handled here:
//!
//! 1. **ANSI color** — whether SGR escape sequences are emitted.
//!    Delegated to [`anstream`] and [`anstyle`]: the CLI wraps
//!    stdout in an `anstream::AutoStream`, which strips SGR codes
//!    on pipes, honors `NO_COLOR` / `CLICOLOR_FORCE`, and respects
//!    an explicit `--color` choice. Formatters just write
//!    `{STYLE}text{STYLE:#}` into the writer; the stream decides
//!    whether to keep the bytes.
//!
//! 2. **Glyph set** — Unicode vs. ASCII fallback for sigils,
//!    separators, and the like. Orthogonal to color: a no-color
//!    terminal can still render `✗`, and a color terminal with
//!    `--ascii` should still emit `x`. Controlled by [`GlyphSet`]
//!    with an auto-detect fallback for `TERM=dumb`.

use anstyle::{AnsiColor, Color, Style};

// ---------------------------------------------------------------
// Role-based style constants.
//
// Centralized so swapping palette is a one-file edit and every
// formatter call site reads as intent (`style::ERROR`) rather
// than a raw SGR code.
// ---------------------------------------------------------------

/// Errors — the thing the user most needs to notice.
pub const ERROR: Style = Style::new()
    .fg_color(Some(Color::Ansi(AnsiColor::Red)))
    .bold();

/// Warnings — actionable but not blocking.
pub const WARNING: Style = Style::new()
    .fg_color(Some(Color::Ansi(AnsiColor::Yellow)))
    .bold();

/// Info — purely advisory.
pub const INFO: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan)));

/// Success / "passed" — all-clear banner.
pub const SUCCESS: Style = Style::new()
    .fg_color(Some(Color::Ansi(AnsiColor::Green)))
    .bold();

/// File path headers.
pub const PATH: Style = Style::new().bold();

/// Rule identifiers — dimmed so they're secondary to the
/// message.
pub const RULE_ID: Style = Style::new().dimmed();

/// Documentation / policy URLs.
pub const DOCS: Style = Style::new()
    .fg_color(Some(Color::Ansi(AnsiColor::Blue)))
    .underline();

/// "fixable" tag — green to read as a positive affordance.
pub const FIXABLE: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green)));

/// Dimmed ancillary text (counts, timings, footer notes).
pub const DIM: Style = Style::new().dimmed();

// ---------------------------------------------------------------
// Glyphs.
// ---------------------------------------------------------------

/// The set of single-character glyphs used in the human output.
///
/// Two variants are shipped: [`GlyphSet::UNICODE`] (default) for
/// modern terminals and [`GlyphSet::ASCII`] for `TERM=dumb` or
/// explicit `--ascii`. A future variant could add Nerd Font
/// glyphs on `COLORTERM=truecolor`, but isn't needed today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphSet {
    pub error: &'static str,
    pub warning: &'static str,
    pub info: &'static str,
    pub success: &'static str,
    /// Horizontal-rule glyph used for section separators.
    pub rule: &'static str,
    /// Bullet for list items / summary lines.
    pub bullet: &'static str,
    /// Arrow for call-to-action lines (`→ run alint fix`).
    pub arrow: &'static str,
}

impl GlyphSet {
    pub const UNICODE: Self = Self {
        error: "✗",
        warning: "⚠",
        info: "ℹ",
        success: "✓",
        rule: "─",
        bullet: "·",
        arrow: "→",
    };
    pub const ASCII: Self = Self {
        error: "x",
        warning: "!",
        info: "i",
        success: "v",
        rule: "-",
        bullet: "*",
        arrow: "->",
    };

    /// Pick the Unicode set unless the caller forces ASCII or the
    /// environment signals a dumb terminal.
    ///
    /// Reads `$TERM` as the only signal. Thin wrapper around
    /// [`GlyphSet::decide`] — test that directly if you're
    /// exercising the decision logic.
    #[must_use]
    pub fn detect(force_ascii: bool) -> Self {
        Self::decide(force_ascii, std::env::var("TERM").ok().as_deref())
    }

    /// Pure version of [`GlyphSet::detect`] — takes `TERM` as an
    /// explicit argument so tests don't have to mutate process
    /// env (which is `unsafe` under edition 2024).
    #[must_use]
    pub fn decide(force_ascii: bool, term: Option<&str>) -> Self {
        if force_ascii || matches!(term, Some("dumb")) {
            Self::ASCII
        } else {
            Self::UNICODE
        }
    }
}

impl Default for GlyphSet {
    fn default() -> Self {
        Self::UNICODE
    }
}

// ---------------------------------------------------------------
// ColorChoice.
// ---------------------------------------------------------------

/// How to resolve whether to emit ANSI color codes. Parsed from
/// `--color=<auto|always|never>`.
///
/// `Auto` delegates to `anstream::AutoStream`, which consults
/// `NO_COLOR`, `CLICOLOR_FORCE`, and TTY status. `Always` /
/// `Never` override those heuristics (useful when piping into a
/// pager that understands ANSI, or when capturing output for a
/// snapshot test).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

impl ColorChoice {
    /// Map to `anstream`'s own enum so `AutoStream::new` accepts
    /// it directly.
    #[must_use]
    pub fn to_anstream(self) -> anstream::ColorChoice {
        match self {
            Self::Auto => anstream::ColorChoice::Auto,
            Self::Always => anstream::ColorChoice::Always,
            Self::Never => anstream::ColorChoice::Never,
        }
    }
}

impl std::str::FromStr for ColorChoice {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "auto" | "" => Ok(Self::Auto),
            "always" | "yes" | "true" | "on" => Ok(Self::Always),
            "never" | "no" | "false" | "off" => Ok(Self::Never),
            other => Err(format!(
                "invalid --color value {other:?}; expected auto|always|never"
            )),
        }
    }
}

// ---------------------------------------------------------------
// OSC 8 hyperlinks.
// ---------------------------------------------------------------

/// Write `text` as an OSC 8 hyperlink targeting `url` when
/// `enabled`, or as plain `text` otherwise.
///
/// The OSC 8 sequence (`ESC ] 8 ; ; URL ESC \ text ESC ] 8 ; ; ESC \`)
/// is understood by modern terminals (`iTerm2`, `Kitty`, `WezTerm`,
/// `Alacritty`, `VSCode`'s integrated terminal, Windows Terminal,
/// GNOME Terminal, …). Terminals that don't recognize it are
/// supposed to pass the payload through unchanged — in practice
/// most do, so we only emit the sequence when the CLI has
/// *positively* detected hyperlink support via the
/// `supports-hyperlinks` crate.
///
/// The surrounding SGR (underline + blue) is the caller's
/// responsibility — we keep concerns separate so the same helper
/// can render a cross-reference or a docs link with different
/// styling.
pub fn write_hyperlink(
    w: &mut dyn std::io::Write,
    url: &str,
    text: &str,
    enabled: bool,
) -> std::io::Result<()> {
    if enabled {
        // ST = ESC \ (C1 string terminator). BEL (\x07) works too
        // in most terminals but ESC \ is the standard spelling.
        write!(w, "\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
    } else {
        write!(w, "{text}")
    }
}

// ---------------------------------------------------------------
// Per-render options.
// ---------------------------------------------------------------

/// Renderer options shared across the human formatter family.
/// Kept as a struct so new knobs (`--compact`, timing, etc.) can
/// be added without touching every call site.
///
/// The `Default` impl gives Unicode glyphs, no hyperlinks, and
/// `None` for width — the formatter then falls back to
/// [`HumanOptions::DEFAULT_WIDTH`] columns.
#[derive(Debug, Clone, Copy, Default)]
pub struct HumanOptions {
    pub glyphs: GlyphSet,
    /// Whether the output sink supports OSC 8 hyperlinks. Detected
    /// by the CLI (via `supports-hyperlinks`) and threaded down
    /// here so formatters decide per-call whether to emit the
    /// OSC 8 sequence.
    pub hyperlinks: bool,
    /// Terminal width in columns, used for stretching section
    /// separators. `None` signals "no TTY / couldn't detect" and
    /// formatters fall back to [`HumanOptions::DEFAULT_WIDTH`].
    pub width: Option<usize>,
    /// Use the one-line-per-violation compact renderer instead of
    /// the grouped full layout. Designed for piping into editors /
    /// grep / `wc -l`. The full-layout formatter calls through to
    /// [`crate::write_human_compact`] when this is `true`.
    pub compact: bool,
}

impl HumanOptions {
    /// Width used when no terminal is attached (pipes, files,
    /// non-TTY log capture). Chosen to match POSIX `COLUMNS`
    /// default and what most CLI tools settle on.
    pub const DEFAULT_WIDTH: usize = 80;

    /// Effective render width — the detected terminal width or
    /// `DEFAULT_WIDTH` when detection failed. Capped at a sane
    /// max so a 1000-col terminal doesn't produce section headers
    /// longer than the reader can scan.
    #[must_use]
    pub fn effective_width(&self) -> usize {
        self.width.unwrap_or(Self::DEFAULT_WIDTH).clamp(40, 120)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_choice_parses_common_forms() {
        assert_eq!("auto".parse::<ColorChoice>().unwrap(), ColorChoice::Auto);
        assert_eq!(
            "always".parse::<ColorChoice>().unwrap(),
            ColorChoice::Always
        );
        assert_eq!("never".parse::<ColorChoice>().unwrap(), ColorChoice::Never);
        assert_eq!("YES".parse::<ColorChoice>().unwrap(), ColorChoice::Always);
        assert_eq!("off".parse::<ColorChoice>().unwrap(), ColorChoice::Never);
        assert!("sparkles".parse::<ColorChoice>().is_err());
    }

    #[test]
    fn glyph_set_decide_respects_dumb_term() {
        assert_eq!(GlyphSet::decide(false, Some("dumb")), GlyphSet::ASCII);
        assert_eq!(
            GlyphSet::decide(false, Some("xterm-256color")),
            GlyphSet::UNICODE
        );
        assert_eq!(GlyphSet::decide(false, None), GlyphSet::UNICODE);
    }

    #[test]
    fn glyph_set_force_ascii_overrides_term() {
        assert_eq!(
            GlyphSet::decide(true, Some("xterm-256color")),
            GlyphSet::ASCII
        );
        assert_eq!(GlyphSet::decide(true, Some("dumb")), GlyphSet::ASCII);
        assert_eq!(GlyphSet::decide(true, None), GlyphSet::ASCII);
    }

    #[test]
    fn hyperlink_enabled_emits_osc8_sequence() {
        let mut out = Vec::new();
        write_hyperlink(&mut out, "https://example.com", "click", true).unwrap();
        let s = String::from_utf8(out).unwrap();
        // ESC ] 8 ; ; URL ESC \ TEXT ESC ] 8 ; ; ESC \
        assert_eq!(s, "\x1b]8;;https://example.com\x1b\\click\x1b]8;;\x1b\\");
    }

    #[test]
    fn hyperlink_disabled_emits_plain_text() {
        let mut out = Vec::new();
        write_hyperlink(&mut out, "https://example.com", "click", false).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "click");
    }
}
