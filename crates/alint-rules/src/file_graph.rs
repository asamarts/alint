//! `file_graph` — assemble the repo's *file → file* reference
//! graph from path-based edges and assert a global structural
//! property. The graph layer the 1-level cross-file kinds
//! (`registry_paths_resolve`, `import_gate`, `pair_hash`) can't
//! express. `require:` modes: `acyclic` (no dependency cycle),
//! `forbidden_edges` (layering firewall), `no_dangling` (every
//! edge resolves to an existing path), `no_orphans` (no
//! unreferenced node, save declared `roots`), and `fresh` (a
//! generated file embeds its source's current content hash).
//!
//! The four reference-graph modes use `from_content` edges: edges
//! are extracted from each node's content (`crate::extract`,
//! regex/structured/lines) and *resolved as paths* — relative to
//! the referencing file or the repo root. Bare module specifiers
//! (no leading `.` under `relative_to_file`), absolute paths,
//! URLs, and computed/interpolated references are **dropped, not
//! mis-resolved**: resolving module *names* is the package-graph
//! non-goal. `fresh` instead uses `derive_target` edges (a name
//! template, source → generated file) and a content-hash marker —
//! never mtime, which is meaningless on a fresh clone (see
//! `pair_hash`, whose digest machinery it reuses).
//!
//! Pure-parse and extraction-based — it never shells out, so it
//! stays out of `SPAWNING_RULE_KINDS`. Cross-file: needs the whole
//! index (`requires_full_index`), never `--changed`-scoped.
//! Design + rationale: `docs/design/v0.12/file_dependency_graph.md`.
//!
//! ```yaml
//! # Layering — domain code must not reach into infra.
//! - id: domain-not-depend-on-infra
//!   kind: file_graph
//!   nodes: "src/**/*.ts"
//!   edges:
//!     from_content:
//!       extract: { regex: 'from\s+"(\.[^"]+)"' }
//!       resolve: relative_to_file        # | relative_to_repo_root
//!   require:
//!     forbidden_edges:
//!       - { from: "src/domain/**", to: "src/infra/**" }
//!
//! # Acyclicity — the clearest capability gap (nothing else does it).
//! - id: no-proto-import-cycles
//!   kind: file_graph
//!   nodes: "proto/**/*.proto"
//!   edges:
//!     from_content:
//!       extract: { regex: 'import\s+"([^"]+)"' }
//!       resolve: relative_to_repo_root
//!   require: acyclic
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::slice;

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use regex::Regex;
use serde::Deserialize;

use crate::extract::{Extract, ExtractSpec, extract_values, is_non_literal};
use crate::pair_hash::Algorithm;

/// How a content-extracted reference string is turned into a path.
#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Resolve {
    /// Join the reference onto the referencing file's directory.
    /// Only explicitly-relative refs (leading `.`) are resolved;
    /// bare specifiers (module names) are dropped.
    #[default]
    RelativeToFile,
    /// Treat the reference as a path from the repo root (the proto
    /// `import "a/b.proto"` shape).
    RelativeToRepoRoot,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FromContentSpec {
    extract: ExtractSpec,
    #[serde(default)]
    resolve: Resolve,
}

/// The `edges:` block — exactly one extractor (validated in
/// `build`): `from_content` (the reference-graph modes) or
/// `derive_target` (a name template → `fresh` codegen-freshness or
/// `no_dangling` derived-sibling existence).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgesSpec {
    #[serde(default)]
    from_content: Option<FromContentSpec>,
    #[serde(default)]
    derive_target: Option<DeriveTargetSpec>,
}

/// Name-template edges: derive the generated target's path from the
/// source node's path — `from` is a regex matched against the node
/// path, `to` is a replacement template using its captures (e.g.
/// `from: '(.*)\.proto'`, `to: '$1.pb.go'`; a constant `to` maps
/// many sources to one target). The `fresh` mode's edge.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeriveTargetSpec {
    from: String,
    to: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ForbiddenEdgeSpec {
    from: String,
    to: String,
}

/// `require:` is either a bare string (`acyclic`) or a map
/// (`{ forbidden_edges: [...] }`). A scalar and a map are
/// structurally distinct, so an untagged enum decodes them
/// unambiguously (the proven `cross_file_value_equals` `targets:`
/// pattern — not the externally-tagged-enum-from-map trap
/// `crate::extract` documents).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RequireSpec {
    Named(NamedRequire),
    Map(RequireMap),
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum NamedRequire {
    Acyclic,
    /// Every path-shaped reference must resolve to a path on disk.
    NoDangling,
    /// No node is unreferenced (bare form — no entry-point roots).
    NoOrphans,
}

/// The map form of `require:`. A struct-of-options (validated to
/// exactly-one in `build`) rather than an externally-tagged enum,
/// so it decodes from a YAML map cleanly. The bare-string modes
/// (`acyclic`, `no_dangling`, `no_orphans` with no roots) live in
/// `NamedRequire`; the configured modes live here, and future ones
/// (`fresh: { … }`) join as additional `Option` fields.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequireMap {
    #[serde(default)]
    forbidden_edges: Option<Vec<ForbiddenEdgeSpec>>,
    #[serde(default)]
    no_orphans: Option<NoOrphansSpec>,
    #[serde(default)]
    fresh: Option<FreshSpec>,
}

/// Options for the `no_orphans` map form: `roots` lists globs whose
/// nodes are allowed to be unreferenced (graph entry points). Bare
/// `require: no_orphans` (no roots) is the `NamedRequire` form.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoOrphansSpec {
    #[serde(default)]
    roots: Vec<String>,
}

/// Options for the `fresh` map form: the `derive_target` output must
/// embed the source's current `hash` digest, captured by `marker`
/// (a regex whose group 1 is the hex digest). Content-hash, never
/// mtime — portable on a fresh clone.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FreshSpec {
    #[serde(default)]
    hash: Algorithm,
    marker: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    nodes: String,
    edges: EdgesSpec,
    require: RequireSpec,
}

/// A resolved forbidden-edge prohibition: an edge whose source
/// matches `from` and whose (resolved) target matches `to` is a
/// violation. The raw globs are kept for the message.
#[derive(Debug)]
struct ForbiddenPattern {
    from: Scope,
    to: Scope,
    from_glob: String,
    to_glob: String,
}

