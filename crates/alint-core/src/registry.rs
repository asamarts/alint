use std::collections::HashMap;
use std::sync::Arc;

use crate::config::RuleSpec;
use crate::did_you_mean;
use crate::error::{Error, Result};
#[cfg(debug_assertions)]
use crate::kind_meta::{
    Capability, CapabilityCondition, ContentAccess, DispatchClass, GitOperation,
};
use crate::kind_meta::{
    FixOpMeta, GitTrackedSupport, KindMeta, PathsSupport, ScopeFilterSupport, SinceSupport,
    fix_op_meta,
};
#[cfg(debug_assertions)]
use crate::rule::GitTrackedMode;
use crate::rule::Rule;

pub type RuleBuilder = fn(&RuleSpec) -> Result<Box<dyn Rule>>;

#[derive(Clone, Copy)]
enum StoredBuilder {
    Direct(RuleBuilder),
    Optionless(RuleBuilder),
}

impl StoredBuilder {
    fn build(self, spec: &RuleSpec) -> Result<Box<dyn Rule>> {
        match self {
            Self::Direct(builder) => builder(spec),
            Self::Optionless(builder) => {
                spec.deny_unknown_options()?;
                builder(spec)
            }
        }
    }
}

#[derive(Debug, Clone)]
enum StoredKindMeta {
    Static(&'static KindMeta),
    Owned(Arc<KindMeta>),
}

impl StoredKindMeta {
    fn as_ref(&self) -> &KindMeta {
        match self {
            Self::Static(meta) => meta,
            Self::Owned(meta) => meta,
        }
    }
}

#[derive(Clone)]
struct RegisteredKind {
    builder: StoredBuilder,
    metadata: StoredKindMeta,
}

/// Canonical rule-kind registrations plus alias-to-canonical mappings.
///
/// Builders and metadata share one canonical entry, avoiding duplicate keys
/// and making it impossible for them to drift. Aliases store no builder or
/// metadata: build and metadata lookups both resolve through the canonical
/// entry.
#[derive(Clone, Default)]
pub struct RuleRegistry {
    kinds: HashMap<String, RegisteredKind>,
    aliases: HashMap<String, String>,
}

impl std::fmt::Debug for RuleRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuleRegistry")
            .field("kinds", &self.kinds.keys().collect::<Vec<_>>())
            .field("aliases", &self.aliases)
            .finish()
    }
}

/// Read-only capability view over a rule registry.
///
/// The catalog borrows the registry: built-in static records and plugin-owned
/// records stay in place, so passing it through a config load performs no
/// metadata cloning or allocation.
#[derive(Debug, Clone, Copy)]
pub struct KindCatalog<'registry> {
    registry: &'registry RuleRegistry,
}

impl<'registry> KindCatalog<'registry> {
    /// Resolve a canonical kind or alias to its capability record.
    #[must_use]
    pub fn meta(self, kind: &str) -> Option<&'registry KindMeta> {
        self.registry.meta(kind)
    }

    /// Resolve a registered alias, leaving canonical and unknown names intact.
    #[must_use]
    pub fn canonical<'name>(self, name: &'name str) -> &'name str
    where
        'registry: 'name,
    {
        self.registry.canonical_kind(name)
    }

    /// Iterate over every canonical kind and its capability record.
    pub fn kinds(self) -> impl Iterator<Item = (&'registry str, &'registry KindMeta)> {
        self.registry.kind_metadata()
    }

    /// Look up one fix operation's trust metadata.
    #[must_use]
    pub fn fix_op(self, name: &str) -> Option<&'static FixOpMeta> {
        fix_op_meta(name)
    }
}

