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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::slice;

use alint_core::{
    AnalysisFailureKind, Context, Error, Extract, ExtractSpec, Level, RelatedLocation, Result,
    Rule, RuleSpec, Scope, Violation, extract_values, is_non_literal, unreadable_file_violation,
};
use regex::Regex;
use serde::Deserialize;

use crate::pair_hash::Algorithm;

/// How a content-extracted reference string is turned into a path.
#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "FileGraphResolve")]
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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "FileGraphFromContentSpec")]
struct FromContentSpec {
    extract: ExtractSpec,
    #[serde(default)]
    resolve: Resolve,
}

/// The `edges:` block - exactly one extractor (validated in
/// `build`): `from_content` (the reference-graph modes) or
/// `derive_target` (a name template → `fresh` codegen-freshness or
/// `no_dangling` derived-sibling existence).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "FileGraphEdgesSpec")]
#[schemars(extend(
    "oneOf" = [
        {"required": ["from_content"]},
        {"required": ["derive_target"]}
    ]
))]
struct EdgesSpec {
    #[serde(default)]
    #[schemars(schema_with = "from_content_schema")]
    from_content: Option<FromContentSpec>,
    #[serde(default)]
    #[schemars(schema_with = "derive_target_schema")]
    derive_target: Option<DeriveTargetSpec>,
}

/// Name-template edges: derive the generated target's path from the
/// source node's path - `from` is a regex matched against the node
/// path, `to` is a replacement template using its captures (e.g.
/// `from: '(.*)\.proto'`, `to: '$1.pb.go'`; a constant `to` maps
/// many sources to one target). The `fresh` mode's edge.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "FileGraphDeriveTargetSpec")]
struct DeriveTargetSpec {
    from: String,
    to: String,
}

fn from_content_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    generator.subschema_for::<FromContentSpec>()
}

fn derive_target_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    generator.subschema_for::<DeriveTargetSpec>()
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "FileGraphForbiddenEdgeSpec")]
struct ForbiddenEdgeSpec {
    from: String,
    to: String,
}

/// `require:` is either a bare string (`acyclic`) or a map
/// (`{ forbidden_edges: [...] }`). A scalar and a map are
/// structurally distinct, so an untagged enum decodes them
/// unambiguously (the proven `cross_file_value_equals` `targets:`
/// pattern - not the externally-tagged-enum-from-map trap
/// `crate::extract` documents).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(
    untagged,
    expecting = "a mode name (`acyclic`, `no_dangling`, or `no_orphans`), or a map of require options"
)]
#[schemars(rename = "FileGraphRequireSpec")]
enum RequireSpec {
    Named(NamedRequire),
    Map(RequireMap),
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "FileGraphNamedRequire")]
enum NamedRequire {
    Acyclic,
    /// Every path-shaped reference must resolve to a path in the walked tree.
    NoDangling,
    /// No node is unreferenced (bare form - no entry-point roots).
    NoOrphans,
}

/// The map form of `require:`. A struct-of-options (validated to
/// exactly-one in `build`) rather than an externally-tagged enum,
/// so it decodes from a YAML map cleanly. The bare-string modes
/// (`acyclic`, `no_dangling`, `no_orphans` with no roots) live in
/// `NamedRequire`; the configured modes live here, and future ones
/// (`fresh: { … }`) join as additional `Option` fields.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "FileGraphRequireMap")]
#[schemars(extend(
    "oneOf" = [
        {"required": ["forbidden_edges"]},
        {"required": ["no_orphans"]},
        {"required": ["fresh"]}
    ]
))]
struct RequireMap {
    #[serde(default)]
    #[schemars(schema_with = "forbidden_edges_schema")]
    forbidden_edges: Option<Vec<ForbiddenEdgeSpec>>,
    #[serde(default)]
    #[schemars(schema_with = "no_orphans_schema")]
    no_orphans: Option<NoOrphansSpec>,
    #[serde(default)]
    #[schemars(schema_with = "fresh_schema")]
    fresh: Option<FreshSpec>,
}

fn forbidden_edges_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "array",
        "minItems": 1,
        "items": {
            "additionalProperties": false,
            "type": "object",
            "properties": {
                "from": { "type": "string" },
                "to": { "type": "string" }
            },
            "required": ["from", "to"]
        }
    })
}