/// The resolved structural assertion.
#[derive(Debug)]
enum Require {
    Acyclic,
    /// Every path-shaped edge must resolve to an existing path.
    NoDangling,
    /// No node is unreferenced, except those matching a `roots` glob.
    NoOrphans {
        roots: Option<Scope>,
    },
    ForbiddenEdges(Vec<ForbiddenPattern>),
    /// The `derive_target` output must embed the source's current
    /// digest, captured by `marker` (group 1).
    Fresh {
        algo: Algorithm,
        marker: Regex,
    },
}

/// How edges are formed — paired with `require` in `build`:
/// `FromContent` ⇒ the reference-graph modes; `DeriveTarget` ⇒
/// `fresh`.
#[derive(Debug)]
enum EdgeSource {
    FromContent { extract: Extract, resolve: Resolve },
    DeriveTarget { from: Regex, to: String },
}

#[derive(Debug)]
pub struct FileGraphRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    nodes: Scope,
    edges: EdgeSource,
    require: Require,
}

impl Rule for FileGraphRule {
    alint_core::rule_common_impl!();

    fn requires_full_index(&self) -> bool {
        // Cross-file: the graph spans the whole tree (an edge can
        // point at any node, a cycle can route through any file),
        // so the rule must see the full index, never a
        // `--changed`-scoped subset.
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        // Stable node ordering → byte-identical violation output
        // (the snapshot discipline the parallel walker upholds).
        let mut nodes: Vec<PathBuf> = ctx
            .index
            .files()
            .filter(|e| self.nodes.matches(&e.path, ctx.index))
            .map(|e| e.path.to_path_buf())
            .collect();
        nodes.sort();

        Ok(match &self.require {
            Require::ForbiddenEdges(pats) => self.check_forbidden(ctx, &nodes, pats),
            Require::Acyclic => self.check_acyclic(ctx, &nodes),
            Require::NoDangling => self.check_no_dangling(ctx, &nodes),
            Require::NoOrphans { roots } => self.check_no_orphans(ctx, &nodes, roots.as_ref()),
            Require::Fresh { algo, marker } => self.check_fresh(ctx, &nodes, *algo, marker),
        })
    }
}

impl FileGraphRule {
    /// One violation per (node, target) edge that any prohibition
    /// matches. A node no `from` glob selects is never even read.
    fn check_forbidden(
        &self,
        ctx: &Context<'_>,
        nodes: &[PathBuf],
        pats: &[ForbiddenPattern],
    ) -> Vec<Violation> {
        let mut out = Vec::new();
        for node in nodes {
            let applicable: Vec<&ForbiddenPattern> = pats
                .iter()
                .filter(|p| p.from.matches(node, ctx.index))
                .collect();
            if applicable.is_empty() {
                continue;
            }
            let mut targets = self.node_targets(ctx, node, &mut out);
            targets.sort_unstable();
            targets.dedup();
            for target in &targets {
                for p in &applicable {
                    if p.to.matches(target, ctx.index) {
                        out.push(self.forbidden_violation(node, target, p));
                    }
                }
            }
        }
        out
    }

