//! The config trust gates: what an `extends:`'d, remote, nested, or bundled
//! source may and may not declare (process-spawning rule kinds and fix ops,
//! fix-tier promotions, content-fixer demotion, environment reads, `ignore:`,
//! `when` expressions, top-level settings), plus the provenance markers that
//! carry each rule's source classes through composition and template
//! expansion.

use alint_core::{Error, Result};
use serde_yaml_ng::Mapping;

use crate::{RawConfig, instance_vars, render_template_vars};

/// Internal raw-mapping marker carried through composition until template
/// expansion. It is deliberately not part of the public config schema or the
/// typed [`RuleSpec`]: [`RawConfig::finalize`] consumes it before deserializing
/// the effective rule.
///
/// The marker is monotonic. [`merge`](crate::merge) combines it with logical OR, so a later
/// field override cannot accidentally erase the fact that an untrusted remote
/// or nested source helped define the effective rule/template.
pub(crate) const UNTRUSTED_FIX_SOURCE_MARKER: &str = "__alint_internal_untrusted_fix_source";

/// Internal raw-mapping marker recording which kinds of `extends:` source
/// contributed to a rule or template: a mapping from a [`SourceClass`] key to
/// the first such source's name (for the error message). Like
/// [`UNTRUSTED_FIX_SOURCE_MARKER`] it is monotonic across [`merge`](crate::merge) and template
/// expansion, and [`RawConfig::finalize`] removes it before deserializing the
/// effective rule. It lets the finalize-time checks ask "did an extended / a
/// remote / an untrusted remote source shape this EFFECTIVE rule?" after
/// id-based field-merging has hidden where each field came from.
pub(crate) const PROVENANCE_MARKER: &str = "__alint_internal_provenance";

/// Internal raw-mapping marker on a rule whose own (template-instance) `vars:`
/// holds values interpolated from the environment: a mapping from each such
/// var's name to its raw, pre-interpolation text (`{{env.NPM_TOKEN}}`).
/// Recorded by the local-config parser (only local configs are interpolated),
/// replaced together with `vars:` when a later source overrides the rule's
/// `vars:` ([`merge_mapping_fields`]), and removed by [`RawConfig::finalize`]
/// before the effective rule is deserialized.
pub(crate) const ENV_VARS_MARKER: &str = "__alint_internal_env_vars";

/// A class of `extends:` source recorded in [`PROVENANCE_MARKER`].
#[derive(Clone, Copy)]
pub(crate) enum SourceClass {
    /// Any config reached through `extends:` (local, remote, or bundled).
    Extended,
    /// An `https://` `extends:` entry, allowlisted in `trusted_extends:` or not.
    Remote,
    /// An `https://` `extends:` entry NOT allowlisted in `trusted_extends:`.
    UntrustedRemote,
}

impl SourceClass {
    fn key(self) -> &'static str {
        match self {
            Self::Extended => "extended",
            Self::Remote => "remote",
            Self::UntrustedRemote => "untrusted_remote",
        }
    }
}

/// The [`PROVENANCE_MARKER`] contents of one rule, read back as source names.
#[derive(Debug, Default)]
pub(crate) struct Provenance {
    pub(crate) extended: Option<String>,
    remote: Option<String>,
    untrusted_remote: Option<String>,
}

impl Provenance {
    pub(crate) fn read(mapping: &Mapping) -> Self {
        let marker = mapping
            .get(PROVENANCE_MARKER)
            .and_then(serde_yaml_ng::Value::as_mapping);
        let source = |class: SourceClass| {
            marker
                .and_then(|m| m.get(class.key()))
                .map(|v| v.as_str().unwrap_or("an extended config").to_string())
        };
        Self {
            extended: source(SourceClass::Extended),
            remote: source(SourceClass::Remote),
            untrusted_remote: source(SourceClass::UntrustedRemote),
        }
    }
}

/// Record that `source` (of class `class`) contributed every mapping in
/// `mappings`. An earlier record of the same class is kept: the first source
/// named is as good as any for the error, and keeping it makes the mark
/// idempotent across a re-merge.
pub(crate) fn mark_provenance_in(mappings: &mut [Mapping], class: SourceClass, source: &str) {
    for mapping in mappings {
        let mut marker = match mapping.remove(PROVENANCE_MARKER) {
            Some(serde_yaml_ng::Value::Mapping(m)) => m,
            _ => Mapping::new(),
        };
        if !marker.contains_key(class.key()) {
            marker.insert(class.key().into(), source.into());
        }
        mapping.insert(PROVENANCE_MARKER.into(), marker.into());
    }
}

/// Union `incoming`'s [`PROVENANCE_MARKER`] into `existing`'s, keeping
/// `existing`'s source for a class both carry.
pub(crate) fn union_provenance(existing: &mut Mapping, incoming: &Mapping) {
    let Some(theirs) = incoming
        .get(PROVENANCE_MARKER)
        .and_then(serde_yaml_ng::Value::as_mapping)
    else {
        return;
    };
    let mut ours = match existing.remove(PROVENANCE_MARKER) {
        Some(serde_yaml_ng::Value::Mapping(m)) => m,
        _ => Mapping::new(),
    };
    for (class, source) in theirs {
        if !ours.contains_key(class) {
            ours.insert(class.clone(), source.clone());
        }
    }
    existing.insert(PROVENANCE_MARKER.into(), ours.into());
}

/// One `vars.<NAME>` read in a `when` expression of an untrusted remote.
#[derive(Debug, Clone)]
pub(crate) struct UntrustedWhenVarRead {
    source: String,
    /// `rule` or `template`.
    what: &'static str,
    id: String,
    /// The field path, e.g. `when` or `require[0].when_iter`.
    field: String,
    var: String,
}

/// Refuse a template instance that a REMOTE ruleset helped shape (its `vars:`,
/// its `extends_template:`, or any other field) when substituting its vars into
/// the template turns a `since:` into a legacy `${VAR}` environment reference.
/// The per-source gate ([`reject_env_expansion_in`]) sees only each raw value,
/// so `since: "{{vars.a}}{{vars.b}}"` with remote `vars: {a: "$", b: "{SECRET}"}`
/// (or a template `since: "${{{vars.name}}}"` with a remote-chosen name) passes
/// it and then expands into `${SECRET}`. Only a `since:` that both carries a
/// placeholder and renders into a `${` is refused: a literal `${ALINT_BASE_SHA}`
/// the user's own template wrote is not assembled by the remote.
pub(crate) fn reject_remote_assembled_env_refs(
    rule: &Mapping,
    templates_by_id: &std::collections::HashMap<String, &Mapping>,
    id: &str,
) -> Result<()> {
    let Some(source) = Provenance::read(rule).remote else {
        return Ok(());
    };
    let Some(template) = rule
        .get("extends_template")
        .and_then(|v| v.as_str())
        .and_then(|t| templates_by_id.get(t))
    else {
        return Ok(());
    };
    let vars = instance_vars(rule);
    if let Some(field) = find_assembled_env_ref(template, &vars, "since") {
        return Err(Error::rule_config(
            id,
            format!(
                "`{field}` expands into an environment variable reference (`${{...}}`) \
                 built from template variables that a remote ruleset ({source}) supplies; \
                 a remote ruleset may not read the environment. Set the value in your \
                 own top-level config instead"
            ),
        ));
    }
    Ok(())
}

