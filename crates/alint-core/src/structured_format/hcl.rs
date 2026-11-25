//! Bounded HCL2 parsing (`hcl-rs` 0.20 / `hcl-edit` 0.10) into `serde_json::Value`.
//!
//! `hcl-edit` 0.10 enforces internal recursion bounds (`MAX_NESTING_DEPTH = 64`
//! across blocks, objects, arrays, parens, calls, unary runs, conditionals,
//! `for` expressions, templates, and directives; `MAX_OPERATOR_DEPTH = 128`
//! across chained binary operators), so HCL parsing runs directly on normal
//! worker stacks without spawning a 512 MiB parse thread or carrying a
//! safety-critical recursion pre-scanner (#608).
//!
//! Three lightweight guards remain here for resource and worst-case CPU safety:
//! 1. [`MAX_HCL_BYTES`] (4 MiB): `hcl-edit` builds a lossless CST alongside
//!    `serde_json::Value` (~100-125x peak RSS amplification on dense
//!    arrays/traversals), so capping input at 4 MiB (matching
//!    `MAX_DOTENV_BYTES`) keeps worst-case peak RSS under ~500 MiB while
//!    accepting real-world large Terraform/Nomad/Packer files.
//! 2. [`MAX_HCL_OBJECT_ITEMS`] (1,024 per object expression) and
//!    [`MAX_HCL_OBJECT_WORK`] (64,000,000 byte-entries per file): `hcl-edit`
//!    stores object literal entries in a `VecMap` with linear duplicate checks
//!    (`O(K^2 * L)`), so capping per-object entries and cumulative quadratic
//!    work keeps worst-case CPU bounded while leaving top-level bodies,
//!    `locals {}` / resource blocks (`FnvHashSet`), and heredocs unrestricted.
//! 3. Direct `hcl::edit::structure::Body -> Value` conversion using `BTreeMap`
//!    (bypassing `hcl::Body` and `hcl::from_str`'s `IntoJsonSpec` `VecMap`
//!    `O(N^2)` path) with cumulative [`MAX_HCL_BLOCK_DEPTH`] (64) across
//!    nested blocks and their labels, enforced identically in both
//!    [`hcl_to_value`] and [`parse_edit_body`].

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use serde::Deserialize as _;
use serde_json::{Map, Value};

/// Maximum HCL input size accepted for parsing (4 MiB, matching
/// `MAX_DOTENV_BYTES`). Bounds lossless-CST peak RSS amplification (~100-125x
/// on dense inputs -> <= ~500 MiB) while comfortably admitting large real-world
/// Terraform, Nomad, and Packer files.
pub(crate) const MAX_HCL_BYTES: usize = 4 * 1024 * 1024;

/// Maximum `=` / `:` entry assignments inside a single `{ ... }` object
/// expression. `hcl-edit` 0.10 validates object-key uniqueness with
/// `VecMap::contains_key` (`O(K^2 * L)`), so a single object literal with
/// thousands of keys stalls parsing. Top-level attributes and block bodies
/// (`locals {}`, `resource "..." "..." {}`) use `FnvHashSet` (`O(N)`) and are
/// unaffected.
pub(crate) const MAX_HCL_OBJECT_ITEMS: usize = 1_024;

/// File-wide quadratic object-key work budget across object expressions.
/// Counts cumulative key-expression bytes and multiplicative `VecMap::eq` cost
/// for nested objects inside object keys, rejecting adversarial wide/nested
/// object-key inputs while admitting large legitimate lookup maps and objects
/// with multi-megabyte heredoc values.
pub(crate) const MAX_HCL_OBJECT_WORK: u64 = 64_000_000;

/// Maximum total heredoc-backtracking rescan bytes per file (`16 * MAX_HCL_BYTES`
/// = 64 MiB). Admits dozens of legitimate empty heredocs (`<<EOF\nEOF`) inside
/// multi-MiB Terraform files while strictly bounding worst-case rescan work.
const MAX_HCL_RESCAN_BYTES: usize = 16 * MAX_HCL_BYTES;

/// Maximum nesting depth of object literals opened inside an enclosing object
/// literal's key expression (`{ { ... } = 1 }`). Bounds `hcl-edit`'s `VecMap::eq`
/// recursive key comparisons (`O(K^(2D))`) before `body_to_value` rejects
/// non-string keys, while still admitting realistic expression keys such as
/// `{ jsonencode({ a = { b = 1 } }) = "v" }`.
const MAX_HCL_KEY_OBJECT_DEPTH: usize = 4;

/// Maximum entries allowed in a single object literal opened inside an
/// enclosing object's key expression.
const MAX_HCL_KEY_OBJECT_ITEMS: usize = 64;

/// Maximum cumulative nesting depth contributed by HCL blocks and their labels
/// (`sum(1 + block.labels.len())` along any root-to-leaf block path). `hcl-edit`
/// parses block labels iteratively into a `Vec`, but the HCL JSON specification
/// nests each block identifier and each label into its own JSON object level;
/// capping cumulative depth at 64 (matching `hcl-edit`'s `MAX_NESTING_DEPTH`)
/// prevents nested multi-label blocks (`64 x 64` = 4,160+ levels) from
/// overflowing the stack during conversion or `serde_json::Value::drop`.
pub(crate) const MAX_HCL_BLOCK_DEPTH: usize = 64;

/// Maximum scope-stack depth tracked by [`check_object_width`]. Set well above
/// `hcl-edit`'s own `MAX_NESTING_DEPTH = 64` so valid HCL never hits it while
/// capping scanner memory on adversarial delimiter streams (e.g. 4 MiB of `{`).
const MAX_HCL_SCAN_DEPTH: usize = 256;

/// Validate HCL input against size and object-expression width/work limits
/// before invoking `hcl-edit`.
pub(crate) fn check_limits(text: &str) -> Result<(), String> {
    if text.len() > MAX_HCL_BYTES {
        return Err(format!(
            "HCL input exceeds the maximum supported size ({MAX_HCL_BYTES} bytes)"
        ));
    }
    check_object_width(text)
}