/// Options for the `no_orphans` map form: `roots` lists globs whose
/// nodes are allowed to be unreferenced (graph entry points). Bare
/// `require: no_orphans` (no roots) is the `NamedRequire` form.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "FileGraphNoOrphansSpec")]
struct NoOrphansSpec {
    #[serde(default)]
    roots: Vec<String>,
}

/// Options for the `fresh` map form: the `derive_target` output must
/// embed the source's current `hash` digest, captured by `marker`
/// (a regex whose group 1 is the hex digest). Content-hash, never
/// mtime - portable on a fresh clone.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "FileGraphFreshSpec")]
struct FreshSpec {
    #[serde(default)]
    #[schemars(extend("default" = "sha256"))]
    hash: Algorithm,
    marker: String,
}

fn no_orphans_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    generator.subschema_for::<NoOrphansSpec>()
}

fn fresh_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    generator.subschema_for::<FreshSpec>()
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    nodes: String,
    edges: EdgesSpec,
    require: RequireSpec,
}

crate::options_schema_for!(Options);

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

/// How edges are formed - paired with `require` in `build`:
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

    fn visit_sensitive_paths(
        &self,
        explicit: bool,
        ctx: &Context<'_>,
        visit: &mut dyn FnMut(&Path),
    ) {
        let manifest_sensitive = self.nodes.sensitive_manifest_sources().next().is_some();
        for path in self.nodes.sensitive_manifest_sources() {
            visit(path);
        }
        let implicit = matches!(
            &self.edges,
            EdgeSource::FromContent { extract, .. } if extract.redacts_by_default()
        );
        if !explicit && !implicit && !manifest_sensitive {
            return;
        }
        for entry in ctx.index.files() {
            if !self.nodes.matches(&entry.path, ctx.index) {
                continue;
            }
            visit(&entry.path);
            if explicit
                && let EdgeSource::DeriveTarget { from, to } = &self.edges
                && let Some(target) =
                    alint_core::derive_target(from, to, &entry.path.to_string_lossy())
            {
                visit(&target);
            }
        }
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

        for comp in cyclic_components(&adj, nodes.len()) {
            out.push(self.cycle_violation(nodes, &comp));
        }
        out
    }

    /// One violation per path-shaped edge whose resolved target
    /// exists nowhere in the index (as a file or a directory).
    /// References that aren't path-shaped (bare module names, URLs)
    /// are dropped, not flagged - `no_dangling` is a
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
    /// to a path (unreadable / unparseable nodes push an analysis failure and
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
            if let Some(target) = alint_core::derive_target(from, to, &node_str) {
                return vec![target];
            }
            // `None` is either "the node doesn't match `from`" (no edge, silent)
            // or "it matches but derives an out-of-repo target" (an escape).
            // Re-check the match to tell them apart, and reconstruct the derived
            // string only for the escape message — the path itself is never read.
            if let Some(caps) = from.captures(&node_str) {
                let mut derived = String::new();
                caps.expand(to, &mut derived);
                out.push(Self::node_violation(
                    node,
                    &format!("derives the out-of-repo target {derived:?} (escapes the repo root)"),
                ));
            }
            return Vec::new();
        }
        let EdgeSource::FromContent { extract, resolve } = &self.edges else {
            return Vec::new();
        };
        let abs = ctx.root.join(node);
        let text = match crate::io::read_capped(&abs) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(crate::io::ReadCapError::TooLarge(n)) => {
                out.push(alint_core::too_large_file_violation(node, n));
                return Vec::new();
            }
            Err(crate::io::ReadCapError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                out.push(alint_core::missing_file_violation(node));
                return Vec::new();
            }
            Err(crate::io::ReadCapError::Io(e)) => {
                out.push(unreadable_file_violation(node, &e));
                return Vec::new();
            }
        };
        let refs = match extract_values(extract, &text) {
            Ok(v) => v,
            Err(e) => {
                let mut finding = Violation::new(format!("edge extraction failed: {e}"))
                    .with_path(node.to_path_buf())
                    .as_analysis_failure(AnalysisFailureKind::ResourceParse);
                if extract.redacts_by_default() {
                    finding.redact_message(&self.id);
                }
                out.push(finding);
                return Vec::new();
            }
        };
        if let Some(mut violation) =
            crate::non_string_extract_violation(node, "edge extract", &refs.non_strings)
        {
            self.redact_edge_finding(&mut violation, node, "non_string", std::iter::empty());
            out.push(violation);
        }
        refs.strings
            .iter()
            .filter(|r| !is_non_literal(r))
            .filter_map(|r| resolve_ref(r, node, *resolve))
            .collect()
    }

    fn redact_edge_finding<'a>(
        &self,
        finding: &mut Violation,
        source: &Path,
        category: &str,
        evidence: impl IntoIterator<Item = &'a str>,
    ) {
        let EdgeSource::FromContent { extract, .. } = &self.edges else {
            return;
        };
        if !extract.redacts_by_default() {
            return;
        }
        let locator = format!("edge\0{}\0{extract:?}\0{category}", crate::slash(source));
        finding.redact_evidence(&self.id, &locator, evidence);
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
            // `to:` template must never read a file outside the tree --
            // lexically OR through an in-repo symlink (`out -> /elsewhere`),
            // which the symlink-blind `normalize_confined` alone misses.
            // `file_graph` has no `allow_out_of_root` opt-in, so any escape
            // is denied.
            let crate::pathsafe::Confined::In(target) =
                crate::pathsafe::confine_read(Path::new(&target), ctx.root, false)
            else {
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
                Err(crate::io::ReadCapError::TooLarge(n)) => {
                    out.push(alint_core::too_large_file_violation(source, n));
                    continue;
                }
                Err(crate::io::ReadCapError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                    out.push(alint_core::missing_file_violation(source));
                    continue;
                }
                Err(crate::io::ReadCapError::Io(e)) => {
                    out.push(unreadable_file_violation(source, &e));
                    continue;
                }
            };
            let digest = algo.hex(&src_bytes);

            let tgt_bytes = match crate::io::read_capped(&ctx.root.join(&target)) {
                Ok(bytes) => bytes,
                Err(crate::io::ReadCapError::TooLarge(n)) => {
                    out.push(alint_core::too_large_file_violation(&target, n));
                    continue;
                }
                Err(crate::io::ReadCapError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                    out.push(self.fresh_violation(
                        &target,
                        source,
                        &format!(
                            "is missing (the derived output for {})",
                            crate::slash(source)
                        ),
                    ));
                    continue;
                }
                Err(crate::io::ReadCapError::Io(e)) => {
                    out.push(unreadable_file_violation(&target, &e));
                    continue;
                }
            };
            let tgt_text = String::from_utf8_lossy(&tgt_bytes);
            let fresh = marker
                .captures_iter(&tgt_text)
                .filter_map(|c| c.get(1))
                .any(|m| m.as_str().eq_ignore_ascii_case(&digest));
            if !fresh {
                out.push(self.fresh_violation(
                    &target,
                    source,
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
        // Keyed on the node so it can't collide with an `orphan_violation` (or a
        // `fresh_violation`) on the same path — both are path-only and line-less,
        // so without distinct keys a baseline could mask a genuinely-new node
        // finding (baseline.md §6 collision-invariant).
        Violation::new(format!("file_graph node {} {reason}", crate::slash(node)))
            .with_path(node.to_path_buf())
            .with_baseline_key(format!("file_graph-node\u{0}{}", crate::slash(node)))
    }

    fn forbidden_violation(&self, src: &Path, target: &Path, pat: &ForbiddenPattern) -> Violation {
        let target_text = crate::slash(target);
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "{} has a forbidden dependency edge to {} (forbidden_edges: from {:?} to {:?})",
                crate::slash(src),
                target_text,
                pat.from_glob,
                pat.to_glob,
            )
        });
        let mut finding = Violation::new(msg)
            .with_path(src.to_path_buf())
            .with_baseline_key(format!(
                "forbidden\u{0}{}\u{0}{target_text}\u{0}{}=>{}",
                crate::slash(src),
                pat.from_glob,
                pat.to_glob
            ))
            .with_related_location(RelatedLocation::new(target, "dependency target"));
        self.redact_edge_finding(
            &mut finding,
            src,
            "forbidden",
            std::iter::once(target_text.as_str()),
        );
        finding
    }

    fn cycle_violation(&self, nodes: &[PathBuf], comp: &CyclicComponent) -> Violation {
        let name = |i: usize| crate::slash(&nodes[i]);
        let mut rendered: String = comp
            .cycle
            .iter()
            .map(|&i| name(i))
            .collect::<Vec<_>>()
            .join(" \u{2192} ");
        // Close the loop so the cycle reads unambiguously.
        rendered.push_str(" \u{2192} ");
        rendered.push_str(&name(comp.cycle[0]));
        let members: Vec<String> = comp.members.iter().map(|&i| name(i)).collect();
        let msg = self.message.clone().unwrap_or_else(|| {
            if comp.cycle.len() == comp.members.len() {
                // The component IS one simple cycle: render it as the path.
                format!("dependency cycle ({} files): {rendered}", members.len())
            } else {
                // Interlocking cycles: name every participating file, plus one
                // concrete cycle as an example.
                format!(
                    "dependency cycle among {} files: {} (e.g. {rendered})",
                    members.len(),
                    members.join(", ")
                )
            }
        });
        // A cycle is a SET of files; key on the sorted member set of the whole
        // strongly connected component, so a new file joining it changes the
        // fingerprint (anchoring on one member would mask the larger cycle). For
        // a component that is one simple cycle this is the same key the
        // per-cycle reporting produced.
        let mut key_members = members;
        key_members.sort();
        let mut finding = Violation::new(msg)
            .with_path(nodes[comp.members[0]].clone())
            .with_baseline_key(format!("cycle\u{0}{}", key_members.join("\u{0}")))
            .with_related_locations(
                comp.members
                    .iter()
                    .copied()
                    .filter(|&member| member != comp.members[0])
                    .map(|member| RelatedLocation::new(&nodes[member], "cycle member")),
            );
        self.redact_edge_finding(
            &mut finding,
            &nodes[comp.members[0]],
            "cycle",
            key_members.iter().map(String::as_str),
        );
        finding
    }

    fn dangling_violation(&self, src: &Path, target: &Path) -> Violation {
        let target_text = crate::slash(target);
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "{} references {target_text}, which does not resolve to any path (missing or ignored)",
                crate::slash(src),
            )
        });
        let mut finding = Violation::new(msg)
            .with_path(src.to_path_buf())
            .with_baseline_key(format!(
                "dangling\u{0}{}\u{0}{target_text}",
                crate::slash(src),
            ))
            .with_related_location(RelatedLocation::new(target, "unresolved target"));
        self.redact_edge_finding(
            &mut finding,
            src,
            "dangling",
            std::iter::once(target_text.as_str()),
        );
        finding
    }

    fn orphan_violation(&self, node: &Path) -> Violation {
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "{} is an orphan: no other node references it (and it is not a declared root)",
                crate::slash(node),
            )
        });
        let mut finding = Violation::new(msg)
            .with_path(node.to_path_buf())
            .with_baseline_key(format!("file_graph-orphan\u{0}{}", crate::slash(node)));
        self.redact_edge_finding(&mut finding, node, "orphan", std::iter::empty());
        finding
    }

    /// A stale / missing `derive_target` output. Anchored on the
    /// target (the file that needs regenerating); `reason` already
    /// names the source.
    fn fresh_violation(&self, target: &Path, source: &Path, reason: &str) -> Violation {
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("{} {reason}", crate::slash(target)));
        Violation::new(msg)
            .with_path(target.to_path_buf())
            .with_baseline_key(format!("file_graph-fresh\u{0}{}", crate::slash(target)))
            .with_related_location(RelatedLocation::new(source, "freshness source"))
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

