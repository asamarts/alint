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
//!    `hcl::Body` and `serde_json::Value` (~100-125x peak RSS amplification on
//!    dense arrays/traversals), so capping input at 4 MiB (matching
//!    `MAX_DOTENV_BYTES`) keeps worst-case peak RSS under ~500 MiB while
//!    accepting real-world large Terraform/Nomad/Packer files.
//! 2. [`MAX_HCL_OBJECT_ITEMS`] (4,096 per `{ ... }` scope): `hcl-edit` stores
//!    object literal entries in a `VecMap` with linear duplicate checks, which
//!    scales quadratically on a single giant `{ k0 = 1, k1 = 1, ... }` literal.
//! 3. Direct `BTreeMap`-backed `Body -> Value` conversion (bypassing
//!    `hcl::from_str`'s `IntoJsonSpec -> Expression::Object(VecMap)` `O(N^2)`
//!    path on bodies/blocks) with [`MAX_HCL_BLOCK_LABELS`] (64) bounding
//!    block-label nesting depth.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use serde::Deserialize as _;
use serde_json::{Map, Value};

/// Maximum HCL input size accepted for parsing (4 MiB, matching
/// `MAX_DOTENV_BYTES`). Bounds lossless-CST peak RSS amplification (~100-125x
/// on dense inputs -> <= ~500 MiB) while comfortably admitting large real-world
/// Terraform, Nomad, and Packer files.
pub const MAX_HCL_BYTES: usize = 4 * 1024 * 1024;

/// Maximum `=` / `:` entry assignments inside a single `{ ... }` brace scope.
/// `hcl-edit` 0.10 validates object-key uniqueness with `VecMap::contains_key`
/// (`O(N^2)`), so a single object literal with tens of thousands of keys stalls
/// parsing for seconds to minutes. Top-level attributes and separate sibling
/// blocks are unaffected (each block has its own scope).
pub(crate) const MAX_HCL_OBJECT_ITEMS: usize = 4_096;

/// Maximum number of labels on a single HCL block. `hcl-edit` parses block
/// labels iteratively into a `Vec`, but the HCL JSON specification nests each
/// label into an object level; bounding label count to 64 (matching
/// `hcl-edit`'s `MAX_NESTING_DEPTH`) prevents a crafted `b "a" "a" ... {}`
/// block from overflowing the stack during value conversion or `Value::drop`.
pub(crate) const MAX_HCL_BLOCK_LABELS: usize = 64;

/// Validate HCL input against size and per-brace-scope width limits before
/// invoking `hcl-edit` / `hcl-rs`.
pub(crate) fn check_limits(text: &str) -> Result<(), String> {
    if text.len() > MAX_HCL_BYTES {
        return Err(format!(
            "HCL input exceeds the maximum supported size ({MAX_HCL_BYTES} bytes)"
        ));
    }
    check_object_width(text)
}

/// Parse HCL into a spanned `hcl::edit::structure::Body` after enforcing
/// [`check_limits`]. Used by `structured_fix::hcl` so fixes share the same
/// size and width bounds as checks.
pub(crate) fn parse_edit_body(text: &str) -> Result<hcl::edit::structure::Body, String> {
    check_limits(text)?;
    hcl::edit::parser::parse_body(text).map_err(|e| e.to_string())
}

/// Parse HCL (Terraform / Nomad / Packer) into the uniform `serde_json::Value`
/// tree matching the HCL JSON Specification (`hcl::from_str::<Value>`): blocks
/// nest by type then labels (`resource "t" "n" { ... }` -> `$.resource.t.n`),
/// a repeated block type/label pair becomes an array, values keep their HCL
/// type, and unevaluated expressions arrive as opaque strings.
pub(crate) fn hcl_to_value(text: &str) -> Result<Value, String> {
    check_limits(text)?;
    let body = hcl::parse(text).map_err(|e| e.to_string())?;
    body_to_value(body)
}

enum WidthScope {
    Brace(usize),
    String,
    Heredoc { tag_start: usize, tag_end: usize },
}

fn bump_brace_count(scopes: &mut [WidthScope]) -> Result<(), String> {
    if let Some(WidthScope::Brace(count)) = scopes.last_mut() {
        *count += 1;
        if *count > MAX_HCL_OBJECT_ITEMS {
            return Err(format!(
                "HCL object or block exceeds the maximum supported width ({MAX_HCL_OBJECT_ITEMS} entries)"
            ));
        }
    }
    Ok(())
}

fn scan_string_scope(b: &[u8], i: &mut usize, scopes: &mut Vec<WidthScope>) {
    let n = b.len();
    while *i < n {
        match b[*i] {
            b'\\' => *i = (*i + 2).min(n),
            b'"' => {
                *i += 1;
                scopes.pop();
                break;
            }
            b'$' | b'%' => match b.get(*i + 1) {
                Some(&b'{') => {
                    *i += 2;
                    scopes.push(WidthScope::Brace(0));
                    break;
                }
                Some(&next) if next == b[*i] => *i += 2,
                _ => *i += 1,
            },
            _ => *i += 1,
        }
    }
}