/// Parse HCL into a spanned `hcl::edit::structure::Body` after enforcing
/// [`check_limits`] and cumulative [`MAX_HCL_BLOCK_DEPTH`]. Used by both
/// [`hcl_to_value`] and `structured_fix::hcl` so checks and fixes share the
/// exact same size, width, work, and block-label depth bounds.
pub(crate) fn parse_edit_body(text: &str) -> Result<hcl::edit::structure::Body, String> {
    check_limits(text)?;
    let body = hcl::edit::parser::parse_body(text).map_err(|e| e.to_string())?;
    check_edit_body_depth(&body, 0)?;
    Ok(body)
}

/// Parse HCL (Terraform / Nomad / Packer) into the uniform `serde_json::Value`
/// tree matching the HCL JSON Specification (`hcl::from_str::<Value>`): blocks
/// nest by type then labels (`resource "t" "n" { ... }` -> `$.resource.t.n`),
/// a repeated block type/label pair becomes an array, values keep their HCL
/// type, and unevaluated expressions arrive as opaque strings.
pub(crate) fn hcl_to_value(text: &str) -> Result<Value, String> {
    let body = parse_edit_body(text)?;
    body_to_value(body, 0)
}

#[inline]
fn is_hcl_id_start(c: char) -> bool {
    c == '_' || unicode_ident::is_xid_start(c)
}

#[inline]
fn is_hcl_id_continue(c: char) -> bool {
    c == '_' || c == '-' || unicode_ident::is_xid_continue(c)
}

#[derive(Debug, Clone, Copy)]
enum WidthScope {
    /// Inside a root or block body (`locals { ... }`, `resource "t" "n" { ... }`).
    /// `in_expr` is `true` while scanning an attribute's RHS expression (so `{`
    /// opens an `ObjectBrace`, not a nested `BlockBrace`); `last_was_op` tracks
    /// whether the previous token was an operator that can continue across a
    /// newline inside an expression; `last_was_dot` distinguishes legacy attr
    /// splat (`a.*`) from binary `*`.
    BlockBrace {
        in_expr: bool,
        last_was_op: bool,
        last_was_dot: bool,
    },
    /// Inside an HCL object expression (`{ k = v, ... }` or `{ k: v, ... }`).
    ObjectBrace {
        count: usize,
        /// Cumulative bytes consumed in key positions across entries of this object.
        key_bytes: u64,
        /// Byte offset where the current entry's key expression began.
        key_start: usize,
        /// Whether any non-whitespace/non-comment token has been seen for the
        /// current entry's key expression (so leading comments/whitespace do not
        /// inflate `key_bytes`).
        key_seen: bool,
        /// `false` while scanning an entry's key; flips to `true` on `=` / `:` at
        /// `ternary_depth == 0` and resets to `false` on `,` or newline at end of
        /// value expression.
        in_value: bool,
        /// Tracks whether the previous value token was an operator that can
        /// continue across a newline.
        last_was_op: bool,
        /// Tracks `last_was_dot` for legacy attr splat (`a.*`) in value expressions.
        last_was_dot: bool,
        /// Unparenthesized `? ... :` ternary nesting depth inside a value.
        ternary_depth: usize,
        /// Multiplicative `VecMap::eq` cost multiplier contributed by nested
        /// `ObjectBrace`s inside the current entry's key expression.
        current_key_eq_cost: u64,
        /// Worst-case `VecMap::eq` cost of any child object inside this object
        /// (propagated upward if this object itself sits inside an outer key).
        max_child_eq_cost: u64,
        /// Whether this object was opened while an enclosing `ObjectBrace` was in
        /// key position (`in_value == false`).
        in_ancestor_key: bool,
    },
    /// Inside a `{ for ... }` object comprehension (uses `ForExpr`, not `VecMap`).
    ForBrace,
    /// Inside `[ ... ]` (array literal or list `for` expression).
    Bracket,
    /// Inside `( ... )` (parenthesized expression or function arguments).
    Paren,
    /// Inside `${ ... }` or `%{ ... }` control header (`if`/`for`/`else`/`endif`/`endfor`).
    InterpBrace,
    /// Inside `"..."` double-quoted string or string template.
    String,
    /// Inside `<<TAG\n...\nTAG` or `<<-TAG\n...\nTAG` heredoc template.
    Heredoc {
        tag_start: usize,
        tag_end: usize,
        /// Whether at least one body line ending has been scanned inside this
        /// heredoc. `hcl-edit`'s `heredoc_template` greedily looks for
        /// `(line_ending, space0, delim, !is_id_continue)` first and only
        /// falls back to closing an empty heredoc on line 1 (`empty_close`) if
        /// no later body-terminated delimiter exists or a template element
        /// backtracks.
        seen_body_line: bool,
        empty_close: Option<usize>,
        /// Whether an unescaped `${` or `%{` appeared inside this heredoc (only
        /// template markers can trigger mid-file winnow `opt(...)` backtracking).
        saw_marker: bool,
        /// `total_work` captured when this heredoc opened, restored on empty-close
        /// backtrack so scanned expressions inside the discarded branch are not
        /// double-charged.
        work_at_open: u64,
        /// Index into the scanner's snapshot table storing the scope stack right
        /// after this heredoc would close empty on line 1.
        snapshot_idx: Option<usize>,
    },
    /// Inside a `%{if ...}...%{endif}` or `%{for ...}...%{endfor}` directive
    /// body, where `hcl-edit` (`parser/template.rs`) ignores `"` and outer
    /// heredoc terminators and only stops on `${` / `%{`.
    TemplateDirective,
}

/// Deferred empty-heredoc branch `(scopes, resume_at, work_at_open)`.
type BacktrackBranch = (Vec<WidthScope>, usize, u64);