/// Rule fields holding a `when` expression: the rule gate itself and the
/// per-iteration filter of the `for_each_*` / `every_matching_has` kinds.
const WHEN_FIELDS: &[&str] = &["when", "when_iter"];

/// Why a rule's `when` expression is refused from an untrusted source.
enum WhenRefusal {
    /// The expression reads `env.<NAME>`.
    ReadsEnv { field: String, name: String },
    /// The expression reads `vars.<NAME>`, whose value the user's own config
    /// interpolated from the environment (`raw` is its pre-interpolation text).
    ReadsEnvVar {
        field: String,
        name: String,
        raw: String,
    },
    /// The expression carries a `{{...}}` placeholder, so what it reads is only
    /// known after template expansion (checked again there).
    Placeholder { field: String },
}

/// Call `f` with the field path and source of every `when` / `when_iter` in
/// `rule` and its nested `require:` rules (any depth), stopping at the first
/// `Some`.
fn find_in_whens<T>(rule: &Mapping, f: &mut impl FnMut(String, &str) -> Option<T>) -> Option<T> {
    fn walk<T>(
        rule: &Mapping,
        prefix: &str,
        f: &mut impl FnMut(String, &str) -> Option<T>,
    ) -> Option<T> {
        for field in WHEN_FIELDS {
            if let Some(src) = rule.get(*field).and_then(|v| v.as_str())
                && let Some(found) = f(format!("{prefix}{field}"), src)
            {
                return Some(found);
            }
        }
        let require = rule.get("require").and_then(|v| v.as_sequence())?;
        require.iter().enumerate().find_map(|(i, nested)| {
            walk(nested.as_mapping()?, &format!("{prefix}require[{i}]."), f)
        })
    }
    walk(rule, "", f)
}

/// The first `when` / `when_iter` in `rule` (or any nested `require:` rule) that
/// reads the environment -- directly, or through a var named in `env_vars`
/// (env-derived top-level vars) -- or that holds a placeholder when
/// `allow_placeholders` is false. An expression that does not parse is left to
/// the rule builder, which reports the parse error with the rule's id.
fn find_when_refusal(
    rule: &Mapping,
    allow_placeholders: bool,
    env_vars: &std::collections::HashMap<String, String>,
) -> Option<WhenRefusal> {
    find_in_whens(rule, &mut |field, src| {
        if src.contains("{{") && !allow_placeholders {
            return Some(WhenRefusal::Placeholder { field });
        }
        let expr = alint_core::when::parse(src).ok()?;
        if let Some(name) = expr.first_env_ref() {
            return Some(WhenRefusal::ReadsEnv {
                field,
                name: name.to_string(),
            });
        }
        expr.var_refs().into_iter().find_map(|name| {
            env_vars.get(name).map(|raw| WhenRefusal::ReadsEnvVar {
                field: field.clone(),
                name: name.to_string(),
                raw: raw.clone(),
            })
        })
    })
}

fn when_refusal_error(what: &str, id: &str, refusal: &WhenRefusal, source: &str) -> Error {
    match refusal {
        WhenRefusal::ReadsEnv { field, name } => Error::Other(format!(
            "{source}: {what} `{id}`: `{field}:` reads `env.{name}`; an untrusted extends \
             source may not read the environment. Add the URL to `trusted_extends:` to \
             allow it"
        )),
        WhenRefusal::ReadsEnvVar { field, name, raw } => Error::Other(format!(
            "{source}: {what} `{id}`: `{field}:` reads `vars.{name}`, whose value comes \
             from the environment (`{raw}`); an untrusted extends source may not read the \
             environment. Add the URL to `trusted_extends:` to allow it"
        )),
        WhenRefusal::Placeholder { field } => Error::Other(format!(
            "{source}: {what} `{id}`: `{field}:` contains a `{{{{...}}}}` placeholder; an \
             untrusted extends source may not build a `when` expression from template \
             variables, because it could read the environment. Add the URL to \
             `trusted_extends:` to allow it"
        )),
    }
}

/// Refuse an `env.*` read in a `when:` / `when_iter:` declared by an untrusted
/// remote (an `https://` source not in `trusted_extends:`). Each `when` is a
/// boolean oracle on the consumer's environment: three rules gated on
/// `env.TOKEN matches "^g"` / `"^h"` / `"^i"` leak a secret one character at
/// a time through which of them fires. Checked on the PARSED expression (so
/// spacing and parentheses cannot hide a read), in rules and templates, at every
/// `require:` depth. A template's `when` may not carry a placeholder at all: its
/// variables are supplied later, by whichever instance expands it. An instance
/// that an untrusted remote shaped is re-checked after expansion
/// ([`reject_untrusted_assembled_when`]).
///
/// A `vars.*` read is the same oracle when the user's config interpolated that
/// var from the environment, but which vars are env-derived is only known once
/// every local config has merged. So every `vars.*` read is returned, to be
/// judged by `finalize` ([`reject_untrusted_env_var_reads`]).
pub(crate) fn reject_env_reads_in_when(
    rules: &[Mapping],
    templates: &[Mapping],
    source: &str,
) -> Result<Vec<UntrustedWhenVarRead>> {
    let id_of = |m: &Mapping| {
        m.get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)")
            .to_string()
    };
    let no_env_vars = std::collections::HashMap::new();
    let mut var_reads = Vec::new();
    for (what, list, allow_placeholders) in [("rule", rules, true), ("template", templates, false)]
    {
        for m in list {
            // A rule's own fields are never substituted, so a placeholder there is
            // inert text the `when` parser rejects at build; only env reads matter.
            if let Some(refusal) = find_when_refusal(m, allow_placeholders, &no_env_vars) {
                return Err(when_refusal_error(what, &id_of(m), &refusal, source));
            }
            find_in_whens(m, &mut |field, src| {
                let expr = alint_core::when::parse(src).ok()?;
                for var in expr.var_refs() {
                    var_reads.push(UntrustedWhenVarRead {
                        source: source.to_string(),
                        what,
                        id: id_of(m),
                        field: field.clone(),
                        var: var.to_string(),
                    });
                }
                None::<()>
            });
        }
    }
    Ok(var_reads)
}