/// One strongly connected set of mutually-dependent nodes (size >= 2): every
/// member lies on at least one directed cycle with every other member.
#[derive(Debug)]
struct CyclicComponent {
    /// Node indices, ascending.
    members: Vec<usize>,
    /// A shortest directed cycle through the smallest member, starting at it.
    cycle: Vec<usize>,
}

/// Every strongly connected component of `adj` (node indices `0..n`) that
/// contains a cycle, each with a representative shortest cycle through its
/// smallest member; sorted by smallest member, so the report is deterministic.
/// Iterative Tarjan - no recursion-depth limit on deep graphs - so it is linear
/// in nodes + edges, and every node on ANY cycle is in exactly one component.
///
/// (Reporting one cycle per DFS back-edge -- the earlier approach -- missed
/// nodes: with `a->b, a->d, b->c, c->a, d->b`, `b` is fully explored before `d`
/// is visited, so `d`, which lies on `d->b->c->a->d`, was never reported.)
/// Self-loops are dropped by the caller, so a component is never a lone node.
fn cyclic_components(adj: &BTreeMap<usize, Vec<usize>>, n: usize) -> Vec<CyclicComponent> {
    const UNSEEN: usize = usize::MAX;
    let empty: Vec<usize> = Vec::new();
    let neighbors = |v: usize| adj.get(&v).unwrap_or(&empty);
    let mut index = vec![UNSEEN; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut next = 0usize;
    let mut comps: Vec<Vec<usize>> = Vec::new();

    for start in 0..n {
        if index[start] != UNSEEN {
            continue;
        }
        // (node, next neighbour position) frames of the simulated recursion.
        let mut call: Vec<(usize, usize)> = vec![(start, 0)];
        index[start] = next;
        low[start] = next;
        next += 1;
        stack.push(start);
        on_stack[start] = true;
        while let Some(&(v, child)) = call.last() {
            let nb = neighbors(v);
            if child < nb.len() {
                if let Some(frame) = call.last_mut() {
                    frame.1 += 1;
                }
                let w = nb[child];
                if index[w] == UNSEEN {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    call.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                call.pop();
                if let Some(&(u, _)) = call.last() {
                    low[u] = low[u].min(low[v]);
                }
                if low[v] == index[v] {
                    let mut comp = Vec::new();
                    while let Some(w) = stack.pop() {
                        on_stack[w] = false;
                        comp.push(w);
                        if w == v {
                            break;
                        }
                    }
                    if comp.len() >= 2 {
                        comp.sort_unstable();
                        comps.push(comp);
                    }
                }
            }
        }
    }
    comps.sort_unstable_by_key(|c| c[0]);
    comps
        .into_iter()
        .map(|members| {
            let cycle = shortest_cycle_through(adj, &members);
            CyclicComponent { members, cycle }
        })
        .collect()
}

/// A shortest directed cycle through `members[0]` (the smallest member) using
/// only edges inside the component, starting at that member. Breadth-first over
/// the ascending adjacency lists, so the choice is deterministic. Every member of
/// a strongly connected component reaches every other, so a cycle always exists;
/// the fallback (the bare member list) is unreachable in practice.
fn shortest_cycle_through(adj: &BTreeMap<usize, Vec<usize>>, members: &[usize]) -> Vec<usize> {
    let start = members[0];
    let in_comp: HashSet<usize> = members.iter().copied().collect();
    let mut parent: HashMap<usize, usize> = HashMap::new();
    let mut queue = std::collections::VecDeque::from([start]);
    while let Some(u) = queue.pop_front() {
        for &w in adj.get(&u).into_iter().flatten() {
            if !in_comp.contains(&w) {
                continue;
            }
            if w == start {
                let mut path = vec![u];
                let mut cur = u;
                while cur != start {
                    cur = parent[&cur];
                    path.push(cur);
                }
                path.reverse();
                return path;
            }
            if w != start && !parent.contains_key(&w) {
                parent.insert(w, u);
                queue.push_back(w);
            }
        }
    }
    members.to_vec()
}

/// Resolve the map form of `require:` - exactly one of
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

pub static META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::FullIndex { path_scope: false },
    paths: alint_core::PathsSupport::Forbidden,
    scope_filter: alint_core::ScopeFilterSupport::Rejected {
        reason: std::borrow::Cow::Borrowed("scope_filter is not supported by this kind"),
    },
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("nodes"),
            selector_type: alint_core::SelectorType::Glob,
            rerootable: false,
        },
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("edges.from_content.extract"),
            selector_type: alint_core::SelectorType::Extract,
            rerootable: false,
        },
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("edges.derive_target.from"),
            selector_type: alint_core::SelectorType::Regex,
            rerootable: false,
        },
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("edges.derive_target.to"),
            selector_type: alint_core::SelectorType::PathTemplate,
            rerootable: false,
        },
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("require.no_orphans.roots"),
            selector_type: alint_core::SelectorType::GlobList,
            rerootable: false,
        },
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("require.forbidden_edges[].from"),
            selector_type: alint_core::SelectorType::Glob,
            rerootable: false,
        },
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("require.forbidden_edges[].to"),
            selector_type: alint_core::SelectorType::Glob,
            rerootable: false,
        },
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("require.fresh.marker"),
            selector_type: alint_core::SelectorType::Regex,
            rerootable: false,
        },
    ]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::OtherFiles,
    formats: std::borrow::Cow::Borrowed(alint_core::Format::ALL),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[]),
    expect_matches: false,
    trace: alint_core::TraceMeta::SELECTION,
    nested_rules: false,
};

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
#[path = "file_graph_tests.rs"]
mod tests;