fn push_scope(scopes: &mut Vec<WidthScope>, scope: WidthScope) -> Result<(), String> {
    if scopes.len() >= MAX_HCL_SCAN_DEPTH {
        return Err(format!(
            "HCL input exceeds the maximum supported nesting depth ({MAX_HCL_SCAN_DEPTH})"
        ));
    }
    scopes.push(scope);
    Ok(())
}

fn bump_object_entry(
    scopes: &mut [WidthScope],
    pos: usize,
    total_work: &mut u64,
) -> Result<(), String> {
    if let Some(WidthScope::ObjectBrace {
        count,
        key_bytes,
        key_start,
        key_seen,
        in_value,
        last_was_op,
        last_was_dot,
        ternary_depth,
        current_key_eq_cost,
        in_ancestor_key,
        ..
    }) = scopes.last_mut()
    {
        if *in_value || *ternary_depth > 0 {
            return Ok(());
        }
        let this_key_bytes = if *key_seen {
            pos.saturating_sub(*key_start) as u64
        } else {
            0
        };
        *key_bytes = key_bytes.saturating_add(this_key_bytes);
        *in_value = true;
        *last_was_op = true;
        *last_was_dot = false;
        *count += 1;
        let max_items = if *in_ancestor_key {
            MAX_HCL_KEY_OBJECT_ITEMS
        } else {
            MAX_HCL_OBJECT_ITEMS
        };
        if *count > max_items {
            return Err(format!(
                "HCL object exceeds the maximum supported width ({max_items} entries)"
            ));
        }
        let eq_work = (*count as u64).saturating_mul((*current_key_eq_cost).max(1));
        let entry_work = (*key_bytes).max(eq_work);
        *total_work = total_work.saturating_add(entry_work);
        if *total_work > MAX_HCL_OBJECT_WORK {
            return Err(format!(
                "HCL object literals exceed the maximum supported parse complexity ({MAX_HCL_OBJECT_WORK})"
            ));
        }
    }
    Ok(())
}

fn note_object_key_token(scopes: &mut [WidthScope], pos: usize) {
    if let Some(WidthScope::ObjectBrace {
        key_start,
        key_seen,
        in_value: false,
        ..
    }) = scopes.last_mut()
        && !*key_seen
    {
        *key_start = pos;
        *key_seen = true;
    }
}

fn reset_object_entry(scopes: &mut [WidthScope], next_pos: usize) {
    if let Some(WidthScope::ObjectBrace {
        key_start,
        key_seen,
        in_value,
        last_was_op,
        last_was_dot,
        ternary_depth,
        current_key_eq_cost,
        ..
    }) = scopes.last_mut()
    {
        *in_value = false;
        *last_was_op = false;
        *last_was_dot = false;
        *ternary_depth = 0;
        *current_key_eq_cost = 0;
        *key_start = next_pos;
        *key_seen = false;
    }
}

fn mark_enclosing_heredoc_saw_marker(scopes: &mut [WidthScope]) {
    if let Some(WidthScope::Heredoc { saw_marker, .. }) = scopes
        .iter_mut()
        .rev()
        .find(|s| matches!(s, WidthScope::Heredoc { .. }))
    {
        *saw_marker = true;
    }
}

/// Skip HCL `ws` (ASCII whitespace plus `#`, `//`, and `/* ... */` comments)
/// starting at `idx`. Matches `hcl-edit/src/parser/trivia.rs::ws`.
fn skip_hcl_ws(b: &[u8], mut idx: usize) -> usize {
    let n = b.len();
    loop {
        while idx < n && matches!(b[idx], b' ' | b'\t' | b'\r' | b'\n') {
            idx += 1;
        }
        if (idx < n && b[idx] == b'#') || (idx + 1 < n && b[idx] == b'/' && b[idx + 1] == b'/') {
            while idx < n && b[idx] != b'\n' {
                idx += 1;
            }
        } else if idx + 1 < n && b[idx] == b'/' && b[idx + 1] == b'*' {
            idx += 2;
            while idx + 1 < n && !(b[idx] == b'*' && b[idx + 1] == b'/') {
                idx += 1;
            }
            idx = (idx + 2).min(n);
        } else {
            return idx;
        }
    }
}

/// Handle a `%{` template directive opener starting at `*i` (which points at
/// `b'%'` of `b"%{"`). Mirrors `hcl-edit/src/parser/template.rs`:
/// - `if` / `for` enters a `TemplateDirective` body after the `%{ ... }` header;
/// - `endif` / `endfor` closes the innermost `TemplateDirective` body;
/// - `else` stays inside the current `TemplateDirective` body.
fn enter_directive_control(
    b: &[u8],
    i: &mut usize,
    scopes: &mut Vec<WidthScope>,
) -> Result<(), String> {
    *i += 2; // consume `%{`
    let mut kw = *i;
    if b.get(kw) == Some(&b'~') {
        kw += 1;
    }
    kw = skip_hcl_ws(b, kw);
    let tail = &b[kw..];
    if tail.starts_with(b"endif") || tail.starts_with(b"endfor") {
        if matches!(scopes.last(), Some(WidthScope::TemplateDirective)) {
            scopes.pop();
        }
    } else if tail.starts_with(b"if") || tail.starts_with(b"for") {
        push_scope(scopes, WidthScope::TemplateDirective)?;
    }
    push_scope(scopes, WidthScope::InterpBrace)
}