/// Refuse every `vars.<NAME>` read an untrusted remote's `when` made (recorded
/// per source by [`reject_env_reads_in_when`]) of a top-level var whose
/// EFFECTIVE value the user's config interpolated from the environment.
pub(crate) fn reject_untrusted_env_var_reads(
    reads: &[UntrustedWhenVarRead],
    env_vars: &std::collections::HashMap<String, String>,
) -> Result<()> {
    for read in reads {
        if let Some(raw) = env_vars.get(&read.var) {
            let refusal = WhenRefusal::ReadsEnvVar {
                field: read.field.clone(),
                name: read.var.clone(),
                raw: raw.clone(),
            };
            return Err(when_refusal_error(
                read.what,
                &read.id,
                &refusal,
                &read.source,
            ));
        }
    }
    Ok(())
}

/// The post-expansion half of [`reject_env_reads_in_when`]: an instance that an
/// untrusted remote shaped (its `vars:`, its `extends_template:`, ...) may fill a
/// TRUSTED template's `when: "{{vars.cond}}"` with `env.TOKEN matches "^g"`
/// (or with a read of an env-derived top-level var, named in `env_vars`).
/// Re-check every `when` the expansion produced from a placeholder. A `when`
/// the template wrote literally is the template author's choice, not the
/// remote's, and stays allowed.
pub(crate) fn reject_untrusted_assembled_when(
    rule: &Mapping,
    templates_by_id: &std::collections::HashMap<String, &Mapping>,
    id: &str,
    env_vars: &std::collections::HashMap<String, String>,
) -> Result<()> {
    let Some(source) = Provenance::read(rule).untrusted_remote else {
        return Ok(());
    };
    let Some(template) = rule
        .get("extends_template")
        .and_then(|v| v.as_str())
        .and_then(|t| templates_by_id.get(t))
    else {
        return Ok(());
    };
    let vars = instance_vars(rule);
    // Keep only the `when` fields that held a placeholder, rendered; the rest of
    // the template is irrelevant to what the remote could assemble.
    let rendered = render_placeholder_whens(template, &vars);
    if let Some(refusal) = find_when_refusal(&rendered, true, env_vars) {
        return Err(when_refusal_error("rule", id, &refusal, &source));
    }
    Ok(())
}

/// Refuse to expand a template into an instance when an untrusted remote shaped
/// either of them and the template substitutes an env-derived instance var
/// (`vars: {token: "{{env.NPM_TOKEN}}"}` on the user's own instance, recorded in
/// [`ENV_VARS_MARKER`]) into ANY field. A remote can add a field to a template
/// the user defined under the same id (`message: "{{vars.token}}"`, or a
/// `pattern:` / `paths:` that turns the secret into an oracle), and the value
/// would then reach rule output, CI logs and SARIF.
pub(crate) fn reject_untrusted_env_instance_vars(
    rule: &Mapping,
    templates_by_id: &std::collections::HashMap<String, &Mapping>,
    id: &str,
) -> Result<()> {
    fn strings(v: &serde_yaml_ng::Value, f: &mut impl FnMut(&str)) {
        match v {
            serde_yaml_ng::Value::String(s) => f(s),
            serde_yaml_ng::Value::Sequence(seq) => seq.iter().for_each(|v| strings(v, f)),
            serde_yaml_ng::Value::Mapping(m) => m.values().for_each(|v| strings(v, f)),
            _ => {}
        }
    }
    let Some(env_vars) = rule.get(ENV_VARS_MARKER).and_then(|v| v.as_mapping()) else {
        return Ok(());
    };
    let Some((template_id, template)) = rule
        .get("extends_template")
        .and_then(|v| v.as_str())
        .and_then(|t| templates_by_id.get(t).map(|m| (t, *m)))
    else {
        return Ok(());
    };
    let Some(source) = Provenance::read(rule)
        .untrusted_remote
        .or_else(|| Provenance::read(template).untrusted_remote)
    else {
        return Ok(());
    };
    // Scan with the expansion's own placeholder parser, so spacing variants
    // (`{{ vars.token }}`) are found exactly when expansion would substitute them.
    let hit = std::cell::RefCell::new(None::<String>);
    let mut probe = |s: &str| {
        alint_core::template::render_message(s, |ns, key| {
            let mut hit = hit.borrow_mut();
            if hit.is_none() && ns == "vars" && env_vars.contains_key(key) {
                *hit = Some(key.to_string());
            }
            None
        });
    };
    for (k, v) in template {
        if k.as_str() != Some(PROVENANCE_MARKER) {
            strings(v, &mut probe);
        }
    }
    let Some(name) = hit.into_inner() else {
        return Ok(());
    };
    let raw = env_vars
        .get(name.as_str())
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    Err(Error::rule_config(
        id,
        format!(
            "template `{template_id}` substitutes `{{{{vars.{name}}}}}`, whose value comes \
             from the environment (`{raw}`), but an untrusted extends source ({source}) \
             shapes this rule or its template; an untrusted extends source may not read \
             the environment. Add the URL to `trusted_extends:` to allow it"
        ),
    ))
}

/// A copy of `rule` keeping only the `when` / `when_iter` fields that contain a
/// `{{` placeholder (rendered with `vars`) and the `require:` structure leading
/// to them, so field paths in an error still line up with the template.
fn render_placeholder_whens(
    rule: &Mapping,
    vars: &std::collections::HashMap<String, String>,
) -> Mapping {
    let mut out = Mapping::new();
    for field in WHEN_FIELDS {
        if let Some(src) = rule.get(*field).and_then(|v| v.as_str())
            && src.contains("{{")
        {
            out.insert((*field).into(), render_template_vars(src, vars).into());
        }
    }
    if let Some(require) = rule.get("require").and_then(|v| v.as_sequence()) {
        let nested: Vec<serde_yaml_ng::Value> = require
            .iter()
            .map(|n| {
                n.as_mapping().map_or(serde_yaml_ng::Value::Null, |m| {
                    render_placeholder_whens(m, vars).into()
                })
            })
            .collect();
        out.insert("require".into(), nested.into());
    }
    out
}

/// The path of the first `key:` string in `m` (at any depth) whose value holds a
/// `{{` placeholder and renders, with `vars`, into a `${` env reference.
fn find_assembled_env_ref(
    m: &Mapping,
    vars: &std::collections::HashMap<String, String>,
    key: &str,
) -> Option<String> {
    fn walk(
        v: &serde_yaml_ng::Value,
        vars: &std::collections::HashMap<String, String>,
        key: &str,
        path: &str,
    ) -> Option<String> {
        match v {
            serde_yaml_ng::Value::Mapping(m) => m.iter().find_map(|(k, v)| {
                let name = k.as_str().unwrap_or_default();
                let child = if path.is_empty() {
                    name.to_string()
                } else {
                    format!("{path}.{name}")
                };
                if name == key
                    && let Some(raw) = v.as_str()
                    && raw.contains("{{")
                    && render_template_vars(raw, vars).contains("${")
                {
                    return Some(child);
                }
                walk(v, vars, key, &child)
            }),
            serde_yaml_ng::Value::Sequence(seq) => seq
                .iter()
                .enumerate()
                .find_map(|(i, v)| walk(v, vars, key, &format!("{path}[{i}]"))),
            _ => None,
        }
    }
    walk(&serde_yaml_ng::Value::Mapping(m.clone()), vars, key, "")
}

