Output demo/alint.gif
# alint README demo, as code.
#
# Regenerate:  demo/render.sh              (writes and re-times demo/alint.gif)
# Requires:    https://github.com/charmbracelet/vhs
#
# WHY a tape and not a hand-recorded GIF: alint is a drift-prevention tool, and a
# hand-recorded demo is precisely the kind of artifact that goes stale (an old
# version string, an output format that has since changed) with nothing to catch
# it. The tape is generated, reproducible, and re-runnable in CI, so the demo can
# never silently drift from what the binary actually prints.
#
# The recording runs against a scratch COPY of demo/fixture, so it never mutates
# the tracked fixture (the `fix` step rewrites files). The two artifacts git must
# not carry, a tracked `target/` and a `.DS_Store`, are synthesized here at record
# time rather than committed.


Require alint
Require git

Set Shell bash

# GEOMETRY IS LOAD-BEARING. Do not "improve" these numbers, and do not run this
# tape with a bare `vhs`. Use demo/render.sh.
#
# 1760x1520 is 2x the 880x760 the README displays. Most people read GitHub on a
# HiDPI screen, where a 1x asset is upscaled 2x by the browser and small
# antialiased terminal glyphs turn to mush. That WAS the bug. At 2x, HiDPI maps
# the GIF 1:1 to device pixels (sharp) and 1x displays downscale it exactly 2:1
# (clean). The README <img> carries width="880" to match; do not remove it.
#
# FontSize 30 gives a ~91-column terminal at this width. The demo runs plain
# `alint check` (no --width): alint reads the terminal size itself and lays the
# output out to fit, which is what a real user sees and what we want to show. Do
# not shrink the font to cram in more columns; we do not need them, and small
# glyphs are the whole problem.
#
# THE CATCH: VHS screenshots a headless browser in real time and cannot keep up
# with this canvas, so it silently DROPS frames while still tagging the GIF
# 25fps. The raw output plays 2.4x too fast with no warning. demo/render.sh
# re-times it to its target duration (TRUE_DURATION=29.16s). Render with that script, never bare vhs.
Set FontSize 30
Set Width 1760
Set Height 1520
Set Padding 40
Set TypingSpeed 45ms
Set Theme "Catppuccin Mocha"

# --- setup (not recorded) --------------------------------------------------
Hide
Type "export PS1='$ ' && clear"
Enter
Type "cd $(mktemp -d) && cp -r ${ALINT_DEMO_FIXTURE:-$OLDPWD/demo/fixture}/. ."
Enter
# Synthesize the two artifacts we deliberately do not commit. The .DS_Store
# carries the real "Bud1" magic (00 00 00 01 42 75 64 31): the macos-junk rule
# is content-verified, so a bare-newline stub would no longer be flagged.
Type "mkdir -p target/debug && printf 'binary\n' > target/debug/acme && printf '\000\000\000\001Bud1' > .DS_Store"
Enter
Type "git init -q . && git add -A && clear"
Enter
Sleep 500ms
Show

# --- 1. the problem --------------------------------------------------------
Type "# A Rust repo with the usual structural drift. Nothing here checks for it."
Enter
Sleep 1.5s

Type "alint check --no-docs"
Enter
Sleep 6s

# --- 2. fix what is mechanical ----------------------------------------------
# NOTE: the narration deliberately does NOT restate the violation counts. The
# output already shows them, and a hard-coded "3 errors, 4 fixable" in the
# script is precisely the kind of claim that goes stale the next time a bundled
# rule changes. Let the tool speak for itself.
Type "# Some of that is mechanical. alint fixes it in place."
Enter
Sleep 1.5s

Type "alint fix"
Enter
Sleep 5s

# --- 3. what is left is a human decision ------------------------------------
Type "alint check --no-docs --compact"
Enter
Sleep 4s

Type "# What remains is structural. That needs a decision, not a rewrite."
Enter
Sleep 2.5s
