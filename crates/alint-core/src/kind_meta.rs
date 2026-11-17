//! Static per-kind capability metadata.
//!
//! A [`KindMeta`] record describes what a rule kind can select and do without
//! constructing a rule instance. Built-in kinds use borrowed static data; future
//! plugin kinds can use the owned forms of the same [`Cow`] fields.

use std::borrow::Cow;

use serde::ser::{SerializeSeq, SerializeStruct};
use serde::{Serialize, Serializer};

use crate::structured_format::Format;

/// How the engine dispatches a rule kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DispatchClass {
    /// The engine reads each matching file once and dispatches its bytes to the rule.
    PerFile {
        /// Whether the rule must see the complete repository index under `--changed`.
        full_index: bool,
    },
    /// The rule evaluates the repository index as a whole.
    FullIndex {
        /// Whether a path scope is available as the `--changed` skip predicate.
        path_scope: bool,
    },
    /// The rule is evaluated rule-major over an enumerable path scope.
    RuleMajorScoped,
    /// The rule runs once and has no path scope.
    SingleShot,
}

/// Whether the common `paths:` field is part of a kind's contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathsSupport {
    Required,
    Optional,
    Forbidden,
}

/// Which `scope_filter:` predicates a kind accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScopeFilterSupport {
    Full,
    AncestorOnly,
    Rejected { reason: Cow<'static, str> },
}

/// How `git_tracked_only:` narrows a kind's index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitTrackedSupport {
    None,
    FileOnly,
    DirAware,
}

/// Whether a Git range's `since:` input is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SinceSupport {
    None,
    Optional,
    Required,
}

/// The syntax and semantics of a kind-specific selector field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectorType {
    Glob,
    GlobList,
    BasenameGlob,
    LiteralPath,
    PathTemplate,
    Regex,
    DiffGlob,
    Extract,
}

/// One kind-specific selector accepted by a rule kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SelectorMeta {
    pub name: Cow<'static, str>,
    #[serde(rename = "type")]
    pub selector_type: SelectorType,
    /// Whether a nested config can confine this selector by prefixing its root.
    pub rerootable: bool,
}

/// Git operations a kind may request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitOperation {
    TrackedSet,
    Blame,
    Diff,
    Log,
    VerifyCommit,
    /// Mutates the repository index without deleting the working-tree file.
    Untrack,
}

/// A host capability a kind may exercise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Capability {
    SpawnUserCommand,
    Git {
        operations: Cow<'static, [GitOperation]>,
    },
    HostKeyring,
    EnvRead,
    EnvExport,
    AnalysisTime,
    OutOfRootRead,
    WritesDuringCheck,
    UnixOnly,
}

/// Machine-checkable predicates for option-dependent capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionPredicate {
    ContainsLegacyEnvReference,
}

/// When a declared capability is active for a rule instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CapabilityCondition {
    Always,
    /// The option's effective value enables the capability: `true` for a
    /// boolean, or present/non-empty for an optional value. This does not mean
    /// merely that the YAML key was written (for example,
    /// `git_tracked_only: false` does not activate Git).
    OptionSet {
        option: Cow<'static, str>,
    },
    OptionMatches {
        option: Cow<'static, str>,
        predicate: OptionPredicate,
    },
    FixOpConfigured {
        op: Cow<'static, str>,
    },
    CommonFieldSet {
        field: Cow<'static, str>,
    },
}

impl CapabilityCondition {
    /// Whether this condition holds for the rule `spec` declares: the
    /// option's effective value, not its mere presence. The engine reads it
    /// to decide whether a rule uses a capability, and the trust gates to
    /// decide whether a rule runs Git.
    #[must_use]
    pub fn holds_for(&self, spec: &crate::RuleSpec) -> bool {
        match self {
            Self::FixOpConfigured { op } => spec
                .fix
                .as_ref()
                .is_some_and(|fix| fix.op_name() == op.as_ref()),
            Self::CommonFieldSet { field } => match field.as_ref() {
                "scope_filter.changed_since" => spec
                    .scope_filter
                    .as_ref()
                    .and_then(|filter| filter.changed_since.as_ref())
                    .is_some(),
                // Git currently has no other common-field condition. Keep
                // unknown future fields inactive until their effective-value
                // semantics are defined here rather than treating mere YAML
                // presence as truth.
                _ => false,
            },
            _ => self.holds_for_options(&spec.extra),
        }
    }