impl RuleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Borrow this registry as the capability catalog consumed by config loaders.
    #[must_use]
    pub fn catalog(&self) -> KindCatalog<'_> {
        KindCatalog { registry: self }
    }

    /// Register a built-in kind and its borrowed static capability record.
    pub fn register(&mut self, kind: &str, meta: &'static KindMeta, builder: RuleBuilder) {
        self.insert_kind(
            kind,
            RegisteredKind {
                builder: StoredBuilder::Direct(builder),
                metadata: StoredKindMeta::Static(meta),
            },
        );
    }

    /// Register a runtime-discovered kind and its owned capability record.
    pub fn register_owned(&mut self, kind: &str, meta: Arc<KindMeta>, builder: RuleBuilder) {
        self.insert_kind(
            kind,
            RegisteredKind {
                builder: StoredBuilder::Direct(builder),
                metadata: StoredKindMeta::Owned(meta),
            },
        );
    }

    /// Register an option-less rule kind with its capability record.
    pub fn register_optionless(
        &mut self,
        kind: &str,
        meta: &'static KindMeta,
        builder: RuleBuilder,
    ) {
        self.insert_kind(
            kind,
            RegisteredKind {
                builder: StoredBuilder::Optionless(builder),
                metadata: StoredKindMeta::Static(meta),
            },
        );
    }

    fn insert_kind(&mut self, kind: &str, registration: RegisteredKind) {
        self.aliases.remove(kind);
        self.kinds.insert(kind.to_string(), registration);
    }

    /// Register `alias` as another spelling of an existing canonical kind.
    ///
    /// The alias stores no duplicate builder or metadata and therefore cannot
    /// diverge from its canonical kind.
    pub fn register_alias(&mut self, alias: &str, canonical: &str) {
        assert!(
            !self.aliases.contains_key(canonical),
            "alias `{alias}` targets alias `{canonical}`; alias chains are not supported"
        );
        assert!(
            self.kinds.contains_key(canonical),
            "alias `{alias}` targets unregistered canonical kind `{canonical}`"
        );
        assert!(
            !self.kinds.contains_key(alias),
            "alias `{alias}` conflicts with a canonical kind"
        );
        assert!(
            self.aliases
                .insert(alias.to_string(), canonical.to_string())
                .is_none(),
            "alias `{alias}` is already registered"
        );
    }

    /// Resolve a kind or alias to its canonical capability record.
    pub fn meta(&self, kind: &str) -> Option<&KindMeta> {
        let canonical = self.canonical_kind(kind);
        self.kinds
            .get(canonical)
            .map(|registration| registration.metadata.as_ref())
    }

    /// Canonical kinds and their capability metadata.
    pub fn kind_metadata(&self) -> impl Iterator<Item = (&str, &KindMeta)> {
        self.kinds
            .iter()
            .map(|(kind, registration)| (kind.as_str(), registration.metadata.as_ref()))
    }

    /// Registered alias-to-canonical mappings, in arbitrary order.
    pub fn aliases(&self) -> impl Iterator<Item = (&str, &str)> {
        self.aliases
            .iter()
            .map(|(alias, canonical)| (alias.as_str(), canonical.as_str()))
    }

    /// Resolve `name` to its canonical kind: the target of an alias registered
    /// via [`register_alias`](Self::register_alias), or `name` itself when it is
    /// already canonical (or not registered at all).
    pub fn canonical_kind<'a>(&'a self, name: &'a str) -> &'a str {
        self.aliases.get(name).map_or(name, String::as_str)
    }

    /// Every registered canonical kind, in arbitrary order.
    pub fn canonical_kinds(&self) -> impl Iterator<Item = &str> {
        self.kinds.keys().map(String::as_str)
    }

    /// Validate the configured common fields against the canonical kind record,
    /// then construct the rule. Validation deliberately precedes the builder so
    /// every built-in, alias, nested rule, and plugin kind gets the same
    /// fail-loud contract.
    pub fn build(&self, spec: &RuleSpec) -> Result<Box<dyn Rule>> {
        let canonical = self.canonical_kind(&spec.kind);
        let registration = self
            .kinds
            .get(canonical)
            .ok_or_else(|| Error::UnknownRuleKind(spec.kind.clone()))?;
        let validate = |spec: &RuleSpec| {
            validate_configured_fields(
                &ConfiguredFields::from_rule_spec(spec),
                &spec.kind,
                registration.metadata.as_ref(),
            )
            .map_err(|violation| Error::rule_config(&spec.id, violation.message))
        };
        validate(spec).map_err(|error| spec.redact_unless_reproduced(error, validate))?;
        let rule = build_rule(registration, spec)?;
        #[cfg(debug_assertions)]
        assert_instance_conformance(
            canonical,
            registration.metadata.as_ref(),
            spec,
            rule.as_ref(),
        );
        Ok(rule)
    }

    /// [`Self::build`] for a nested-rule instantiation whose field set was already
    /// validated when the parent rule was built: templating changes only string
    /// values, never which fields or option keys are present.
    pub fn build_instantiated(&self, spec: &RuleSpec) -> Result<Box<dyn Rule>> {
        let canonical = self.canonical_kind(&spec.kind);
        let registration = self
            .kinds
            .get(canonical)
            .ok_or_else(|| Error::UnknownRuleKind(spec.kind.clone()))?;
        build_rule(registration, spec)
    }

    /// Validate `rule`'s nested `require:` rules (see [`Rule::validate_nested`]),
    /// with the configuration-error redaction [`Self::build`] applies.
    pub fn validate_nested(&self, spec: &RuleSpec, rule: &dyn Rule) -> Result<()> {
        rule.validate_nested(self).map_err(|error| {
            spec.redact_unless_reproduced(error, |masked| self.build(masked)?.validate_nested(self))
        })
    }

    pub fn known_kinds(&self) -> impl Iterator<Item = &str> {
        self.kinds
            .keys()
            .map(String::as_str)
            .chain(self.aliases.keys().map(String::as_str))
    }
}

/// One common rule field whose presence is described by [`KindMeta`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ConfiguredField {
    Paths,
    ScopeFilter,
    Fix,
    ExpectMatches,
    GitTrackedOnly,
    RespectGitignore,
    RootOnly,
    Since,
}

/// One predicate inside a configured `scope_filter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ConfiguredScopeFilterField {
    ChangedSince,
    HasAncestor,
    IncludeManifestPaths,
    ExcludeManifestPaths,
}

/// Presence-only view of the common rule fields described by [`KindMeta`].
///
/// The DSL builds the same view from an effective raw mapping before it drops
/// composition provenance; [`RuleRegistry`] builds it from a typed [`RuleSpec`].
/// Keeping the policy on this neutral view makes those two checks identical.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConfiguredFields<'a> {
    fields: u16,
    scope_filter_fields: u8,
    fix_op: Option<&'a str>,
    fix_is_null: bool,
    scope_filter_is_null: bool,
}

