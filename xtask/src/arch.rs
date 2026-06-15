//! `xtask gen-arch` — emit the code-extracted crate dependency graph
//! and gate the hand-modeled C4 models against reality.
//!
//! From `cargo metadata` it generates two content-diff-gated artifacts, so the
//! structural picture can't drift from the Cargo manifests:
//! - `docs/design/architecture/model/crate-graph.gen.c4`: the crate-to-crate
//!   runtime + dev/build edges (referencing the hand-authored crate elements in
//!   `alint.c4` by qualified path; dev/build edges use the `dev` relationship
//!   kind, dashed) plus the `crateGraph` `LikeC4` view. This is what renders
//!   interactively on alint.org and exports to Mermaid for GitHub (`DIAGRAMS.md`).
//! - `docs/design/architecture/crate-graph.md`: the page itself — the embedded
//!   `crateGraph` view plus a crate-by-tier table.
//!
//! `docs/design/architecture/workspace.dsl` is the hand-written Structurizr C4
//! model (intent); `--check` also verifies its crate components equal the
//! `cargo metadata` member set.
//!
//! Mirrors `gen_schema`/`facts`. Design: `docs/design/architecture-as-code.md`
//! (ADR-0001, Phase 4 / WS3).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

const CRATE_GRAPH_MD: &str = "docs/design/architecture/crate-graph.md";
const CRATE_GRAPH_C4: &str = "docs/design/architecture/model/crate-graph.gen.c4";
const MODEL_C4: &str = "docs/design/architecture/model/alint.c4";
const WORKSPACE_DSL: &str = "docs/design/architecture/workspace.dsl";

/// A workspace crate plus its intra-workspace dependencies and role.
pub(crate) struct Crate {
    pub(crate) name: String,
    description: String,
    /// Sorted normal (runtime) intra-workspace dependency names — the
    /// production architecture; the tier + acyclic computations use these.
    pub(crate) deps: Vec<String>,
    /// Sorted dev/build-only intra-workspace dependency names (test
    /// harnesses, xtask's test deps). Rendered as dashed edges; excluded
    /// from tiers and the cycle check (a normal+dev cycle is legal in Cargo).
    dev_deps: Vec<String>,
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    #[serde(default)]
    description: Option<String>,
    dependencies: Vec<MetaDep>,
}

#[derive(Deserialize)]
struct MetaDep {
    name: String,
    /// `null` for a normal (runtime) dependency; `"dev"` / `"build"`
    /// otherwise. `cargo metadata` lists the same dep once per kind.
    #[serde(default)]
    kind: Option<String>,
}

