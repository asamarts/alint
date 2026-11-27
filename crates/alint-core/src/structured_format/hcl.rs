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
use std::rc::Rc;

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

/// Maximum deferred empty-heredoc backtrack branches explored across one file.
/// Bounds peak worklist memory even when ambiguous empty heredocs with template
/// markers cluster near EOF (where each branch's remaining byte cost is small).
const MAX_HCL_BACKTRACK_BRANCHES: usize = 512;

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

#[derive(Debug, Clone)]
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
        /// Unparenthesized `? ... :` ternary nesting depth inside a key or value.
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
        /// Line-1 empty-heredoc fallback `(resume_at, scope_snapshot)`. Held
        /// directly inside the active heredoc scope via `Rc` so non-backtracking
        /// heredocs reclaim their snapshot immediately on close instead of
        /// accumulating across the file.
        empty_close: Option<(usize, Rc<[WidthScope]>)>,
        /// Whether an unescaped `${` or `%{` appeared inside this heredoc (only
        /// template markers can trigger mid-file winnow `opt(...)` backtracking).
        saw_marker: bool,
        /// `total_work` captured when this heredoc opened, restored on empty-close
        /// backtrack so scanned expressions inside the discarded branch are not
        /// double-charged.
        work_at_open: u64,
    },
    /// Inside a `%{if ...}...%{endif}` or `%{for ...}...%{endfor}` directive
    /// body, where `hcl-edit` (`parser/template.rs`) ignores `"` and outer
    /// heredoc terminators and only stops on `${` / `%{`.
    TemplateDirective,
}