impl<'a> ConfiguredFields<'a> {
    pub fn new(fix_op: Option<&'a str>) -> Self {
        Self {
            fields: 0,
            scope_filter_fields: 0,
            fix_op,
            fix_is_null: false,
            scope_filter_is_null: false,
        }
    }

    #[must_use]
    pub fn with(mut self, field: ConfiguredField, present: bool) -> Self {
        let bit = 1 << (field as u8);
        self.fields = if present {
            self.fields | bit
        } else {
            self.fields & !bit
        };
        self
    }

    /// Record that the configured `fix` value is explicitly null. Other
    /// operation-less shapes are left to `FixSpec`'s type/shape diagnostics.
    #[must_use]
    pub fn with_null_fix(mut self, is_null: bool) -> Self {
        self.fix_is_null = is_null;
        self
    }

    /// Record that the configured `scope_filter` value is explicitly null.
    /// A null value would otherwise deserialize to `None` and silently disable
    /// the configured field despite the public schema requiring a mapping.
    #[must_use]
    pub fn with_null_scope_filter(mut self, is_null: bool) -> Self {
        self.scope_filter_is_null = is_null;
        self
    }

    #[must_use]
    pub fn with_scope_filter(mut self, field: ConfiguredScopeFilterField, present: bool) -> Self {
        let bit = 1 << (field as u8);
        self.scope_filter_fields = if present {
            self.scope_filter_fields | bit
        } else {
            self.scope_filter_fields & !bit
        };
        self
    }

    fn has(self, field: ConfiguredField) -> bool {
        self.fields & (1 << (field as u8)) != 0
    }

    fn has_scope_filter(self, field: ConfiguredScopeFilterField) -> bool {
        self.scope_filter_fields & (1 << (field as u8)) != 0
    }

    fn from_rule_spec(spec: &'a RuleSpec) -> Self {
        let mut fields = Self::new(spec.fix.as_ref().map(crate::FixSpec::op_name))
            .with(ConfiguredField::Paths, spec.paths.is_some())
            .with(ConfiguredField::ScopeFilter, spec.scope_filter.is_some())
            .with(ConfiguredField::Fix, spec.fix.is_some())
            // A typed RuleSpec cannot distinguish an omitted false boolean from
            // an explicitly configured false. The raw DSL check can and rejects
            // either spelling on a kind that does not expose this field.
            .with(ConfiguredField::ExpectMatches, spec.expect_matches)
            .with(
                ConfiguredField::GitTrackedOnly,
                spec.extra.contains_key("git_tracked_only"),
            )
            .with(
                ConfiguredField::RespectGitignore,
                spec.extra.contains_key("respect_gitignore"),
            )
            .with(
                ConfiguredField::RootOnly,
                spec.extra.contains_key("root_only"),
            )
            .with(ConfiguredField::Since, spec.extra.contains_key("since"));
        if let Some(scope) = &spec.scope_filter {
            fields = fields
                .with_scope_filter(
                    ConfiguredScopeFilterField::ChangedSince,
                    scope.changed_since.is_some(),
                )
                .with_scope_filter(
                    ConfiguredScopeFilterField::HasAncestor,
                    scope.has_ancestor.is_some(),
                )
                .with_scope_filter(
                    ConfiguredScopeFilterField::IncludeManifestPaths,
                    scope.include_manifest_paths.is_some(),
                )
                .with_scope_filter(
                    ConfiguredScopeFilterField::ExcludeManifestPaths,
                    scope.exclude_manifest_paths.is_some(),
                );
        }
        fields
    }
}

/// One common field that conflicts with a kind's capability record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredFieldViolation {
    pub field: String,
    pub message: String,
}

fn violation(field: impl Into<String>, message: String) -> ConfiguredFieldViolation {
    ConfiguredFieldViolation {
        field: field.into(),
        message,
    }
}

/// Validate common-field presence against one canonical kind record.
///
/// This is public for `alint-dsl`, which performs the same check while the
/// effective mapping still carries template / `extends:` field provenance.
pub fn validate_configured_fields(
    fields: &ConfiguredFields<'_>,
    kind: &str,
    meta: &KindMeta,
) -> std::result::Result<(), ConfiguredFieldViolation> {
    validate_paths_field(fields, kind, meta)?;
    validate_scope_filter_field(fields, kind, meta)?;
    validate_fix_field(fields, kind, meta)?;
    validate_expect_matches_field(fields, kind, meta)?;
    validate_metadata_options(fields, kind, meta)
}

fn validate_paths_field(
    fields: &ConfiguredFields<'_>,
    kind: &str,
    meta: &KindMeta,
) -> std::result::Result<(), ConfiguredFieldViolation> {
    match (meta.paths, fields.has(ConfiguredField::Paths)) {
        (PathsSupport::Required, false) => Err(violation(
            "paths",
            format!("`{kind}` requires a `paths` field"),
        )),
        (PathsSupport::Forbidden, true) => Err(violation(
            "paths",
            format!("`paths` is not supported by `{kind}` and would be ignored; remove the field"),
        )),
        (PathsSupport::Required | PathsSupport::Optional, true)
        | (PathsSupport::Optional | PathsSupport::Forbidden, false) => Ok(()),
    }
}