/// Consume `$${` / `%%{` escaped markers or enter `${` / `%{` controls inside
/// template scopes (`String`, `Heredoc`, `TemplateDirective`). Returns `true`
/// if a control scope was pushed (caller should yield back to the main loop).
fn scan_template_marker(
    b: &[u8],
    i: &mut usize,
    scopes: &mut Vec<WidthScope>,
) -> Result<bool, String> {
    let tail = &b[*i..];
    if tail.starts_with(b"$${") || tail.starts_with(b"%%{") {
        // `hcl-edit` (`parser/string.rs::escaped_marker`) matches exact 3-byte
        // `$${` / `%%{` before `${` / `%{`. Any longer run of `$` or `%` before
        // `{` (e.g. `$$${`) emits literal prefix chars via `*i += 1` until the
        // trailing 3-byte `$${` / `%%{` matches here.
        *i += 3;
        Ok(false)
    } else if tail.starts_with(b"${") {
        mark_enclosing_heredoc_saw_marker(scopes);
        *i += 2;
        push_scope(scopes, WidthScope::InterpBrace)?;
        Ok(true)
    } else if tail.starts_with(b"%{") {
        mark_enclosing_heredoc_saw_marker(scopes);
        enter_directive_control(b, i, scopes)?;
        Ok(true)
    } else {
        *i += 1;
        Ok(false)
    }
}

fn scan_string_scope(
    text: &str,
    i: &mut usize,
    scopes: &mut Vec<WidthScope>,
) -> Result<(), String> {
    let b = text.as_bytes();
    let n = b.len();
    while *i < n {
        match b[*i] {
            b'\\' => {
                // Step past `\` and one full UTF-8 scalar so invalid non-ASCII
                // escapes like `\é` never land on a continuation byte.
                let next_len = text[*i + 1..].chars().next().map_or(0, char::len_utf8);
                *i = (*i + 1 + next_len).min(n);
            }
            b'"' => {
                *i += 1;
                scopes.pop();
                set_expr_op(scopes, false, false);
                break;
            }
            b'$' | b'%' => {
                if scan_template_marker(b, i, scopes)? {
                    break;
                }
            }
            _ => *i += 1,
        }
    }
    Ok(())
}

fn scan_directive_body_scope(
    b: &[u8],
    i: &mut usize,
    scopes: &mut Vec<WidthScope>,
) -> Result<(), String> {
    let n = b.len();
    while *i < n {
        match b[*i] {
            b'$' | b'%' => {
                if scan_template_marker(b, i, scopes)? {
                    break;
                }
            }
            _ => *i += 1,
        }
    }
    Ok(())
}

fn match_heredoc_delimiter(text: &str, pos: usize, tag: &[u8]) -> Option<usize> {
    let b = text.as_bytes();
    let n = b.len();
    let mut ws = pos;
    while ws < n && matches!(b[ws], b' ' | b'\t') {
        ws += 1;
    }
    if b[ws..].starts_with(tag) {
        let after = ws + tag.len();
        let ends_ident = text[after..]
            .chars()
            .next()
            .is_none_or(|c| !is_hcl_id_continue(c));
        if ends_ident {
            return Some(after);
        }
    }
    None
}

fn scan_heredoc_scope(
    text: &str,
    i: &mut usize,
    scopes: &mut Vec<WidthScope>,
    snapshots: &mut Vec<Vec<WidthScope>>,
    worklist: &mut Vec<BacktrackBranch>,
) -> Result<(), String> {
    let WidthScope::Heredoc {
        tag_start,
        tag_end,
        seen_body_line,
        empty_close,
        saw_marker,
        work_at_open,
        snapshot_idx,
    } = *scopes.last().expect("caller matched Heredoc")
    else {
        return Ok(());
    };
    let b = text.as_bytes();
    let n = b.len();
    let tag = &b[tag_start..tag_end];
    if (*i == 0 || b[*i - 1] == b'\n')
        && let Some(after) = match_heredoc_delimiter(text, *i, tag)
    {
        if seen_body_line {
            // If an unescaped `${` / `%{` appeared inside this heredoc after an
            // empty-heredoc candidate on line 1, `hcl-edit`'s `opt(...)` could
            // have backtracked mid-file (e.g. `%{x}`, `${<}`, `${"`). Queue the
            // empty-close branch so both interpretations are bounded.
            if saw_marker && let (Some(resume_at), Some(idx)) = (empty_close, snapshot_idx) {
                worklist.push((snapshots[idx].clone(), resume_at, work_at_open));
            }
            *i = after;
            scopes.pop();
            set_expr_op(scopes, false, false);
            return Ok(());
        }
        // Line 1 matches `[space0]<delim>[!id_continue]`. Record it as the
        // empty-heredoc fallback (matching `hcl-edit`'s `opt(...)` in
        // `heredoc_template`) and continue scanning for a greedy body-line
        // terminator.
        if empty_close.is_none() {
            let mut snap = scopes.clone();
            snap.pop();
            set_expr_op(&mut snap, false, false);
            let idx = snapshots.len();
            snapshots.push(snap);
            if let Some(WidthScope::Heredoc {
                empty_close,
                snapshot_idx,
                ..
            }) = scopes.last_mut()
            {
                *empty_close = Some(after);
                *snapshot_idx = Some(idx);
            }
        }
    }
    while *i < n {
        match b[*i] {
            b'\n' => {
                *i += 1;
                if let Some(WidthScope::Heredoc { seen_body_line, .. }) = scopes.last_mut() {
                    *seen_body_line = true;
                }
                break;
            }
            b'$' | b'%' => {
                if scan_template_marker(b, i, scopes)? {
                    break;
                }
            }
            _ => *i += 1,
        }
    }
    Ok(())
}

fn set_expr_op(scopes: &mut [WidthScope], in_expr: bool, last_was_op: bool) {
    match scopes.last_mut() {
        Some(WidthScope::BlockBrace {
            in_expr: cur_in_expr,
            last_was_op: cur_last,
            last_was_dot,
        }) => {
            if in_expr {
                *cur_in_expr = true;
            }
            *cur_last = last_was_op;
            *last_was_dot = false;
        }
        Some(WidthScope::ObjectBrace {
            last_was_op: cur_last,
            last_was_dot,
            ..
        }) => {
            *cur_last = last_was_op;
            *last_was_dot = false;
        }
        _ => {}
    }
}