/// Deferred empty-heredoc branch `(scopes, resume_at, work_at_open)`.
type BacktrackBranch = (Rc<[WidthScope]>, usize, u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ScopeShape {
    BlockBrace {
        in_expr: bool,
        last_was_op: bool,
        last_was_dot: bool,
    },
    ObjectBrace {
        in_value: bool,
        last_was_op: bool,
        last_was_dot: bool,
        ternary_depth: usize,
        in_ancestor_key: bool,
    },
    ForBrace,
    Bracket,
    Paren,
    InterpBrace,
    String,
    Heredoc {
        tag_start: usize,
        tag_end: usize,
        seen_body_line: bool,
        saw_marker: bool,
    },
    TemplateDirective,
}

#[derive(Debug, Clone)]
struct ObjectScopeCost {
    count: usize,
    key_bytes: u64,
    current_key_eq_cost: u64,
    max_child_eq_cost: u64,
}

#[derive(Debug, Clone)]
struct BranchCost {
    total_work: u64,
    object_costs: Vec<ObjectScopeCost>,
}

impl BranchCost {
    fn capture(total_work: u64, scopes: &[WidthScope]) -> Self {
        let mut object_costs = Vec::new();
        for scope in scopes {
            if let WidthScope::ObjectBrace {
                count,
                key_bytes,
                current_key_eq_cost,
                max_child_eq_cost,
                ..
            } = scope
            {
                object_costs.push(ObjectScopeCost {
                    count: *count,
                    key_bytes: *key_bytes,
                    current_key_eq_cost: *current_key_eq_cost,
                    max_child_eq_cost: *max_child_eq_cost,
                });
            }
        }
        Self {
            total_work,
            object_costs,
        }
    }

    fn dominates(&self, other: &Self) -> bool {
        self.total_work >= other.total_work
            && self.object_costs.len() == other.object_costs.len()
            && self
                .object_costs
                .iter()
                .zip(&other.object_costs)
                .all(|(a, b)| {
                    a.count >= b.count
                        && a.key_bytes >= b.key_bytes
                        && a.current_key_eq_cost >= b.current_key_eq_cost
                        && a.max_child_eq_cost >= b.max_child_eq_cost
                })
    }
}

fn scope_shape(scopes: &[WidthScope]) -> Vec<ScopeShape> {
    scopes
        .iter()
        .map(|scope| match scope {
            WidthScope::BlockBrace {
                in_expr,
                last_was_op,
                last_was_dot,
            } => ScopeShape::BlockBrace {
                in_expr: *in_expr,
                last_was_op: *last_was_op,
                last_was_dot: *last_was_dot,
            },
            WidthScope::ObjectBrace {
                in_value,
                last_was_op,
                last_was_dot,
                ternary_depth,
                in_ancestor_key,
                ..
            } => ScopeShape::ObjectBrace {
                in_value: *in_value,
                last_was_op: *last_was_op,
                last_was_dot: *last_was_dot,
                ternary_depth: *ternary_depth,
                in_ancestor_key: *in_ancestor_key,
            },
            WidthScope::ForBrace => ScopeShape::ForBrace,
            WidthScope::Bracket => ScopeShape::Bracket,
            WidthScope::Paren => ScopeShape::Paren,
            WidthScope::InterpBrace => ScopeShape::InterpBrace,
            WidthScope::String => ScopeShape::String,
            WidthScope::Heredoc {
                tag_start,
                tag_end,
                seen_body_line,
                saw_marker,
                ..
            } => ScopeShape::Heredoc {
                tag_start: *tag_start,
                tag_end: *tag_end,
                seen_body_line: *seen_body_line,
                saw_marker: *saw_marker,
            },
            WidthScope::TemplateDirective => ScopeShape::TemplateDirective,
        })
        .collect()
}

struct BacktrackState {
    rescan_budget: usize,
    branches: usize,
    worklist: Vec<BacktrackBranch>,
    visited: std::collections::HashMap<(usize, Vec<ScopeShape>), Vec<BranchCost>>,
}

impl BacktrackState {
    fn new() -> Self {
        Self {
            rescan_budget: MAX_HCL_RESCAN_BYTES,
            branches: 0,
            worklist: Vec::new(),
            visited: std::collections::HashMap::new(),
        }
    }

    /// Record `(resume_at, scopes, work_at_open)` and charge budgets if this
    /// state is not already dominated by a previously scheduled branch.
    /// Returns `Ok(true)` when the caller should scan/enqueue this branch.
    fn schedule(
        &mut self,
        n: usize,
        resume_at: usize,
        scopes: &[WidthScope],
        work_at_open: u64,
    ) -> Result<bool, String> {
        let key = (resume_at, scope_shape(scopes));
        let cost = BranchCost::capture(work_at_open, scopes);
        let entries = self.visited.entry(key).or_default();
        if entries.iter().any(|prev| prev.dominates(&cost)) {
            return Ok(false);
        }
        let rescan_cost = n.saturating_sub(resume_at);
        if self.branches >= MAX_HCL_BACKTRACK_BRANCHES || rescan_cost > self.rescan_budget {
            return Err(format!(
                "HCL heredoc backtracking exceeds the maximum supported complexity ({MAX_HCL_RESCAN_BYTES} bytes)"
            ));
        }
        entries.retain(|prev| !cost.dominates(prev));
        entries.push(cost);
        self.branches += 1;
        self.rescan_budget -= rescan_cost;
        Ok(true)
    }
}

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
    backtrack: &mut BacktrackState,
) -> Result<(), String> {
    let Some(&WidthScope::Heredoc {
        tag_start,
        tag_end,
        seen_body_line,
        ref empty_close,
        ..
    }) = scopes.last()
    else {
        return Ok(());
    };
    let has_empty_close = empty_close.is_some();
    let b = text.as_bytes();
    let n = b.len();
    let tag = &b[tag_start..tag_end];
    if (*i == 0 || b[*i - 1] == b'\n')
        && let Some(after) = match_heredoc_delimiter(text, *i, tag)
    {
        if seen_body_line {
            let popped = scopes.pop();
            // If an unescaped `${` / `%{` appeared inside this heredoc after an
            // empty-heredoc candidate on line 1, `hcl-edit`'s `opt(...)` could
            // have backtracked mid-file (e.g. `%{x}`, `${<}`, `${"`). Deduplicate
            // dominated states so sequential valid heredocs stay O(k), and charge
            // budgets eagerly before queueing so adversarial inputs stay bounded.
            if let Some(WidthScope::Heredoc {
                saw_marker: true,
                empty_close: Some((resume_at, snap)),
                work_at_open,
                ..
            }) = popped
                && backtrack.schedule(n, resume_at, &snap, work_at_open)?
            {
                backtrack.worklist.push((snap, resume_at, work_at_open));
            }
            *i = after;
            set_expr_op(scopes, false, false);
            return Ok(());
        }
        // Line 1 matches `[space0]<delim>[!id_continue]`. Record it as the
        // empty-heredoc fallback (matching `hcl-edit`'s `opt(...)` in
        // `heredoc_template`) and continue scanning for a greedy body-line
        // terminator.
        if !has_empty_close {
            let prefix_len = scopes.len().saturating_sub(1);
            let mut snap = Vec::with_capacity(prefix_len);
            snap.extend_from_slice(&scopes[..prefix_len]);
            set_expr_op(&mut snap, false, false);
            let snap: Rc<[WidthScope]> = Rc::from(snap);
            if let Some(WidthScope::Heredoc { empty_close, .. }) = scopes.last_mut() {
                *empty_close = Some((after, snap));
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
            if let Some(WidthScope::ObjectBrace { ternary_depth, .. }) = scopes.last_mut() {
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
            } else if let Some(WidthScope::ObjectBrace { ternary_depth, .. }) = scopes.last_mut()
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
            if let Some(first) = text[*i..].chars().next() {
                *i += first.len_utf8();
                if is_hcl_id_start(first) {
                    // Consume a complete HCL identifier (including `-` and Unicode
                    // XID continue chars) so hyphenated block types/labels like
                    // `env us-east-1 { ... }` and trailing-`-` identifiers never
                    // trip the binary `-` operator path.
                    while let Some(ch) = text[*i..].chars().next() {
                        if is_hcl_id_continue(ch) {
                            *i += ch.len_utf8();
                        } else {
                            break;
                        }
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
    let mut backtrack = BacktrackState::new();
    let mut scopes: Vec<WidthScope> = vec![WidthScope::BlockBrace {
        in_expr: false,
        last_was_op: false,
        last_was_dot: false,
    }];

    loop {
        while i < n {
            match scopes.last().expect("root BlockBrace always present") {
                WidthScope::String => scan_string_scope(text, &mut i, &mut scopes)?,
                WidthScope::TemplateDirective => {
                    scan_directive_body_scope(b, &mut i, &mut scopes)?;
                }
                WidthScope::Heredoc { .. } => {
                    scan_heredoc_scope(text, &mut i, &mut scopes, &mut backtrack)?;
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

        // If EOF was reached while ANY enclosing heredoc had an unvisited
        // line-1 empty-heredoc fallback (even if an inner `InterpBrace` /
        // `String` / `TemplateDirective` was left unclosed above it), emulate
        // `hcl-edit`'s `opt(...)` fallback and resume right after line 1's
        // delimiter.
        let mut eof_fallback: Option<(Rc<[WidthScope]>, usize, u64)> = None;
        for scope in scopes.iter().rev() {
            if let WidthScope::Heredoc {
                empty_close: Some((resume_at, snap)),
                work_at_open,
                ..
            } = scope
                && backtrack.schedule(n, *resume_at, snap, *work_at_open)?
            {
                eof_fallback = Some((Rc::clone(snap), *resume_at, *work_at_open));
                break;
            }
        }
        if let Some((next_scopes, resume_at, work_at_open)) = eof_fallback {
            scopes = next_scopes.to_vec();
            total_work = work_at_open;
            i = resume_at;
            continue;
        }

        // Also scan any deferred empty-heredoc branch recorded when a heredoc
        // containing `${` / `%{` markers closed on a later line (covering
        // mid-file winnow `opt(...)` backtracking such as `# %{x}` or `# ${<}`).
        // Its rescan and branch budgets were already charged eagerly on push.
        if let Some((next_scopes, resume_at, work_at_open)) = backtrack.worklist.pop() {
            scopes = next_scopes.to_vec();
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

pub(crate) fn body_to_value(
    body: hcl::edit::structure::Body,
    depth: usize,
) -> Result<Value, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_ascii_non_identifier_punctuation_never_panics() {
        for input in [
            "a = “smart”\n",
            "a = 1 — 2\n",
            "a = €100\n",
            "a = 🔥\n",
            "a =\u{00a0}1\n",
            "“\n",
        ] {
            // Must return Ok/Err cleanly — never panic on a UTF-8 continuation byte.
            let _ = check_limits(input);
            let _ = hcl_to_value(input);
        }
    }

    #[test]
    fn repeated_empty_heredoc_candidates_stay_bounded_in_memory() {
        // Vector 2A: non-marker heredocs matching line 1 must reclaim snapshots
        // immediately on close rather than retaining O(N * depth) snapshots.
        let mut no_marker = "(".repeat(250);
        no_marker.push('\n');
        for _ in 0..4_000 {
            no_marker.push_str("<<A\nA\nA\n");
        }
        assert!(check_limits(&no_marker).is_ok());

        // Vector 2B: marker-bearing ambiguous heredocs near EOF must hit the
        // eager branch/rescan cap before queueing unbounded worklist entries.
        let mut with_marker = "(".repeat(250);
        with_marker.push('\n');
        for _ in 0..2_000 {
            with_marker.push_str("<<A\nA${}\nA\n");
        }
        let err = check_limits(&with_marker).unwrap_err();
        assert!(err.contains("HCL heredoc backtracking"), "got: {err}");
    }

    #[test]
    fn sequential_valid_heredocs_with_line1_prefix_and_interpolation_do_not_blow_up() {
        use std::fmt::Write as _;

        // 15 valid sequential heredocs whose line 1 starts with the delimiter
        // followed by punctuation and an interpolation (`Note: ${var.a}`).
        // Without state deduplication, this generated 2^15 - 1 branches and
        // falsely tripped MAX_HCL_BACKTRACK_BRANCHES (512).
        let mut hcl = String::new();
        for idx in 0..15 {
            let _ = writeln!(hcl, "m{idx} = <<Note\nNote: ${{var.a}}\nNote");
        }
        let val = hcl_to_value(&hcl).expect("15 valid sequential heredocs must parse cleanly");
        assert_eq!(val.as_object().map(Map::len), Some(15));
    }

    #[test]
    fn ternary_in_object_key_keeps_key_position_and_enforces_key_object_limits() {
        use std::fmt::Write as _;

        // `{ true ? 1 : <inner_obj> = 1 }` must NOT let `:` flip `in_value`
        // before the outer `=`, so the inner object stays subject to
        // `MAX_HCL_KEY_OBJECT_ITEMS` (64).
        let mut inner = String::from("{ ");
        for idx in 0..=64 {
            let _ = write!(inner, "a{idx} = 0, ");
        }
        inner.push('}');
        let hcl = format!("x = {{ true ? 1 : {inner} = 1 }}\n");
        let err = check_limits(&hcl).unwrap_err();
        assert!(
            err.contains("64 entries"),
            "expected key-object width limit (64 entries), got: {err}"
        );
    }
}