    /// One violation per distinct dependency cycle among the
    /// nodes. Only node → node edges form the graph (an edge to a
    /// non-node file can't be part of a node cycle).
    fn check_acyclic(&self, ctx: &Context<'_>, nodes: &[PathBuf]) -> Vec<Violation> {
        let mut out = Vec::new();
        let index_of: HashMap<&Path, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, p)| (p.as_path(), i))
            .collect();

        let mut adj: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (i, node) in nodes.iter().enumerate() {
            let mut neigh: Vec<usize> = self
                .node_targets(ctx, node, &mut out)
                .iter()
                .filter_map(|t| index_of.get(t.as_path()).copied())
                .filter(|&j| j != i) // drop degenerate self-loops
                .collect();
            neigh.sort_unstable();
            neigh.dedup();
            if !neigh.is_empty() {
                adj.insert(i, neigh);
            }
        }

        for cycle in collect_cycles(&adj, nodes.len()) {
            out.push(self.cycle_violation(nodes, &cycle));
        }
        out
    }

    /// One violation per path-shaped edge whose resolved target
    /// exists nowhere in the index (as a file or a directory).
    /// References that aren't path-shaped (bare module names, URLs)
    /// are dropped, not flagged — `no_dangling` is a
    /// reference-integrity check, not a module resolver.
    fn check_no_dangling(&self, ctx: &Context<'_>, nodes: &[PathBuf]) -> Vec<Violation> {
        let mut out = Vec::new();
        let dirs: HashSet<&Path> = ctx.index.dirs().map(|e| &*e.path).collect();
        for node in nodes {
            let mut targets = self.node_targets(ctx, node, &mut out);
            targets.sort_unstable();
            targets.dedup();
            for target in &targets {
                let exists = ctx.index.contains_file(target) || dirs.contains(target.as_path());
                if !exists {
                    out.push(self.dangling_violation(node, target));
                }
            }
        }
        out
    }

    /// One violation per node that no *other* node references, unless
    /// it matches a `roots` glob (a declared graph entry point).
    /// Reverse-edge analysis over the node → node sub-graph.
    fn check_no_orphans(
        &self,
        ctx: &Context<'_>,
        nodes: &[PathBuf],
        roots: Option<&Scope>,
    ) -> Vec<Violation> {
        let mut out = Vec::new();
        let node_set: HashSet<&Path> = nodes.iter().map(PathBuf::as_path).collect();

        let mut referenced: HashSet<PathBuf> = HashSet::new();
        for node in nodes {
            for target in self.node_targets(ctx, node, &mut out) {
                // A self-reference can't un-orphan a node; only an
                // edge from a *different* node counts.
                if target.as_path() != node.as_path() && node_set.contains(target.as_path()) {
                    referenced.insert(target);
                }
            }
        }

        for node in nodes {
            if referenced.contains(node) || roots.is_some_and(|r| r.matches(node, ctx.index)) {
                continue;
            }
            out.push(self.orphan_violation(node));
        }
        out
    }

    /// Resolve a node to its outgoing edge targets. For `from_content`
    /// edges this reads the node and resolves every content reference
    /// to a path (unreadable / unparseable nodes push a violation and
    /// yield no edges; references that don't resolve to a path are
    /// dropped). For `derive_target` edges it derives the single target
    /// from the node *path* (no file read): the node through the
    /// `from`→`to` capture template; a node the `from` regex doesn't
    /// match has no edge. The reference-graph modes (`no_dangling`,
    /// `acyclic`, `no_orphans`, `forbidden`) all flow through here;
    /// `fresh` uses `check_fresh` directly (it also needs the digest).
    fn node_targets(
        &self,
        ctx: &Context<'_>,
        node: &Path,
        out: &mut Vec<Violation>,
    ) -> Vec<PathBuf> {
        if let EdgeSource::DeriveTarget { from, to } = &self.edges {
            let node_str = node.to_string_lossy();
            let Some(caps) = from.captures(&node_str) else {
                return Vec::new();
            };
            let mut derived = String::new();
            caps.expand(to, &mut derived);
            let Some(target) = crate::pathsafe::normalize_confined(Path::new(&derived)) else {
                out.push(Self::node_violation(
                    node,
                    &format!("derives the out-of-repo target {derived:?} (escapes the repo root)"),
                ));
                return Vec::new();
            };
            return vec![target];
        }
        let EdgeSource::FromContent { extract, resolve } = &self.edges else {
            return Vec::new();
        };
        let abs = ctx.root.join(node);
        let text = match crate::io::read_capped(&abs) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(crate::io::ReadCapError::TooLarge(n)) => {
                out.push(Self::node_violation(
                    node,
                    &format!("is too large to analyze ({})", crate::io::over_cap(n)),
                ));
                return Vec::new();
            }
            Err(crate::io::ReadCapError::Io(e)) => {
                out.push(Self::node_violation(
                    node,
                    &format!("could not be read: {e}"),
                ));
                return Vec::new();
            }
        };
        let refs = match extract_values(extract, &text) {
            Ok(v) => v,
            Err(e) => {
                out.push(Self::node_violation(
                    node,
                    &format!("edge extraction failed: {e}"),
                ));
                return Vec::new();
            }
        };
        refs.iter()
            .filter(|r| !is_non_literal(r))
            .filter_map(|r| resolve_ref(r, node, *resolve))
            .collect()
    }

    /// One violation per stale `derive_target` output: the source's
    /// current digest is not present as a `marker`-captured value in
    /// the derived file (or the derived file is missing). Sources
    /// whose path doesn't match the `from` regex are skipped.
    fn check_fresh(
        &self,
        ctx: &Context<'_>,
        nodes: &[PathBuf],
        algo: Algorithm,
        marker: &Regex,
    ) -> Vec<Violation> {
        let EdgeSource::DeriveTarget { from, to } = &self.edges else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for source in nodes {
            let src_str = source.to_string_lossy();
            let Some(caps) = from.captures(&src_str) else {
                continue; // not a codegen source this template covers
            };
            let mut target = String::new();
            caps.expand(to, &mut target);
            // Confine before any read: an absolute or root-escaping
            // `to:` template must never read a file outside the tree.
            let Some(target) = crate::pathsafe::normalize_confined(Path::new(&target)) else {
                out.push(Self::node_violation(
                    source,
                    &format!(
                        "derives the out-of-repo freshness target {target:?} (escapes the repo root)"
                    ),
                ));
                continue;
            };

            let src_bytes = match crate::io::read_capped(&ctx.root.join(source)) {
                Ok(b) => b,
                Err(e) => {
                    out.push(Self::node_violation(source, &read_cap_reason(&e)));
                    continue;
                }
            };
            let digest = algo.hex(&src_bytes);

            let Ok(tgt_bytes) = crate::io::read_capped(&ctx.root.join(&target)) else {
                out.push(self.fresh_violation(
                    &target,
                    &format!(
                        "is missing or unreadable (the derived output for {})",
                        crate::slash(source)
                    ),
                ));
                continue;
            };
            let tgt_text = String::from_utf8_lossy(&tgt_bytes).into_owned();
            let fresh = marker
                .captures_iter(&tgt_text)
                .filter_map(|c| c.get(1))
                .any(|m| m.as_str() == digest);
            if !fresh {
                out.push(self.fresh_violation(
                    &target,
                    &format!(
                        "is out of date with {}: it carries no {} freshness marker matching the \
                         source's current digest (regenerate it)",
                        crate::slash(source),
                        algo.label(),
                    ),
                ));
            }
        }
        out
    }

    fn node_violation(node: &Path, reason: &str) -> Violation {
        Violation::new(format!("file_graph node {} {reason}", crate::slash(node)))
            .with_path(node.to_path_buf())
    }

    fn forbidden_violation(&self, src: &Path, target: &Path, pat: &ForbiddenPattern) -> Violation {
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "{} has a forbidden dependency edge to {} (forbidden_edges: from {:?} to {:?})",
                crate::slash(src),
                crate::slash(target),
                pat.from_glob,
                pat.to_glob,
            )
        });
        Violation::new(msg).with_path(src.to_path_buf())
    }

    fn cycle_violation(&self, nodes: &[PathBuf], cycle: &[usize]) -> Violation {
        let mut rendered: String = cycle
            .iter()
            .map(|&i| crate::slash(&nodes[i]))
            .collect::<Vec<_>>()
            .join(" \u{2192} ");
        // Close the loop so the cycle reads unambiguously.
        rendered.push_str(" \u{2192} ");
        rendered.push_str(&crate::slash(&nodes[cycle[0]]));
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("dependency cycle ({} files): {rendered}", cycle.len()));
        Violation::new(msg).with_path(nodes[cycle[0]].clone())
    }

    fn dangling_violation(&self, src: &Path, target: &Path) -> Violation {
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "{} references {}, which does not resolve to any path on disk",
                crate::slash(src),
                crate::slash(target),
            )
        });
        Violation::new(msg).with_path(src.to_path_buf())
    }

    fn orphan_violation(&self, node: &Path) -> Violation {
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "{} is an orphan: no other node references it (and it is not a declared root)",
                crate::slash(node),
            )
        });
        Violation::new(msg).with_path(node.to_path_buf())
    }

    /// A stale / missing `derive_target` output. Anchored on the
    /// target (the file that needs regenerating); `reason` already
    /// names the source.
    fn fresh_violation(&self, target: &Path, reason: &str) -> Violation {
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("{} {reason}", crate::slash(target)));
        Violation::new(msg).with_path(target.to_path_buf())
    }
}