    /// [`Self::holds_for`] for a rule written as the mapping `options`, its
    /// `fix:` and `scope_filter:` read from it too: a nested `require:` entry,
    /// before it is built.
    #[must_use]
    pub fn holds_for_options(&self, options: &crate::yaml::Mapping) -> bool {
        match self {
            Self::Always => true,
            Self::OptionSet { option } => options
                .get(option.as_ref())
                .is_some_and(|value| option_value_is_set(value.parser())),
            Self::OptionMatches { option, predicate } => {
                let Some(value) = options
                    .get(option.as_ref())
                    .and_then(|value| value.as_str())
                else {
                    return false;
                };
                match predicate {
                    OptionPredicate::ContainsLegacyEnvReference => value.contains("${"),
                }
            }
            Self::FixOpConfigured { op } => options
                .get("fix")
                .and_then(|fix| fix.as_mapping())
                .is_some_and(|fix| fix.contains_key(op.as_ref())),
            Self::CommonFieldSet { field } => match field.as_ref() {
                "scope_filter.changed_since" => options
                    .get("scope_filter")
                    .and_then(|filter| filter.as_mapping())
                    .and_then(|filter| filter.get("changed_since"))
                    .is_some_and(|since| !since.is_null()),
                _ => false,
            },
        }
    }
}

/// Whether an option's value turns its capability on: `true`, or a value
/// that is present and not empty.
fn option_value_is_set(value: &ayaml::Value) -> bool {
    match value {
        ayaml::Value::Null => false,
        ayaml::Value::Bool(value) => *value,
        ayaml::Value::String(value) => !value.is_empty(),
        ayaml::Value::Sequence(value) => !value.is_empty(),
        ayaml::Value::Mapping(value) => !value.is_empty(),
        ayaml::Value::Number(_) => true,
        ayaml::Value::Tagged(value) => option_value_is_set(&value.value),
    }
}

/// A capability and the condition under which a kind may use it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapabilityMeta {
    pub capability: Capability,
    pub condition: CapabilityCondition,
}

/// The maximum content access shape used by a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentAccess {
    PathOnly,
    Metadata,
    /// Reads a prefix. `max_bytes: None` means the length comes from a rule
    /// option and therefore has no kind-wide fixed maximum.
    Prefix {
        max_bytes: Option<usize>,
        /// Whether the same evaluation also reads file metadata.
        metadata: bool,
    },
    /// Reads a suffix. `max_bytes: None` means the length comes from a rule
    /// option and therefore has no kind-wide fixed maximum.
    Suffix {
        max_bytes: Option<usize>,
        /// Whether the same evaluation also reads file metadata.
        metadata: bool,
    },
    WholeFile,
    OtherFiles,
    Blame,
    Delegated,
}

/// A repository path option outside the kind's normal scope whose change can
/// alter the kind's verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DependencyMeta {
    pub field: Cow<'static, str>,
}

/// Static behavior and capability inventory for one canonical rule kind.
///
/// The boolean fields are independent schema capabilities, not states of one
/// state machine, so keeping them explicit makes record audits mechanical.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindMeta {
    pub dispatch: DispatchClass,
    pub paths: PathsSupport,
    pub scope_filter: ScopeFilterSupport,
    pub git_tracked_only: GitTrackedSupport,
    pub respect_gitignore: bool,
    pub root_only: bool,
    pub selectors: Cow<'static, [SelectorMeta]>,
    pub since: SinceSupport,
    /// Capabilities intrinsic to this kind. Use
    /// [`effective_capabilities`](Self::effective_capabilities) when reporting
    /// or auditing the complete surface: it also derives capabilities activated
    /// by common engine fields.
    pub capabilities: Cow<'static, [CapabilityMeta]>,
    pub content: ContentAccess,
    pub formats: Cow<'static, [Format]>,
    pub dependencies: Cow<'static, [DependencyMeta]>,
    pub fix_ops: Cow<'static, [Cow<'static, str>]>,
    pub expect_matches: bool,
    pub trace: crate::trace::TraceMeta,
    pub nested_rules: bool,
}