fn set_expr_dot(scopes: &mut [WidthScope]) {
    match scopes.last_mut() {
        Some(WidthScope::BlockBrace {
            in_expr,
            last_was_op,
            last_was_dot,
        }) => {
            *in_expr = true;
            *last_was_op = true;
            *last_was_dot = true;
        }
        Some(WidthScope::ObjectBrace {
            last_was_op,
            last_was_dot,
            ..
        }) => {
            *last_was_op = true;
            *last_was_dot = true;
        }
        _ => {}
    }
}

fn take_last_was_dot(scopes: &mut [WidthScope]) -> bool {
    match scopes.last_mut() {
        Some(
            WidthScope::BlockBrace { last_was_dot, .. }
            | WidthScope::ObjectBrace { last_was_dot, .. },
        ) => {
            let was = *last_was_dot;
            *last_was_dot = false;
            was
        }
        _ => false,
    }
}

fn scan_heredoc_opener(
    text: &str,
    i: &mut usize,
    scopes: &mut Vec<WidthScope>,
    total_work: u64,
) -> Result<(), String> {
    let b = text.as_bytes();
    let mut j = *i + 2;
    if b.get(j) == Some(&b'-') {
        j += 1;
    }
    let tag_start = j;
    if let Some(first) = text[j..].chars().next()
        && is_hcl_id_start(first)
    {
        j += first.len_utf8();
        while let Some(ch) = text[j..].chars().next() {
            if is_hcl_id_continue(ch) {
                j += ch.len_utf8();
            } else {
                break;
            }
        }
    }
    let tag_end = j;
    if tag_end > tag_start {
        if b.get(j) == Some(&b'\r') {
            j += 1;
        }
        if b.get(j) == Some(&b'\n') {
            note_object_key_token(scopes, *i);
            *i = j + 1;
            push_scope(
                scopes,
                WidthScope::Heredoc {
                    tag_start,
                    tag_end,
                    seen_body_line: false,
                    empty_close: None,
                    saw_marker: false,
                    work_at_open: total_work,
                    snapshot_idx: None,
                },
            )?;
            return Ok(());
        }
    }
    // Not a valid `<<[-]<ident>\r?\n` heredoc opener: consume ONLY the first
    // `<` byte as a binary `<` operator so adjacent `<<<EOT` (`<` followed by
    // `<<EOT\n`) still recognizes the heredoc opener on the next iteration.
    note_object_key_token(scopes, *i);
    set_expr_op(scopes, true, true);
    *i += 1;
    Ok(())
}

fn skip_hcl_ident(text: &str, idx: usize) -> Option<usize> {
    let first = text[idx..].chars().next()?;
    if !is_hcl_id_start(first) {
        return None;
    }
    let mut end = idx + first.len_utf8();
    while let Some(ch) = text[end..].chars().next() {
        if is_hcl_id_continue(ch) {
            end += ch.len_utf8();
        } else {
            break;
        }
    }
    Some(end)
}

/// Whether the `{` at `brace_pos` opens an HCL `for` object expression
/// (`{ for k [, v] in <expr> : ... }`), which parses as `ForExpr` rather than
/// an `Object` (`VecMap`). Matching `for <ident> [, <ident>] in` prevents both
/// false-positive `in_ancestor_key` throttling on `{ for k, v in { ... } : ... }`
/// and spoofing of real object literals.
fn is_for_object_intro(text: &str, brace_pos: usize) -> bool {
    let b = text.as_bytes();
    let mut idx = skip_hcl_ws(b, brace_pos + 1);
    let Some(after_for) = skip_hcl_ident(text, idx) else {
        return false;
    };
    if &text[idx..after_for] != "for" {
        return false;
    }
    idx = skip_hcl_ws(b, after_for);
    let Some(after_first) = skip_hcl_ident(text, idx) else {
        return false;
    };
    idx = skip_hcl_ws(b, after_first);
    if b.get(idx) == Some(&b',') {
        idx = skip_hcl_ws(b, idx + 1);
        let Some(after_second) = skip_hcl_ident(text, idx) else {
            return false;
        };
        idx = skip_hcl_ws(b, after_second);
    }
    let Some(after_in) = skip_hcl_ident(text, idx) else {
        return false;
    };
    &text[idx..after_in] == "in"
}

fn open_brace_scope(text: &str, pos: usize, scopes: &mut Vec<WidthScope>) -> Result<(), String> {
    let next_scope = match scopes.last() {
        Some(WidthScope::BlockBrace { in_expr: false, .. }) => WidthScope::BlockBrace {
            in_expr: false,
            last_was_op: false,
            last_was_dot: false,
        },
        _ if is_for_object_intro(text, pos) => {
            note_object_key_token(scopes, pos);
            WidthScope::ForBrace
        }
        _ => {
            note_object_key_token(scopes, pos);
            let in_ancestor_key = scopes.iter().any(|s| {
                matches!(
                    s,
                    WidthScope::ObjectBrace {
                        in_value: false,
                        ..
                    }
                )
            });
            if in_ancestor_key {
                let key_obj_depth = scopes
                    .iter()
                    .filter(|s| {
                        matches!(
                            s,
                            WidthScope::ObjectBrace {
                                in_ancestor_key: true,
                                ..
                            }
                        )
                    })
                    .count();
                if key_obj_depth >= MAX_HCL_KEY_OBJECT_DEPTH {
                    return Err(format!(
                        "HCL object key nesting exceeds the maximum supported depth ({MAX_HCL_KEY_OBJECT_DEPTH})"
                    ));
                }
            }
            WidthScope::ObjectBrace {
                count: 0,
                key_bytes: 0,
                key_start: pos + 1,
                key_seen: false,
                in_value: false,
                last_was_op: false,
                last_was_dot: false,
                ternary_depth: 0,
                current_key_eq_cost: 0,
                max_child_eq_cost: 1,
                in_ancestor_key,
            }
        }
    };
    push_scope(scopes, next_scope)
}