fn scan_heredoc_scope(
    text: &str,
    tag_start: usize,
    tag_end: usize,
    i: &mut usize,
    scopes: &mut Vec<WidthScope>,
) {
    let b = text.as_bytes();
    let n = b.len();
    let tag = &b[tag_start..tag_end];
    if *i == 0 || b[*i - 1] == b'\n' {
        let mut ws = *i;
        while ws < n && matches!(b[ws], b' ' | b'\t') {
            ws += 1;
        }
        if b[ws..].starts_with(tag) {
            let after = ws + tag.len();
            let ends_ident = text[after..]
                .chars()
                .next()
                .is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '_' | '-')));
            if ends_ident {
                *i = after;
                scopes.pop();
                return;
            }
        }
    }
    while *i < n {
        match b[*i] {
            b'\n' => {
                *i += 1;
                break;
            }
            b'$' | b'%' => match b.get(*i + 1) {
                Some(&b'{') => {
                    *i += 2;
                    scopes.push(WidthScope::Brace(0));
                    break;
                }
                Some(&next) if next == b[*i] => *i += 2,
                _ => *i += 1,
            },
            _ => *i += 1,
        }
    }
}

/// Iterative, zero-recursion scan counting `=` / `:` assignments directly
/// inside each `{ ... }` scope while skipping comments, double-quoted string
/// bodies, and heredoc bodies (still descending into `${...}` / `%{...}`
/// template interpolations inside strings and heredocs).
fn check_object_width(text: &str) -> Result<(), String> {
    let b = text.as_bytes();
    let n = b.len();
    let mut i = 0usize;
    let mut scopes: Vec<WidthScope> = Vec::new();

    while i < n {
        match scopes.last() {
            Some(WidthScope::String) => scan_string_scope(b, &mut i, &mut scopes),
            Some(&WidthScope::Heredoc { tag_start, tag_end }) => {
                scan_heredoc_scope(text, tag_start, tag_end, &mut i, &mut scopes);
            }
            None | Some(WidthScope::Brace(_)) => match b[i] {
                b'#' => {
                    while i < n && b[i] != b'\n' {
                        i += 1;
                    }
                }
                b'/' if b.get(i + 1) == Some(&b'/') => {
                    while i < n && b[i] != b'\n' {
                        i += 1;
                    }
                }
                b'/' if b.get(i + 1) == Some(&b'*') => {
                    i += 2;
                    while i + 1 < n && !(b[i] == b'*' && b[i + 1] == b'/') {
                        i += 1;
                    }
                    i = (i + 2).min(n);
                }
                b'"' => {
                    i += 1;
                    scopes.push(WidthScope::String);
                }
                b'<' if b.get(i + 1) == Some(&b'<') => {
                    let mut j = i + 2;
                    if b.get(j) == Some(&b'-') {
                        j += 1;
                    }
                    let tag_start = j;
                    while j < n && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'_' | b'-')) {
                        j += 1;
                    }
                    let tag_end = j;
                    if tag_end > tag_start {
                        if b.get(j) == Some(&b'\r') {
                            j += 1;
                        }
                        if b.get(j) == Some(&b'\n') {
                            i = j + 1;
                            scopes.push(WidthScope::Heredoc { tag_start, tag_end });
                        } else {
                            i = tag_end;
                        }
                    } else {
                        i += 2;
                    }
                }
                b'{' => {
                    scopes.push(WidthScope::Brace(0));
                    i += 1;
                }
                b'}' => {
                    if matches!(scopes.last(), Some(WidthScope::Brace(_))) {
                        scopes.pop();
                    }
                    i += 1;
                }
                b'!' | b'<' | b'>' if b.get(i + 1) == Some(&b'=') => {
                    i += 2;
                }
                b'=' => {
                    if matches!(b.get(i + 1), Some(b'=' | b'>')) {
                        i += 2;
                    } else {
                        i += 1;
                        bump_brace_count(&mut scopes)?;
                    }
                }
                b':' => {
                    if b.get(i + 1) == Some(&b':') {
                        i += 2;
                    } else {
                        i += 1;
                        bump_brace_count(&mut scopes)?;
                    }
                }
                _ => i += 1,
            },
        }
    }

    Ok(())
}

enum JsonNode {
    Map(BTreeMap<String, JsonNode>),
    Body(Vec<hcl::Body>),
    Expr(hcl::Expression),
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

    fn into_value(self) -> Result<Value, String> {
        match self {
            JsonNode::Map(map) => {
                let mut out = Map::new();
                for (key, node) in map {
                    out.insert(key, node.into_value()?);
                }
                Ok(Value::Object(out))
            }
            JsonNode::Body(mut bodies) => {
                if bodies.len() == 1 {
                    body_to_value(bodies.remove(0))
                } else {
                    let mut arr = Vec::with_capacity(bodies.len());
                    for body in bodies {
                        arr.push(body_to_value(body)?);
                    }
                    Ok(Value::Array(arr))
                }
            }
            JsonNode::Expr(expr) => Value::deserialize(expr).map_err(|e| e.to_string()),
        }
    }
}

fn body_to_value(body: hcl::Body) -> Result<Value, String> {
    let mut map: BTreeMap<String, JsonNode> = BTreeMap::new();
    for structure in body {
        match structure {
            hcl::Structure::Attribute(attr) => {
                map.insert(attr.key.into_inner(), JsonNode::Expr(attr.expr));
            }
            hcl::Structure::Block(block) => {
                if block.labels.len() > MAX_HCL_BLOCK_LABELS {
                    return Err(format!(
                        "HCL block exceeds the maximum supported label count ({MAX_HCL_BLOCK_LABELS})"
                    ));
                }
                let mut node = JsonNode::Body(vec![block.body]);
                for label in block.labels.into_iter().rev() {
                    let mut wrapper = BTreeMap::new();
                    wrapper.insert(label.into_inner(), node);
                    node = JsonNode::Map(wrapper);
                }
                node.deep_merge_into(&mut map, block.identifier.into_inner());
            }
        }
    }
    JsonNode::Map(map).into_value()
}