/// Rule kinds that spawn an arbitrary user-supplied process.
/// Every one is trust-gated identically by
/// [`reject_command_rules_in`]: it may only be declared in the
/// user's own top-level config, never introduced via `extends:`.
/// Keep in sync with the rule implementations in `alint-rules`
/// that shell out — `command` (per-file CLI),
/// `generated_file_fresh` (runs a generator), `command_idempotent`
/// (runs a checker). Adding a spawn-capable rule kind without
/// adding it here is a code-execution gap.
pub const SPAWNING_RULE_KINDS: &[&str] = &["command", "generated_file_fresh", "command_idempotent"];

/// Fix ops that shell out, trust-gated identically to
/// [`SPAWNING_RULE_KINDS`]: a spawning fix may be declared **only** in the
/// user's own top-level config, never introduced via `extends:` / a nested
/// `.alint.yml` / a `templates:` block / bundled (auto-fix.md 5.5). Enforced by
/// [`reject_spawning_fix_ops_in`] (rules, at every `require:` depth),
/// [`reject_spawning_fix_op_templates_in`] (inherited templates), and a
/// `finalize` backstop that refuses one in ANY source's templates. Adding a
/// spawn-capable op without listing it here is a code-execution gap, exactly as
/// for rule kinds; `spawning_fix_ops_are_gated` asserts every entry is a real op.
/// `git_untrack` (`git rm --cached`) and `command` (a user-supplied `run:` fix
/// command on the `command` rule) are the two spawning fix ops.
pub const SPAWNING_FIX_OPS: &[&str] = &["git_untrack", "command"];

/// Reject any process-spawning rule kind (see
/// [`SPAWNING_RULE_KINDS`]) in the given mapping list. Used by the
/// `extends:` resolver to enforce that only the user's own
/// top-level config can declare a rule that shells out. Same trust
/// model as `alint_core::facts::reject_custom_facts_in` —
/// extending a ruleset must never gain you arbitrary code
/// execution. `source` is shown in the error to help the user
/// identify which extended config introduced the violation.
///
/// (Kept its original name for API stability; it now gates the
/// whole spawning-kind set, not only `kind: command`.)
pub fn reject_command_rules_in(rules: &[Mapping], source: &str) -> Result<()> {
    for rule in rules {
        reject_spawning_in_rule(rule, source)?;
    }
    Ok(())
}

/// Reject a per-rule fix-tier PROMOTION (`fix: { <op>: { applicability: safe } }`)
/// declared in an inherited config. `file_remove` defaults to `Unsafe` -- a bare
/// `alint fix` will not delete a file irreversibly -- and a user may promote it
/// back to `Safe` on a specific rule, but that is ONLY the user's own top-level
/// config's call (auto-fix.md 5.5: an inherited fixer may be *demoted*, never
/// *promoted*). An extended ruleset promoting `file_remove` to `Safe` would
/// silently opt a repo into auto-deletion, so it is refused here. Same trust
/// model as [`reject_command_rules_in`]. Scans nested `require:` blocks too.
pub fn reject_fix_promotion_in(rules: &[Mapping], source: &str) -> Result<()> {
    for rule in rules {
        reject_fix_promotion_in_rule(rule, source)?;
    }
    Ok(())
}

fn reject_fix_promotion_in_rule(rule: &Mapping, source: &str) -> Result<()> {
    if let Some(fix) = rule.get("fix").and_then(|v| v.as_mapping()) {
        for (op, args) in fix {
            // A non-mapping op value never reaches here as a valid fixer: the
            // `fix:` deserializer requires every op's options to be a mapping, so
            // a sequence-shaped `[safe]` that this scan cannot see is refused
            // there instead of loading as a positional promotion.
            let promotes = args
                .as_mapping()
                .and_then(|m| m.get("applicability"))
                .and_then(|v| v.as_str())
                .is_some_and(|a| a.eq_ignore_ascii_case("safe") || a.contains("{{"));
            if promotes {
                let id = rule
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(unknown)");
                let op = op.as_str().unwrap_or("<fix>");
                return Err(Error::Other(format!(
                    "rule {id:?}: `fix.{op}.applicability: safe` promotes a fix to auto-apply and \
                     is only allowed in your own top-level config; an extended config ({source}) \
                     may not opt this repo into auto-applying a destructive fix (it may still \
                     demote to `suggestion`/`never`). Declare the promotion in your top-level \
                     `rules:`."
                )));
            }
        }
    }
    if let Some(require) = rule.get("require").and_then(|v| v.as_sequence()) {
        for nested in require {
            if let Some(nested_map) = nested.as_mapping() {
                reject_fix_promotion_in_rule(nested_map, source)?;
            }
        }
    }
    Ok(())
}