fn validate_scope_filter_field(
    fields: &ConfiguredFields<'_>,
    kind: &str,
    meta: &KindMeta,
) -> std::result::Result<(), ConfiguredFieldViolation> {
    if !fields.has(ConfiguredField::ScopeFilter) {
        return Ok(());
    }
    match &meta.scope_filter {
        ScopeFilterSupport::Rejected { reason } => {
            let only_changed_since = fields
                .has_scope_filter(ConfiguredScopeFilterField::ChangedSince)
                && !fields.has_scope_filter(ConfiguredScopeFilterField::HasAncestor)
                && !fields.has_scope_filter(ConfiguredScopeFilterField::IncludeManifestPaths)
                && !fields.has_scope_filter(ConfiguredScopeFilterField::ExcludeManifestPaths);
            if only_changed_since && !matches!(meta.since, SinceSupport::None) {
                Err(violation(
                    "scope_filter.changed_since",
                    format!(
                        "`scope_filter.changed_since` is not supported by `{kind}`; use the \
                         top-level `since` field"
                    ),
                ))
            } else {
                Err(violation(
                    "scope_filter",
                    format!(
                        "`scope_filter` is not supported by `{kind}` and would be ignored: {reason}"
                    ),
                ))
            }
        }
        _ if fields.scope_filter_is_null => Err(violation(
            "scope_filter",
            "`scope_filter` must be a mapping with at least one predicate".to_string(),
        )),
        ScopeFilterSupport::Full => Ok(()),
        ScopeFilterSupport::AncestorOnly => {
            let unsupported = if fields.has_scope_filter(ConfiguredScopeFilterField::ChangedSince) {
                Some("changed_since")
            } else if fields.has_scope_filter(ConfiguredScopeFilterField::IncludeManifestPaths) {
                Some("include_manifest_paths")
            } else if fields.has_scope_filter(ConfiguredScopeFilterField::ExcludeManifestPaths) {
                Some("exclude_manifest_paths")
            } else {
                None
            };
            let Some(field) = unsupported else {
                return Ok(());
            };
            let field = format!("scope_filter.{field}");
            Err(violation(
                field.clone(),
                format!(
                    "`{field}` is not supported by `{kind}`; only \
                     `scope_filter.has_ancestor` is supported"
                ),
            ))
        }
    }
}

fn validate_fix_field(
    fields: &ConfiguredFields<'_>,
    kind: &str,
    meta: &KindMeta,
) -> std::result::Result<(), ConfiguredFieldViolation> {
    if !fields.has(ConfiguredField::Fix) {
        return Ok(());
    }
    // A malformed non-null shape must reach `FixSpec` deserialization, which
    // names the actual type/shape error. Null is the one operation-less shape
    // serde otherwise turns into `None`, so reject it here rather than silently
    // treating a present `fix` key as absent.
    if fields.fix_op.is_none() && !fields.fix_is_null {
        return Ok(());
    }
    let Some(op) = fields.fix_op else {
        let message = if meta.fix_ops.is_empty() {
            format!("`fix` is not supported by `{kind}`; this kind has no fix operations")
        } else {
            let supported = meta
                .fix_ops
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<&str>>()
                .join(", ");
            format!(
                "`fix` for `{kind}` must select exactly one operation; supported fix operations: \
                 {supported}"
            )
        };
        return Err(violation("fix", message));
    };
    if meta
        .fix_ops
        .iter()
        .any(|supported| supported.as_ref() == op)
    {
        return Ok(());
    }
    let message = if meta.fix_ops.is_empty() {
        format!("`fix.{op}` is not supported by `{kind}`; this kind has no fix operations")
    } else {
        let supported = meta
            .fix_ops
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<&str>>()
            .join(", ");
        format!("`fix.{op}` is not supported by `{kind}`; supported fix operations: {supported}")
    };
    Err(violation(format!("fix.{op}"), message))
}

fn validate_expect_matches_field(
    fields: &ConfiguredFields<'_>,
    kind: &str,
    meta: &KindMeta,
) -> std::result::Result<(), ConfiguredFieldViolation> {
    if !fields.has(ConfiguredField::ExpectMatches) || meta.expect_matches {
        return Ok(());
    }
    Err(violation(
        "expect_matches",
        format!(
            "`expect_matches` is not supported by `{kind}`; use it only on rules with an \
             enumerable file `paths` scope (existence and selector rules have their own \
             empty-set semantics)"
        ),
    ))
}