/// `cargo metadata --no-deps`, reduced to workspace crates and the
/// edges between them. Deterministic: names + edges only, fully sorted.
pub(crate) fn workspace_crates(root: &Path) -> Result<Vec<Crate>> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .context("run `cargo metadata`")?;
    if !output.status.success() {
        bail!(
            "`cargo metadata` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let meta: Metadata = serde_json::from_slice(&output.stdout).context("parse cargo metadata")?;

    let members: BTreeSet<String> = meta.packages.iter().map(|p| p.name.clone()).collect();
    let mut crates: Vec<Crate> = meta
        .packages
        .iter()
        .map(|p| {
            let mut deps: BTreeSet<String> = BTreeSet::new();
            let mut dev_deps: BTreeSet<String> = BTreeSet::new();
            for d in &p.dependencies {
                // Self-edges never occur, but guard; and only workspace members.
                if d.name == p.name || !members.contains(&d.name) {
                    continue;
                }
                if d.kind.is_none() {
                    deps.insert(d.name.clone());
                } else {
                    dev_deps.insert(d.name.clone());
                }
            }
            // A dep listed as both normal and dev counts as normal (runtime).
            for n in &deps {
                dev_deps.remove(n);
            }
            Crate {
                name: p.name.clone(),
                description: p.description.clone().unwrap_or_default(),
                deps: deps.into_iter().collect(),
                dev_deps: dev_deps.into_iter().collect(),
            }
        })
        .collect();
    crates.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(crates)
}

/// Map each crate NAME to its fully-qualified `LikeC4` element path (e.g.
/// `alint-core` -> `alint.cli.core`), parsed from `alint.c4`. Crates nest inside
/// containers inside the `alint` system, and cross-file references in the
/// generated fragment must be qualified, so we track the element nesting (only
/// `system`/`container` ids contribute to the path).
fn crate_id_map(model_c4: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let mut stack: Vec<String> = Vec::new();
    for raw in model_c4.lines() {
        let line = raw.trim();
        if let Some((lhs, rhs)) = line.split_once("= crate ") {
            let id = lhs.trim();
            if let Some(end) = rhs
                .trim_start()
                .strip_prefix('\'')
                .and_then(|r| r.find('\'').map(|e| (r, e)))
            {
                let (rest, e) = end;
                let mut path: Vec<&str> = stack
                    .iter()
                    .filter(|s| !s.is_empty())
                    .map(String::as_str)
                    .collect();
                path.push(id);
                map.insert(rest[..e].to_string(), path.join("."));
            }
        } else if line.ends_with('{') {
            // Push the element id for system/container openers (they nest the
            // crates); push a placeholder for every other `{` so braces balance.
            let segment = line
                .split_once(" = ")
                .map(|(lhs, rhs)| (lhs.trim(), rhs.split_whitespace().next().unwrap_or("")))
                .filter(|(id, kind)| !id.is_empty() && matches!(*kind, "system" | "container"))
                .map(|(id, _)| id.to_string())
                .unwrap_or_default();
            stack.push(segment);
        } else if line == "}" {
            stack.pop();
        }
    }
    map
}

/// The crate dependency graph as a `LikeC4` model fragment: the runtime and
/// dev/build edges between the (hand-authored) crate elements, plus the
/// `crateGraph` view. Generated from `cargo metadata`, so it can't drift from
/// the manifests; dev/build edges use the `dev` relationship kind (dashed).
fn render_crate_graph_c4(crates: &[Crate], id_of: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let _ = writeln!(
        &mut out,
        "// GENERATED by `cargo run -p xtask -- gen-arch`. Do not edit by hand."
    );
    let _ = writeln!(
        &mut out,
        "// Source: `cargo metadata` (intra-workspace dependency edges). The crate"
    );
    let _ = writeln!(
        &mut out,
        "// elements live in alint.c4; this adds their edges + the crateGraph view."
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "model {{");
    let _ = writeln!(out, "  // Runtime (normal) dependency edges.");
    for c in crates {
        let Some(src) = id_of.get(&c.name) else {
            continue;
        };
        for dep in &c.deps {
            if let Some(dst) = id_of.get(dep) {
                let _ = writeln!(out, "  {src} -> {dst}");
            }
        }
    }
    let _ = writeln!(
        out,
        "  // Dev/build-only edges (dashed via the `dev` kind)."
    );
    for c in crates {
        let Some(src) = id_of.get(&c.name) else {
            continue;
        };
        for dep in &c.dev_deps {
            if let Some(dst) = id_of.get(dep) {
                let _ = writeln!(out, "  {src} -[dev]-> {dst}");
            }
        }
    }
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);
    let mut ids: Vec<&str> = crates
        .iter()
        .filter_map(|c| id_of.get(&c.name).map(String::as_str))
        .collect();
    ids.sort_unstable();
    let _ = writeln!(out, "views {{");
    let _ = writeln!(out, "  view crateGraph {{");
    let _ = writeln!(out, "    title 'Crate dependency graph'");
    let _ = writeln!(out, "    include {}", ids.join(", "));
    let _ = writeln!(out, "  }}");
    let _ = writeln!(out, "}}");
    out
}

/// Longest dependency chain from each crate to a sink (`alint-core` = 0).
/// Safe because the graph is acyclic (guarded by a test).
fn tiers(crates: &[Crate]) -> BTreeMap<String, usize> {
    let by_name: BTreeMap<&str, &Crate> = crates.iter().map(|c| (c.name.as_str(), c)).collect();
    let mut memo: BTreeMap<String, usize> = BTreeMap::new();
    for c in crates {
        depth(&c.name, &by_name, &mut memo);
    }
    memo
}

fn depth(
    name: &str,
    by_name: &BTreeMap<&str, &Crate>,
    memo: &mut BTreeMap<String, usize>,
) -> usize {
    if let Some(&d) = memo.get(name) {
        return d;
    }
    let d = by_name.get(name).map_or(0, |c| {
        c.deps
            .iter()
            .map(|dep| depth(dep, by_name, memo))
            .max()
            .map_or(0, |m| m + 1)
    });
    memo.insert(name.to_string(), d);
    d
}