/// The `fix:` ops a remote `extends:` could abuse to mutate YOUR files, so they
/// demote to `suggestion` from an untrusted (non-`trusted_extends:`) remote
/// (auto-fix.md 5.5). Two kinds: ops that write ruleset-authored BYTES (a regex
/// `replace`ment template, the inline `content:` / `content_from:` of a create /
/// prepend / append, a `set_value`), AND ops the ruleset can AIM at a file to
/// change its content or meaning even though the bytes are the victim's own
/// (`sync_from` picks which file overwrites which; `sort` reorders / dedups a
/// file the rule's `paths:` selects). The genuinely fixed-behavior ops (the
/// hygiene normalizers, `file_remove`, `file_rename`, `chmod`, `remove_value`,
/// `dir_create`, `relocate`, `git_untrack`) are honored at their own tier from
/// any source; a destructive one like `file_remove` is already gated by its
/// Unsafe tier, independent of source.
pub(crate) const CONTENT_INJECTING_FIX_OPS: &[&str] = &[
    "replace",
    "file_create",
    "file_prepend",
    "file_append",
    // `set_value` writes the host rule's `equals:` value into the file -- ruleset-
    // authored bytes, so a remote must PROPOSE it, never auto-write (auto-fix.md
    // 5.5). `remove_value` is a DELETION (no ruleset bytes) -> fixed-behavior,
    // gated by its Unsafe tier like `file_remove`, so it is deliberately absent.
    "set_value",
    // `sync_from` overwrites a target with another file's bytes wholesale, and the
    // host `cross_file` rule's `source:` chooses which file overwrites which -- so
    // an untrusted remote could aim it at your files. It PROPOSES, never
    // auto-writes, from an untrusted remote (auto-fix.md 5.5).
    "sync_from",
    // `create_and_register` appends a ruleset-chosen member value into a manifest
    // list (and, in a follow-up, creates a file from ruleset-authored content), so
    // an untrusted remote must PROPOSE it, never auto-write (auto-fix.md 5.5).
    "create_and_register",
    // `sort` writes no ruleset bytes -- it reorders (and, with `unique`, dedups)
    // the victim's OWN lines -- but the host `ordered_block` rule's `paths:` +
    // `comparator` let an untrusted remote AIM a reorder at an order-SIGNIFICANT
    // file (`.gitignore` negation order, `CODEOWNERS` last-match precedence) to
    // change its meaning at the Safe tier. Same "aim it at your files" risk as
    // `sync_from`, so it PROPOSES, never auto-writes, from an untrusted remote.
    "sort",
    // `indent_style` rewrites the victim's OWN leading whitespace (tabs -> spaces),
    // writing no ruleset bytes -- but a remote's `paths:` can AIM the reindent at an
    // indent-SIGNIFICANT file (a `Makefile` recipe needs a literal tab; converting
    // it to spaces is a HARD build break). It is `Unsafe` by DEFAULT (so a bare fix
    // never auto-applies it), and content-injecting here too as belt-and-suspenders:
    // it PROPOSES, never auto-writes, from an untrusted remote even if a rule set an
    // explicit `applicability: safe`. Same aim risk as `sort`.
    "indent_style",
    // `insert_line` splices the host `ordered_block` rule's `require:` lines --
    // ruleset-authored bytes -- into the victim's file (e.g. a `CODEOWNERS` owner
    // line), the clearest injection case. An untrusted remote must PROPOSE it.
    "insert_line",
    // `insert_header` inserts the host `file_header` rule's `content` /
    // `content_from` header bytes near the top of the victim's file -- ruleset-
    // authored bytes, like `file_prepend` (which it refines). An untrusted remote
    // must PROPOSE it.
    "insert_header",
    // `file_normalize_line_endings` writes no ruleset bytes -- it rewrites the
    // victim's OWN line endings -- but line endings are SIGNIFICANCE-bearing, so a
    // remote's `paths:` can AIM a CRLF rewrite at a `#!/bin/sh` shebang (breaking
    // it) at the Safe tier. Same aim risk as `sort` / `indent_style`, so it PROPOSES,
    // never auto-writes, from an untrusted remote (audit: partition MED).
    "file_normalize_line_endings",
    // `file_rename` writes no ruleset bytes, but a remote's `paths:` + `filename_case`
    // can AIM a mass case-rename at the victim's source, breaking case-sensitive
    // imports at the Safe tier. Aimable like `sort`, so it PROPOSES from an untrusted
    // remote (audit: partition MED). (The rename TARGET is derived, never
    // remote-chosen, so this is availability-only, not a content-injection.)
    "file_rename",
    // The hygiene normalizers + `chmod` below write no ruleset bytes, but a remote's
    // `paths:` can AIM them at a file where the "cosmetic" change is load-bearing, so
    // an untrusted remote demotes each to a suggestion (asamarts, 2026-09-27 audit R3
    // -- extends the "aimable at the Safe tier" principle to non-semantic hygiene).
    // `file_strip_bidi` / `file_strip_zero_width` are DELIBERATELY EXCLUDED (stay in
    // FIXED_BEHAVIOR): they are security-POSITIVE (they remove Trojan-Source / other
    // zero-width attacks), so honoring them from any source is the safe default --
    // demoting them would let a remote-sourced attack survive.
    // `file_trim_trailing_whitespace`: trailing whitespace is significant in some
    // formats (a Markdown hard line break is two trailing spaces).
    "file_trim_trailing_whitespace",
    // `file_append_final_newline`: the final byte can matter to downstream tooling.
    "file_append_final_newline",
    // `file_strip_bom`: a BOM is an encoding signature some consumers require.
    "file_strip_bom",
    // `file_collapse_blank_lines`: blank-line runs are significant where they delimit
    // paragraphs / sections.
    "file_collapse_blank_lines",
    // `chmod`: a +/-x flip aimed at a script (loses +x -> breaks) or a data file
    // (gains +x -> exec surface) is a real permission change -- only the 0o111 bits
    // move, but the effect is remote-aimable. Unsafe-gated locally, demoted from a
    // remote.
    "chmod",
];

/// Demote every content-injecting fixer in `rules` to `applicability: suggestion`
/// (unless it already declares the stricter `suggestion` / `never`), so a remote
/// `extends:` the user has NOT listed in `trusted_extends:` can PROPOSE a content
/// edit but never auto-write it. Rewrites the raw `Mapping` in place, before the
/// merge, so the built fixer carries the capped tier. Scans nested `require:`
/// blocks too (a `for_each_dir` etc. can bury a content fixer). Mirrors the
/// read-only [`reject_fix_promotion_in`] navigation.
pub(crate) fn demote_content_fixers_in(rules: &mut [Mapping]) {
    for rule in rules.iter_mut() {
        demote_content_fixers_in_rule(rule);
    }
}

/// Tag raw rule/template mappings whose eventual effective fixer must be capped
/// after composition and template expansion. The source-local demotion still
/// happens immediately; this marker closes mixed-source cases where the fixer is
/// acquired only later.
pub(crate) fn mark_untrusted_fix_sources_in(mappings: &mut [Mapping]) {
    for mapping in mappings {
        mapping.insert(
            serde_yaml_ng::Value::from(UNTRUSTED_FIX_SOURCE_MARKER),
            serde_yaml_ng::Value::Bool(true),
        );
    }
}