static SCOPE_FILTER_CHANGED_SINCE_CAPABILITY: CapabilityMeta = CapabilityMeta {
    capability: Capability::Git {
        operations: Cow::Borrowed(&[GitOperation::Diff]),
    },
    condition: CapabilityCondition::CommonFieldSet {
        field: Cow::Borrowed("scope_filter.changed_since"),
    },
};

impl KindMeta {
    /// All capabilities this kind can exercise, including those supplied by
    /// common engine fields rather than its kind-specific option model.
    pub fn effective_capabilities(&self) -> impl Iterator<Item = &CapabilityMeta> {
        let scope_filter_git = matches!(&self.scope_filter, ScopeFilterSupport::Full)
            .then_some(&SCOPE_FILTER_CHANGED_SINCE_CAPABILITY);
        self.capabilities.iter().chain(scope_filter_git)
    }

    /// Allocation-free serialized view of [`Self::effective_capabilities`].
    pub fn serialized_capabilities(&self) -> impl Serialize + '_ {
        SerializedCapabilities(self)
    }
}

struct SerializedCapabilities<'a>(&'a KindMeta);

impl Serialize for SerializedCapabilities<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let derived_len = usize::from(matches!(&self.0.scope_filter, ScopeFilterSupport::Full));
        let mut seq = serializer.serialize_seq(Some(self.0.capabilities.len() + derived_len))?;
        for capability in self.0.effective_capabilities() {
            seq.serialize_element(capability)?;
        }
        seq.end()
    }
}

impl Serialize for KindMeta {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("KindMeta", 16)?;
        state.serialize_field("dispatch", &self.dispatch)?;
        state.serialize_field("paths", &self.paths)?;
        state.serialize_field("scope_filter", &self.scope_filter)?;
        state.serialize_field("git_tracked_only", &self.git_tracked_only)?;
        state.serialize_field("respect_gitignore", &self.respect_gitignore)?;
        state.serialize_field("root_only", &self.root_only)?;
        state.serialize_field("selectors", &self.selectors)?;
        state.serialize_field("since", &self.since)?;
        state.serialize_field("capabilities", &SerializedCapabilities(self))?;
        state.serialize_field("content", &self.content)?;
        state.serialize_field("formats", &self.formats)?;
        state.serialize_field("dependencies", &self.dependencies)?;
        state.serialize_field("fix_ops", &self.fix_ops)?;
        state.serialize_field("expect_matches", &self.expect_matches)?;
        state.serialize_field("trace", &self.trace)?;
        state.serialize_field("nested_rules", &self.nested_rules)?;
        state.end()
    }
}

/// Trust class applied to a fix operation before a rule-specific tier is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FixTrustClass {
    Spawning,
    ContentInjecting,
    FixedBehaviour,
}

/// Static trust metadata for one fix operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixOpMeta {
    pub name: Cow<'static, str>,
    pub trust: FixTrustClass,
}