fn render_crate_graph(crates: &[Crate]) -> String {
    let mut out = String::new();
    let _ = writeln!(&mut out, "# Crate dependency graph");
    let _ = writeln!(&mut out);
    let _ = writeln!(
        &mut out,
        "<!-- GENERATED by `cargo run -p xtask -- gen-arch`. Do not edit by hand."
    );
    let _ = writeln!(
        &mut out,
        "     Edit the crates' Cargo.toml and regenerate; `gen-arch --check` gates it. -->"
    );
    let _ = writeln!(&mut out);
    let _ = writeln!(
        &mut out,
        "The intra-workspace dependency graph of alint's Cargo workspace, grouped by"
    );
    let _ = writeln!(
        &mut out,
        "container. Solid edges are runtime dependencies; dashed edges are dev/build-only"
    );
    let _ = writeln!(
        &mut out,
        "(test harnesses, tooling). `alint-core` is the foundation (no runtime dependencies);"
    );
    let _ = writeln!(
        &mut out,
        "the tiers below count runtime edges only. The crate elements and edges are gated"
    );
    let _ = writeln!(
        &mut out,
        "against `cargo metadata`, so this can't drift from the manifests."
    );
    let _ = writeln!(&mut out);

    let _ = writeln!(
        &mut out,
        "<likec4-view view-id=\"crateGraph\"></likec4-view>"
    );
    let _ = writeln!(&mut out);
    let _ = writeln!(
        &mut out,
        "(The diagram renders interactively on alint.org. GitHub strips the web component; \
         the same view is a static Mermaid flowchart in \
         [DIAGRAMS.md](https://github.com/asamarts/alint/blob/main/docs/design/architecture/DIAGRAMS.md).)"
    );
    let _ = writeln!(&mut out);

    let _ = writeln!(&mut out, "## Crates by tier");
    let _ = writeln!(&mut out);
    let _ = writeln!(
        &mut out,
        "Tier = longest runtime-dependency chain to the foundation (`alint-core` = 0); \
         dev/build-only edges don't count."
    );
    let _ = writeln!(&mut out);
    let _ = writeln!(&mut out, "| Tier | Crate | Role |");
    let _ = writeln!(&mut out, "|---|---|---|");
    let tier = tiers(crates);
    let mut rows: Vec<&Crate> = crates.iter().collect();
    rows.sort_by(|a, b| (tier[&a.name], &a.name).cmp(&(tier[&b.name], &b.name)));
    for c in rows {
        let _ = writeln!(
            &mut out,
            "| {} | `{}` | {} |",
            tier[&c.name],
            c.name,
            c.description.replace('|', "\\|").replace('\n', " ")
        );
    }
    out
}

pub fn run(check: bool) -> Result<()> {
    let root = crate::workspace_root()?;
    let crates = workspace_crates(&root)?;
    // Defensive: the tier walk recurses along dependency edges and would
    // loop on a cycle. Cargo forbids cycles, so this only trips on a bug
    // in the extraction; fail loudly rather than hang.
    if !is_acyclic(&crates) {
        bail!("the intra-workspace dependency graph has a cycle");
    }
    let model_c4 =
        fs::read_to_string(root.join(MODEL_C4)).with_context(|| format!("read {MODEL_C4}"))?;
    let id_of = crate_id_map(&model_c4);

    let md = render_crate_graph(&crates);
    let c4 = render_crate_graph_c4(&crates, &id_of);
    let md_path = root.join(CRATE_GRAPH_MD);
    let c4_path = root.join(CRATE_GRAPH_C4);

    if check {
        for (rel, path, rendered) in [
            (CRATE_GRAPH_MD, &md_path, &md),
            (CRATE_GRAPH_C4, &c4_path, &c4),
        ] {
            let committed =
                fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
            if committed != *rendered {
                bail!(
                    "{rel} is stale. Run `cargo run -p xtask -- gen-arch` to \
                     regenerate and commit the result."
                );
            }
        }
        check_dsl_consistency(&root, &crates)?;
        println!(
            "{CRATE_GRAPH_MD} + {CRATE_GRAPH_C4} up to date; workspace.dsl crate components consistent"
        );
        return Ok(());
    }

    fs::write(&md_path, &md).with_context(|| format!("write {}", md_path.display()))?;
    fs::write(&c4_path, &c4).with_context(|| format!("write {}", c4_path.display()))?;
    println!("wrote {CRATE_GRAPH_MD} + {CRATE_GRAPH_C4}");
    Ok(())
}