fn validate_metadata_options(
    fields: &ConfiguredFields<'_>,
    kind: &str,
    meta: &KindMeta,
) -> std::result::Result<(), ConfiguredFieldViolation> {
    if fields.has(ConfiguredField::GitTrackedOnly)
        && matches!(meta.git_tracked_only, GitTrackedSupport::None)
    {
        return Err(violation(
            "git_tracked_only",
            format!("`git_tracked_only` is not supported by `{kind}` and would be ignored"),
        ));
    }
    if fields.has(ConfiguredField::RespectGitignore) && !meta.respect_gitignore {
        return Err(violation(
            "respect_gitignore",
            format!("`respect_gitignore` is not supported by `{kind}` and would be ignored"),
        ));
    }
    if fields.has(ConfiguredField::RootOnly) && !meta.root_only {
        return Err(violation(
            "root_only",
            format!("`root_only` is not supported by `{kind}` and would be ignored"),
        ));
    }
    match (meta.since, fields.has(ConfiguredField::Since)) {
        (SinceSupport::Required, false) => Err(violation(
            "since",
            format!("`{kind}` requires a `since` field"),
        )),
        (SinceSupport::None, true) => Err(violation(
            "since",
            format!("`since` is not supported by `{kind}` and would be ignored"),
        )),
        (SinceSupport::Required | SinceSupport::Optional, true)
        | (SinceSupport::Optional | SinceSupport::None, false) => Ok(()),
    }
}

#[cfg(debug_assertions)]
fn assert_dispatch_and_content(kind: &str, meta: &KindMeta, rule: &dyn Rule) {
    let per_file = rule.as_per_file().is_some();
    let full_index = rule.requires_full_index();
    let path_scope = rule.path_scope().is_some();
    let dispatch_matches = match meta.dispatch {
        DispatchClass::PerFile {
            full_index: expected_full_index,
        } => per_file && full_index == expected_full_index,
        DispatchClass::FullIndex {
            path_scope: expected_path_scope,
        } => !per_file && full_index && path_scope == expected_path_scope,
        DispatchClass::RuleMajorScoped => !per_file && !full_index && path_scope,
        DispatchClass::SingleShot => !per_file && !full_index && !path_scope,
    };
    assert!(
        dispatch_matches,
        "{kind}: instance dispatch (per_file={per_file}, full_index={full_index}, \
         path_scope={path_scope}) disagrees with metadata {:?}",
        meta.dispatch
    );
    if let Some(per_file_rule) = rule.as_per_file() {
        let bounded_read = per_file_rule.max_bytes_needed();
        match meta.content {
            ContentAccess::Prefix { max_bytes, .. } | ContentAccess::Suffix { max_bytes, .. } => {
                assert!(
                    bounded_read.is_some(),
                    "{kind}: bounded content metadata disagrees with an unbounded per-file rule"
                );
                if let Some(expected) = max_bytes {
                    assert_eq!(
                        bounded_read,
                        Some(expected),
                        "{kind}: fixed content bound disagrees with metadata"
                    );
                }
            }
            ContentAccess::WholeFile => assert!(
                bounded_read.is_none(),
                "{kind}: whole-file metadata disagrees with a bounded per-file rule"
            ),
            other => panic!(
                "{kind}: per-file dispatch cannot declare non-content access metadata {other:?}"
            ),
        }
    }
}

#[cfg(debug_assertions)]
fn assert_instance_conformance(kind: &str, meta: &KindMeta, spec: &RuleSpec, rule: &dyn Rule) {
    assert_dispatch_and_content(kind, meta, rule);

    let git_tracked_only = spec
        .extra
        .get("git_tracked_only")
        .and_then(yaml_serde::Value::as_bool)
        .unwrap_or(false);
    let expected_git_mode = match (meta.git_tracked_only, git_tracked_only) {
        (GitTrackedSupport::FileOnly, true) => GitTrackedMode::FileOnly,
        (GitTrackedSupport::DirAware, true) => GitTrackedMode::DirAware,
        _ => GitTrackedMode::Off,
    };
    assert_eq!(
        rule.git_tracked_mode(),
        expected_git_mode,
        "{kind}: instance git-tracked mode disagrees with metadata"
    );

    let declares_blame = meta.capabilities.iter().any(|capability| {
        matches!(capability.condition, CapabilityCondition::Always)
            && matches!(
                &capability.capability,
                Capability::Git { operations }
                    if operations.contains(&GitOperation::Blame)
            )
    });
    assert_eq!(
        rule.wants_git_blame(),
        declares_blame,
        "{kind}: instance blame requirement disagrees with metadata"
    );
    let declares_analysis_time = meta.capabilities.iter().any(|capability| {
        matches!(capability.condition, CapabilityCondition::Always)
            && matches!(capability.capability, Capability::AnalysisTime)
    });
    assert_eq!(
        rule.wants_analysis_time(),
        declares_analysis_time,
        "{kind}: instance analysis-time requirement disagrees with metadata"
    );
    assert_eq!(
        rule.supports_expect_matches(),
        meta.expect_matches,
        "{kind}: instance expect_matches support disagrees with metadata"
    );
    assert_eq!(
        rule.changed_dependencies().len(),
        meta.dependencies.len(),
        "{kind}: instance changed dependencies disagree with metadata"
    );
    assert_eq!(
        !rule.nested_rule_specs().is_empty(),
        meta.nested_rules,
        "{kind}: instance nested-rule support disagrees with metadata"
    );

    if meta.fix_ops.is_empty() {
        assert!(
            rule.fixer().is_none(),
            "{kind}: instance exposes a fixer but metadata declares no fix operations"
        );
    }
    if let Some(fix) = &spec.fix {
        let op = fix.op_name();
        assert!(
            meta.fix_ops.iter().any(|declared| declared == op),
            "{kind}: configured fix operation `{op}` is absent from metadata"
        );
        assert!(
            rule.fixer().is_some(),
            "{kind}: configured fix operation `{op}` produced no fixer"
        );
    }
}