/// The complete fix-operation trust inventory. Kept beside [`FixOpMeta`] so
/// loaders can classify a raw `fix:` block before deserializing it.
pub(crate) const FIX_OPS: &[FixOpMeta] = &[
    FixOpMeta {
        name: Cow::Borrowed("file_create"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("file_remove"),
        trust: FixTrustClass::FixedBehaviour,
    },
    FixOpMeta {
        name: Cow::Borrowed("file_prepend"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("file_append"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("file_rename"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("file_trim_trailing_whitespace"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("file_append_final_newline"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("file_normalize_line_endings"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("file_strip_bidi"),
        trust: FixTrustClass::FixedBehaviour,
    },
    FixOpMeta {
        name: Cow::Borrowed("file_strip_zero_width"),
        trust: FixTrustClass::FixedBehaviour,
    },
    FixOpMeta {
        name: Cow::Borrowed("file_strip_bom"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("file_collapse_blank_lines"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("replace"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("set_value"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("remove_value"),
        trust: FixTrustClass::FixedBehaviour,
    },
    FixOpMeta {
        name: Cow::Borrowed("chmod"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("git_untrack"),
        trust: FixTrustClass::Spawning,
    },
    FixOpMeta {
        name: Cow::Borrowed("command"),
        trust: FixTrustClass::Spawning,
    },
    FixOpMeta {
        name: Cow::Borrowed("dir_create"),
        trust: FixTrustClass::FixedBehaviour,
    },
    FixOpMeta {
        name: Cow::Borrowed("sync_from"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("relocate"),
        trust: FixTrustClass::FixedBehaviour,
    },
    FixOpMeta {
        name: Cow::Borrowed("create_and_register"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("sort"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("indent_style"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("insert_line"),
        trust: FixTrustClass::ContentInjecting,
    },
    FixOpMeta {
        name: Cow::Borrowed("insert_header"),
        trust: FixTrustClass::ContentInjecting,
    },
];

pub(crate) fn fix_op_meta(name: &str) -> Option<&'static FixOpMeta> {
    FIX_OPS.iter().find(|meta| meta.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FixSpec;
    use std::collections::BTreeSet;

    #[test]
    fn fix_op_metadata_is_an_exhaustive_unique_partition() {
        let names: BTreeSet<&str> = FIX_OPS.iter().map(|meta| meta.name.as_ref()).collect();
        assert_eq!(
            names.len(),
            FIX_OPS.len(),
            "fix-op metadata names must be unique"
        );
        assert_eq!(
            names,
            FixSpec::ALL_OP_NAMES.iter().copied().collect(),
            "every fix operation must have trust metadata"
        );
        assert_eq!(
            [
                FIX_OPS
                    .iter()
                    .filter(|meta| meta.trust == FixTrustClass::Spawning)
                    .count(),
                FIX_OPS
                    .iter()
                    .filter(|meta| meta.trust == FixTrustClass::ContentInjecting)
                    .count(),
                FIX_OPS
                    .iter()
                    .filter(|meta| meta.trust == FixTrustClass::FixedBehaviour)
                    .count(),
            ],
            [2, 18, 6]
        );
    }

    #[test]
    fn full_scope_serializes_the_engine_derived_git_capability() {
        let meta = KindMeta {
            dispatch: DispatchClass::PerFile { full_index: false },
            paths: PathsSupport::Required,
            scope_filter: ScopeFilterSupport::Full,
            git_tracked_only: GitTrackedSupport::None,
            respect_gitignore: false,
            root_only: false,
            selectors: Cow::Borrowed(&[]),
            since: SinceSupport::None,
            capabilities: Cow::Borrowed(&[]),
            content: ContentAccess::WholeFile,
            formats: Cow::Borrowed(&[]),
            dependencies: Cow::Borrowed(&[]),
            fix_ops: Cow::Borrowed(&[]),
            expect_matches: true,
            trace: crate::TraceMeta::ASSERTION,
            nested_rules: false,
        };

        let capabilities: Vec<&CapabilityMeta> = meta.effective_capabilities().collect();
        assert!(matches!(
            capabilities.as_slice(),
            [CapabilityMeta {
                capability: Capability::Git { operations },
                condition: CapabilityCondition::CommonFieldSet { field },
            }] if operations.as_ref() == [GitOperation::Diff]
                && field == "scope_filter.changed_since"
        ));
        let json = serde_json::to_value(&meta).unwrap();
        assert_eq!(
            json["capabilities"][0]["condition"]["field"],
            "scope_filter.changed_since"
        );
    }
}