fn close_brace_scope(scopes: &mut Vec<WidthScope>) {
    if scopes.len() <= 1
        || !matches!(
            scopes.last(),
            Some(
                WidthScope::BlockBrace { .. }
                    | WidthScope::ObjectBrace { .. }
                    | WidthScope::ForBrace
                    | WidthScope::InterpBrace
            )
        )
    {
        return;
    }
    match scopes.pop() {
        Some(WidthScope::ObjectBrace {
            count,
            max_child_eq_cost,
            in_ancestor_key,
            ..
        }) => {
            if in_ancestor_key {
                let pairs =
                    ((count as u64).saturating_mul((count as u64).saturating_add(1)) / 2).max(1);
                let self_eq_cost = pairs.saturating_mul(max_child_eq_cost.max(1));
                if let Some(WidthScope::ObjectBrace {
                    in_value,
                    current_key_eq_cost,
                    max_child_eq_cost: parent_max,
                    ..
                }) = scopes
                    .iter_mut()
                    .rev()
                    .find(|s| matches!(s, WidthScope::ObjectBrace { .. }))
                {
                    *parent_max = (*parent_max).max(self_eq_cost);
                    if !*in_value {
                        *current_key_eq_cost = current_key_eq_cost.saturating_add(self_eq_cost);
                    }
                }
            }
            set_expr_op(scopes, false, false);
        }
        Some(WidthScope::ForBrace) => set_expr_op(scopes, false, false),
        _ => {}
    }
}