/// The user-facing reason fragment for a capped-read failure.
fn read_cap_reason(e: &crate::io::ReadCapError) -> String {
    match e {
        crate::io::ReadCapError::TooLarge(n) => {
            format!("is too large to analyze ({})", crate::io::over_cap(*n))
        }
        crate::io::ReadCapError::Io(e) => format!("could not be read: {e}"),
    }
}

/// Resolve a content reference to a normalised, repo-relative path,
/// or `None` when it is not a path we should follow (a bare module
/// name, an absolute path, a URL, or one that escapes the root).
fn resolve_ref(reference: &str, from_file: &Path, mode: Resolve) -> Option<PathBuf> {
    let reference = reference.trim();
    if reference.is_empty() {
        return None;
    }
    let joined = match mode {
        Resolve::RelativeToFile => {
            // Only explicitly-relative references are filesystem
            // paths; a bare `foo/bar` is a module specifier.
            if !reference.starts_with('.') {
                return None;
            }
            let base = from_file.parent().unwrap_or_else(|| Path::new(""));
            base.join(reference)
        }
        Resolve::RelativeToRepoRoot => {
            if reference.starts_with('/') || reference.contains("://") {
                return None;
            }
            PathBuf::from(reference)
        }
    };
    // Confine to the repo root: rejects absolute paths and every `..`
    // escape (including the double-dot cancellation `../../x` a
    // first-component check misses). `None` → not a path we follow.
    crate::pathsafe::normalize_confined(&joined)
}

/// A representative set of directed cycles in `adj` (node indices
/// `0..n`), each canonicalised (rotated to start at its smallest
/// index, so the same cycle always reports identically) and the
/// whole set sorted. Iterative DFS — no recursion-depth limit on
/// deep graphs.
///
/// This is *not* an enumeration of every distinct simple cycle
/// (that is exponential and needs Johnson's algorithm); it records
/// one cycle per DFS back-edge — enough to surface every file that
/// participates in a cycle. A node already fully explored (BLACK)
/// is not revisited, so a cycle reachable only through it from a
/// later DFS root may go unlisted even though its members are
/// flagged via another cycle.
fn collect_cycles(adj: &BTreeMap<usize, Vec<usize>>, n: usize) -> Vec<Vec<usize>> {
    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;

    let mut state = vec![WHITE; n];
    let mut cycles: BTreeSet<Vec<usize>> = BTreeSet::new();
    let empty: Vec<usize> = Vec::new();

    for start in 0..n {
        if state[start] != WHITE {
            continue;
        }
        let mut path: Vec<usize> = vec![start];
        let mut next_child: Vec<usize> = vec![0];
        state[start] = GRAY;

        while let Some(&node) = path.last() {
            let neighbors = adj.get(&node).unwrap_or(&empty);
            let child = next_child[path.len() - 1];
            if child < neighbors.len() {
                next_child[path.len() - 1] += 1;
                let next = neighbors[child];
                match state[next] {
                    WHITE => {
                        state[next] = GRAY;
                        path.push(next);
                        next_child.push(0);
                    }
                    GRAY => {
                        // Back-edge: the cycle is the path suffix
                        // from `next` to the current node.
                        if let Some(pos) = path.iter().position(|&x| x == next) {
                            cycles.insert(canonical_cycle(&path[pos..]));
                        }
                    }
                    _ => {} // BLACK: fully explored, no new cycle.
                }
            } else {
                state[node] = BLACK;
                path.pop();
                next_child.pop();
            }
        }
    }
    cycles.into_iter().collect()
}

/// Rotate a cycle so its smallest node index leads (direction
/// preserved), giving every rotation of the same cycle one
/// canonical form.
fn canonical_cycle(cycle: &[usize]) -> Vec<usize> {
    let min_pos = cycle
        .iter()
        .enumerate()
        .min_by_key(|&(_, &v)| v)
        .map_or(0, |(i, _)| i);
    let mut out = Vec::with_capacity(cycle.len());
    out.extend_from_slice(&cycle[min_pos..]);
    out.extend_from_slice(&cycle[..min_pos]);
    out
}

/// Resolve the map form of `require:` — exactly one of
/// `forbidden_edges` / `no_orphans` (the bare-string modes are
/// resolved in `build`).
fn resolve_map_require(map: RequireMap, cfg: &impl Fn(String) -> Error) -> Result<Require> {
    let set = [
        map.forbidden_edges.is_some(),
        map.no_orphans.is_some(),
        map.fresh.is_some(),
    ];
    if set.iter().filter(|&&on| on).count() != 1 {
        return Err(cfg(
            "`require` map must set exactly one of `forbidden_edges` / `no_orphans` / `fresh`"
                .into(),
        ));
    }

    if let Some(edges) = map.forbidden_edges {
        if edges.is_empty() {
            return Err(cfg(
                "`require.forbidden_edges` must list at least one {from, to} pattern".into(),
            ));
        }
        let mut pats = Vec::with_capacity(edges.len());
        for (i, e) in edges.into_iter().enumerate() {
            if e.from.trim().is_empty() || e.to.trim().is_empty() {
                return Err(cfg(format!(
                    "`require.forbidden_edges[{i}]` needs a non-empty `from` and `to`"
                )));
            }
            let from = Scope::from_patterns(slice::from_ref(&e.from))
                .map_err(|err| cfg(format!("invalid `forbidden_edges[{i}].from` glob: {err}")))?;
            let to = Scope::from_patterns(slice::from_ref(&e.to))
                .map_err(|err| cfg(format!("invalid `forbidden_edges[{i}].to` glob: {err}")))?;
            pats.push(ForbiddenPattern {
                from,
                to,
                from_glob: e.from,
                to_glob: e.to,
            });
        }
        return Ok(Require::ForbiddenEdges(pats));
    }

    if let Some(spec) = map.no_orphans {
        if spec.roots.iter().any(|r| r.trim().is_empty()) {
            return Err(cfg(
                "`require.no_orphans.roots` entries must not be empty".into()
            ));
        }
        let roots = if spec.roots.is_empty() {
            None
        } else {
            Some(
                Scope::from_patterns(&spec.roots)
                    .map_err(|err| cfg(format!("invalid `no_orphans.roots` glob: {err}")))?,
            )
        };
        return Ok(Require::NoOrphans { roots });
    }

    let fresh = map.fresh.expect("exactly-one ensures `fresh` is set");
    if fresh.marker.trim().is_empty() {
        return Err(cfg("`require.fresh.marker` must not be empty".into()));
    }
    let marker = Regex::new(&fresh.marker)
        .map_err(|e| cfg(format!("invalid `require.fresh.marker` regex: {e}")))?;
    if marker.captures_len() < 2 {
        return Err(cfg(
            "`require.fresh.marker` needs a capture group for the digest \
             (e.g. 'sha256:([0-9a-f]{64})')"
                .into(),
        ));
    }
    Ok(Require::Fresh {
        algo: fresh.hash,
        marker,
    })
}