/// The hand-modeled C4 `workspace.dsl` must declare exactly the
/// `cargo metadata` crate set as components — no more, no less.
fn check_dsl_consistency(root: &Path, crates: &[Crate]) -> Result<()> {
    let path = root.join(WORKSPACE_DSL);
    let dsl = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let declared = dsl_crate_components(&dsl);
    let members: BTreeSet<String> = crates.iter().map(|c| c.name.clone()).collect();
    if declared != members {
        let missing: Vec<&String> = members.difference(&declared).collect();
        let extra: Vec<&String> = declared.difference(&members).collect();
        bail!(
            "{WORKSPACE_DSL} crate components drifted from `cargo metadata`. \
             missing (add to the model): {missing:?}; extra (remove or rename): {extra:?}."
        );
    }
    Ok(())
}

/// Crate names declared as C4 components — the first quoted token on any
/// `component "..."` line that looks like a workspace crate. Non-crate
/// elements (`CLI`, `alint.org site`, ...) don't match the filter.
fn dsl_crate_components(dsl: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in dsl.lines() {
        if !line.contains("component \"") {
            continue;
        }
        if let Some(name) = first_quoted(line) {
            if name == "alint" || name == "xtask" || name.starts_with("alint-") {
                set.insert(name);
            }
        }
    }
    set
}

fn first_quoted(s: &str) -> Option<String> {
    let open = s.find('"')?;
    let rest = &s[open + 1..];
    let close = rest.find('"')?;
    Some(rest[..close].to_string())
}

/// DFS cycle check over the intra-workspace edges.
fn is_acyclic(crates: &[Crate]) -> bool {
    let by_name: BTreeMap<&str, &Crate> = crates.iter().map(|c| (c.name.as_str(), c)).collect();
    // 0 = unvisited, 1 = on stack, 2 = done.
    let mut color: BTreeMap<&str, u8> = by_name.keys().map(|k| (*k, 0u8)).collect();
    for c in crates {
        if color[c.name.as_str()] == 0 && has_back_edge(c.name.as_str(), &by_name, &mut color) {
            return false;
        }
    }
    true
}

fn has_back_edge(
    name: &str,
    by_name: &BTreeMap<&str, &Crate>,
    color: &mut BTreeMap<&str, u8>,
) -> bool {
    if let Some(slot) = color.get_mut(name) {
        *slot = 1;
    }
    if let Some(c) = by_name.get(name) {
        for dep in &c.deps {
            match color.get(dep.as_str()).copied().unwrap_or(2) {
                1 => return true,
                0 if has_back_edge(by_name[dep.as_str()].name.as_str(), by_name, color) => {
                    return true;
                }
                _ => {}
            }
        }
    }
    if let Some(slot) = color.get_mut(name) {
        *slot = 2;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `crate-graph.md` must be regenerated + committed when the
    /// workspace dependency structure changes; `--check` is what CI +
    /// preflight run.
    #[test]
    fn gen_arch_check_passes_on_committed_tree() {
        run(true).expect("gen-arch --check should pass on the committed tree");
    }

    /// The workspace dependency graph is a DAG.
    #[test]
    fn workspace_graph_is_acyclic() {
        let crates = workspace_crates(&crate::workspace_root().expect("root")).expect("metadata");
        assert!(
            is_acyclic(&crates),
            "the intra-workspace dependency graph has a cycle"
        );
    }

    /// `alint-core` is the foundation and must stay a dependency sink —
    /// the engine core cannot depend on rules / DSL / output.
    #[test]
    fn alint_core_is_a_dependency_sink() {
        let crates = workspace_crates(&crate::workspace_root().expect("root")).expect("metadata");
        let core = crates
            .iter()
            .find(|c| c.name == "alint-core")
            .expect("alint-core present");
        assert!(
            core.deps.is_empty(),
            "alint-core must stay dependency-free; has {:?}",
            core.deps
        );
    }

    /// The C4 model's crate components equal the real member set.
    #[test]
    fn workspace_dsl_components_match_cargo_metadata() {
        let root = crate::workspace_root().expect("root");
        let crates = workspace_crates(&root).expect("metadata");
        check_dsl_consistency(&root, &crates)
            .expect("workspace.dsl crate components must match cargo metadata");
    }
}