fn scan_delim_token(
    text: &str,
    byte: u8,
    pos: usize,
    scopes: &mut Vec<WidthScope>,
) -> Result<(), String> {
    match byte {
        b'{' => open_brace_scope(text, pos, scopes)?,
        b'}' => close_brace_scope(scopes),
        b'[' => {
            note_object_key_token(scopes, pos);
            push_scope(scopes, WidthScope::Bracket)?;
        }
        b'(' => {
            note_object_key_token(scopes, pos);
            push_scope(scopes, WidthScope::Paren)?;
        }
        b']' if matches!(scopes.last(), Some(WidthScope::Bracket)) => {
            scopes.pop();
            set_expr_op(scopes, false, false);
        }
        b')' if matches!(scopes.last(), Some(WidthScope::Paren)) => {
            scopes.pop();
            set_expr_op(scopes, false, false);
        }
        _ => {}
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn scan_code_token(
    text: &str,
    i: &mut usize,
    scopes: &mut Vec<WidthScope>,
    total_work: &mut u64,
) -> Result<(), String> {
    let b = text.as_bytes();
    let n = b.len();
    match b[*i] {
        b' ' | b'\t' | b'\r' => *i += 1,
        b'\n' => {
            match scopes.last_mut() {
                Some(WidthScope::BlockBrace {
                    in_expr,
                    last_was_op,
                    last_was_dot,
                }) if !*last_was_op => {
                    *in_expr = false;
                    *last_was_dot = false;
                }
                Some(WidthScope::ObjectBrace {
                    in_value: true,
                    last_was_op: false,
                    ternary_depth: 0,
                    ..
                }) => {
                    reset_object_entry(scopes, *i + 1);
                }
                _ => {}
            }
            *i += 1;
        }
        b'#' => {
            while *i < n && b[*i] != b'\n' {
                *i += 1;
            }
        }
        b'/' if b.get(*i + 1) == Some(&b'/') => {
            while *i < n && b[*i] != b'\n' {
                *i += 1;
            }
        }
        b'/' if b.get(*i + 1) == Some(&b'*') => {
            *i += 2;
            while *i + 1 < n && !(b[*i] == b'*' && b[*i + 1] == b'/') {
                *i += 1;
            }
            *i = (*i + 2).min(n);
        }
        b'"' => {
            note_object_key_token(scopes, *i);
            *i += 1;
            push_scope(scopes, WidthScope::String)?;
        }
        b'<' if b.get(*i + 1) == Some(&b'<') => {
            scan_heredoc_opener(text, i, scopes, *total_work)?;
        }
        b'{' | b'}' | b'[' | b']' | b'(' | b')' => {
            scan_delim_token(text, b[*i], *i, scopes)?;
            *i += 1;
        }
        b',' => {
            if matches!(scopes.last(), Some(WidthScope::ObjectBrace { .. })) {
                reset_object_entry(scopes, *i + 1);
            } else {
                set_expr_op(scopes, true, true);
            }
            *i += 1;
        }
        b'.' => {
            note_object_key_token(scopes, *i);
            set_expr_dot(scopes);
            *i += 1;
        }
        b'*' => {
            note_object_key_token(scopes, *i);
            if take_last_was_dot(scopes) {
                // Legacy attribute splat (`a.*`): completes the traversal
                // rather than acting as a binary `*` operator.
                set_expr_op(scopes, false, false);
            } else {
                set_expr_op(scopes, true, true);
            }
            *i += 1;
        }
        b'?' => {
            note_object_key_token(scopes, *i);
            if let Some(WidthScope::ObjectBrace {
                in_value: true,
                ternary_depth,
                ..
            }) = scopes.last_mut()
            {
                *ternary_depth = ternary_depth.saturating_add(1);
            }
            set_expr_op(scopes, true, true);
            *i += 1;
        }
        b'!' | b'<' | b'>' if b.get(*i + 1) == Some(&b'=') => {
            note_object_key_token(scopes, *i);
            set_expr_op(scopes, true, true);
            *i += 2;
        }
        b'=' => {
            if matches!(b.get(*i + 1), Some(b'=' | b'>')) {
                note_object_key_token(scopes, *i);
                set_expr_op(scopes, true, true);
                *i += 2;
            } else {
                let pos = *i;
                set_expr_op(scopes, true, true);
                *i += 1;
                bump_object_entry(scopes, pos, total_work)?;
            }
        }
        b':' => {
            if b.get(*i + 1) == Some(&b':') {
                note_object_key_token(scopes, *i);
                set_expr_op(scopes, false, true);
                *i += 2;
            } else if let Some(WidthScope::ObjectBrace {
                in_value: true,
                ternary_depth,
                ..
            }) = scopes.last_mut()
                && *ternary_depth > 0
            {
                *ternary_depth -= 1;
                set_expr_op(scopes, true, true);
                *i += 1;
            } else {
                let pos = *i;
                set_expr_op(scopes, true, true);
                *i += 1;
                bump_object_entry(scopes, pos, total_work)?;
            }
        }
        b'+' | b'-' | b'/' | b'%' | b'!' | b'<' | b'>' | b'&' | b'|' => {
            note_object_key_token(scopes, *i);
            set_expr_op(scopes, true, true);
            *i += 1;
        }
        _ => {
            note_object_key_token(scopes, *i);
            if let Some(first) = text[*i..].chars().next()
                && is_hcl_id_start(first)
            {
                // Consume a complete HCL identifier (including `-` and Unicode
                // XID continue chars) so hyphenated block types/labels like
                // `env us-east-1 { ... }` and trailing-`-` identifiers never
                // trip the binary `-` operator path.
                *i += first.len_utf8();
                while let Some(ch) = text[*i..].chars().next() {
                    if is_hcl_id_continue(ch) {
                        *i += ch.len_utf8();
                    } else {
                        break;
                    }
                }
            } else {
                *i += 1;
            }
            set_expr_op(scopes, false, false);
        }
    }
    Ok(())
}

/// Iterative, zero-recursion scan counting `=` / `:` assignments directly
/// inside each `{ ... }` object expression (plus cumulative key-comparison and
/// nested-object-key complexity) while skipping comments, double-quoted string
/// bodies, heredoc bodies, and `%{if}`/`%{for}` directive bodies (still
/// descending into `${...}` / `%{...}` template expressions).
fn check_object_width(text: &str) -> Result<(), String> {
    let b = text.as_bytes();
    let n = b.len();
    let mut i = 0usize;
    let mut total_work = 0u64;
    let mut rescan_budget = MAX_HCL_RESCAN_BYTES;
    let mut snapshots: Vec<Vec<WidthScope>> = Vec::new();
    let mut worklist: Vec<BacktrackBranch> = Vec::new();
    let mut scopes: Vec<WidthScope> = vec![WidthScope::BlockBrace {
        in_expr: false,
        last_was_op: false,
        last_was_dot: false,
    }];

    loop {
        while i < n {
            match *scopes.last().expect("root BlockBrace always present") {
                WidthScope::String => scan_string_scope(text, &mut i, &mut scopes)?,
                WidthScope::TemplateDirective => {
                    scan_directive_body_scope(b, &mut i, &mut scopes)?;
                }
                WidthScope::Heredoc { .. } => {
                    scan_heredoc_scope(text, &mut i, &mut scopes, &mut snapshots, &mut worklist)?;
                }
                WidthScope::BlockBrace { .. }
                | WidthScope::ObjectBrace { .. }
                | WidthScope::ForBrace
                | WidthScope::Bracket
                | WidthScope::Paren
                | WidthScope::InterpBrace => {
                    scan_code_token(text, &mut i, &mut scopes, &mut total_work)?;
                }
            }
        }

        // If EOF was reached while ANY enclosing heredoc had a line-1
        // empty-heredoc fallback (even if an inner `InterpBrace` / `String` /
        // `TemplateDirective` was left unclosed above it), emulate `hcl-edit`'s
        // `opt(...)` fallback and resume right after line 1's delimiter.
        if let Some(heredoc_idx) = scopes.iter().rposition(|s| {
            matches!(
                s,
                WidthScope::Heredoc {
                    empty_close: Some(_),
                    ..
                }
            )
        }) && let WidthScope::Heredoc {
            empty_close: Some(resume_at),
            work_at_open,
            snapshot_idx: Some(snap_idx),
            ..
        } = scopes[heredoc_idx]
        {
            let cost = n.saturating_sub(resume_at);
            if cost > rescan_budget {
                return Err(format!(
                    "HCL heredoc backtracking exceeds the maximum supported complexity ({MAX_HCL_RESCAN_BYTES} bytes)"
                ));
            }
            rescan_budget -= cost;
            scopes = snapshots[snap_idx].clone();
            total_work = work_at_open;
            i = resume_at;
            continue;
        }

        // Also scan any deferred empty-heredoc branch recorded when a heredoc
        // containing `${` / `%{` markers closed on a later line (covering
        // mid-file winnow `opt(...)` backtracking such as `# %{x}` or `# ${<}`).
        if let Some((next_scopes, resume_at, work_at_open)) = worklist.pop() {
            let cost = n.saturating_sub(resume_at);
            if cost > rescan_budget {
                return Err(format!(
                    "HCL heredoc backtracking exceeds the maximum supported complexity ({MAX_HCL_RESCAN_BYTES} bytes)"
                ));
            }
            rescan_budget -= cost;
            scopes = next_scopes;
            total_work = work_at_open;
            i = resume_at;
            continue;
        }
        break;
    }

    Ok(())
}

/// Enforce cumulative [`MAX_HCL_BLOCK_DEPTH`] across nested blocks and their
/// labels on a parsed `hcl::edit::structure::Body`.
fn check_edit_body_depth(body: &hcl::edit::structure::Body, depth: usize) -> Result<(), String> {
    for structure in body {
        if let Some(block) = structure.as_block() {
            let next_depth = depth.saturating_add(1).saturating_add(block.labels.len());
            if next_depth > MAX_HCL_BLOCK_DEPTH {
                return Err(format!(
                    "HCL block and label nesting exceeds the maximum supported depth ({MAX_HCL_BLOCK_DEPTH})"
                ));
            }
            check_edit_body_depth(&block.body, next_depth)?;
        }
    }
    Ok(())
}

enum JsonNode {
    Map(BTreeMap<String, JsonNode>),
    Body(Vec<hcl::edit::structure::Body>),
    Expr(hcl::edit::expr::Expression),
}

impl JsonNode {
    fn deep_merge_into(self, map: &mut BTreeMap<String, JsonNode>, key: String) {
        match map.entry(key) {
            Entry::Occupied(mut occupied) => occupied.get_mut().deep_merge(self),
            Entry::Vacant(vacant) => {
                vacant.insert(self);
            }
        }
    }

    fn deep_merge(&mut self, other: JsonNode) {
        match (self, other) {
            (JsonNode::Map(lhs), JsonNode::Map(rhs)) => {
                for (key, node) in rhs {
                    node.deep_merge_into(lhs, key);
                }
            }
            (JsonNode::Body(lhs), JsonNode::Body(mut rhs)) => {
                lhs.append(&mut rhs);
            }
            (lhs, rhs) => *lhs = rhs,
        }
    }

    fn into_value(self, depth: usize) -> Result<Value, String> {
        match self {
            JsonNode::Map(map) => {
                let mut out = Map::new();
                for (key, node) in map {
                    out.insert(key, node.into_value(depth.saturating_add(1))?);
                }
                Ok(Value::Object(out))
            }
            JsonNode::Body(mut bodies) => {
                if bodies.len() == 1 {
                    body_to_value(bodies.remove(0), depth)
                } else {
                    let mut arr = Vec::with_capacity(bodies.len());
                    for body in bodies {
                        arr.push(body_to_value(body, depth)?);
                    }
                    Ok(Value::Array(arr))
                }
            }
            JsonNode::Expr(expr) => edit_expr_to_value(expr),
        }
    }
}

fn body_to_value(body: hcl::edit::structure::Body, depth: usize) -> Result<Value, String> {
    let mut map: BTreeMap<String, JsonNode> = BTreeMap::new();
    for structure in body {
        match structure {
            hcl::edit::structure::Structure::Attribute(attr) => {
                map.insert(
                    attr.key.into_value().into_string(),
                    JsonNode::Expr(attr.value),
                );
            }
            hcl::edit::structure::Structure::Block(block) => {
                let next_depth = depth.saturating_add(1).saturating_add(block.labels.len());
                if next_depth > MAX_HCL_BLOCK_DEPTH {
                    return Err(format!(
                        "HCL block and label nesting exceeds the maximum supported depth ({MAX_HCL_BLOCK_DEPTH})"
                    ));
                }
                let mut node = JsonNode::Body(vec![block.body]);
                for label in block.labels.into_iter().rev() {
                    let key = match label {
                        hcl::edit::structure::BlockLabel::Ident(ident) => {
                            ident.into_value().into_string()
                        }
                        hcl::edit::structure::BlockLabel::String(s) => s.into_value(),
                    };
                    let mut wrapper = BTreeMap::new();
                    wrapper.insert(key, node);
                    node = JsonNode::Map(wrapper);
                }
                node.deep_merge_into(&mut map, block.ident.into_value().into_string());
            }
        }
    }
    JsonNode::Map(map).into_value(depth)
}

/// Convert an `hcl::edit::expr::Expression` directly into `serde_json::Value`
/// matching `hcl::from_str::<Value>`, avoiding intermediate `hcl::expr::Object`
/// `VecMap` reconstructions on object literals.
fn edit_expr_to_value(expr: hcl::edit::expr::Expression) -> Result<Value, String> {
    use hcl::edit::expr::Expression as E;
    match expr {
        E::Null(_) => Ok(Value::Null),
        E::Bool(b) => Ok(Value::Bool(b.into_value())),
        E::Number(n) => {
            let num: hcl::Number = n.into_value();
            Value::deserialize(hcl::Expression::Number(num)).map_err(|e| e.to_string())
        }
        E::String(s) => Ok(Value::String(s.into_value())),
        E::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for elem in arr {
                out.push(edit_expr_to_value(elem)?);
            }
            Ok(Value::Array(out))
        }
        E::Object(obj) => {
            let mut out = Map::new();
            for (key, val) in obj {
                let k = edit_object_key_to_string(key)?;
                let v = edit_expr_to_value(val.into_expr())?;
                out.insert(k, v);
            }
            Ok(Value::Object(out))
        }
        E::Parenthesis(parens) => edit_expr_to_value(parens.into_inner()),
        other => Value::deserialize(hcl::Expression::from(other)).map_err(|e| e.to_string()),
    }
}

fn edit_object_key_to_string(key: hcl::edit::expr::ObjectKey) -> Result<String, String> {
    match key {
        hcl::edit::expr::ObjectKey::Ident(ident) => Ok(ident.into_value().into_string()),
        hcl::edit::expr::ObjectKey::Expression(expr) => edit_key_expr_to_string(expr),
    }
}

fn edit_key_expr_to_string(expr: hcl::edit::expr::Expression) -> Result<String, String> {
    use hcl::edit::expr::Expression as E;
    match expr {
        E::String(s) => Ok(s.into_value()),
        E::Parenthesis(parens) => edit_key_expr_to_string(parens.into_inner()),
        E::Null(_) => Err("invalid type: unit, expected a string".to_string()),
        E::Bool(b) => Err(format!(
            "invalid type: boolean `{}`, expected a string",
            b.into_value()
        )),
        E::Number(_) => Err("invalid type: number, expected a string".to_string()),
        E::Array(_) => Err("invalid type: sequence, expected a string".to_string()),
        E::Object(_) => Err("invalid type: map, expected a string".to_string()),
        other => String::deserialize(hcl::Expression::from(other)).map_err(|e| e.to_string()),
    }
}