/// Resolve the `edges:` block to its single extractor.
fn resolve_edges(edges: EdgesSpec, cfg: &impl Fn(String) -> Error) -> Result<EdgeSource> {
    match (edges.from_content, edges.derive_target) {
        (Some(_), Some(_)) => Err(cfg(
            "`edges` must set exactly one of `from_content` / `derive_target`".into(),
        )),
        (None, None) => Err(cfg(
            "`edges` must set `from_content` or `derive_target`".into()
        )),
        (Some(fc), None) => {
            let extract = fc
                .extract
                .resolve()
                .map_err(|e| cfg(format!("invalid `edges.from_content.extract`: {e}")))?;
            if let Extract::Regex(p) = &extract {
                Regex::new(p)
                    .map_err(|e| cfg(format!("invalid `edges.from_content.extract.regex`: {e}")))?;
            }
            Ok(EdgeSource::FromContent {
                extract,
                resolve: fc.resolve,
            })
        }
        (None, Some(dt)) => {
            if dt.from.trim().is_empty() || dt.to.trim().is_empty() {
                return Err(cfg(
                    "`edges.derive_target` needs a non-empty `from` and `to`".into(),
                ));
            }
            let from = Regex::new(&dt.from)
                .map_err(|e| cfg(format!("invalid `edges.derive_target.from` regex: {e}")))?;
            Ok(EdgeSource::DeriveTarget { from, to: dt.to })
        }
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    alint_core::reject_scope_filter_on_cross_file(spec, "file_graph")?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let cfg = |msg: String| Error::rule_config(&spec.id, msg);

    if opts.nodes.trim().is_empty() {
        return Err(cfg("`nodes` glob must not be empty".into()));
    }
    let nodes = Scope::from_patterns(slice::from_ref(&opts.nodes))
        .map_err(|e| cfg(format!("invalid `nodes` glob: {e}")))?;

    let edges = resolve_edges(opts.edges, &cfg)?;
    let require = match opts.require {
        RequireSpec::Named(NamedRequire::Acyclic) => Require::Acyclic,
        RequireSpec::Named(NamedRequire::NoDangling) => Require::NoDangling,
        RequireSpec::Named(NamedRequire::NoOrphans) => Require::NoOrphans { roots: None },
        RequireSpec::Map(map) => resolve_map_require(map, &cfg)?,
    };

    // The edge type and the assertion must agree: `fresh` is the
    // `derive_target` mode; every other mode is `from_content`.
    match (&edges, &require) {
        (EdgeSource::FromContent { .. }, Require::Fresh { .. }) => {
            return Err(cfg(
                "`require: fresh` needs `edges.derive_target`, not `edges.from_content`".into(),
            ));
        }
        (EdgeSource::DeriveTarget { .. }, r)
            if !matches!(r, Require::Fresh { .. } | Require::NoDangling) =>
        {
            return Err(cfg(
                "`edges.derive_target` supports `require: fresh` (codegen freshness) \
                 and `require: no_dangling` (the derived target must exist), not the \
                 content-graph modes (acyclic / no_orphans / forbidden_edges)"
                    .into(),
            ));
        }
        _ => {}
    }

    Ok(Box::new(FileGraphRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        nodes,
        edges,
        require,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{FileEntry, FileIndex};

    fn index(files: &[&str]) -> FileIndex {
        FileIndex::from_entries(
            files
                .iter()
                .map(|p| FileEntry {
                    path: Path::new(p).into(),
                    is_dir: false,
                    size: 1,
                })
                .collect(),
        )
    }

    fn scope(pat: &str) -> Scope {
        Scope::from_patterns(slice::from_ref(&pat.to_string())).expect("valid glob")
    }

    fn forbidden(
        nodes: &str,
        regex: &str,
        resolve: Resolve,
        from: &str,
        to: &str,
    ) -> FileGraphRule {
        FileGraphRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            nodes: scope(nodes),
            edges: EdgeSource::FromContent {
                extract: Extract::Regex(regex.into()),
                resolve,
            },
            require: Require::ForbiddenEdges(vec![ForbiddenPattern {
                from: scope(from),
                to: scope(to),
                from_glob: from.into(),
                to_glob: to.into(),
            }]),
        }
    }

    fn acyclic(nodes: &str, regex: &str, resolve: Resolve) -> FileGraphRule {
        FileGraphRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            nodes: scope(nodes),
            edges: EdgeSource::FromContent {
                extract: Extract::Regex(regex.into()),
                resolve,
            },
            require: Require::Acyclic,
        }
    }

    fn eval(r: &FileGraphRule, root: &Path, idx: &FileIndex) -> Vec<Violation> {
        let ctx = Context {
            root,
            index: idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        r.evaluate(&ctx).expect("evaluate ok")
    }

    #[test]
    fn forbidden_edge_fires_on_relative_import() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/domain")).unwrap();
        std::fs::create_dir_all(root.join("src/infra")).unwrap();
        // domain reaches into infra — forbidden.
        std::fs::write(
            root.join("src/domain/order.ts"),
            "import { db } from \"../infra/db\";\n",
        )
        .unwrap();
        std::fs::write(root.join("src/infra/db.ts"), "export const db = 1;\n").unwrap();
        let idx = index(&["src/domain/order.ts", "src/infra/db.ts"]);
        let r = forbidden(
            "src/**/*.ts",
            r#"from\s+"(\.[^"]+)""#,
            Resolve::RelativeToFile,
            "src/domain/**",
            "src/infra/**",
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("src/domain/order.ts"));
        assert!(v[0].message.contains("src/infra/db"));
    }

    #[test]
    fn forbidden_edge_silent_when_layering_respected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/domain")).unwrap();
        std::fs::create_dir_all(root.join("src/infra")).unwrap();
        // infra → domain is allowed; domain imports a sibling only.
        std::fs::write(
            root.join("src/domain/order.ts"),
            "import { money } from \"./money\";\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/domain/money.ts"),
            "export const money = 1;\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/infra/db.ts"),
            "import { order } from \"../domain/order\";\n",
        )
        .unwrap();
        let idx = index(&[
            "src/domain/order.ts",
            "src/domain/money.ts",
            "src/infra/db.ts",
        ]);
        let r = forbidden(
            "src/**/*.ts",
            r#"from\s+"(\.[^"]+)""#,
            Resolve::RelativeToFile,
            "src/domain/**",
            "src/infra/**",
        );
        assert!(eval(&r, root, &idx).is_empty());
    }

    #[test]
    fn bare_specifier_is_dropped_not_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/domain")).unwrap();
        // A bare module specifier that lexically contains "infra"
        // must NOT be treated as a path edge into src/infra.
        std::fs::write(
            root.join("src/domain/order.ts"),
            "import x from \"@company/infra-sdk\";\n",
        )
        .unwrap();
        let idx = index(&["src/domain/order.ts"]);
        let r = forbidden(
            "src/**/*.ts",
            r#"from\s+"([^"]+)""#,
            Resolve::RelativeToFile,
            "src/domain/**",
            "**/infra*/**",
        );
        assert!(
            eval(&r, root, &idx).is_empty(),
            "bare specifier must not resolve to a path edge",
        );
    }

    #[test]
    fn acyclic_fires_on_two_and_three_cycles() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("proto")).unwrap();
        // a -> b -> c -> a (a 3-cycle).
        std::fs::write(root.join("proto/a.proto"), "import \"proto/b.proto\";\n").unwrap();
        std::fs::write(root.join("proto/b.proto"), "import \"proto/c.proto\";\n").unwrap();
        std::fs::write(root.join("proto/c.proto"), "import \"proto/a.proto\";\n").unwrap();
        let idx = index(&["proto/a.proto", "proto/b.proto", "proto/c.proto"]);
        let r = acyclic(
            "proto/**/*.proto",
            r#"import\s+"([^"]+)""#,
            Resolve::RelativeToRepoRoot,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "one distinct cycle: {v:?}");
        assert!(v[0].message.contains("dependency cycle"));
        // Canonical: starts at the smallest path (proto/a.proto).
        assert!(
            v[0].message
                .contains("proto/a.proto \u{2192} proto/b.proto")
        );
    }

    #[test]
    fn acyclic_silent_on_a_dag() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("proto")).unwrap();
        // a -> b -> c, no back-edge.
        std::fs::write(root.join("proto/a.proto"), "import \"proto/b.proto\";\n").unwrap();
        std::fs::write(root.join("proto/b.proto"), "import \"proto/c.proto\";\n").unwrap();
        std::fs::write(root.join("proto/c.proto"), "// leaf\n").unwrap();
        let idx = index(&["proto/a.proto", "proto/b.proto", "proto/c.proto"]);
        let r = acyclic(
            "proto/**/*.proto",
            r#"import\s+"([^"]+)""#,
            Resolve::RelativeToRepoRoot,
        );
        assert!(eval(&r, root, &idx).is_empty());
    }

    #[test]
    fn self_loop_is_not_a_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("proto")).unwrap();
        std::fs::write(root.join("proto/a.proto"), "import \"proto/a.proto\";\n").unwrap();
        let idx = index(&["proto/a.proto"]);
        let r = acyclic(
            "proto/**/*.proto",
            r#"import\s+"([^"]+)""#,
            Resolve::RelativeToRepoRoot,
        );
        assert!(eval(&r, root, &idx).is_empty(), "a self-edge is degenerate");
    }

    #[test]
    fn resolve_ref_drops_non_path_references() {
        let f = Path::new("src/a/b.ts");
        // Relative-to-file: bare specifier dropped, relative kept.
        assert_eq!(
            resolve_ref("./c", f, Resolve::RelativeToFile),
            Some(PathBuf::from("src/a/c"))
        );
        assert_eq!(
            resolve_ref("../d/e", f, Resolve::RelativeToFile),
            Some(PathBuf::from("src/d/e"))
        );
        assert_eq!(resolve_ref("react", f, Resolve::RelativeToFile), None);
        // Root-escaping reference is dropped.
        assert_eq!(
            resolve_ref("../../../etc/passwd", f, Resolve::RelativeToFile),
            None
        );
        // Relative-to-root: bare path kept, absolute / URL dropped.
        assert_eq!(
            resolve_ref("a/b.proto", f, Resolve::RelativeToRepoRoot),
            Some(PathBuf::from("a/b.proto"))
        );
        assert_eq!(resolve_ref("/abs", f, Resolve::RelativeToRepoRoot), None);
        assert_eq!(
            resolve_ref("https://x/y", f, Resolve::RelativeToRepoRoot),
            None
        );
    }

    /// Generic constructor for the require modes the `forbidden` /
    /// `acyclic` helpers don't cover.
    fn mk(nodes: &str, regex: &str, resolve: Resolve, require: Require) -> FileGraphRule {
        FileGraphRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            nodes: scope(nodes),
            edges: EdgeSource::FromContent {
                extract: Extract::Regex(regex.into()),
                resolve,
            },
            require,
        }
    }

    #[test]
    fn no_dangling_fires_on_missing_then_silent_when_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/real.md"), "# real\n").unwrap();
        let r = mk(
            "docs/**/*.md",
            r"\]\((\.[^)]+)\)",
            Resolve::RelativeToFile,
            Require::NoDangling,
        );

        // a.md links a sibling that doesn't exist -> dangling.
        std::fs::write(root.join("docs/a.md"), "see [x](./missing.md)\n").unwrap();
        let idx = index(&["docs/a.md", "docs/real.md"]);
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("docs/missing.md"));
        assert!(v[0].message.contains("docs/a.md"));

        // a.md links the existing real.md -> silent.
        std::fs::write(root.join("docs/a.md"), "see [r](./real.md)\n").unwrap();
        assert!(eval(&r, root, &idx).is_empty());
    }

    #[test]
    fn no_dangling_dedups_a_repeated_edge() {
        // A node that references the same missing target twice yields ONE
        // dangling violation, not two (the duplicate edge is deduped).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        let r = mk(
            "docs/**/*.md",
            r"\]\((\.[^)]+)\)",
            Resolve::RelativeToFile,
            Require::NoDangling,
        );
        std::fs::write(
            root.join("docs/a.md"),
            "[x](./missing.md) and [y](./missing.md)\n",
        )
        .unwrap();
        let idx = index(&["docs/a.md"]);
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "repeated dangling edge deduped: {v:?}");
        assert!(v[0].message.contains("docs/missing.md"));
    }

    #[test]
    fn no_orphans_fires_on_unreferenced_node() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("proto")).unwrap();
        // a -> b -> c. Nothing references a, so a is the orphan.
        std::fs::write(root.join("proto/a.proto"), "import \"proto/b.proto\";\n").unwrap();
        std::fs::write(root.join("proto/b.proto"), "import \"proto/c.proto\";\n").unwrap();
        std::fs::write(root.join("proto/c.proto"), "// leaf\n").unwrap();
        let idx = index(&["proto/a.proto", "proto/b.proto", "proto/c.proto"]);
        let r = mk(
            "proto/**/*.proto",
            r#"import\s+"([^"]+)""#,
            Resolve::RelativeToRepoRoot,
            Require::NoOrphans { roots: None },
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "only proto/a.proto is unreferenced: {v:?}");
        assert!(v[0].message.contains("proto/a.proto"));
        assert!(v[0].message.contains("orphan"));
    }

    #[test]
    fn no_orphans_roots_exempts_entry_point() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("proto")).unwrap();
        std::fs::write(root.join("proto/a.proto"), "import \"proto/b.proto\";\n").unwrap();
        std::fs::write(root.join("proto/b.proto"), "import \"proto/c.proto\";\n").unwrap();
        std::fs::write(root.join("proto/c.proto"), "// leaf\n").unwrap();
        let idx = index(&["proto/a.proto", "proto/b.proto", "proto/c.proto"]);
        let r = mk(
            "proto/**/*.proto",
            r#"import\s+"([^"]+)""#,
            Resolve::RelativeToRepoRoot,
            Require::NoOrphans {
                roots: Some(scope("proto/a.proto")),
            },
        );
        assert!(
            eval(&r, root, &idx).is_empty(),
            "the declared root is exempt from the orphan check"
        );
    }

    #[test]
    fn build_accepts_named_and_map_require_forms() {
        use crate::test_support::spec_yaml;
        let base = "id: t\nkind: file_graph\nnodes: \"**/*\"\nedges:\n  \
                    from_content:\n    extract:\n      regex: 'x'\n";
        for tail in [
            "require: no_dangling\nlevel: error\n",
            "require: no_orphans\nlevel: error\n",
            "require:\n  no_orphans:\n    roots: [\"src/main.rs\"]\nlevel: error\n",
        ] {
            let yaml = format!("{base}{tail}");
            assert!(build(&spec_yaml(&yaml)).is_ok(), "should build: {yaml}");
        }
        // Two map modes at once -> rejected.
        let bad = format!(
            "{base}require:\n  forbidden_edges:\n    - {{from: a, to: b}}\n  \
             no_orphans: {{}}\nlevel: error\n"
        );
        assert!(
            build(&spec_yaml(&bad)).is_err(),
            "setting two map modes must be rejected"
        );
    }

    fn fresh_rule(nodes: &str, from: &str, to: &str, marker: &str) -> FileGraphRule {
        FileGraphRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            nodes: scope(nodes),
            edges: EdgeSource::DeriveTarget {
                from: Regex::new(from).expect("valid from regex"),
                to: to.into(),
            },
            require: Require::Fresh {
                algo: Algorithm::Sha256,
                marker: Regex::new(marker).expect("valid marker regex"),
            },
        }
    }

    #[test]
    fn fresh_silent_when_marker_matches_then_stale_when_source_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("proto")).unwrap();
        let src = "message A {}\n";
        std::fs::write(root.join("proto/a.proto"), src).unwrap();
        let hash = Algorithm::Sha256.hex(src.as_bytes());
        std::fs::write(
            root.join("proto/a.pb.go"),
            format!("// @generated sha256:{hash}\npackage a\n"),
        )
        .unwrap();
        let idx = index(&["proto/a.proto", "proto/a.pb.go"]);
        let r = fresh_rule(
            "proto/**/*.proto",
            r"(.*)\.proto",
            "$1.pb.go",
            r"sha256:([0-9a-f]{64})",
        );
        assert!(eval(&r, root, &idx).is_empty(), "marker matches -> fresh");

        // The source changes; the generated marker is now stale.
        std::fs::write(root.join("proto/a.proto"), "message A { reserved 1; }\n").unwrap();
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("proto/a.pb.go"));
        assert!(v[0].message.contains("out of date"));
    }

    #[test]
    fn fresh_fires_when_derived_target_missing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("proto")).unwrap();
        std::fs::write(root.join("proto/a.proto"), "message A {}\n").unwrap();
        let idx = index(&["proto/a.proto"]);
        let r = fresh_rule(
            "proto/**/*.proto",
            r"(.*)\.proto",
            "$1.pb.go",
            r"sha256:([0-9a-f]{64})",
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("proto/a.pb.go"));
        assert!(v[0].message.contains("missing or unreadable"));
    }

    #[test]
    fn fresh_skips_sources_not_matching_from() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("proto")).unwrap();
        std::fs::write(root.join("proto/notes.txt"), "not generated\n").unwrap();
        let idx = index(&["proto/notes.txt"]);
        // `from` matches only *.proto, so notes.txt is skipped.
        let r = fresh_rule(
            "proto/**/*",
            r"(.*)\.proto",
            "$1.pb.go",
            r"sha256:([0-9a-f]{64})",
        );
        assert!(
            eval(&r, root, &idx).is_empty(),
            "a source not matching `from` is skipped"
        );
    }

    fn derive_dangling_rule(nodes: &str, from: &str, to: &str) -> FileGraphRule {
        FileGraphRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            nodes: scope(nodes),
            edges: EdgeSource::DeriveTarget {
                from: Regex::new(from).expect("valid from regex"),
                to: to.into(),
            },
            require: Require::NoDangling,
        }
    }

    #[test]
    fn derive_target_no_dangling_requires_the_derived_sibling() {
        // The elasticsearch shape: every licenses/X-LICENSE.txt must
        // be accompanied by a sibling X-NOTICE.txt. No file read — the
        // edge is derived purely from the node path.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("licenses")).unwrap();
        std::fs::write(root.join("licenses/arrow-LICENSE.txt"), "A\n").unwrap();
        std::fs::write(root.join("licenses/arrow-NOTICE.txt"), "N\n").unwrap();
        let r = derive_dangling_rule("licenses/**", r"(.+)-LICENSE\.txt", "$1-NOTICE.txt");

        // sibling present -> silent.
        let idx = index(&["licenses/arrow-LICENSE.txt", "licenses/arrow-NOTICE.txt"]);
        assert!(eval(&r, root, &idx).is_empty(), "sibling present -> silent");

        // a second license with no NOTICE sibling -> dangling.
        let idx = index(&[
            "licenses/arrow-LICENSE.txt",
            "licenses/arrow-NOTICE.txt",
            "licenses/lucene-LICENSE.txt",
        ]);
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("licenses/lucene-NOTICE.txt"));
        assert!(v[0].message.contains("licenses/lucene-LICENSE.txt"));
    }

    #[test]
    fn derive_target_root_escape_fires_and_is_never_read() {
        // Security regression (v0.12 path-confinement): an absolute or
        // `..`-escaping `to:` must produce an "escapes the repo root"
        // violation, never a filesystem read of the out-of-tree path.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("proto")).unwrap();
        std::fs::write(root.join("proto/a.proto"), "x").unwrap();
        let idx = index(&["proto/a.proto"]);

        // no_dangling + absolute `to:` -> escapes (the read oracle shape).
        let r = derive_dangling_rule("proto/**/*.proto", r"(.*)\.proto", "/etc/passwd");
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].message.contains("escapes the repo root"),
            "{}",
            v[0].message
        );

        // no_dangling + `..`-escape -> escapes.
        let r2 = derive_dangling_rule("proto/**/*.proto", r"(.*)/(.*)\.proto", "../../$2.out");
        assert!(
            eval(&r2, root, &idx)
                .iter()
                .any(|x| x.message.contains("escapes")),
        );

        // fresh + absolute `to:` -> escapes, WITHOUT reading the abs file.
        let f = fresh_rule(
            "proto/**/*.proto",
            r"(.*)\.proto",
            "/etc/hostname",
            r"sha256:([0-9a-f]{64})",
        );
        let vf = eval(&f, root, &idx);
        assert_eq!(vf.len(), 1, "{vf:?}");
        assert!(
            vf[0].message.contains("escapes the repo root"),
            "{}",
            vf[0].message
        );
    }

    #[test]
    fn derive_target_no_dangling_skips_nodes_not_matching_from() {
        // A node the `from` regex doesn't capture has no derived edge,
        // so it can't dangle (README.md is not an `*-LICENSE.txt`).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("licenses")).unwrap();
        std::fs::write(root.join("licenses/README.md"), "# licenses\n").unwrap();
        let idx = index(&["licenses/README.md"]);
        let r = derive_dangling_rule("licenses/**", r"(.+)-LICENSE\.txt", "$1-NOTICE.txt");
        assert!(
            eval(&r, root, &idx).is_empty(),
            "a node not matching `from` has no edge"
        );
    }

    #[test]
    fn build_fresh_mode_and_edge_coupling() {
        use crate::test_support::spec_yaml;
        let ok = "id: t\nkind: file_graph\nnodes: \"**/*.proto\"\nedges:\n  \
                  derive_target:\n    from: '(.*)\\.proto'\n    to: '$1.pb.go'\nrequire:\n  \
                  fresh:\n    marker: 'sha256:([0-9a-f]{64})'\nlevel: error\n";
        assert!(
            build(&spec_yaml(ok)).is_ok(),
            "derive_target + fresh should build: {ok}"
        );

        // `fresh` with `from_content` -> rejected (wrong edge type).
        let bad_fc = "id: t\nkind: file_graph\nnodes: \"**/*\"\nedges:\n  from_content:\n    \
                      extract:\n      regex: 'x'\nrequire:\n  fresh:\n    \
                      marker: 'h:([0-9a-f])'\nlevel: error\n";
        assert!(
            build(&spec_yaml(bad_fc)).is_err(),
            "fresh needs derive_target"
        );

        // `derive_target` with a content-graph mode (acyclic) -> rejected.
        let bad_dt = "id: t\nkind: file_graph\nnodes: \"**/*\"\nedges:\n  derive_target:\n    \
                      from: 'a'\n    to: 'b'\nrequire: acyclic\nlevel: error\n";
        assert!(
            build(&spec_yaml(bad_dt)).is_err(),
            "derive_target rejected for the content-graph modes"
        );

        // `derive_target` with `no_dangling` -> builds (v0.12 decouple:
        // the derived sibling must merely exist).
        let dt_nd = "id: t\nkind: file_graph\nnodes: \"licenses/**\"\nedges:\n  \
                     derive_target:\n    from: '(.+)-LICENSE\\.txt'\n    to: '$1-NOTICE.txt'\n\
                     require: no_dangling\nlevel: error\n";
        assert!(
            build(&spec_yaml(dt_nd)).is_ok(),
            "derive_target + no_dangling should build: {dt_nd}"
        );

        // A marker with no capture group -> rejected.
        let bad_marker = "id: t\nkind: file_graph\nnodes: \"**/*\"\nedges:\n  \
                          derive_target:\n    from: 'a'\n    to: 'b'\nrequire:\n  fresh:\n    \
                          marker: 'nogroup'\nlevel: error\n";
        assert!(
            build(&spec_yaml(bad_marker)).is_err(),
            "marker needs a capture group"
        );
    }
}