pub(crate) fn has_untrusted_fix_source(mapping: &Mapping) -> bool {
    mapping
        .get(UNTRUSTED_FIX_SOURCE_MARKER)
        .and_then(serde_yaml_ng::Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn take_untrusted_fix_source(mapping: &mut Mapping) -> bool {
    mapping
        .remove(UNTRUSTED_FIX_SOURCE_MARKER)
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// Field-merge one raw rule/template mapping while preserving the monotonic
/// provenance bit. Treating the marker like an ordinary last-wins YAML field
/// would let a later mapping erase an earlier untrusted contribution.
pub(crate) fn merge_mapping_fields(existing: &mut Mapping, incoming: Mapping) {
    let untrusted_fix_source =
        has_untrusted_fix_source(existing) || has_untrusted_fix_source(&incoming);
    union_provenance(existing, &incoming);
    // The env-derived mark describes the `vars:` it was recorded with; a later
    // `vars:` replaces the map wholesale, so it replaces (or drops) the mark too.
    if incoming.contains_key("vars") {
        existing.remove(ENV_VARS_MARKER);
    }
    for (key, value) in incoming {
        if !matches!(
            key.as_str(),
            Some(UNTRUSTED_FIX_SOURCE_MARKER | PROVENANCE_MARKER)
        ) {
            existing.insert(key, value);
        }
    }
    if untrusted_fix_source {
        existing.insert(
            serde_yaml_ng::Value::from(UNTRUSTED_FIX_SOURCE_MARKER),
            serde_yaml_ng::Value::Bool(true),
        );
    }
}

pub(crate) fn demote_content_fixers_in_rule(rule: &mut Mapping) {
    if let Some(fix) = rule
        .get_mut("fix")
        .and_then(serde_yaml_ng::Value::as_mapping_mut)
    {
        for (op, args) in fix.iter_mut() {
            let is_content = op
                .as_str()
                .is_some_and(|o| CONTENT_INJECTING_FIX_OPS.contains(&o));
            if !is_content {
                continue;
            }
            let Some(args_map) = args.as_mapping_mut() else {
                continue;
            };
            // Cap to `suggestion`, but never PROMOTE a stricter declared tier: a
            // ruleset that opted its own content fix out entirely (`never`) or down
            // to `suggestion` already stays there. A demote is one-directional.
            let already_stricter = args_map
                .get("applicability")
                .and_then(serde_yaml_ng::Value::as_str)
                .is_some_and(|a| {
                    a.eq_ignore_ascii_case("suggestion") || a.eq_ignore_ascii_case("never")
                });
            if !already_stricter {
                args_map.insert(
                    serde_yaml_ng::Value::from("applicability"),
                    serde_yaml_ng::Value::from("suggestion"),
                );
            }
        }
    }
    // Recurse the NESTED-RULE `require:` block (`for_each_dir` etc. carry a
    // `Vec<NestedRuleSpec>` here). NOTE: `ordered_block` also has a `require:` key,
    // but its items are SCALARS (exact lines), so `as_mapping_mut()` skips them --
    // the two same-named features never cross wires. Keep this mapping-only guard
    // if either feature changes (an ordered_block require line authored as a
    // mapping must not be treated as a nested rule).
    if let Some(require) = rule
        .get_mut("require")
        .and_then(serde_yaml_ng::Value::as_sequence_mut)
    {
        for nested in require {
            if let Some(nested_map) = nested.as_mapping_mut() {
                demote_content_fixers_in_rule(nested_map);
            }
        }
    }
}

/// Reject a spawning `kind` in `rule` OR in any of its nested `require:`
/// specs, recursively. `for_each_dir` / `for_each_file` /
/// `every_matching_has` carry a `require:` block of nested rules
/// (`Vec<NestedRuleSpec>`) whose `kind`/`command` flatten into the parent
/// rule's options — a third spawn vector the top-level `kind` check (and a
/// post-`finalize` scan) would miss, since the nested spec is buried in the
/// parent's options and never becomes a top-level `RuleSpec`. We must scan
/// the raw mappings here, before instantiation, at every depth. (If a new
/// rule kind ever adds another `Vec<NestedRuleSpec>` option field, gate it
/// here too.)
fn reject_spawning_in_rule(rule: &Mapping, source: &str) -> Result<()> {
    if let Some(kind) = find_spawning_kind(rule) {
        let id = rule
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)");
        return Err(Error::Other(format!(
            "rule {id:?}: `kind: {kind}` spawns a process and is only allowed in the \
             user's top-level config; declaring one in an extended config ({source}) - \
             including inside a `require:` block - is refused because it would let a \
             ruleset run arbitrary code"
        )));
    }
    Ok(())
}

/// The spawning kind (see [`SPAWNING_RULE_KINDS`]) declared by `rule` or by any
/// nested `require:` spec, recursively. Shared by the per-source rule gate, the
/// per-source template gate, and the `finalize` template backstop so all three
/// scan to the SAME depth: a template-nested `require: [{kind: command}]` must
/// be refused exactly like a rule-nested one.
pub(crate) fn find_spawning_kind(rule: &Mapping) -> Option<&str> {
    if let Some(kind) = rule.get("kind").and_then(|v| v.as_str())
        && SPAWNING_RULE_KINDS.contains(&kind)
    {
        return Some(kind);
    }
    rule.get("require")
        .and_then(|v| v.as_sequence())
        .into_iter()
        .flatten()
        .filter_map(serde_yaml_ng::Value::as_mapping)
        .find_map(find_spawning_kind)
}

/// The first trust-relevant field of a template (a `kind`, or a
/// `fix.<op>.applicability`, at any `require:` depth) whose value contains a
/// `{{` placeholder, named for the error message.
pub(crate) fn find_template_placeholder_in_guarded_field(rule: &Mapping) -> Option<String> {
    let has_placeholder = |v: &serde_yaml_ng::Value| v.as_str().is_some_and(|s| s.contains("{{"));
    if rule.get("kind").is_some_and(has_placeholder) {
        return Some("kind".to_string());
    }
    if let Some(fix) = rule.get("fix").and_then(|v| v.as_mapping()) {
        for (op, args) in fix {
            if args
                .as_mapping()
                .and_then(|m| m.get("applicability"))
                .is_some_and(has_placeholder)
            {
                let op = op.as_str().unwrap_or("<fix>");
                return Some(format!("fix.{op}.applicability"));
            }
        }
    }
    rule.get("require")
        .and_then(|v| v.as_sequence())
        .into_iter()
        .flatten()
        .filter_map(serde_yaml_ng::Value::as_mapping)
        .find_map(find_template_placeholder_in_guarded_field)
}

/// Remove every explicit `applicability: safe` from `rule`'s fix ops (at every
/// `require:` depth) so each falls back to its op's default tier. Applied to an
/// effective rule with untrusted provenance: such a rule may never carry a
/// promotion, whoever authored the fix block it acquired.
pub(crate) fn strip_fix_promotions_in_rule(rule: &mut Mapping) {
    if let Some(fix) = rule
        .get_mut("fix")
        .and_then(serde_yaml_ng::Value::as_mapping_mut)
    {
        for (_op, args) in fix.iter_mut() {
            if let Some(args_map) = args.as_mapping_mut() {
                let promotes = args_map
                    .get("applicability")
                    .and_then(serde_yaml_ng::Value::as_str)
                    .is_some_and(|a| a.eq_ignore_ascii_case("safe"));
                if promotes {
                    args_map.remove("applicability");
                }
            }
        }
    }
    if let Some(require) = rule
        .get_mut("require")
        .and_then(serde_yaml_ng::Value::as_sequence_mut)
    {
        for nested in require {
            if let Some(nested_map) = nested.as_mapping_mut() {
                strip_fix_promotions_in_rule(nested_map);
            }
        }
    }
}

/// Reject any *spawning* fix op (see [`SPAWNING_FIX_OPS`]) declared in the given
/// mapping list. The fix-op analogue of [`reject_command_rules_in`]: a fixer that
/// shells out (today `git_untrack`, `git rm --cached`) is a code-execution
/// surface, so it may be declared ONLY in the user's own top-level config, never
/// introduced via `extends:` / a nested `.alint.yml` / bundled (auto-fix.md 5.5).
/// The existing spawn gate keys on the rule *kind*, so a spawning FIXER attached
/// to a non-spawning kind (a `git_untrack` fix on `file_absent`) slips past it --
/// this gate closes that gap. `source` names the offending config in the error.
/// Scans nested `require:` blocks at every depth, exactly like the kind gate.
pub fn reject_spawning_fix_ops_in(rules: &[Mapping], source: &str) -> Result<()> {
    for rule in rules {
        reject_spawning_fix_op_in_rule(rule, source)?;
    }
    Ok(())
}

fn reject_spawning_fix_op_in_rule(rule: &Mapping, source: &str) -> Result<()> {
    if let Some(op) = find_spawning_fix_op(rule) {
        let id = rule
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)");
        return Err(Error::Other(format!(
            "rule {id:?}: `fix.{op}` spawns a process and is only allowed in the \
             user's top-level config; declaring one in an extended config ({source}) - \
             including inside a `require:` block or a `templates:` entry - is refused \
             because it would let a ruleset run arbitrary code on a bare `alint fix`"
        )));
    }
    Ok(())
}

/// The name of a spawning fix op (see [`SPAWNING_FIX_OPS`]) declared in `rule`'s
/// `fix:` block or any nested `require:` (recursively), if any. Shared by the
/// per-source refusal ([`reject_spawning_fix_op_in_rule`]) and the `finalize`
/// template backstop so both scan to the SAME depth: a spawning fix must be
/// refused whether it sits at a rule/template's top level OR inside its `require:`.
pub(crate) fn find_spawning_fix_op(rule: &Mapping) -> Option<&str> {
    if let Some(fix) = rule.get("fix").and_then(|v| v.as_mapping()) {
        for (op, _args) in fix {
            if let Some(op) = op.as_str()
                && SPAWNING_FIX_OPS.contains(&op)
            {
                return Some(op);
            }
        }
    }
    if let Some(require) = rule.get("require").and_then(|v| v.as_sequence()) {
        for nested in require {
            if let Some(nested_map) = nested.as_mapping()
                && let Some(op) = find_spawning_fix_op(nested_map)
            {
                return Some(op);
            }
        }
    }
    None
}

/// Reject a spawning fix op declared inside a `templates:` block of an inherited
/// ruleset. A template's `fix:` block splices into its referencing rule at
/// `finalize` (after the per-rule gate above), so a spawning fixer smuggled
/// through a `templates:` entry would otherwise expand into a spawning fix past
/// the gate -- the fix-op analogue of [`reject_spawning_templates_in`]. A
/// template is shaped like a rule (`fix:` + optional `require:`), so the same
/// per-rule scan applies. `finalize` enforces the same invariant for every
/// source; this earlier per-source check names the offending ruleset.
pub fn reject_spawning_fix_op_templates_in(templates: &[Mapping], source: &str) -> Result<()> {
    for template in templates {
        reject_spawning_fix_op_in_rule(template, source)?;
    }
    Ok(())
}

/// Reject any process-spawning rule kind (see [`SPAWNING_RULE_KINDS`])
/// declared inside a `templates:` block of an inherited ruleset. A
/// template instance (`extends_template:`) carries no `kind` of its own,
/// so a spawning template would slip past [`reject_command_rules_in`]
/// (which inspects `rules[].kind`) and expand into a `command` rule at
/// `finalize` time — the C1 code-execution bypass. Spawning kinds are
/// confined to the user's own top-level `rules:`, never a template.
/// `finalize` enforces the same invariant for every source; this earlier
/// per-source check names the offending ruleset (`source`) in the error.
pub fn reject_spawning_templates_in(templates: &[Mapping], source: &str) -> Result<()> {
    for template in templates {
        if let Some(kind) = find_spawning_kind(template) {
            let id = template
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("(unknown)");
            return Err(Error::Other(format!(
                "template {id:?}: `kind: {kind}` spawns a process and is not allowed in \
                 an inherited ruleset ({source}); a spawning kind may only appear in a \
                 top-level `rules:` entry, never in a `templates:` block, because a \
                 template instance expands after the spawn gate and would let the \
                 ruleset run arbitrary code"
            )));
        }
    }
    Ok(())
}

/// Reject a fix-tier PROMOTION (`fix: { <op>: { applicability: safe } }`)
/// declared inside a `templates:` block of an inherited ruleset -- the template
/// analogue of [`reject_fix_promotion_in`]. A template instance
/// (`extends_template:`) carries the template's `fix:` block, which is spliced
/// into the referencing rule at `finalize` time -- AFTER the rule-level
/// promotion gate has run -- so a promotion smuggled through a template would
/// silently opt a repo into auto-applying a destructive fix (a bare `alint fix`
/// deleting files), defeating the whole point of that gate (5.5: inherited
/// fixers may be demoted, never promoted). Same trust model as
/// [`reject_spawning_templates_in`]; each template is rule-shaped, so it is
/// scanned exactly like a rule (its `fix` block plus any nested `require:`).
///
/// A `finalize` backstop cannot substitute here: unlike a spawning kind (never
/// legal in any template), a promotion in the user's OWN top-level template is
/// legitimate, so the refusal must be source-aware -- which only this
/// extends-time, per-source check is.
pub fn reject_fix_promotion_templates_in(templates: &[Mapping], source: &str) -> Result<()> {
    for template in templates {
        reject_fix_promotion_in_rule(template, source)?;
    }
    Ok(())
}

/// Reject a non-default `allow_out_of_root:` in an inherited ruleset.
/// Like [`reject_command_rules_in`], the path-confinement escape hatch
/// may only be opened by the user's own top-level config — an
/// `extends:`'d ruleset granting itself reads outside the repo root is
/// the exact threat confinement exists to stop. `source` names the
/// offending config. See `docs/design/v0.12/allow_out_of_root.md`.
pub fn reject_allow_out_of_root_in(allow: &alint_core::AllowOutOfRoot, source: &str) -> Result<()> {
    if !allow.is_confined() {
        return Err(Error::Other(format!(
            "`allow_out_of_root:` is only allowed in the user's top-level config; \
             declaring it in an extended config ({source}) is refused because it would \
             let a ruleset grant itself reads outside the repo root"
        )));
    }
    Ok(())
}

/// Reject a `baseline:` in an inherited ruleset. Like
/// [`reject_allow_out_of_root_in`], the baseline path is a trusted top-level
/// input: an `extends:`'d ruleset that pointed the gate at its own baseline
/// could silently suppress findings the user never reviewed. `source` names
/// the offending config. See `docs/design/baseline.md` §2.3.
pub fn reject_baseline_in(baseline: &Option<std::path::PathBuf>, source: &str) -> Result<()> {
    if baseline.is_some() {
        return Err(Error::Other(format!(
            "`baseline:` is only allowed in the user's top-level config; \
             declaring it in an extended config ({source}) is refused because it would \
             let a ruleset choose which findings the gate suppresses"
        )));
    }
    Ok(())
}

/// Reject a top-level `ignore:` from an untrusted remote (an `https://` source
/// not in `trusted_extends:`). `ignore:` removes paths from the walk for EVERY
/// rule, the user's own included, so a remote ruleset declaring `ignore:
/// ["src/**"]` would silently switch off checks it never wrote, the same "choose
/// which findings disappear" power [`reject_baseline_in`] withholds from every
/// extended source. Local and bundled sources, and allowlisted remotes, keep
/// contributing `ignore:` entries. `source` names the offending config.
pub fn reject_untrusted_ignore_in(ignore: &[String], source: &str) -> Result<()> {
    if !ignore.is_empty() {
        return Err(Error::Other(format!(
            "{source}: `ignore:` is not allowed from an untrusted extends source (it can \
             hide files from every rule); add the URL to `trusted_extends:` to allow it"
        )));
    }
    Ok(())
}

/// Reject a `trusted_extends:` in an inherited ruleset. The allowlist grants a
/// remote's content-injecting fixers auto-apply rights, so a remote (or any
/// non-top-level config) that could set it would allowlist ITSELF, defeating the
/// gate. Only the user's own top-level config (and its `.alint.d/` drop-ins) may
/// grant trust. `source` names the offending config. See auto-fix.md 5.5.
pub fn reject_trusted_extends_in(trusted_extends: &[String], source: &str) -> Result<()> {
    if !trusted_extends.is_empty() {
        return Err(Error::Other(format!(
            "`trusted_extends:` is only allowed in the user's top-level config; \
             declaring it in an extended config ({source}) is refused because it would \
             let a ruleset allowlist itself into auto-applying its own content fixers"
        )));
    }
    Ok(())
}

/// Reject a legacy `${VAR}` environment reference that a REMOTE ruleset could
/// route into `git_commit_message`'s `since:` -- the one field alint still
/// expands POSIX-style at evaluate time. Remote bodies are deliberately never
/// `{{env.*}}`-interpolated (a pinned third-party ruleset must not read the
/// consumer's environment), but `since:` expansion runs whatever the rule's
/// source, and the resolved value is echoed in the "could not resolve commit
/// range" error -- an exfiltration channel into CI logs and SARIF. Scans every
/// `since:` (including one a remote contributes to a user rule by field-merge,
/// with no `kind`) and every `vars:` value (which a template could substitute
/// into `since:`), at every `require:` depth. A `${` ASSEMBLED from several
/// vars only exists after expansion, so `finalize` re-checks it there
/// ([`reject_remote_assembled_env_refs`]).
pub(crate) fn reject_env_expansion_in(
    rules: &[Mapping],
    templates: &[Mapping],
    source: &str,
) -> Result<()> {
    fn offending(m: &Mapping) -> Option<String> {
        let has_env = |v: &serde_yaml_ng::Value| v.as_str().is_some_and(|s| s.contains("${"));
        if m.get("since").is_some_and(has_env) {
            return Some("since".to_string());
        }
        if let Some(vars) = m.get("vars").and_then(|v| v.as_mapping()) {
            for (k, v) in vars {
                if has_env(v) {
                    return Some(format!("vars.{}", k.as_str().unwrap_or("<var>")));
                }
            }
        }
        m.get("require")
            .and_then(|v| v.as_sequence())
            .into_iter()
            .flatten()
            .filter_map(serde_yaml_ng::Value::as_mapping)
            .find_map(offending)
    }
    for m in rules.iter().chain(templates) {
        if let Some(field) = offending(m) {
            let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("(unknown)");
            return Err(Error::Other(format!(
                "{id:?}: `{field}` references an environment variable (`${{...}}`), which \
                 a remote ruleset ({source}) may not read; set the value in your own \
                 top-level config instead"
            )));
        }
    }
    Ok(())
}

/// Reject YAML constructs in `rules:` / `templates:` that the trust gates and
/// the typed deserializer would read differently. Those entries are kept as raw
/// [`Mapping`]s so composition can field-merge them, and every gate inspects
/// them with `Mapping::get("kind")` and friends. But serde strips a custom tag
/// when it resolves a field, so `!x kind: command` (a tagged KEY) is invisible
/// to `get("kind")` yet becomes `kind: command` in the [`alint_core::RuleSpec`],
/// and `kind: !x command` (a tagged VALUE) defeats `as_str()` the same way. A
/// non-string key and a `<<` merge key have no meaning in the DSL either (merge
/// keys are NOT applied to config), so all of them are refused here, for every
/// source, before any gate runs: the gates and serde then see one shape.
///
/// The other top-level fields deserialize straight into typed structs, which
/// the gates read after serde has resolved them, so only these two raw lists
/// need the check.
pub(crate) fn reject_ambiguous_yaml_in(raw: &RawConfig, source: &str) -> Result<()> {
    for (section, list) in [("rules", &raw.rules), ("templates", &raw.templates)] {
        for (i, m) in list.iter().enumerate() {
            let mut path = format!("{section}[{i}]");
            if let Some(problem) = find_ambiguous_yaml_in_mapping(m, &mut path) {
                return Err(Error::Other(format!(
                    "{source}: {problem}; YAML tags, non-string keys and `<<` merge \
                     keys are not supported in alint rules or templates"
                )));
            }
        }
    }
    Ok(())
}

fn find_ambiguous_yaml_in_mapping(m: &Mapping, path: &mut String) -> Option<String> {
    use serde_yaml_ng::Value;
    for (k, v) in m {
        let key = match k {
            Value::String(s) if s == "<<" => {
                return Some(format!("`{path}` uses a `<<` merge key"));
            }
            Value::String(s) => s.as_str(),
            Value::Tagged(t) => {
                return Some(format!("`{path}` has a key with the YAML tag `{}`", t.tag));
            }
            _ => return Some(format!("`{path}` has a non-string key")),
        };
        let len = path.len();
        path.push('.');
        path.push_str(key);
        let found = find_ambiguous_yaml_in_value(v, path);
        path.truncate(len);
        if found.is_some() {
            return found;
        }
    }
    None
}

fn find_ambiguous_yaml_in_value(v: &serde_yaml_ng::Value, path: &mut String) -> Option<String> {
    use serde_yaml_ng::Value;
    use std::fmt::Write as _;
    match v {
        Value::Tagged(t) => Some(format!("`{path}` has the YAML tag `{}`", t.tag)),
        Value::Mapping(m) => find_ambiguous_yaml_in_mapping(m, path),
        Value::Sequence(seq) => seq.iter().enumerate().find_map(|(i, item)| {
            let len = path.len();
            let _ = write!(path, "[{i}]");
            let found = find_ambiguous_yaml_in_value(item, path);
            path.truncate(len);
            found
        }),
        _ => None,
    }
}