/// Run `registration`'s builder on `spec`, redacting an error that could quote
/// a sensitive value (see [`RuleSpec::redact_unless_reproduced`]). Inlined:
/// `for_each_dir` builds a nested rule per iterated entry through it.
#[inline]
fn build_rule(registration: &RegisteredKind, spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let build = |spec: &RuleSpec| {
        registration
            .builder
            .build(spec)
            .map_err(|error| enrich_error(error, &spec.kind))
    };
    build(spec)
        .map_err(|error| spec.redact_unless_reproduced(error, |masked| build(masked).map(drop)))
}

/// Apply [`did_you_mean::enrich`] to the message of a `RuleConfig`
/// error. Other error variants pass through unchanged.
fn enrich_error(err: Error, kind: &str) -> Error {
    match err {
        Error::RuleConfig { rule_id, message } => {
            let enriched = did_you_mean::enrich(kind, &message);
            Error::RuleConfig {
                rule_id,
                message: enriched,
            }
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::Level;
    use std::borrow::Cow;

    use crate::{
        ContentAccess, DispatchClass, GitTrackedSupport, PathsSupport, ScopeFilterSupport,
        SinceSupport,
    };

    static TEST_META: KindMeta = KindMeta {
        dispatch: DispatchClass::SingleShot,
        paths: PathsSupport::Forbidden,
        scope_filter: ScopeFilterSupport::Rejected {
            reason: Cow::Borrowed("not file-scoped"),
        },
        git_tracked_only: GitTrackedSupport::None,
        respect_gitignore: false,
        root_only: false,
        selectors: Cow::Borrowed(&[]),
        since: SinceSupport::None,
        capabilities: Cow::Borrowed(&[]),
        content: ContentAccess::PathOnly,
        formats: Cow::Borrowed(&[]),
        dependencies: Cow::Borrowed(&[]),
        fix_ops: Cow::Borrowed(&[]),
        expect_matches: false,
        trace: crate::TraceMeta::ASSERTION,
        nested_rules: false,
    };

    fn fake_spec(kind: &str) -> RuleSpec {
        RuleSpec {
            id: "t".into(),
            kind: kind.into(),
            level: Level::Error,
            paths: None,
            message: None,
            policy_url: None,
            when: None,
            expect_matches: false,
            sensitive: false,
            sensitive_fields: std::collections::BTreeSet::new(),
            fix: None,
            scope_filter: None,
            extra: yaml_serde::Mapping::new(),
        }
    }

    fn parsed_spec(fields: &str) -> RuleSpec {
        yaml_serde::from_str(&format!("id: t\nkind: test_kind\nlevel: error\n{fields}")).unwrap()
    }

    #[derive(Debug)]
    struct FakeRule {
        id: String,
    }

    impl Rule for FakeRule {
        fn id(&self) -> &str {
            &self.id
        }

        fn level(&self) -> Level {
            Level::Error
        }

        fn evaluate(&self, _ctx: &crate::Context<'_>) -> Result<Vec<crate::Violation>> {
            Ok(Vec::new())
        }
    }

    // The function must match the public `RuleBuilder` signature used by the registry.
    #[allow(clippy::unnecessary_wraps)]
    fn ok_builder(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
        Ok(Box::new(FakeRule {
            id: spec.kind.clone(),
        }))
    }

    /// A rule whose nested rules fail validation with `message`.
    #[derive(Debug)]
    struct NestedFailure {
        message: String,
    }

    impl Rule for NestedFailure {
        fn id(&self) -> &'static str {
            "t"
        }

        fn level(&self) -> Level {
            Level::Error
        }

        fn evaluate(&self, _ctx: &crate::Context<'_>) -> Result<Vec<crate::Violation>> {
            Ok(Vec::new())
        }

        fn validate_nested(&self, _registry: &RuleRegistry) -> Result<()> {
            Err(Error::rule_config("t", self.message.clone()))
        }
    }

    // The functions must match the public `RuleBuilder` signature.
    #[allow(clippy::unnecessary_wraps)]
    fn structural_nested_builder(_spec: &RuleSpec) -> Result<Box<dyn Rule>> {
        Ok(Box::new(NestedFailure {
            message: "nested rule #0: unknown field `colour`".to_string(),
        }))
    }

    #[allow(clippy::unnecessary_wraps)]
    fn quoting_nested_builder(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
        Ok(Box::new(NestedFailure {
            message: format!(
                "nested rule #0: invalid regex `{}`",
                spec.extra["pattern"].as_str().unwrap_or_default()
            ),
        }))
    }

    #[test]
    fn validate_nested_redacts_only_an_error_that_could_quote_a_secret() {
        let mut registry = RuleRegistry::new();
        registry.register("structural", &TEST_META, structural_nested_builder);
        registry.register("quoting", &TEST_META, quoting_nested_builder);
        let validate = |kind: &str, secret: bool| {
            let mut spec: RuleSpec = yaml_serde::from_str(&format!(
                "id: t\nkind: {kind}\nlevel: error\npattern: canary\n"
            ))
            .unwrap();
            if secret {
                spec.sensitive_fields.insert("pattern".to_string());
            }
            let rule = registry.build(&spec).unwrap();
            registry
                .validate_nested(&spec, rule.as_ref())
                .unwrap_err()
                .to_string()
        };
        // Nothing secret: each error is the nested rule's own.
        assert!(validate("structural", false).contains("unknown field `colour`"));
        assert!(validate("quoting", false).contains("invalid regex `canary`"));
        // A secret pattern: an error the masked rerun reproduces stays
        // readable, and one that quotes the pattern is redacted.
        let structural = validate("structural", true);
        assert!(
            structural.contains("unknown field `colour`"),
            "{structural}"
        );
        let quoting = validate("quoting", true);
        assert!(
            quoting.contains(crate::REDACTED_CONFIGURATION_ERROR) && !quoting.contains("canary"),
            "{quoting}"
        );
    }

    #[test]
    fn new_registry_has_no_kinds() {
        let r = RuleRegistry::new();
        assert_eq!(r.known_kinds().count(), 0);
    }

    #[test]
    fn register_inserts_and_overwrites_a_kind() {
        let mut r = RuleRegistry::new();
        r.register("my_kind", &TEST_META, ok_builder);
        r.register("my_kind", &TEST_META, ok_builder);
        let kinds: Vec<&str> = r.known_kinds().collect();
        assert_eq!(kinds, vec!["my_kind"]);
    }

    #[test]
    fn alias_reads_the_canonical_static_metadata_record() {
        let mut r = RuleRegistry::new();
        r.register("file_content_matches", &TEST_META, ok_builder);
        r.register_alias("content_matches", "file_content_matches");

        let canonical = r.meta("file_content_matches").unwrap();
        let alias = r.meta("content_matches").unwrap();
        assert_eq!(canonical, &TEST_META);
        assert!(std::ptr::eq(alias, canonical));
        assert_eq!(r.kind_metadata().count(), 1);
    }

    #[test]
    fn borrowed_catalog_resolves_aliases_and_fix_operations() {
        let mut registry = RuleRegistry::new();
        registry.register("canonical", &TEST_META, ok_builder);
        registry.register_alias("alias", "canonical");
        let catalog = registry.catalog();

        assert_eq!(catalog.canonical("alias"), "canonical");
        assert!(std::ptr::eq(
            catalog.meta("alias").unwrap(),
            catalog.meta("canonical").unwrap()
        ));
        assert_eq!(
            catalog
                .kinds()
                .map(|(kind, _meta)| kind)
                .collect::<Vec<_>>(),
            vec!["canonical"]
        );
        assert_eq!(
            catalog.fix_op("command").unwrap().trust,
            crate::FixTrustClass::Spawning
        );
        assert!(catalog.fix_op("not_a_fix_op").is_none());
    }

    #[test]
    fn owned_metadata_survives_the_callers_arc_and_is_shared_by_aliases() {
        let meta = Arc::new(TEST_META.clone());
        let ptr = Arc::as_ptr(&meta);
        let mut r = RuleRegistry::new();
        r.register_owned("plugin/check", Arc::clone(&meta), ok_builder);
        r.register_alias("plugin/check_alias", "plugin/check");
        drop(meta);

        assert_eq!(std::ptr::from_ref(r.meta("plugin/check").unwrap()), ptr);
        assert!(std::ptr::eq(
            r.meta("plugin/check_alias").unwrap(),
            r.meta("plugin/check").unwrap()
        ));
    }

    #[test]
    fn build_rejects_unknown_kind_with_clear_error() {
        let r = RuleRegistry::new();
        let err = r.build(&fake_spec("not_real")).unwrap_err();
        match err {
            Error::UnknownRuleKind(name) => assert_eq!(name, "not_real"),
            other => panic!("expected UnknownRuleKind, got {other:?}"),
        }
    }

    #[test]
    fn build_rejects_unsupported_common_fields_from_metadata() {
        let mut registry = RuleRegistry::new();
        registry.register("test_kind", &TEST_META, ok_builder);

        for (fields, rejected) in [
            ("paths: '**/*'\n", "`paths`"),
            (
                "scope_filter:\n  has_ancestor: Cargo.toml\n",
                "`scope_filter`",
            ),
            ("expect_matches: true\n", "`expect_matches`"),
            ("fix:\n  file_remove: {}\n", "`fix.file_remove`"),
        ] {
            let error = registry
                .build(&parsed_spec(fields))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(rejected) && error.contains("test_kind"),
                "{fields:?}: {error}"
            );
        }
    }

    #[test]
    fn build_rejects_missing_metadata_required_fields() {
        let mut meta = TEST_META.clone();
        meta.paths = PathsSupport::Required;
        meta.since = SinceSupport::Required;
        let mut registry = RuleRegistry::new();
        registry.register_owned("test_kind", Arc::new(meta), ok_builder);

        let missing_paths = registry.build(&parsed_spec("")).unwrap_err().to_string();
        assert!(
            missing_paths.contains("requires a `paths` field"),
            "{missing_paths}"
        );

        let missing_since = registry
            .build(&parsed_spec("paths: '**/*'\n"))
            .unwrap_err()
            .to_string();
        assert!(
            missing_since.contains("requires a `since` field"),
            "{missing_since}"
        );
    }

    #[test]
    fn build_rejects_unsupported_kind_options_from_metadata() {
        let mut registry = RuleRegistry::new();
        registry.register("test_kind", &TEST_META, ok_builder);

        for field in [
            "git_tracked_only: true",
            "respect_gitignore: false",
            "root_only: true",
            "since: main",
        ] {
            let error = registry
                .build(&parsed_spec(&format!("{field}\n")))
                .unwrap_err()
                .to_string();
            let name = field.split(':').next().unwrap();
            assert!(
                error.contains(name) && error.contains("test_kind"),
                "{field:?}: {error}"
            );
        }
    }

    #[test]
    fn ancestor_only_scope_rejects_other_predicates_and_range_kinds_point_to_since() {
        let mut ancestor_meta = TEST_META.clone();
        ancestor_meta.scope_filter = ScopeFilterSupport::AncestorOnly;
        let mut registry = RuleRegistry::new();
        registry.register_owned("test_kind", Arc::new(ancestor_meta), ok_builder);
        let error = registry
            .build(&parsed_spec("scope_filter:\n  changed_since: main\n"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("scope_filter.changed_since"), "{error}");
        assert!(error.contains("scope_filter.has_ancestor"), "{error}");

        let mut range_meta = TEST_META.clone();
        range_meta.since = SinceSupport::Optional;
        let mut registry = RuleRegistry::new();
        registry.register_owned("test_kind", Arc::new(range_meta), ok_builder);
        let error = registry
            .build(&parsed_spec("scope_filter:\n  changed_since: main\n"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("top-level `since`"), "{error}");
    }

    #[test]
    fn alias_builds_with_the_canonical_builder_and_original_spec() {
        let mut r = RuleRegistry::new();
        r.register("file_content_matches", &TEST_META, ok_builder);
        r.register_alias("content_matches", "file_content_matches");

        assert_eq!(r.known_kinds().count(), 2);
        assert_eq!(r.canonical_kind("content_matches"), "file_content_matches");
        assert_eq!(
            r.canonical_kind("file_content_matches"),
            "file_content_matches"
        );
        assert_eq!(r.canonical_kind("not_registered"), "not_registered");
        assert_eq!(
            r.build(&fake_spec("content_matches")).unwrap().id(),
            "content_matches",
            "the canonical builder must receive the original alias-bearing spec"
        );

        let canon: Vec<&str> = r.canonical_kinds().collect();
        assert_eq!(canon, vec!["file_content_matches"]);
    }

    #[test]
    #[should_panic(expected = "targets unregistered canonical kind")]
    fn alias_registration_rejects_an_unknown_canonical_kind() {
        let mut r = RuleRegistry::new();
        r.register_alias("alias", "missing");
    }

    #[test]
    #[should_panic(expected = "alias chains are not supported")]
    fn alias_registration_rejects_an_alias_target() {
        let mut r = RuleRegistry::new();
        r.register("canonical", &TEST_META, ok_builder);
        r.register_alias("first", "canonical");
        r.register_alias("second", "first");
    }

    #[test]
    fn known_kinds_iterator_lists_all_registered() {
        let mut r = RuleRegistry::new();
        r.register("a", &TEST_META, ok_builder);
        r.register("b", &TEST_META, ok_builder);
        r.register("c", &TEST_META, ok_builder);
        let mut kinds: Vec<&str> = r.known_kinds().collect();
        kinds.sort_unstable();
        assert_eq!(kinds, vec!["a", "b", "c"]);
    }

    /// `build_instantiated` skips `validate_configured_fields` because the
    /// parent already validated each nested spec at build time with
    /// placeholder tokens. That is sound only while the configured field set of
    /// an instantiation does not depend on the token values.
    #[test]
    fn nested_instantiation_field_set_is_token_independent() {
        let nested: crate::config::NestedRuleSpec = yaml_serde::from_str(
            "kind: file_exists\npaths: \"{path}/README.md\"\nmessage: \"{dir} needs a README\"\n\
             root_only: false\nscope_filter:\n  has_ancestor: Cargo.toml\n",
        )
        .unwrap();
        let placeholder = crate::template::PathTokens::from_path(std::path::Path::new("_"));
        for path in ["pkgs/a", "pkgs/[weird]{x}/*?", "a/b/c/d.e.f", "", "!neg"] {
            let tokens = crate::template::PathTokens::from_path(std::path::Path::new(path));
            let a = nested.instantiate("parent", 0, crate::Level::Error, &placeholder);
            let b = nested.instantiate("parent", 0, crate::Level::Error, &tokens);
            assert_eq!(
                format!("{:?}", ConfiguredFields::from_rule_spec(&a)),
                format!("{:?}", ConfiguredFields::from_rule_spec(&b)),
                "{path:?}"
            );
            let keys = |s: &crate::RuleSpec| s.extra.keys().cloned().collect::<Vec<_>>();
            assert_eq!(keys(&a), keys(&b), "{path:?}");
        }
    }
}
